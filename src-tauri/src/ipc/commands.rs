//! Tauri IPC commands (design.md §5.1).
//!
//! Capture + GStreamer pipeline live behind Windows / Linux cfgs; other
//! hosts get clear stub errors so `cargo test` / `cargo check` /
//! `pnpm build` pass anywhere. CI builds the real backend on
//! `windows-latest` and `ubuntu-latest`.

use arboard::Clipboard;
#[cfg_attr(not(windows), allow(unused_imports))]
use ezstreamer_core::config::{self, validate_bitrate, Profile, ProfilesConfig, MAX_AUDIO_KBPS, MAX_VIDEO_KBPS};
#[cfg_attr(any(windows, target_os = "linux"), allow(unused_imports))]
use ezstreamer_core::error::Error;
use ezstreamer_core::gst::StreamPlan;
use ezstreamer_core::ipc_types::*;
#[cfg(any(windows, target_os = "linux"))]
use ezstreamer_core::gst::{self, retry_backoff_ms, MAX_RETRIES};
use std::sync::{Arc, Mutex};
#[cfg(any(windows, target_os = "linux"))]
use std::time::Duration;
use tauri::State;

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub struct AppState {
    #[cfg(any(windows, target_os = "linux"))]
    pub stream: Mutex<Option<super::gst_stream::GstStream>>,
    #[cfg(not(any(windows, target_os = "linux")))]
    pub stream: Mutex<Option<()>>,
    /// F-ST-04: everything needed to respawn the pipeline after an abnormal exit
    pub session: Mutex<Option<StreamSession>>,
    /// F-ST-04: retry attempt in backoff (Some(n) = "再接続中 n/3")
    pub retrying: Mutex<Option<u32>>,
    pub last_mix: Mutex<AudioMixUpdate>,
    /// live mixer of the running stream (update_audio_mix targets this)
    pub active_mixer: Mutex<Option<Arc<Mutex<ezstreamer_core::audio::Mixer>>>>,
    /// pre-stream preview capture (F-SC-03); stopped by start/stop_stream
    #[cfg(any(windows, target_os = "linux"))]
    pub preview: Mutex<Option<crate::capture::ScreenCapture>>,
    #[cfg(not(any(windows, target_os = "linux")))]
    pub preview: Mutex<Option<()>>,
    /// Live video leg (always WGC → appsrc; direct-input was FFmpeg-only).
    #[cfg(any(windows, target_os = "linux"))]
    pub screen: Mutex<Option<crate::capture::platform::ScreenHandle>>,
    #[cfg(not(any(windows, target_os = "linux")))]
    pub screen: Mutex<Option<()>>,
    #[cfg(any(windows, target_os = "linux"))]
    pub audio_cap: Mutex<Option<crate::capture::AudioCapture>>,
    #[cfg(not(any(windows, target_os = "linux")))]
    pub audio_cap: Mutex<Option<()>>,
}

/// F-ST-04: the live stream's inputs, kept so a retry can respawn the whole
/// pipeline (capture + GStreamer) with the same settings.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
#[derive(Clone)]
pub struct StreamSession {
    pub cfg: StreamConfig,
    pub plan: StreamPlan,
    pub profile: Profile,
    pub mixer: Arc<Mutex<ezstreamer_core::audio::Mixer>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            stream: Mutex::new(None),
            session: Mutex::new(None),
            retrying: Mutex::new(None),
            last_mix: Mutex::new(AudioMixUpdate::default()),
            active_mixer: Mutex::new(None),
            preview: Mutex::new(None),
            screen: Mutex::new(None),
            audio_cap: Mutex::new(None),
        }
    }
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn ping() -> String {
    "ezStreamer".into()
}

// ---------- capture ----------

#[tauri::command]
pub fn get_displays() -> CmdResult<Vec<Display>> {
    #[cfg(any(windows, target_os = "linux"))]
    return crate::capture::platform::list_displays().map_err(err);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(Error::NotImplemented("display enumeration (unsupported platform)")).map_err(err)
}

#[tauri::command]
pub fn get_windows() -> CmdResult<Vec<WindowInfo>> {
    #[cfg(any(windows, target_os = "linux"))]
    return crate::capture::platform::list_windows().map_err(err);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(Error::NotImplemented("window enumeration (unsupported platform)")).map_err(err)
}

#[tauri::command]
pub fn get_audio_devices() -> CmdResult<AudioDevices> {
    #[cfg(any(windows, target_os = "linux"))]
    return crate::capture::platform::list_audio_devices().map_err(err);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(Error::NotImplemented("audio device enumeration (unsupported platform)")).map_err(err)
}

