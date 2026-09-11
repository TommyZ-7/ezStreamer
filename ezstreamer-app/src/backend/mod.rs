//! Backend worker: owns capture backends, the GStreamer pipeline and the
//! persisted config. The egui UI talks to it through a command channel and
//! receives [`UiEvent`]s; there is no Tauri IPC anymore (design v0.3 §5).
//!
//! Long operations (pipeline start/preroll, stop with EOS drain, Portal
//! picker, device enumeration, encoder probe) run on this worker or on
//! short-lived helper threads, never on the UI thread.

#[cfg(all(feature = "media", any(windows, target_os = "linux")))]
#[path = "gst_stream.rs"]
mod gst_stream;
#[cfg(not(all(feature = "media", any(windows, target_os = "linux"))))]
#[path = "gst_stream_stub.rs"]
mod gst_stream;

use crate::capture::platform::{AudioCapture, ScreenCapture, ScreenHandle};
use crate::capture::{self, platform as cap};
use crate::events::{UiEvent, UiSink};
use crate::logging;
use ezstreamer_core::audio::Mixer;
use ezstreamer_core::config::{
    self, validate_bitrate, Profile, ProfilesConfig, MAX_AUDIO_KBPS, MAX_VIDEO_KBPS,
};
use ezstreamer_core::gst::{self, retry_backoff_ms, MAX_RETRIES};
use ezstreamer_core::ipc_types::{AudioMixUpdate, SourceGain, StreamConfig, StreamStatus, VuMeter};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Coarse UI state for the operations that block the worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Starting,
    Stopping,
    Picking,
}

/// Snapshot the UI polls every frame (cheap locks, no worker round-trip).
#[derive(Default)]
pub struct Shared {
    pub status: StreamStatus,
    pub vu: VuMeter,
    pub previewing: bool,
    pub busy: Option<Busy>,
}

pub enum Command {
    /// Config + device lists + encoder probe (startup).
    LoadAll,
    /// Re-enumerate displays/windows/audio devices.
    RefreshSources,
    ProbeEncoders,
    SaveConfig(Box<ProfilesConfig>),
    StartPreview(Box<StreamConfig>),
    StopPreview,
    StartStream(Box<StreamConfig>),
    StopStream,
    PortalPicker {
        cursor: bool,
    },
    UpdateMix(Box<AudioMixUpdate>),
    OpenDir(PathBuf),
    Shutdown,
}

/// F-ST-04: everything needed to respawn the pipeline after an abnormal exit
/// (a retry rebuilds capture + pipeline because `appsrc` channels are
/// single-consumer).
#[derive(Clone)]
struct StreamSession {
    cfg: StreamConfig,
    plan: gst::StreamPlan,
    profile: Profile,
    mixer: Arc<Mutex<Mixer>>,
}

struct SessionState {
    profiles: Mutex<ProfilesConfig>,
    stream: Mutex<Option<gst_stream::GstStream>>,
    session: Mutex<Option<StreamSession>>,
    /// F-ST-04: retry attempt in backoff (Some(n) = "reconnecting n/3")
    retrying: Mutex<Option<u32>>,
    /// live mixer of the running stream (update_audio_mix targets this)
    active_mixer: Mutex<Option<Arc<Mutex<Mixer>>>>,
    /// pre-stream preview capture (F-SC-03); stopped by stream start/stop
    preview: Mutex<Option<ScreenCapture>>,
    screen: Mutex<Option<ScreenHandle>>,
    audio_cap: Mutex<Option<AudioCapture>>,
}

impl SessionState {
    fn new(profiles: ProfilesConfig) -> Self {
        Self {
            profiles: Mutex::new(profiles),
            stream: Mutex::new(None),
            session: Mutex::new(None),
            retrying: Mutex::new(None),
            active_mixer: Mutex::new(None),
            preview: Mutex::new(None),
            screen: Mutex::new(None),
            audio_cap: Mutex::new(None),
        }
    }
}

pub struct Backend {
    tx: Sender<Command>,
    shared: Arc<Mutex<Shared>>,
    events: mpsc::Receiver<UiEvent>,
    worker: Option<JoinHandle<()>>,
}

