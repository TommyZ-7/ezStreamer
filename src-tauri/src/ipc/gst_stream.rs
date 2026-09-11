//! Running GStreamer pipeline (Windows; design.md §4).
//!
//! Topology (single `gst::Pipeline`, built with element APIs — the
//! `gst-launch` string in core is the debuggable reference):
//!
//! ```text
//! video_src(appsrc BGRA) → videoconvert → videoscale → capsfilter
//!   → queue → videoconvert → [vulkanupload] → <encoder> → h264parse → mux.
//! audio_src(appsrc F32LE 48k stereo, Rust Mixer output) → audioconvert →
//!   audioresample → capsfilter → queue → audioconvert → <aacenc> → aacparse → mux.
//! mux(flvmux streamable) → rtmp2sink location=rtmp://…/{key}
//! ```
//!
//! The converters after each `queue` are load-bearing, not redundant:
//! the capsfilter pins BGRA / F32LE for the pacer, but HW encoders accept
//! only subsets (`vah264enc`: NV12; `faac`/`fdkaacenc`: S16LE). `pad_link`
//! checks live caps — not just templates — so without the tail converter
//! the queue→encoder link itself fails (e.g. `vqueue`→`vah264enc`).
//! When formats already match the converter is a passthrough.
//!
//! Vulkan (`vulkanh264enc`) only: `vulkanupload` sits between the tail
//! videoconvert and the encoder, because the encoder sink is
//! `video/x-raw(memory:VulkanImage),format=NV12` (verified with GStreamer
//! 1.28 `gst-inspect` + `gst-launch`).
//!
//! Capture stays in Rust (WGC + WASAPI → `VideoSink`/`AudioSink` pumps);
//! feeder threads move paced frames into the two `appsrc` elements.

use ezstreamer_core::gst::pipeline::EncoderSpec;
use ezstreamer_core::gst::StreamPlan;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

pub struct ExitStatus {
    success: bool,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.success
    }
}

pub struct GstStream {
    pub plan: StreamPlan,
    pub retry_count: u32,
    started_at: Instant,
    video_frames: Arc<AtomicU64>,
    encoded_bytes: Arc<AtomicU64>,
    status: Arc<Mutex<ezstreamer_core::ipc_types::StreamStatus>>,
    done: Arc<Mutex<Option<bool>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl GstStream {
    pub fn status(&self) -> ezstreamer_core::ipc_types::StreamStatus {
        let elapsed = self.started_at.elapsed().as_secs().max(1);
        let frames = self.video_frames.load(Ordering::Relaxed);
        let bytes = self.encoded_bytes.load(Ordering::Relaxed);
        let expected = self.plan.fps as u64 * elapsed;
        ezstreamer_core::ipc_types::StreamStatus {
            is_live: self.handle.is_some() && self.done.lock().unwrap().is_none(),
            duration_sec: self.started_at.elapsed().as_secs(),
            bitrate_kbps: bytes as f64 * 8.0 / 1000.0 / elapsed as f64,
            dropped_frames: expected.saturating_sub(frames),
            retrying: self.status.lock().unwrap().retrying,
        }
    }

    /// Non-blocking exit check. `Some` = the pipeline thread finished
    /// (error/EOS/user stop); `None` = still running.
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        let done: Option<bool> = *self.done.lock().unwrap();
        done.map(|d| ExitStatus { success: d })
    }

    pub fn take_result(&mut self) -> Option<bool> {
        self.done.lock().unwrap().take()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn mark_retrying(&self, n: u32) {
        self.status.lock().unwrap().retrying = Some(n);
    }
}

/// Spawn the pipeline thread. Returns the stream handle once the pipeline
/// is Playing (or an error naming the missing element — e.g. GStreamer
/// runtime absent — so the UI can show an actionable message).
pub fn spawn_pipeline(
    plan: StreamPlan,
    video_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    retry: u32,
) -> Result<GstStream, String> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app::{AppLeakyType, AppSrc};

    gst::init().map_err(|e| {
        format!("GStreamer init failed: {e} (install the MSVC runtime, design §13.2)")
    })?;

