//! Linux screen capture: Portal (ashpd) + PipeWire (design.md §3.1.2, §3.2.2).
//!
//! - Screen/window: xdg-desktop-portal ScreenCast. The OS picker is the only
//!   selection path (no app-side window enumeration); the returned PipeWire
//!   node is captured as BGRA frames → scale to profile → [`VideoSink`].
//!
//! Compile verification happens in CI (`cargo check --features capture-linux`
//! on Ubuntu 24.04 and Arch Linux); runtime needs a Wayland + PipeWire session.

use super::{CaptureError, Result};
use base64::Engine;
use ezstreamer_core::config::{Profile, ScreenTarget, ScreenTargetKind};
use ezstreamer_core::video::{bgra_to_rgba, scale_bgra, scale_bgra_strided, VideoSink};
use pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::pod::Pod;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

fn err<E: std::fmt::Display>(e: E) -> CaptureError {
    CaptureError::Failed(e.to_string())
}

// ---------------------------------------------------------------------------
// Portal session state (picker → node id → PipeWire fd)

struct PortalState {
    fd: std::os::fd::OwnedFd,
    node_id: u32,
    kind: ScreenTargetKind,
}

static PORTAL: Mutex<Option<PortalState>> = Mutex::new(None);

/// Open the OS picker and remember the chosen stream (F-SC-02 Wayland path).
/// `cursor` maps to the portal cursor mode (F-SC-04; initial ON).
pub async fn portal_picker(cursor: bool) -> Result<ScreenTarget> {
    use ashpd::desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType};

    let cursor_mode = if cursor { CursorMode::Embedded } else { CursorMode::Hidden };
    let screencast = Screencast::new().await.map_err(err)?;
    let session = screencast.create_session().await.map_err(err)?;
    let request = screencast
        .select_sources(
            &session,
            cursor_mode,
            SourceType::Monitor | SourceType::Window,
            false,
            None,
            PersistMode::DoNot,
        )
        .await
        .map_err(err)?;
    // ashpd 0.7: select_sources' response is empty; the picker result (streams)
    // is delivered by the Start request (ashpd screencast module docs).
    request.response().map_err(err)?;
    let started = screencast
        .start(&session, &ashpd::WindowIdentifier::default())
        .await
        .map_err(err)?;
    let streams = started.response().map_err(err)?.streams().to_vec();
    let Some(stream) = streams.first() else {
        return Err(CaptureError::Failed(
            "portal picker returned no stream".into(),
        ));
    };
    let node_id = stream.pipe_wire_node_id();
    let kind = match stream.source_type() {
        Some(ashpd::desktop::screencast::SourceType::Monitor) => ScreenTargetKind::Display,
        _ => ScreenTargetKind::Window,
    };
    let fd = screencast
        .open_pipe_wire_remote(&session)
        .await
        .map_err(err)?;
    // dup: the PipeWire remote fd must outlive the portal proxies
    let owned = unsafe { std::os::fd::OwnedFd::from_raw_fd(libc::dup(fd)) };

    *PORTAL.lock().unwrap() = Some(PortalState {
        fd: owned,
        node_id,
        kind,
    });

    Ok(ScreenTarget {
        kind,
        id: format!("portal:{node_id}"),
    })
}

// ---------------------------------------------------------------------------
// screen capture