impl Backend {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let sink = UiSink::new(event_tx);
        let shared = Arc::new(Mutex::new(Shared::default()));
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("ezstreamer-backend".into())
            .spawn(move || worker_loop(rx, sink, worker_shared))
            .expect("spawn backend worker");
        Self {
            tx,
            shared,
            events: event_rx,
            worker: Some(worker),
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    /// Non-blocking drain of one pending event; the UI loops until `None`.
    pub fn try_recv_event(&self) -> Option<UiEvent> {
        self.events.try_recv().ok()
    }

    pub fn shared(&self) -> Arc<Mutex<Shared>> {
        self.shared.clone()
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// worker

fn worker_loop(rx: mpsc::Receiver<Command>, sink: UiSink, shared: Arc<Mutex<Shared>>) {
    let profiles = load_profiles();
    let state = Arc::new(SessionState::new(profiles));
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Shutdown) => break,
            Ok(cmd) => handle_command(cmd, &state, &sink, &shared),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        tick(&state, &shared, &sink);
    }
    shutdown(&state);
    logging::info("backend worker stopped");
}

fn load_profiles() -> ProfilesConfig {
    match config::load(&config::config_path()) {
        Ok(cfg) => cfg,
        Err(e) => {
            logging::error(&format!("config load failed, using defaults: {e}"));
            ProfilesConfig::default()
        }
    }
}

fn handle_command(
    cmd: Command,
    state: &Arc<SessionState>,
    sink: &UiSink,
    shared: &Arc<Mutex<Shared>>,
) {
    match cmd {
        Command::Shutdown => {}
        Command::LoadAll => {
            let cfg = load_profiles();
            *state.profiles.lock().unwrap() = cfg.clone();
            sink.send(UiEvent::Config(Box::new(cfg)));
            spawn_enumerate(sink.clone());
            spawn_probe(sink.clone());
        }
        Command::RefreshSources => spawn_enumerate(sink.clone()),
        Command::ProbeEncoders => spawn_probe(sink.clone()),
        Command::SaveConfig(cfg) => cmd_save_config(*cfg, state, sink),
        Command::StartPreview(cfg) => {
            set_busy(shared, Some(Busy::Starting));
            cmd_start_preview(*cfg, state, sink, shared);
        }
        Command::StopPreview => cmd_stop_preview(state, sink, shared),
        Command::StartStream(cfg) => {
            set_busy(shared, Some(Busy::Starting));
            cmd_start_stream(*cfg, state, sink, shared);
            set_busy(shared, None);
        }
        Command::StopStream => {
            set_busy(shared, Some(Busy::Stopping));
            cmd_stop_stream(state, sink, shared);
            set_busy(shared, None);
        }
        Command::PortalPicker { cursor } => cmd_portal_picker(cursor, sink, shared),
        Command::UpdateMix(mix) => cmd_update_mix(*mix, state, sink),
        Command::OpenDir(path) => open_dir(&path, sink),
    }
}

fn set_busy(shared: &Arc<Mutex<Shared>>, busy: Option<Busy>) {
    shared.lock().unwrap().busy = busy;
}

fn spawn_enumerate(sink: UiSink) {
    let _ = std::thread::Builder::new()
        .name("enumerate".into())
        .spawn(move || {
            match cap::list_displays() {
                Ok(v) => sink.send(UiEvent::Displays(v)),
                Err(e) => {
                    // Linux/Portal has no app-side display list; that is not
                    // an error worth a toast, only a log line.
                    logging::info(&format!("display enumeration: {e}"));
                    sink.send(UiEvent::Displays(Vec::new()));
                }
            }
            match cap::list_windows() {
                Ok(v) => sink.send(UiEvent::Windows(v)),
                Err(e) => {
                    logging::info(&format!("window enumeration: {e}"));
                    sink.send(UiEvent::Windows(Vec::new()));
                }
            }
            match cap::list_audio_devices() {
                Ok(devices) => sink.send(UiEvent::AudioDevices(Box::new(devices))),
                Err(e) => {
                    logging::error(&format!("audio device enumeration: {e}"));
                    sink.send(UiEvent::AudioDevices(Box::default()));
                    sink.error("audio", e);
                }
            }
        });
}

fn spawn_probe(sink: UiSink) {
    let _ = std::thread::Builder::new()
        .name("encoder-probe".into())
        .spawn(move || {
            let infos = gst_stream::probe_encoder_infos();
            sink.send(UiEvent::Encoders(infos));
        });
}

fn portable_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------- pipeline lifecycle ----------

/// Build sinks + capture + GStreamer pipeline (design §4). Used by
/// `StartStream` and the F-ST-04 retry loop.
fn launch_pipeline(
    state: &Arc<SessionState>,
    sess: &StreamSession,
    sink: &UiSink,
    retry: u32,
) -> Result<gst_stream::GstStream, String> {
    use ezstreamer_core::audio::AudioSink;
    use ezstreamer_core::video::VideoSink;

    if sess.cfg.direct_input.as_deref() == Some("ddagrab") {
        eprintln!("direct_input=ddagrab was FFmpeg-only; ignoring (WGC → appsrc)");
    }

    gst_stream::ensure_bundled_runtime();
    let (vsink, video_rx) =
        VideoSink::spawn_appsrc(sess.profile.w, sess.profile.h, sess.profile.fps)
            .map_err(portable_err)?;
    let (asink, audio_rx) = AudioSink::spawn_appsrc(sess.mixer.clone()).map_err(portable_err)?;

    let screen = cap::start_screen(
        sink.clone(),
        &sess.cfg.screen,
        &sess.profile,
        vsink,
        sess.cfg.cursor,
    )
    .map_err(portable_err)?;
    *state.screen.lock().unwrap() = Some(cap::make_handle(screen));

    let audio_cap = cap::start_audio(&sess.cfg.audio, asink, Some(sink.clone())).map_err(|e| {
        stop_capture_backends(state);
        portable_err(e)
    })?;
    *state.audio_cap.lock().unwrap() = Some(audio_cap);

    gst_stream::spawn_pipeline(sess.plan.clone(), video_rx, audio_rx, retry)
        .inspect_err(|_| stop_capture_backends(state))
}

fn stop_capture_backends(state: &SessionState) {
    if let Some(mut s) = state.screen.lock().unwrap().take() {
        s.stop();
    }
    if let Some(mut a) = state.audio_cap.lock().unwrap().take() {
        a.stop();
    }
}

/// Take the preview slot's mutex guard across stop so concurrent preview
/// start/stop serialize: stop + replace + store is atomic.
fn take_preview_slot(state: &SessionState) -> std::sync::MutexGuard<'_, Option<ScreenCapture>> {
    let mut slot = state.preview.lock().unwrap();
    if let Some(mut p) = slot.take() {
        p.stop();
    }
    slot
}