/// Open the OS screen picker (Linux/Wayland: xdg-desktop-portal ScreenCast).
/// The chosen stream is remembered backend-side; subsequent start_preview /
/// start_stream calls capture it without re-picking. `cursor` follows F-SC-04.
#[tauri::command]
pub async fn start_portal_picker(cursor: Option<bool>) -> CmdResult<ScreenTarget> {
    #[cfg(target_os = "linux")]
    return crate::capture::platform::portal_picker(cursor.unwrap_or(true))
        .await
        .map_err(err);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cursor;
        Err(Error::NotImplemented("portal picker (Linux-only)")).map_err(err)
    }
}

// ---------- config ----------

#[tauri::command]
pub fn get_profiles() -> CmdResult<ProfilesConfig> {
    config::load(&config::config_path()).map_err(err)
}

#[tauri::command]
pub fn save_profiles(cfg: ProfilesConfig) -> CmdResult<()> {
    // F-EN-04: guard every profile against Topaz limits before persisting
    for (id, p) in &cfg.profiles {
        validate_bitrate(p.v_kbps, p.a_kbps).map_err(|_| {
            format!("{id}: video {}/audio {}kbps exceeds limits ({}k/{}k)", p.v_kbps, p.a_kbps, MAX_VIDEO_KBPS, MAX_AUDIO_KBPS)
        })?;
    }
    if !(cfg.ingest_url.starts_with("rtmp://") || cfg.ingest_url.starts_with("rtmps://")) {
        return Err("ingest URL must start with rtmp:// or rtmps://".into());
    }
    let path = config::config_path();
    config::save(&path, &cfg).map_err(err)
}

// ---------- encoders (GStreamer registry, design §8.1) ----------

/// Encoder discovery via the GStreamer registry — no child processes, no
/// cache file (registry lookup is microseconds; the FFmpeg generation needed
/// `encoders.json` because it ran 1-frame test encodes).
#[tauri::command]
pub fn probe_encoders(app: tauri::AppHandle) -> CmdResult<Vec<EncoderInfo>> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        super::gst_stream::ensure_bundled_runtime(&app);
        if ::gstreamer::init().is_err() {
            return Ok(vec![EncoderInfo {
                name: "libx264".into(),
                usable: false,
                reason: Some("GStreamer runtime not found".into()),
            }]);
        }
        let mut infos =
            ezstreamer_core::gst::probe_with(|e| super::gst_stream::has_element(e));
        // UI order: manual list (auto handled client-side).
        infos.sort_by_key(|i| {
            ezstreamer_core::gst::MANUAL_ENCODERS
                .iter()
                .position(|m| m == &i.name)
                .unwrap_or(usize::MAX)
        });
        Ok(infos)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = &app;
        Ok(ezstreamer_core::gst::probe_encoders())
    }
}

// ---------- stream lifecycle ----------

/// Build sinks + capture + GStreamer pipeline (design §4). Used by
/// `start_stream` and the F-ST-04 retry loop; a retry rebuilds everything
/// because `appsrc` channels are single-consumer.
#[cfg(any(windows, target_os = "linux"))]
fn launch_pipeline(
    app: &tauri::AppHandle,
    state: &AppState,
    sess: &StreamSession,
    retry: u32,
) -> Result<super::gst_stream::GstStream, String> {
    use ezstreamer_core::audio::AudioSink;
    use ezstreamer_core::video::VideoSink;

    if sess.cfg.direct_input.as_deref() == Some("ddagrab") {
        eprintln!("direct_input=ddagrab was FFmpeg-only; falling back to WGC → appsrc");
    }

    // Pumps: capture backends push here; feeder threads move frames to appsrc.
    super::gst_stream::ensure_bundled_runtime(app);
    let (vsink, video_rx) =
        VideoSink::spawn_appsrc(sess.profile.w, sess.profile.h, sess.profile.fps).map_err(err)?;
    let (asink, audio_rx) =
        AudioSink::spawn_appsrc(sess.mixer.clone()).map_err(err)?;

    let screen = crate::capture::platform::start_screen(
        app.clone(),
        &sess.cfg.screen,
        &sess.profile,
        vsink,
        sess.cfg.cursor,
    )
    .map_err(err)?;
    #[cfg(windows)]
    let handle = crate::capture::platform::ScreenHandle::Wgc(screen);
    #[cfg(target_os = "linux")]
    let handle = crate::capture::platform::ScreenHandle::Portal(screen);
    *state.screen.lock().unwrap() = Some(handle);

    let audio_cap =
        crate::capture::platform::start_audio(&sess.cfg.audio, asink, Some(app.clone())).map_err(
            |e| {
                stop_capture_backends(state);
                err(e)
            },
        )?;
    *state.audio_cap.lock().unwrap() = Some(audio_cap);

    super::gst_stream::spawn_pipeline(sess.plan.clone(), video_rx, audio_rx, retry).map_err(|e| {
        stop_capture_backends(state);
        e
    })
}