    let started_at = Instant::now();
    let video_frames = Arc::new(AtomicU64::new(0));
    let encoded_bytes = Arc::new(AtomicU64::new(0));
    let done: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(
        ezstreamer_core::ipc_types::StreamStatus::default(),
    ));

    let pipeline = gst::Pipeline::new();
    let mk = |factory: &str, name: &str| -> Result<gst::Element, String> {
        gst::ElementFactory::make(factory)
            .name(name)
            .build()
            .map_err(|_| {
                format!("GStreamer element missing: {factory} (runtime/plugins incomplete)")
            })
    };

    // Elements.
    let v_src = mk("appsrc", "video_src")?;
    let v_conv = mk("videoconvert", "vconv")?;
    let v_scale = mk("videoscale", "vscale")?;
    let v_caps = mk("capsfilter", "vcaps")?;
    let v_queue = mk("queue", "vqueue")?;
    // Tail converter: adapts the pinned BGRA caps to whatever the encoder
    // accepts (see topology note above; e.g. NV12 for vah264enc).
    let v_conv2 = mk("videoconvert", "vconv2")?;
    // Vulkan encoders consume NV12 VulkanImage memory, not system memory:
    // `vulkanupload` bridges the tail converter → encoder.
    let v_upload = if plan.encoder.needs_vulkan_upload() {
        Some(mk("vulkanupload", "vupload")?)
    } else {
        None
    };
    let encoder = find_encoder(&plan.encoder)
        .ok_or_else(|| format!("no GStreamer element for {}", plan.encoder.id()))?;
    let v_parse = mk("h264parse", "vparse")?;
    let a_src = mk("appsrc", "audio_src")?;
    let a_conv = mk("audioconvert", "aconv")?;
    let a_res = mk("audioresample", "ares")?;
    let a_caps = mk("capsfilter", "acaps")?;
    let a_queue = mk("queue", "aqueue")?;
    // Tail converter: adapts the pinned F32LE caps to whatever the AAC
    // encoder accepts (faac/fdkaacenc take S16LE only).
    let a_conv2 = mk("audioconvert", "aconv2")?;
    let aacenc = find_aacenc().ok_or_else(|| {
        "no AAC encoder element (voaacenc/avenc_aac/mfaacenc/faac/fdkaacenc) found (runtime/plugins incomplete)"
            .to_string()
    })?;
    let a_parse = mk("aacparse", "aparse")?;
    let mux = mk("flvmux", "mux")?;
    let sink = mk("rtmp2sink", "sink")?;

    // Caps: appsrc and the videoscale capsfilter stay BGRA (FramePacer
    // output); the tail videoconvert adapts to the encoder (NV12 for
    // Vulkan/VAAPI) and `vulkanupload` bridges to VulkanImage memory.
    let vcaps = gst::Caps::builder("video/x-raw")
        .field("format", "BGRA")
        .field("width", plan.w as i32)
        .field("height", plan.h as i32)
        .field("framerate", gst::Fraction::new(plan.fps as i32, 1))
        .build();
    v_caps.set_property("caps", &vcaps);
    let acaps = audio_src_caps();
    a_caps.set_property("caps", &acaps);

    // appsrc streaming attributes (live, timestamped). Timestamps come from
    // the pipeline clock (`do-timestamp`), not from a fixed frame counter:
    // the pacer drops missed ticks after a stall, so per-frame PTS increments
    // fell permanently behind wall clock and desynced A/V.
    //
    // Queues are bounded and leaky (drop old): when the network backpressures,
    // blocks are dropped instead of growing appsrc's internal queue without
    // limit (block=false would otherwise keep accepting pushes forever).
    let v_appsrc = v_src
        .clone()
        .dynamic_cast::<AppSrc>()
        .map_err(|_| "video_src is not appsrc".to_string())?;
    let a_appsrc = a_src
        .clone()
        .dynamic_cast::<AppSrc>()
        .map_err(|_| "audio_src is not appsrc".to_string())?;
    for appsrc in [&v_appsrc, &a_appsrc] {
        appsrc.set_format(gst::Format::Time);
        appsrc.set_is_live(true);
        appsrc.set_do_timestamp(true);
        appsrc.set_leaky_type(AppLeakyType::Downstream);
    }
    v_appsrc.set_max_bytes(0); // 0 = unlimited; max-buffers is the limit
    v_appsrc.set_max_buffers(4); // ≈4 paced frames (~66ms at 60fps)
    a_appsrc.set_max_bytes(0);
    a_appsrc.set_max_buffers(50); // ≈500ms of 10ms mixer blocks
    v_src.set_property("caps", &vcaps);
    a_src.set_property("caps", &acaps);

    // Encoder tuning (Topaz-safe low latency, design §4.3).
    let profile = ezstreamer_core::config::Profile {
        name: String::new(),
        w: plan.w,
        h: plan.h,
        fps: plan.fps,
        v_kbps: plan.v_kbps,
        a_kbps: plan.a_kbps,
        encoder: "auto".into(),
        warn: None,
    };
    let encoder_name = encoder
        .factory()
        .map(|f| f.name().to_string())
        .unwrap_or_else(|| plan.encoder_element.clone());
    apply_string_props(
        &encoder,
        &plan.encoder.gst_props_for(&encoder_name, &profile),
    );
    crate::logging::info(&format!(
        "stream start: encoder={encoder_name} {}x{}@{}fps v={}kbps a={}kbps url={}",
        plan.w, plan.h, plan.fps, plan.v_kbps, plan.a_kbps, plan.rtmp_url
    ));
    // avenc_aac/faac/fdkaacenc expect gint, voaacenc/mfaacenc expect guint;
    // the string form deserializes to either. A typed
    // `set_property(bitrate, &(u32))` panics on gint elements (avenc_aac)
    // and aborts the Tauri main thread (cannot unwind).
    apply_string_props(
        &aacenc,
        &[("bitrate".into(), (plan.a_kbps * 1000).to_string())],
    );
    mux.set_property("streamable", &true);
    configure_rtmp_sink(&sink, &plan.rtmp_url);

    // Video leg: tail videoconvert always present; `vulkanupload` only for
    // Vulkan (bridges system memory → VulkanImage). The conditional element
    // cannot join the static `add_many` list, so it is added separately.
    let mut video_elems = vec![&v_src, &v_conv, &v_scale, &v_caps, &v_queue, &v_conv2];
    if let Some(ref up) = v_upload {
        video_elems.push(up);
    }
    video_elems.extend([&encoder, &v_parse, &mux]);
    pipeline
        .add_many(&[
            &v_src, &v_conv, &v_scale, &v_caps, &v_queue, &v_conv2, &encoder, &v_parse, &a_src,
            &a_conv, &a_res, &a_caps, &a_queue, &a_conv2, &aacenc, &a_parse, &mux, &sink,
        ])
        .map_err(|e| format!("pipeline add failed: {e}"))?;
    if let Some(ref up) = v_upload {
        pipeline
            .add(up)
            .map_err(|e| format!("pipeline add failed: {e}"))?;
    }
    gst::Element::link_many(&video_elems).map_err(|e| format!("video link failed: {e:?}"))?;
    gst::Element::link_many(&[
        &a_src, &a_conv, &a_res, &a_caps, &a_queue, &a_conv2, &aacenc, &a_parse, &mux,
    ])
    .map_err(|e| format!("audio link failed: {e:?}"))?;
    mux.link(&sink)
        .map_err(|e| format!("mux link failed: {e:?}"))?;

    // F-ST-03: measure the *encoded* bitrate at the mux output. Counting
    // pushed BGRA/F32 bytes overstated the wire rate by roughly 10x.
    let mux_pad = mux.static_pad("src").ok_or("mux has no src pad")?;
    let eb = encoded_bytes.clone();
    mux_pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        if let Some(buf) = info.buffer() {
            eb.fetch_add(buf.size() as u64, Ordering::Relaxed);
        }
        gst::PadProbeReturn::Ok
    });

    // Feeders: paced pump channels → appsrc buffers. They wait for PLAYING:
    // `do-timestamp` stamps buffers with the pipeline running time only once
    // the clock is distributed, so pre-PLAYING pushes would reach the muxer
    // without a timestamp. Both streams then share one timebase.
    let vf = video_frames.clone();
    let stop_v = stop.clone();
    let pipeline_v = pipeline.clone();
    std::thread::Builder::new()
        .name("gst-video-feed".into())
        .spawn(move || {
            wait_for_playing(&pipeline_v, &stop_v);
            while !stop_v.load(Ordering::Relaxed) {
                match video_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(frame) => {
                        let buf = gst::Buffer::from_slice(frame);
                        if v_appsrc.push_buffer(buf).is_ok() {
                            vf.fetch_add(1, Ordering::Relaxed);
                        } else {
                            crate::logging::error("video feeder: appsrc push failed (pipeline flushing)");
                            break; // pipeline flushing
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let _ = v_appsrc.end_of_stream();
        })
        .map_err(|e| {
            // Release feeders spawned before this one; otherwise a spawn
            // failure would leave them waiting for PLAYING forever.
            stop.store(true, Ordering::Relaxed);
            format!("video feeder spawn: {e}")
        })?;

    let stop_a = stop.clone();
    let pipeline_a = pipeline.clone();
    std::thread::Builder::new()
        .name("gst-audio-feed".into())
        .spawn(move || {
            wait_for_playing(&pipeline_a, &stop_a);
            while !stop_a.load(Ordering::Relaxed) {
                match audio_rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(block) => {
                        let bytes: Vec<u8> =
                            block.iter().flat_map(|s| s.to_le_bytes()).collect();
                        if a_appsrc.push_buffer(gst::Buffer::from_slice(bytes)).is_ok() {
                        } else {
                            crate::logging::error("audio feeder: appsrc push failed (pipeline flushing)");
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let _ = a_appsrc.end_of_stream();
        })
        .map_err(|e| {
            // Releases the already-spawned video feeder (same reasoning).
            stop.store(true, Ordering::Relaxed);
            format!("audio feeder spawn: {e}")
        })?;

    // Bus supervisor: ERROR/EOS ends the run (F-ST-04 retry in commands);
    // user stop sends EOS and drains, with a hard deadline so a dead RTMP
    // connection cannot block `stop()` (and the Tauri command) forever.
    let bus = pipeline.bus().ok_or("pipeline has no bus")?;
    let done_t = done.clone();
    let stop_t = stop.clone();
    let handle = std::thread::Builder::new()
        .name("gst-bus".into())
        .spawn(move || {
            let ok: bool;
            let mut eos_deadline: Option<Instant> = None;
            let _ = pipeline.set_state(gst::State::Playing);
            loop {
                if stop_t.load(Ordering::Relaxed) {
                    match eos_deadline {
                        None => {
                            let _ = pipeline.send_event(gst::event::Eos::new());
                            eos_deadline = Some(Instant::now() + std::time::Duration::from_secs(3));
                        }
                        Some(deadline) if Instant::now() >= deadline => {
                            let msg = "gst stop: EOS not observed within 3s, forcing shutdown";
                            crate::logging::error(msg);
                            eprintln!("{msg}");
                            ok = true; // user-requested stop: not a stream failure
                            break;
                        }
                        Some(_) => {}
                    }
                }
                match bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
                    None => continue,
                    Some(msg) => match msg.view() {
                        gst::MessageView::Eos(..) => {
                            ok = stop_t.load(Ordering::Relaxed);
                            break;
                        }
                        gst::MessageView::Error(err) => {
                            let msg = format!(
                                "gst bus ERROR from {:?}: {} ({:?})",
                                err.src().map(|s| s.path_string()),
                                err.error(),
                                err.debug()
                            );
                            crate::logging::error(&msg);
                            eprintln!("{msg}");
                            ok = false;
                            break;
                        }
                        _ => {}
                    },
                }
            }
            let _ = pipeline.set_state(gst::State::Null);
            // Releases feeder threads still waiting for PLAYING after an
            // abnormal exit (the pipeline will never reach it).
            stop_t.store(true, Ordering::Relaxed);
            *done_t.lock().unwrap() = Some(ok);
        })
        .map_err(|e| {
            // Releases both feeders: without the bus thread the pipeline
            // never reaches PLAYING, so `wait_for_playing` would spin forever.
            stop.store(true, Ordering::Relaxed);
            format!("bus thread spawn: {e}")
        })?;

    // Fail fast: if the pipeline errors during preroll, report quickly
    // instead of hanging start_stream.
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Some(false) = *done.lock().unwrap() {
        stop.store(true, Ordering::Relaxed);
        crate::logging::error("pipeline failed during preroll");
        return Err("GStreamer pipeline failed during preroll (see log)".into());
    }

    Ok(GstStream {
        plan,
        retry_count: retry,
        started_at,
        video_frames,
        encoded_bytes,
        status,
        done,
        stop,
        handle: Some(handle),
    })
}

/// Apply `(property, value)` pairs after deserializing each value for the
/// property's `ParamSpec`. `Element::set_property_from_str` panics when a
/// value cannot be deserialized (e.g. an enum nick the runtime does not
/// know), and a panic in this sync `start_stream` path aborts the process
/// (review 2026-09-10). Unknown values are logged and skipped instead, so a
/// plan tuned for a newer runtime degrades gracefully on an older one.
fn apply_string_props(element: &gstreamer::Element, props: &[(String, String)]) {
    use gstreamer::prelude::*;

    let element_name = element
        .factory()
        .map(|f| f.name().to_string())
        .unwrap_or_else(|| element.name().to_string());
    for (key, value) in props {
        let Some(pspec) = element.find_property(key.as_str()) else {
            continue; // property absent on this element/runtime
        };
        match gstreamer::glib::Value::deserialize_with_pspec(value, &pspec) {
            Ok(parsed) => element.set_property(key.as_str(), parsed),
            Err(_) => crate::logging::log(
                "warn",
                &format!(
                    "gst: {element_name}: skipping {key}={value} (not valid for this runtime)"
                ),
            ),
        }
    }
}

/// Configure the RTMP sink for a live `appsrc` pipeline.
///
/// `async` MUST be false. `rtmp2sink` inherits `async=true` from
/// `GstBaseSink`, so its READY→PAUSED waits for a preroll buffer. The
/// appsrc feeders only push once the pipeline is PLAYING (`do-timestamp`
/// needs the distributed clock), so with the default the pipeline deadlocks
/// in `PAUSED (pending PLAYING)` forever: flvmux never outputs (0 kbps) and
/// every expected frame counts as dropped, with no bus ERROR/EOS to log.
/// Network sinks have no use for preroll; `async=false` commits PLAYING
/// immediately so the feeders start (preview.06–08 regression).
fn configure_rtmp_sink(sink: &gstreamer::Element, location: &str) {
    sink.set_property("location", location);
    sink.set_property("async", false);
}

/// Block the calling feeder until the pipeline is PLAYING (or stopping).
/// Live sources only produce data in PLAYING, when the clock is distributed.
///
/// Watchdog: a pipeline that never reaches PLAYING (e.g. an async sink
/// waiting for preroll with no feeder pushing yet) stalls both feeders
/// silently — no bus ERROR, UI shows 0 kbps / all frames dropped. Warn with
/// the current/pending state so the next occurrence is diagnosable.
fn wait_for_playing(pipeline: &gstreamer::Pipeline, stop: &AtomicBool) {
    use gstreamer::prelude::*;
    let start = Instant::now();
    let mut next_warn = start + std::time::Duration::from_secs(2);
    while !stop.load(Ordering::Relaxed) && pipeline.current_state() < gstreamer::State::Playing {
        if Instant::now() >= next_warn {
            crate::logging::log(
                "warn",
                &format!(
                    "gst: feeder waiting for PLAYING for {:?} (state={:?}, pending={:?})",
                    start.elapsed(),
                    pipeline.current_state(),
                    pipeline.pending_state(),
                ),
            );
            next_warn = Instant::now() + std::time::Duration::from_secs(10);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// First available encoder element for the spec, or None.
fn find_encoder(spec: &EncoderSpec) -> Option<gstreamer::Element> {
    for name in spec.gst_elements() {
        if gstreamer::ElementFactory::find(name).is_some() {
            if let Ok(e) = gstreamer::ElementFactory::make(name).build() {
                return Some(e);
            }
        }
    }
    None
}

fn find_aacenc() -> Option<gstreamer::Element> {
    // MSVC runtime contents vary by version (voaacenc vs libav vs faac/fdk vs
    // MediaFoundation); try all known AAC encoder factories, first wins.
    // 1.28.6 MSVC confirmed: voaacenc, avenc_aac, mfaacenc all present.
    for name in ["voaacenc", "avenc_aac", "mfaacenc", "faac", "fdkaacenc"] {
        if gstreamer::ElementFactory::find(name).is_some() {
            if let Ok(e) = gstreamer::ElementFactory::make(name).build() {
                return Some(e);
            }
        }
    }
    None
}

/// Caps offered by `audio_src` and pinned by the audio `capsfilter`
/// (48kHz interleaved stereo F32LE, matching the Rust Mixer output).
/// `layout` is load-bearing: audioconvert/audioresample reject layout-less
/// caps at set_caps (`gst_audio_info_from_caps: no layout given`), which
/// surfaces as `audio_src ... not-negotiated` once data flows.
fn audio_src_caps() -> gstreamer::Caps {
    gstreamer::Caps::builder("audio/x-raw")
        .field("format", "F32LE")
        .field("layout", "interleaved")
        .field("rate", 48_000i32)
        .field("channels", 2i32)
        .build()
}

/// True when the registry provides the element factory (for `probe_encoders`).
pub fn has_element(name: &str) -> bool {
    gstreamer::ElementFactory::find(name).is_some()
}

/// Point GStreamer at the runtime bundled beside the app (Release NSIS).
/// No-op in dev (uses the system MSVC runtime) and when the bundle layout
/// is absent. Must run before `gst::init()` in the same process.
///
/// NOTE: the process-startup loader is handled earlier by
/// `crate::gst_preload::preload_bundled_gstreamer_dlls()` (DELAYLOAD +
/// AddDllDirectory in `main()`); this only sets `PATH`/`GST_PLUGIN_PATH`
/// for the registry/scanner. Both flat `<exe>/gstreamer/...` and preview.01
/// `<exe>/resources/gstreamer/...` layouts are probed (core §supervisor).
pub fn ensure_bundled_runtime(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Ok(res) = app.path().resource_dir() else {
        return;
    };
    let Some(bin) = ezstreamer_core::gst::bundled_bin_dir(&res) else {
        return;
    };
    let mut paths = vec![bin];
    if let Some(p) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&p));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        std::env::set_var("PATH", joined);
    }
    if let Some(plugins) = ezstreamer_core::gst::bundled_plugin_dir(&res) {
        std::env::set_var("GST_PLUGIN_PATH", &plugins);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_caps_carry_interleaved_layout() {
        // Regression: without `layout`, audioconvert/audioresample refuse
        // set_caps and the stream dies with audio_src not-negotiated.
        gstreamer::init().unwrap();
        let s = audio_src_caps().to_string();
        assert!(s.contains("format=(string)F32LE"), "caps: {s}");
        assert!(s.contains("layout=(string)interleaved"), "caps: {s}");
        assert!(s.contains("rate=(int)48000"), "caps: {s}");
        assert!(s.contains("channels=(int)2"), "caps: {s}");
    }

    #[test]
    fn apply_string_props_skips_invalid_enum_instead_of_panicking() {
        // Regression (review 2026-09-10): `set_property_from_str` panics on a
        // value it cannot deserialize (e.g. NVENC `preset=high-performance`),
        // and a panic from the sync `start_stream` command aborts the whole
        // process. Invalid values must be skipped; valid ones still apply.
        use gstreamer::prelude::*;

        gstreamer::init().unwrap();
        let elem = gstreamer::ElementFactory::make("appsrc").build().unwrap();
        apply_string_props(&elem, &[("leaky-type".into(), "not-a-real-leak".into())]);
        apply_string_props(&elem, &[("leaky-type".into(), "downstream".into())]);
        let leaky: gstreamer_app::AppLeakyType = elem.property("leaky-type");
        assert_eq!(leaky, gstreamer_app::AppLeakyType::Downstream);
    }

    #[test]
    fn rtmp_sink_is_configured_non_async() {
        // Regression (preview.06–08): `rtmp2sink` defaults to async=true
        // (GstBaseSink) and waits for a preroll buffer, while the feeders
        // only push after PLAYING → the pipeline deadlocked in PAUSED
        // (0 kbps / all frames dropped, no bus ERROR). async=false breaks it.
        use gstreamer::prelude::*;

        gstreamer::init().unwrap();
        // gst-plugins-bad is absent on some dev/CI hosts (Linux CI installs
        // base plugins only); the Windows CI runtime always has it.
        let Ok(sink) = gstreamer::ElementFactory::make("rtmp2sink").build() else {
            eprintln!("skipping: rtmp2sink plugin unavailable (gst-plugins-bad)");
            return;
        };
        configure_rtmp_sink(&sink, "rtmp://127.0.0.1:1/live/k");
        let is_async: bool = sink.property("async");
        assert!(!is_async, "rtmp2sink must not wait for preroll");
        // rtmp2sink re-serializes the URI; only the path is asserted.
        let location: String = sink.property("location");
        assert!(location.contains("/live/k"), "location: {location}");
    }
}