fn stop_preview_impl(state: &SessionState) {
    let _slot = take_preview_slot(state);
}

fn cmd_start_stream(
    cfg: StreamConfig,
    state: &Arc<SessionState>,
    sink: &UiSink,
    shared: &Arc<Mutex<Shared>>,
) {
    if state.stream.lock().unwrap().is_some() {
        sink.error("stream", "stream already running");
        return;
    }
    stop_preview_impl(state); // preview must release the capture backends

    let profile = state
        .profiles
        .lock()
        .unwrap()
        .profiles
        .get(&cfg.profile_id)
        .cloned();
    let Some(profile) = profile else {
        sink.error("stream", format!("unknown profile: {}", cfg.profile_id));
        return;
    };

    // Normalize once so the usable-list branch, `build_plan` and the log
    // agree: whitespace-only behaves like "auto".
    let encoder_override = cfg.encoder_override.trim().to_string();
    let usable: Vec<String> = if encoder_override.is_empty() || encoder_override == "auto" {
        // Auto resolves against registry-present encoders only.
        gst_stream::probe_encoder_infos()
            .into_iter()
            .filter(|i| i.usable)
            .map(|i| i.name)
            .collect()
    } else {
        Vec::new() // manual override is used as-is; usability is the user's choice
    };
    let plan = match gst::build_plan(
        &profile,
        &encoder_override,
        &usable,
        &cfg.ingest_url,
        &cfg.stream_key,
        None,
    ) {
        Ok(plan) => plan,
        Err(e) => {
            sink.error("stream", e);
            return;
        }
    };

    logging::info(&format!(
        "start_stream: profile={} {}x{}@{} encoder_override={} cursor={} ingest={}",
        profile.name,
        profile.w,
        profile.h,
        profile.fps,
        encoder_override,
        cfg.cursor,
        cfg.ingest_url
    ));

    // initial mixer state from the UI selection
    let mixer = Arc::new(Mutex::new(Mixer {
        apps: cfg
            .audio
            .apps
            .iter()
            .map(|a| {
                let g = cfg.app_mix.get(a).copied().unwrap_or(SourceGain {
                    gain: 1.0,
                    muted: false,
                });
                (
                    a.clone(),
                    ezstreamer_core::audio::SourceState {
                        gain: g.gain,
                        muted: g.muted,
                        enabled: true,
                    },
                )
            })
            .collect(),
        mic: ezstreamer_core::audio::SourceState {
            gain: cfg.audio.mic.gain,
            muted: cfg.audio.mic.muted,
            enabled: cfg.audio.mic.enabled,
        },
    }));

    *state.session.lock().unwrap() = Some(StreamSession {
        cfg,
        plan,
        profile,
        mixer: mixer.clone(),
    });

    let result = {
        let session = state.session.lock().unwrap();
        let sess = session.as_ref().expect("session just set");
        launch_pipeline(state, sess, sink, 0)
    };

    match result {
        Ok(proc) => {
            *state.active_mixer.lock().unwrap() = Some(mixer);
            *state.retrying.lock().unwrap() = None;
            *state.stream.lock().unwrap() = Some(proc);
            shared.lock().unwrap().previewing = false;
            sink.send(UiEvent::StreamStarted);
        }
        Err(e) => {
            *state.session.lock().unwrap() = None;
            stop_capture_backends(state);
            sink.error("stream", e);
        }
    }
}