fn stop_capture_backends(state: &AppState) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        if let Some(mut s) = state.screen.lock().unwrap().take() {
            s.stop();
        }
        if let Some(mut a) = state.audio_cap.lock().unwrap().take() {
            a.stop();
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = state;
}

/// Take the preview slot's mutex guard across stop so concurrent
/// start_preview/stop_preview commands serialize: stop + replace + store is
/// atomic, never interleaved.
#[cfg(any(windows, target_os = "linux"))]
fn take_preview_slot(
    state: &AppState,
) -> std::sync::MutexGuard<'_, Option<crate::capture::ScreenCapture>> {
    let mut slot = state.preview.lock().unwrap();
    if let Some(mut p) = slot.take() {
        p.stop();
    }
    slot
}

fn stop_preview_impl(state: &AppState) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let _slot = take_preview_slot(state);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = state;
}

#[tauri::command]
pub fn start_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    cfg: StreamConfig,
) -> CmdResult<StreamStatus> {
    if state.stream.lock().unwrap().is_some() {
        return Err("stream already running".into());
    }
    stop_preview_impl(&state); // preview must release the capture backends

    let profiles = config::load(&config::config_path()).map_err(err)?;
    let profile = profiles
        .profiles
        .get(&cfg.profile_id)
        .cloned()
        .ok_or_else(|| format!("unknown profile: {}", cfg.profile_id))?;

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (&app, &profile, &cfg);
        return Err(Error::NotImplemented("streaming (unsupported platform)")).map_err(err);
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        // Normalize once so the usable-list branch, `build_plan` and the log
        // agree: whitespace-only behaves like "auto" (review Low #3; `""` used
        // to reach `resolve_encoder` and fail with EncoderNotAvailable("")).
        let encoder_override = cfg.encoder_override.trim().to_string();
        let usable: Vec<String> = if encoder_override.is_empty() || encoder_override == "auto" {
            // Auto resolves against registry-present encoders only.
            probe_encoders(app.clone())
                .map(|infos| infos.into_iter().filter(|i| i.usable).map(|i| i.name).collect())
                .unwrap_or_default() // probe failed: resolve falls back to libx264
        } else {
            Vec::new() // manual override is used as-is; usability is the user's choice
        };
        let plan = gst::build_plan(
            &profile,
            &encoder_override,
            &usable,
            &cfg.ingest_url,
            &cfg.stream_key,
            None,
        )
        .map_err(err)?;

        crate::logging::info(&format!(
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
        let mixer = Arc::new(Mutex::new(ezstreamer_core::audio::Mixer {
            apps: cfg
                .audio
                .apps
                .iter()
                .map(|a| {
                    let g = cfg
                        .app_mix
                        .get(a)
                        .copied()
                        .unwrap_or(SourceGain { gain: 1.0, muted: false });
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
        *state.last_mix.lock().unwrap() = AudioMixUpdate {
            apps: cfg
                .audio
                .apps
                .iter()
                .map(|a| {
                    let g = cfg
                        .app_mix
                        .get(a)
                        .copied()
                        .unwrap_or(SourceGain { gain: 1.0, muted: false });
                    (a.clone(), g)
                })
                .collect(),
            mic: MicUpdate {
                enabled: cfg.audio.mic.enabled,
                muted: cfg.audio.mic.muted,
                gain: cfg.audio.mic.gain,
            },
        };

        *state.session.lock().unwrap() = Some(StreamSession {
            cfg,
            plan,
            profile: profile.clone(),
            mixer: mixer.clone(),
        });
        let sess = state.session.lock().unwrap();
        let proc = launch_pipeline(&app, &state, sess.as_ref().expect("session just set"), 0)?;
        drop(sess);
        *state.active_mixer.lock().unwrap() = Some(mixer);
        *state.retrying.lock().unwrap() = None;

        let status = proc.status();
        *state.stream.lock().unwrap() = Some(proc);
        Ok(status)
    }
}

#[tauri::command]
pub fn stop_stream(state: State<'_, AppState>) -> CmdResult<()> {
    *state.retrying.lock().unwrap() = None; // cancels a pending F-ST-04 retry
    #[cfg(any(windows, target_os = "linux"))]
    if let Some(mut p) = state.stream.lock().unwrap().take() {
        p.stop();
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        *state.stream.lock().unwrap() = None;
    }
    stop_capture_backends(&state);
    *state.active_mixer.lock().unwrap() = None;
    *state.session.lock().unwrap() = None;
    stop_preview_impl(&state);
    crate::logging::info("stop_stream");
    Ok(())
}

#[tauri::command]
pub fn get_status(app: tauri::AppHandle, state: State<'_, AppState>) -> StreamStatus {
    let retrying = state.retrying.lock().unwrap().clone();
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (app, retrying);
        return StreamStatus::default();
    }
    #[cfg(any(windows, target_os = "linux"))]
    {
        let mut guard = state.stream.lock().unwrap();
        let Some(p) = guard.as_mut() else {
            // between retries (backoff) or fully stopped
            return StreamStatus { retrying, ..Default::default() };
        };
        let mut st = p.status();
        let mut give_up = false;
        if let Some(exited) = p.try_wait() {
            if exited.success() {
                st.is_live = false;
            } else {
                // F-ST-04: abnormal exit → retry with exponential backoff
                if p.retry_count < MAX_RETRIES && state.session.lock().unwrap().is_some() {
                    if retrying.is_none() {
                        let next = p.retry_count + 1;
                        *state.retrying.lock().unwrap() = Some(next);
                        p.mark_retrying(next);
                        st.retrying = Some(next);
                        spawn_retry_thread(app, next);
                    }
                } else if retrying.is_none() {
                    give_up = true; // retries exhausted → give up
                }
            }
        }
        if give_up {
            st.is_live = false;
            *state.session.lock().unwrap() = None;
            if let Some(mut dead) = guard.take() {
                dead.take_result();
                dead.stop();
            }
        }
        st
    }
}

/// F-ST-04: respawn the whole pipeline (capture + GStreamer) with
/// exponential backoff, at most MAX_RETRIES times; 「再接続中 n/3」 via retrying.
#[cfg(any(windows, target_os = "linux"))]
fn spawn_retry_thread(app: tauri::AppHandle, first_retry: u32) {
    std::thread::Builder::new()
        .name("stream-retry".into())
        .spawn(move || {
            use tauri::Manager;
            let state = app.state::<AppState>();
            for n in first_retry..=MAX_RETRIES {
                crate::logging::info(&format!(
                    "stream retry {n}/{MAX_RETRIES}: waiting {}ms",
                    retry_backoff_ms(n - 1)
                ));
                std::thread::sleep(Duration::from_millis(retry_backoff_ms(n - 1)));
                // user stop cancels the pending retry
                if state.retrying.lock().unwrap().is_none() {
                    return;
                }
                let Some(sess) = state.session.lock().unwrap().clone() else { return };
                stop_capture_backends(&state);
                match launch_pipeline(&app, &state, &sess, n) {
                    Ok(mut proc) => {
                        // Install under the `stream` lock and re-check
                        // `retrying` inside it: a `stop_stream` that landed
                        // during the long `launch_pipeline` must win.
                        // `stop_stream` clears `retrying` before taking
                        // `stream`, so once this lock is granted either the
                        // stop is visible (None) or we install. Without this,
                        // the late install left an invisible live stream
                        // running until the next stop (review Medium).
                        // Lock order `stream → retrying` matches `get_status`.
                        let mut stream = state.stream.lock().unwrap();
                        if state.retrying.lock().unwrap().is_none() {
                            drop(stream); // do not hold the lock while stopping
                            proc.stop();
                            stop_capture_backends(&state);
                            return;
                        }
                        *stream = Some(proc);
                        *state.retrying.lock().unwrap() = None;
                        crate::logging::info(&format!("stream retry {n}/{MAX_RETRIES}: reconnected"));
                        return;
                    }
                    Err(e) => {
                        crate::logging::error(&format!("stream retry {n}/{MAX_RETRIES} failed: {e}"));
                        eprintln!("stream retry {n}/{MAX_RETRIES} failed: {e}");
                    }
                }
            }
            // retries exhausted → stop (design §9: 3回失敗で停止)
            crate::logging::error("stream retries exhausted; giving up");
            *state.retrying.lock().unwrap() = None;
            stop_capture_backends(&state);
            *state.session.lock().unwrap() = None;
            *state.stream.lock().unwrap() = None;
        })
        .ok();
}

// ---------- pre-stream preview (F-SC-03) ----------

/// Capture-only preview: no GStreamer — the capture backend emits
/// `stream://preview` (640x360 PNG @1fps, design §6.4) itself. Runs until
/// `stop_preview` / `start_stream` / `stop_stream`.
#[tauri::command]
pub fn start_preview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    cfg: StreamConfig,
) -> CmdResult<()> {
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (app, state, cfg);
        return Err(Error::NotImplemented("preview (unsupported platform)")).map_err(err);
    }
    #[cfg(any(windows, target_os = "linux"))]
    {
        use ezstreamer_core::video::VideoSink;
        if state.stream.lock().unwrap().is_some() {
            return Err("stream already running".into());
        }
        // Hold the preview slot across stop + replace + store so a concurrent
        // start_preview/stop_preview cannot interleave and leak a capture.
        let mut slot = take_preview_slot(&state);
        let profiles = config::load(&config::config_path()).map_err(err)?;
        let profile = profiles
            .profiles
            .get(&cfg.profile_id)
            .cloned()
            .ok_or_else(|| format!("unknown profile: {}", cfg.profile_id))?;
        let preview_profile = Profile { w: 640, h: 360, fps: 1, ..profile };
        let vsink = VideoSink::spawn(
            crate::capture::null_file(),
            preview_profile.w,
            preview_profile.h,
            preview_profile.fps,
        )
        .map_err(err)?;
        let screen = crate::capture::platform::start_screen(
            app,
            &cfg.screen,
            &preview_profile,
            vsink,
            cfg.cursor,
        )
        .map_err(err)?;
        *slot = Some(screen);
        Ok(())
    }
}

#[tauri::command]
pub fn stop_preview(state: State<'_, AppState>) -> CmdResult<()> {
    stop_preview_impl(&state);
    Ok(())
}

#[tauri::command]
pub fn get_vu(state: State<'_, AppState>) -> VuMeter {
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = state;
    #[cfg(any(windows, target_os = "linux"))]
    if let Some(cap) = state.audio_cap.lock().unwrap().as_ref() {
        if let Some(sink) = &cap.sink {
            return sink.last_vu.lock().unwrap().clone();
        }
    }
    VuMeter::default()
}

#[tauri::command]
pub fn update_audio_mix(state: State<'_, AppState>, mix: AudioMixUpdate) -> CmdResult<()> {
    for g in mix.apps.values() {
        if !(0.0..=2.0).contains(&g.gain) {
            return Err(format!("gain out of range: {}", g.gain));
        }
    }
    if !(0.0..=2.0).contains(&mix.mic.gain) {
        return Err(format!("gain out of range: {}", mix.mic.gain));
    }
    if let Some(mixer) = state.active_mixer.lock().unwrap().as_ref() {
        let mut m = mixer.lock().unwrap();
        // Insert/update only: removing an app here cannot stick while its
        // capture thread is alive — the next mixed block re-registers it via
        // `auto_register` (core/src/audio/sink.rs). The UI's add/remove
        // selection therefore applies on the next stream start; live updates
        // only carry gain/mute.
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
    *state.last_mix.lock().unwrap() = mix;
    Ok(())
}

// ---------- misc ----------

/// Design §9: release capture/stream resources on app exit so the RTMP
/// connection and capture backends are torn down deterministically.
pub fn shutdown(app: &tauri::AppHandle) {
    use tauri::Manager;
    let state = app.state::<AppState>();
    #[cfg(any(windows, target_os = "linux"))]
    {
        if let Some(mut p) = state.stream.lock().unwrap().take() {
            p.stop();
        }
    }
    stop_capture_backends(&state);
    stop_preview_impl(&state);
    *state.session.lock().unwrap() = None;
    crate::logging::info("app exit: capture/pipeline stopped");
}

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> CmdResult<()> {
    let mut cb = Clipboard::new().map_err(err)?;
    cb.set_text(text).map_err(err)
}

#[tauri::command]
pub fn open_logs_dir() -> CmdResult<()> {
    let dir = config::config_dir().join("logs");
    std::fs::create_dir_all(&dir).map_err(err)?;
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&dir).spawn();
    #[cfg(not(any(windows, target_os = "linux")))]
    let r = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "log folder opening is unsupported on this platform in this build",
    ));
    r.map_err(err)?;
    Ok(())
}