pub struct ScreenCapture {
    pub sink: VideoSink,
    stop: Arc<AtomicBool>,
    /// Wake channel for the worker's park loop. The worker owns the PipeWire
    /// loop and stops it itself; stop() only signals + joins, never touching
    /// PipeWire from the outside (no raw loop pointer, no cross-thread stop).
    wake: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ScreenCapture {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        {
            let (lk, cv) = &*self.wake;
            *lk.lock().unwrap() = true;
            cv.notify_one();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.sink.stop();
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_screen(
    app: tauri::AppHandle,
    // Accepted for call-site parity with the WGC backend; the Portal session
    // picked via portal_picker() is authoritative (preview → stream reuse).
    _screen: &ScreenTarget,
    profile: &Profile,
    sink: VideoSink,
    // Portal embeds the cursor at pick time (CursorMode::Hidden); reserved.
    _cursor: bool,
) -> Result<ScreenCapture> {
    // the portal connection is kept in PORTAL so preview → stream can reuse it
    // without re-picking; the capture thread works on its own dup of the fd
    let (fd, node_id) = {
        let portal = PORTAL.lock().unwrap();
        let portal = portal.as_ref().ok_or_else(|| {
            CaptureError::Failed("start_portal_picker() を先に実行してください".into())
        })?;
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(libc::dup(portal.fd.as_raw_fd())) };
        (fd, portal.node_id)
    };
    let stop = Arc::new(AtomicBool::new(false));
    let wake: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let wake2 = wake.clone();
    let stop2 = stop.clone();
    let sink2 = sink.clone();
    let preview_slot: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let preview_slot2 = preview_slot.clone();
    let dst_w = profile.w;
    let dst_h = profile.h;
    // Setup runs on the spawned thread; the result is reported back so the
    // caller fails fast instead of leaking a dead capture. Without this,
    // thread-internal failures were only eprintln! noise while the command
    // returned Ok (dead preview with no error surfaced).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
    let fail_tx = ready_tx.clone();
    let fail = move |msg: String| {
        crate::logging::error(&format!("pw video: {msg}"));
        eprintln!("pw video: {msg}");
        let _ = fail_tx.send(Err(msg));
    };

    let handle = std::thread::Builder::new()
        .name("pw-video".into())
        .spawn(move || {
            let tl = match unsafe {
                pw::thread_loop::ThreadLoopBox::new(Some("ezstreamer-video"), None)
            } {
                Ok(t) => t,
                Err(e) => {
                    fail(format!("PipeWire loop: {e}"));
                    return;
                }
            };
            // Every PipeWire object call below requires the loop lock when
            // made from outside the loop thread. The guard is held for the
            // whole setup and released before the loop starts parking below.
            let _pw_guard = tl.lock();
            let context = match pw::context::ContextBox::new(tl.loop_(), None) {
                Ok(c) => c,
                Err(e) => {
                    fail(format!("PipeWire context: {e}"));
                    return;
                }
            };
            let core = match context.connect_fd(fd, None) {
                Ok(c) => c,
                Err(e) => {
                    fail(format!("PipeWire connect: {e}"));
                    return;
                }
            };
            let props = properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            };
            let stream = match pw::stream::StreamBox::new(&core, "ezstreamer-video", props) {
                Ok(s) => s,
                Err(e) => {
                    fail(format!("PipeWire stream: {e}"));
                    return;
                }
            };

            struct Ud {
                format: pw::spa::param::video::VideoInfoRaw,
                sink: VideoSink,
                stop: Arc<AtomicBool>,
                preview_frame: Arc<Mutex<Option<Vec<u8>>>>,
                dst_w: u32,
                dst_h: u32,
            }
            let ud = Ud {
                format: pw::spa::param::video::VideoInfoRaw::new(),
                sink: sink2,
                stop: stop2.clone(),
                preview_frame: preview_slot2,
                dst_w,
                dst_h,
            };

            let _listener = match stream
                .add_local_listener_with_user_data(ud)
                .state_changed(|_, _, _old, new| {
                    // Error states carry the only visible reason when a
                    // connected stream never delivers (no node, no frames).
                    if let pw::stream::StreamState::Error(e) = new {
                        crate::logging::error(&format!("pw video: stream error: {e}"));
                        eprintln!("pw video: stream error: {e}");
                    }
                })
                .param_changed(|_, ud, id, param| {
                    let Some(param) = param else { return };
                    if id != ParamType::Format.as_raw() {
                        return;
                    }
                    let _ = ud.format.parse(param);
                })
                .process(|stream, ud| {
                    if ud.stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(mut buffer) = stream.dequeue_buffer() else {
                        return;
                    };
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }
                    let data = &mut datas[0];
                    // Portal buffers can pad rows (stride > w*4) and start at
                    // a chunk offset; a packed read shifts every row and
                    // skews the image (review 2026-09-10). Read both before
                    // the mutable `data()` borrow below.
                    let (stride, offset) = {
                        // `Data::chunk()` asserts non-null; guard first so a
                        // malformed buffer cannot panic the RT callback.
                        if data.as_raw().chunk.is_null() {
                            return;
                        }
                        let chunk = data.chunk();
                        (chunk.stride(), chunk.offset() as usize)
                    };
                    let size = ud.format.size();
                    let (w, h) = (size.width.max(1), size.height.max(1));
                    if let Some(bytes) = data.data() {
                        let Some(bytes) = bytes.get(offset..) else {
                            return;
                        };
                        // Non-positive stride is not a valid BGRA layout:
                        // fall back to the packed w*4 assumption.
                        let stride = if stride > 0 {
                            stride as usize
                        } else {
                            (w as usize) * 4
                        };
                        let Some(frame) =
                            scale_bgra_strided(bytes, w, h, stride, ud.dst_w, ud.dst_h)
                        else {
                            crate::logging::log(
                                "warn",
                                "pw video: short buffer (stride/padding mismatch); frame dropped",
                            );
                            return;
                        };
                        ud.sink.push(frame.clone());
                        // park the newest frame for the 1fps preview thread
                        if let Ok(mut slot) = ud.preview_frame.lock() {
                            if slot.is_none() {
                                *slot = Some(frame);
                            }
                        }
                    }
                })
                .register()
            {
                Ok(l) => l,
                Err(e) => {
                    fail(format!("PipeWire listener: {e}"));
                    return;
                }
            };

            // negotiate BGRA; source size/framerate come back in the negotiated
            // Format (param_changed above). Build the pod with the official macros.
            let obj = pw::spa::pod::object!(
                pw::spa::utils::SpaTypes::ObjectParamFormat,
                ParamType::EnumFormat,
                pw::spa::pod::property!(
                    pw::spa::param::format::FormatProperties::MediaType,
                    Id,
                    pw::spa::param::format::MediaType::Video
                ),
                pw::spa::pod::property!(
                    pw::spa::param::format::FormatProperties::MediaSubtype,
                    Id,
                    pw::spa::param::format::MediaSubtype::Raw
                ),
                pw::spa::pod::property!(
                    pw::spa::param::format::FormatProperties::VideoFormat,
                    Id,
                    pw::spa::param::video::VideoFormat::BGRA
                ),
            );
            let values: Vec<u8> = match pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &pw::spa::pod::Value::Object(obj),
            ) {
                Ok(v) => v.0.into_inner(),
                Err(e) => {
                    fail(format!("PipeWire format pod: {e}"));
                    return;
                }
            };
            let mut params = [Pod::from_bytes(&values).unwrap()];

            if let Err(e) = stream.connect(
                pw::spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            ) {
                fail(format!("PipeWire stream connect (node {node_id}): {e}"));
                return;
            }
            // Request dataflow explicitly instead of relying on the default
            // active state; a refusal surfaces through the fail-fast gate.
            if let Err(e) = stream.set_active(true) {
                fail(format!("PipeWire stream activate (node {node_id}): {e}"));
                return;
            }

            let _ = ready_tx.send(Ok(()));
            drop(_pw_guard);
            // A stop() that lands during setup is already recorded in the
            // stop flag / wake channel: skip parking and go straight to a
            // uniform teardown (loop never started: stop() is skipped).
            let parked = !stop2.load(Ordering::Relaxed);
            if parked {
                tl.start();
                // Park on OUR condvar, never tl.wait(): the PipeWire wait must
                // be called with the loop lock held (it is a pthread_cond_wait
                // on the loop mutex — unlocked it returns immediately, which
                // tore the stream down at once: the silent Connecting ->
                // Unconnected with no node and no data). Parking here also
                // avoids a second waiter racing stop()'s internal wait.
                let (lk, cv) = &*wake2;
                let mut stopped = lk.lock().unwrap();
                while !*stopped {
                    stopped = cv.wait(stopped).unwrap();
                }
                // Worker thread (not the loop thread): may block until the
                // loop thread exits. Must run without holding the pw guard.
                tl.stop();
            }
            // Locked teardown: destroying streams/proxies without holding the
            // loop lock trips "called from wrong context" and can leave zombie
            // streams behind (LizardByte/Sunshine#4705 pattern).
            {
                let _guard = tl.lock();
                let _ = stream.set_active(false);
                let _ = stream.disconnect();
                drop(_listener);
                drop(stream);
                drop(core);
                drop(context);
            }
            // tl drops here; the loop is stopped (or never started), so the
            // destroy is clean.
        })
        .map_err(err)?;

    // Fail fast when setup died (or hung): without this gate the command
    // returned Ok while the thread was already gone — dead preview, no error.
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        outcome => {
            let msg = match outcome {
                Ok(Err(e)) => e,
                _ => "PipeWire capture setup timed out".to_string(),
            };
            ScreenCapture {
                sink,
                stop,
                wake,
                handle: Some(handle),
            }
            .stop();
            return Err(CaptureError::Failed(msg));
        }
    }

    // F-SC-03 preview: 1fps 640x360 PNG → `stream://preview` (design §6.4).
    // Runs off the PipeWire RT thread; the process callback parks the newest
    // frame in `preview_slot` and this thread converts/emits at 1fps.
    {
        let slot = preview_slot.clone();
        let app = app.clone();
        let preview_stop = stop.clone();
        let dst_w = dst_w;
        let dst_h = dst_h;
        std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                loop {
                    if preview_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    if last.elapsed() < Duration::from_secs(1) {
                        continue;
                    }
                    let Some(frame) = slot.lock().unwrap().take() else {
                        continue;
                    };
                    last = Instant::now();
                    let small = scale_bgra(&frame, dst_w, dst_h, 640, 360);
                    let rgba = bgra_to_rgba(&small);
                    let Some(png) = crate::capture::encode_png(&rgba, 640, 360) else {
                        continue;
                    };
                    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
                    let _ = app.emit(
                        "stream://preview",
                        ezstreamer_core::ipc_types::PreviewFrame {
                            data_url: format!("data:image/png;base64,{b64}"),
                            w: 640,
                            h: 360,
                        },
                    );
                }
            })
            .map_err(err)?;
    }

    Ok(ScreenCapture {
        sink,
        stop,
        wake,
        handle: Some(handle),
    })
}