fn cmd_stop_stream(state: &Arc<SessionState>, sink: &UiSink, shared: &Arc<Mutex<Shared>>) {
    *state.retrying.lock().unwrap() = None; // cancels a pending F-ST-04 retry
    if let Some(mut p) = state.stream.lock().unwrap().take() {
        p.stop();
    }
    stop_capture_backends(state);
    *state.active_mixer.lock().unwrap() = None;
    *state.session.lock().unwrap() = None;
    stop_preview_impl(state);
    logging::info("stop_stream");
    {
        let mut sh = shared.lock().unwrap();
        sh.status = StreamStatus::default();
        sh.previewing = false;
    }
    sink.send(UiEvent::StreamStopped);
}

/// F-ST-04: respawn the whole pipeline with exponential backoff, at most
/// MAX_RETRIES times; the UI shows "reconnecting n/3" via the shared status.
fn spawn_retry_thread(
    state: Arc<SessionState>,
    first_retry: u32,
    sink: UiSink,
    shared: Arc<Mutex<Shared>>,
) {
    let _ = std::thread::Builder::new()
        .name("stream-retry".into())
        .spawn(move || {
            for n in first_retry..=MAX_RETRIES {
                logging::info(&format!(
                    "stream retry {n}/{MAX_RETRIES}: waiting {}ms",
                    retry_backoff_ms(n - 1)
                ));
                std::thread::sleep(Duration::from_millis(retry_backoff_ms(n - 1)));
                // user stop cancels the pending retry
                if state.retrying.lock().unwrap().is_none() {
                    return;
                }
                let Some(sess) = state.session.lock().unwrap().clone() else {
                    return;
                };
                stop_capture_backends(&state);
                match launch_pipeline(&state, &sess, &sink, n) {
                    Ok(mut proc) => {
                        // Install under the `stream` lock and re-check
                        // `retrying` inside it: a `StopStream` that landed
                        // during the long `launch_pipeline` must win.
                        // Lock order `stream → retrying` matches `tick`.
                        let mut stream = state.stream.lock().unwrap();
                        if state.retrying.lock().unwrap().is_none() {
                            drop(stream); // do not hold the lock while stopping
                            proc.stop();
                            stop_capture_backends(&state);
                            return;
                        }
                        *stream = Some(proc);
                        *state.retrying.lock().unwrap() = None;
                        logging::info(&format!("stream retry {n}/{MAX_RETRIES}: reconnected"));
                        return;
                    }
                    Err(e) => {
                        logging::error(&format!("stream retry {n}/{MAX_RETRIES} failed: {e}"));
                        eprintln!("stream retry {n}/{MAX_RETRIES} failed: {e}");
                    }
                }
            }
            // retries exhausted → stop (design §9: 3回失敗で停止)
            logging::error("stream retries exhausted; giving up");
            *state.retrying.lock().unwrap() = None;
            stop_capture_backends(&state);
            *state.session.lock().unwrap() = None;
            *state.stream.lock().unwrap() = None;
            *shared.lock().unwrap() = Shared::default();
            sink.send(UiEvent::Error(
                "stream: reconnection failed (3 attempts)".into(),
            ));
        });
}

// ---------- periodic status ----------

