//! Linux audio capture: PipeWire capture streams.
//!
//! - system = default sink monitor,
//!   per-app = capture stream targeted at the app's node, mic = source node.

use super::{CaptureError, Result};
use ezstreamer_core::audio::AudioSink;
use ezstreamer_core::ipc_types::AudioSelection;
use pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::Emitter;

fn err<E: std::fmt::Display>(e: E) -> CaptureError {
    CaptureError::Failed(e.to_string())
}

// ---------------------------------------------------------------------------
// audio capture

pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
    pub sink: Option<AudioSink>,
}

impl AudioCapture {
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
        if let Some(s) = self.sink.as_mut() {
            s.stop();
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Per-stream pw properties: (source id, optional TARGET_OBJECT node, capture sink monitor)
fn audio_source_specs(selection: &AudioSelection) -> Vec<(String, Option<String>, bool)> {
    let mut specs = Vec::new();
    if selection.mode == "system" {
        specs.push(("system".into(), None, true));
    }
    if selection.mic.enabled {
        let dev = selection.mic.device.clone();
        // Device ids are `pw:<node>` (Portal-style) or raw node ids; the
        // PipeWire property wants the bare numeric node id.
        let target = if dev == "default" || dev.is_empty() {
            None
        } else {
            Some(dev.rsplit(':').next().unwrap_or(dev.as_str()).to_string())
        };
        specs.push((
            ezstreamer_core::audio::MIC_ID.into(),
            target,
            false,
        ));
    }
    if selection.mode == "apps" {
        for app in &selection.apps {
            let node = app.rsplit(':').next().unwrap_or("").to_string();
            if node.is_empty() {
                continue;
            }
            specs.push((app.clone(), Some(node), false));
        }
    }
    specs
}

pub fn start_audio(
    selection: &AudioSelection,
    sink: AudioSink,
    app: Option<tauri::AppHandle>,
) -> Result<AudioCapture> {
    let specs = audio_source_specs(selection);
    if specs.is_empty() {
        return Err(CaptureError::Failed(
            "no audio source selected".into(),
        ));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let wake: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let wake2 = wake.clone();
    let stop2 = stop.clone();
    let sink2 = sink.clone();
    // Same fail-fast setup reporting as the video thread: callers must not
    // get Ok for a dead capture.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
    let fail_tx = ready_tx.clone();
    let fail = move |msg: String| {
        crate::logging::error(&format!("pw audio: {msg}"));
        eprintln!("pw audio: {msg}");
        if let Some(app) = &app {
            let _ = app.emit(
                "stream://error",
                ezstreamer_core::ipc_types::StreamError {
                    code: "audio".into(),
                    msg: msg.clone(),
                },
            );
        }
        let _ = fail_tx.send(Err(msg));
    };

    let handle = std::thread::Builder::new()
        .name("pw-audio".into())
        .spawn(move || {
            let tl = match unsafe {
                pw::thread_loop::ThreadLoopBox::new(Some("ezstreamer-audio"), None)
            } {
                Ok(t) => t,
                Err(e) => {
                    fail(format!("PipeWire loop: {e}"));
                    return;
                }
            };
            // All PipeWire object calls under the loop lock (see video thread).
            let _pw_guard = tl.lock();
            let context = match pw::context::ContextBox::new(tl.loop_(), None) {
                Ok(c) => c,
                Err(e) => {
                    fail(format!("PipeWire context: {e}"));
                    return;
                }
            };
            let core = match context.connect(None) {
                Ok(c) => c,
                Err(e) => {
                    fail(format!("PipeWire connect: {e}"));
                    return;
                }
            };

            let mut streams = Vec::new();
            // Listeners unregister themselves on drop: keep them alive as long
            // as the streams, otherwise no process callback ever fires (dead
            // silence with no error).
            let mut listeners = Vec::new();
            for (id, target, capture_sink) in specs {
                let props = properties! {
                    *pw::keys::MEDIA_TYPE => "Audio",
                    *pw::keys::MEDIA_CATEGORY => "Capture",
                    *pw::keys::MEDIA_ROLE => "Music",
                };
                let props = {
                    let mut p = props;
                    if capture_sink {
                        p.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
                    }
                    if let Some(t) = &target {
                        p.insert(*pw::keys::TARGET_OBJECT, t.as_str());
                    }
                    p
                };
                let stream =
                    match pw::stream::StreamBox::new(&core, &format!("ezstreamer-{id}"), props) {
                        Ok(s) => s,
                        Err(e) => {
                            fail(format!("PipeWire stream {id}: {e}"));
                            return;
                        }
                    };

                struct Ud {
                    sink: AudioSink,
                    id: String,
                    stop: Arc<AtomicBool>,
                }
                let ud = Ud {
                    sink: sink2.clone(),
                    id: id.clone(),
                    stop: stop2.clone(),
                };

                let ud_id = id.clone();
                match stream
                    .add_local_listener_with_user_data(ud)
                    .state_changed(move |_, _, _old, new| {
                        // Error states carry the only visible reason when a
                        // connected stream never delivers (no node, no samples).
                        if let pw::stream::StreamState::Error(e) = new {
                            crate::logging::error(&format!("pw audio: stream error {ud_id}: {e}"));
                            eprintln!("pw audio: stream error {ud_id}: {e}");
                        }
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
                        if let Some(bytes) = data.data() {
                            let n = bytes.len() / 4;
                            let mut samples = Vec::with_capacity(n);
                            for i in 0..n {
                                let b: [u8; 4] =
                                    bytes[i * 4..i * 4 + 4].try_into().unwrap_or([0; 4]);
                                samples.push(f32::from_le_bytes(b));
                            }
                            ud.sink.push(&ud.id, samples);
                        }
                    })
                    .register()
                {
                    Ok(l) => listeners.push(l),
                    Err(e) => {
                        fail(format!("PipeWire listener {id}: {e}"));
                        return;
                    }
                }

                let mut info = pw::spa::param::audio::AudioInfoRaw::new();
                info.set_format(pw::spa::param::audio::AudioFormat::F32LE);
                info.set_rate(48_000);
                info.set_channels(2);
                let obj = pw::spa::pod::Object {
                    type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                    id: ParamType::EnumFormat.as_raw(),
                    properties: info.into(),
                };
                let values: Vec<u8> = match pw::spa::pod::serialize::PodSerializer::serialize(
                    std::io::Cursor::new(Vec::new()),
                    &pw::spa::pod::Value::Object(obj),
                ) {
                    Ok(v) => v.0.into_inner(),
                    Err(e) => {
                        fail(format!("PipeWire format pod {id}: {e}"));
                        return;
                    }
                };
                let mut params = [Pod::from_bytes(&values).unwrap()];

                if let Err(e) = stream.connect(
                    pw::spa::utils::Direction::Input,
                    None,
                    pw::stream::StreamFlags::AUTOCONNECT
                        | pw::stream::StreamFlags::MAP_BUFFERS
                        | pw::stream::StreamFlags::RT_PROCESS,
                    &mut params,
                ) {
                    fail(format!("PipeWire stream connect {id}: {e}"));
                    return;
                }
                // Request dataflow explicitly (see the video thread).
                if let Err(e) = stream.set_active(true) {
                    fail(format!("PipeWire stream activate {id}: {e}"));
                    return;
                }
                streams.push(stream);
            }

            let _ = ready_tx.send(Ok(()));
            drop(_pw_guard);
            // Same park/stop discipline as the video thread: never tl.wait()
            // (must hold the loop lock; unlocked it misbehaves), park on our
            // own condvar instead.
            let parked = !stop2.load(Ordering::Relaxed);
            if parked {
                tl.start();
                let (lk, cv) = &*wake2;
                let mut stopped = lk.lock().unwrap();
                while !*stopped {
                    stopped = cv.wait(stopped).unwrap();
                }
                tl.stop();
            }
            // Locked teardown (see the video thread).
            {
                let _guard = tl.lock();
                for s in &streams {
                    let _ = s.set_active(false);
                    let _ = s.disconnect();
                }
                drop(listeners);
                drop(streams);
                drop(core);
                drop(context);
            }
            // tl drops here; stopped (or never started), so destroy is clean.
        })
        .map_err(err)?;

    // Fail fast when setup died (or hung); see start_screen.
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        outcome => {
            let msg = match outcome {
                Ok(Err(e)) => e,
                _ => "PipeWire audio setup timed out".to_string(),
            };
            AudioCapture {
                stop,
                wake,
                handle: Some(handle),
                sink: Some(sink),
            }
            .stop();
            return Err(CaptureError::Failed(msg));
        }
    }

    Ok(AudioCapture {
        stop,
        wake,
        handle: Some(handle),
        sink: Some(sink),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ezstreamer_core::audio::{AudioSink, Mixer, SourceState};
    use ezstreamer_core::config::MicSource;
    use ezstreamer_core::ipc_types::AudioSelection;

    fn pw_dump() -> Option<String> {
        std::process::Command::new("pw-dump")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
    }

    fn ezstreamer_nodes() -> Vec<String> {
        pw_dump()
            .map(|out| {
                out.lines()
                    .filter(|l| l.contains("ezstreamer-"))
                    .take(5)
                    .map(|l| l.trim().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Rapid start/stop cycles against the real daemon (when present):
    /// - setup failures surface as Err (fail-fast, never a silent dead capture)
    /// - teardown leaves no zombie ezstreamer-* nodes behind
    /// - process listeners stay registered: mixed PCM reaches the file
    /// Without a daemon (CI) only the fail-fast Err path is asserted.
    #[test]
    fn audio_start_stop_cycle() {
        let daemon = pw_dump().is_some();
        let sink_present = pw_dump()
            .map(|out| out.contains("\"media.class\": \"Audio/Sink\""))
            .unwrap_or(false);
        // Snapshot ambient ezstreamer-* nodes (another instance may be running);
        // only nodes created by this test may remain afterwards.
        let before = ezstreamer_nodes();
        let mut wrote_bytes = 0u64;
        for i in 0..3 {
            let path = std::env::temp_dir().join(format!("ezstreamer-test-audio-{i}.pcm"));
            let _ = std::fs::remove_file(&path);
            let file = std::fs::File::create(&path).unwrap();
            let mixer = Arc::new(Mutex::new(Mixer {
                apps: Default::default(),
                mic: SourceState {
                    gain: 1.0,
                    muted: false,
                    enabled: false,
                },
            }));
            let asink = AudioSink::spawn(file, mixer).unwrap();
            let sel = AudioSelection {
                mode: "system".into(),
                apps: Vec::new(),
                mic: MicSource {
                    device: "default".into(),
                    enabled: false,
                    muted: false,
                    gain: 1.0,
                },
            };
            let mut cap = match start_audio(&sel, asink, None) {
                Ok(c) => c,
                Err(_) => {
                    assert!(!daemon, "start_audio failed despite a live daemon");
                    return;
                }
            };
            std::thread::sleep(Duration::from_millis(800));
            cap.stop();
            wrote_bytes = wrote_bytes.max(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
            let _ = std::fs::remove_file(&path);
        }
        if daemon {
            let leaked: Vec<_> = ezstreamer_nodes()
                .into_iter()
                .filter(|n| !before.contains(n))
                .collect();
            assert!(leaked.is_empty(), "leaked PipeWire nodes: {leaked:?}");
            // Callbacks only fire when a monitor exists; without sinks there is
            // nothing to capture, so data flow is asserted only then.
            if sink_present {
                assert!(wrote_bytes > 0, "no PCM flowed: process listeners dead?");
            }
        }
    }
}