fn tick(state: &Arc<SessionState>, shared: &Arc<Mutex<Shared>>, sink: &UiSink) {
    // F-ST-03: VU levels (50ms cadence is produced by the audio sink; polling
    // at the worker tick is enough for the meters).
    let vu = state
        .audio_cap
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|cap| cap.sink.as_ref())
        .map(|sink| sink.last_vu.lock().unwrap().clone())
        .unwrap_or_default();

    let retrying = *state.retrying.lock().unwrap();
    let mut status = StreamStatus {
        retrying,
        ..Default::default()
    };
    let mut spawn_retry: Option<u32> = None;
    {
        let mut guard = state.stream.lock().unwrap();
        if let Some(proc) = guard.as_mut() {
            let mut st = proc.status();
            st.retrying = retrying;
            let exited = proc.try_wait();
            let mut give_up = false;
            if let Some(exit) = exited {
                if exit.success() {
                    st.is_live = false;
                } else if proc.retry_count < MAX_RETRIES && state.session.lock().unwrap().is_some()
                {
                    if retrying.is_none() {
                        let next = proc.retry_count + 1;
                        *state.retrying.lock().unwrap() = Some(next);
                        proc.mark_retrying(next);
                        st.retrying = Some(next);
                        spawn_retry = Some(next);
                    }
                } else if retrying.is_none() {
                    // retries exhausted → give up
                    give_up = true;
                }
            }
            status = st;
            if give_up {
                status.is_live = false;
                *state.session.lock().unwrap() = None;
                if let Some(mut dead) = guard.take() {
                    dead.take_result();
                    dead.stop();
                }
                status.retrying = None;
            }
        }
    }
    if let Some(n) = spawn_retry {
        spawn_retry_thread(state.clone(), n, sink.clone(), shared.clone());
    }
    let mut sh = shared.lock().unwrap();
    sh.status = status;
    sh.vu = vu;
}

// ---------- preview ----------

/// Capture-only preview: no GStreamer — the capture backend pushes raw RGBA
/// preview frames to the UI itself. Runs until stop/stream start.
fn cmd_start_preview(
    cfg: StreamConfig,
    state: &Arc<SessionState>,
    sink: &UiSink,
    shared: &Arc<Mutex<Shared>>,
) {
    use ezstreamer_core::video::VideoSink;

    if state.stream.lock().unwrap().is_some() {
        sink.error("preview", "stream already running");
        set_busy(shared, None);
        return;
    }
    // Hold the preview slot across stop + replace + store so a concurrent
    // start/stop cannot interleave and leak a capture.
    let mut slot = take_preview_slot(state);
    let profile = state
        .profiles
        .lock()
        .unwrap()
        .profiles
        .get(&cfg.profile_id)
        .cloned();
    let Some(profile) = profile else {
        sink.error("preview", format!("unknown profile: {}", cfg.profile_id));
        set_busy(shared, None);
        return;
    };
    let preview_profile = Profile {
        w: 640,
        h: 360,
        fps: 1,
        ..profile
    };
    let vsink = match VideoSink::spawn(capture::null_file(), 640, 360, 1) {
        Ok(v) => v,
        Err(e) => {
            sink.error("preview", e);
            set_busy(shared, None);
            return;
        }
    };
    match cap::start_screen(
        sink.clone(),
        &cfg.screen,
        &preview_profile,
        vsink,
        cfg.cursor,
    ) {
        Ok(screen) => {
            *slot = Some(screen);
            shared.lock().unwrap().previewing = true;
            sink.send(UiEvent::PreviewStarted);
        }
        Err(e) => {
            sink.error("preview", e);
        }
    }
    set_busy(shared, None);
}

fn cmd_stop_preview(state: &Arc<SessionState>, sink: &UiSink, shared: &Arc<Mutex<Shared>>) {
    stop_preview_impl(state);
    shared.lock().unwrap().previewing = false;
    sink.send(UiEvent::PreviewStopped);
}

// ---------- config / audio mix / misc ----------

fn cmd_save_config(cfg: ProfilesConfig, state: &Arc<SessionState>, sink: &UiSink) {
    // F-EN-04: guard every profile against Topaz limits before persisting
    for (id, p) in &cfg.profiles {
        if let Err(e) = validate_bitrate(p.v_kbps, p.a_kbps) {
            sink.error(
                "config",
                format!(
                    "{e} ({id}: video {}/audio {} kbps, limits {}/{} kbps)",
                    p.v_kbps, p.a_kbps, MAX_VIDEO_KBPS, MAX_AUDIO_KBPS
                ),
            );
            return;
        }
    }
    if !(cfg.ingest_url.starts_with("rtmp://") || cfg.ingest_url.starts_with("rtmps://")) {
        sink.error("config", "ingest URL must start with rtmp:// or rtmps://");
        return;
    }
    if let Err(e) = config::save(&config::config_path(), &cfg) {
        sink.error("config", e);
        return;
    }
    *state.profiles.lock().unwrap() = cfg;
}

fn cmd_update_mix(mix: AudioMixUpdate, state: &Arc<SessionState>, sink: &UiSink) {
    for g in mix.apps.values() {
        if !(0.0..=2.0).contains(&g.gain) {
            sink.error("mix", format!("gain out of range: {}", g.gain));
            return;
        }
    }
    if !(0.0..=2.0).contains(&mix.mic.gain) {
        sink.error("mix", format!("gain out of range: {}", mix.mic.gain));
        return;
    }
    if let Some(mixer) = state.active_mixer.lock().unwrap().as_ref() {
        let mut m = mixer.lock().unwrap();
        // Insert/update only: removing an app here cannot stick while its
        // capture thread is alive — the next mixed block re-registers it via
        // `auto_register`. The UI's add/remove selection therefore applies on
        // the next stream start; live updates only carry gain/mute.
        for (id, g) in &mix.apps {
            m.apps.insert(
                id.clone(),
                ezstreamer_core::audio::SourceState {
                    gain: g.gain,
                    muted: g.muted,
                    enabled: true,
                },
            );
        }
        m.mic = ezstreamer_core::audio::SourceState {
            gain: mix.mic.gain,
            muted: mix.mic.muted,
            enabled: mix.mic.enabled,
        };
    }
}

fn cmd_portal_picker(cursor: bool, sink: &UiSink, shared: &Arc<Mutex<Shared>>) {
    #[cfg(all(feature = "media", target_os = "linux"))]
    {
        set_busy(shared, Some(Busy::Picking));
        let shared2 = shared.clone();
        let sink2 = sink.clone();
        let _ = std::thread::Builder::new()
            .name("portal-picker".into())
            .spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(portable_err)
                    .and_then(|rt| {
                        rt.block_on(cap::portal_picker(cursor))
                            .map_err(portable_err)
                    });
                set_busy(&shared2, None);
                match result {
                    Ok(target) => sink2.send(UiEvent::PortalPicked(target)),
                    Err(e) => sink2.error("portal", e),
                }
            });
    }
    #[cfg(not(all(feature = "media", target_os = "linux")))]
    {
        let _ = (cursor, shared);
        sink.error("portal", "Portal picker requires Linux with media support");
    }
}

fn open_dir(path: &PathBuf, sink: &UiSink) {
    if std::fs::create_dir_all(path).is_err() {
        sink.error("open-dir", format!("cannot create {}", path.display()));
        return;
    }
    #[cfg(windows)]
    let result = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "linux")))]
    let result: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opening a folder is unsupported on this platform",
    ));
    if let Err(e) = result {
        sink.error("open-dir", e);
    }
}

/// Design §9: release capture/stream resources on app exit so the RTMP
/// connection and capture backends are torn down deterministically.
fn shutdown(state: &Arc<SessionState>) {
    if let Some(mut p) = state.stream.lock().unwrap().take() {
        p.stop();
    }
    stop_capture_backends(state);
    stop_preview_impl(state);
    *state.session.lock().unwrap() = None;
    logging::info("app exit: capture/pipeline stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ezstreamer_core::ipc_types::{AudioSelection, MicUpdate};

    #[test]
    fn retry_backoff_sequence_is_documented() {
        assert_eq!(retry_backoff_ms(0), 1000);
        assert_eq!(retry_backoff_ms(1), 2000);
        assert_eq!(retry_backoff_ms(2), 4000);
        assert_eq!(MAX_RETRIES, 3);
    }

    #[test]
    fn shared_defaults_are_idle() {
        let shared = Shared::default();
        assert!(!shared.previewing);
        assert!(shared.busy.is_none());
        assert!(!shared.status.is_live);
    }

    #[test]
    fn mic_update_roundtrip_shape() {
        let mic = MicUpdate {
            enabled: true,
            muted: false,
            gain: 1.25,
        };
        assert_eq!(mic.gain, 1.25);
        assert!(mic.enabled);
    }

    #[test]
    fn audio_selection_defaults_to_system() {
        let sel = AudioSelection::default();
        assert_eq!(sel.mode, "");
        assert!(sel.apps.is_empty());
    }
}
