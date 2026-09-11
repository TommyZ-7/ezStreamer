//! UI-side application state: selections mirrored from `profiles.json`,
//! transient flags and validation helpers. Pure data + logic (no egui), so it
//! is unit-testable on hosts without the media stack.

use crate::ui::i18n::Locale;
use ezstreamer_core::config::{self, LastSources, MicSource, ProfilesConfig, ScreenTarget};
use ezstreamer_core::ipc_types::{
    AudioDevices, AudioSelection, Display, EncoderInfo, SourceGain, StreamConfig, StreamStatus,
    WindowInfo,
};
use std::collections::BTreeMap;
use std::time::Instant;

/// Left-rail steps (requirements §7: 画面 / 音声 / 出力).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Screen,
    Audio,
    Output,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Screen, Tab::Audio, Tab::Output];

    pub fn title_key(self) -> &'static str {
        match self {
            Tab::Screen => "steps.screen",
            Tab::Audio => "steps.audio",
            Tab::Output => "steps.output",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioMode {
    System,
    Apps,
}

impl AudioMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AudioMode::System => "system",
            AudioMode::Apps => "apps",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppMixEntry {
    pub gain: f32,
    pub muted: bool,
}

impl Default for AppMixEntry {
    fn default() -> Self {
        Self {
            gain: 1.0,
            muted: false,
        }
    }
}

/// Built-in profile ids, shown first in the UI.
pub const BUILTIN_PROFILE_IDS: &[&str] = &["low", "mid", "high", "1080p"];

pub struct UiState {
    pub locale: Locale,
    pub tab: Tab,
    pub settings_open: bool,
    pub booted: bool,

    // backend data
    pub displays: Vec<Display>,
    pub windows: Vec<WindowInfo>,
    pub devices: Option<AudioDevices>,
    pub encoders: Vec<EncoderInfo>,
    pub profiles: Option<ProfilesConfig>,

    // selections (persisted through SaveConfig)
    pub screen: ScreenTarget,
    pub audio_mode: AudioMode,
    pub selected_apps: Vec<String>,
    pub mic: MicSource,
    pub cursor: bool,
    pub app_mix: BTreeMap<String, AppMixEntry>,
    pub profile_id: String,
    pub encoder_override: String,
    pub ingest_url: String,
    pub stream_key: String,

    // transient
    pub settings_draft: Option<ProfilesConfig>,
    pub import_path: String,
    pub pending_import: Option<ProfilesConfig>,
    pub toast: Option<(String, Instant)>,
    pub toast_error: bool,
    pub last_error: Option<String>,
    pub starting: bool,
    pub stopping: bool,
    pub persist_due: Option<Instant>,
    pub mix_due: Option<Instant>,
    pub preview_restart_due: Option<Instant>,
    pub status: StreamStatus,
}

impl UiState {
    pub fn new(os_locale: Locale) -> Self {
        let defaults = ProfilesConfig::default();
        Self {
            locale: os_locale,
            tab: Tab::Screen,
            settings_open: false,
            booted: false,
            displays: Vec::new(),
            windows: Vec::new(),
            devices: None,
            encoders: Vec::new(),
            profiles: None,
            screen: defaults.last_sources.screen,
            audio_mode: AudioMode::System,
            selected_apps: Vec::new(),
            mic: defaults.last_sources.mic,
            cursor: defaults.last_sources.cursor,
            app_mix: BTreeMap::new(),
            profile_id: defaults.active_profile,
            encoder_override: defaults.encoder_override,
            ingest_url: defaults.ingest_url,
            stream_key: defaults.last_stream_key,
            settings_draft: None,
            import_path: String::new(),
            pending_import: None,
            toast: None,
            toast_error: false,
            last_error: None,
            starting: false,
            stopping: false,
            persist_due: None,
            mix_due: None,
            preview_restart_due: None,
            status: StreamStatus::default(),
        }
    }

    /// F-CF-02: restore selections from `profiles.json` (locale included).
    pub fn apply_config(&mut self, cfg: ProfilesConfig) {
        self.locale = Locale::from_code(&cfg.locale).unwrap_or(self.locale);
        self.ingest_url = cfg.ingest_url.clone();
        self.stream_key = cfg.last_stream_key.clone();
        self.profile_id = cfg.active_profile.clone();
        self.encoder_override = cfg.encoder_override.clone();
        self.screen = cfg.last_sources.screen.clone();
        self.selected_apps = cfg.last_sources.include_apps.clone();
        self.mic = cfg.last_sources.mic.clone();
        self.cursor = cfg.last_sources.cursor;
        self.audio_mode = if self.selected_apps.is_empty() {
            AudioMode::System
        } else {
            AudioMode::Apps
        };
        self.profiles = Some(cfg);
        self.booted = true;
    }

    /// Current selections as a persistable config (None until booted).
    pub fn to_config(&self) -> Option<ProfilesConfig> {
        let mut cfg = self.profiles.clone()?;
        cfg.locale = self.locale.code().to_string();
        cfg.ingest_url = self.ingest_url.clone();
        cfg.active_profile = self.profile_id.clone();
        cfg.encoder_override = self.encoder_override.clone();
        cfg.last_stream_key = self.stream_key.clone();
        cfg.last_sources = LastSources {
            screen: self.screen.clone(),
            include_apps: self.selected_apps.clone(),
            mic: self.mic.clone(),
            cursor: self.cursor,
        };
        Some(cfg)
    }

    pub fn stream_config(&self) -> StreamConfig {
        StreamConfig {
            ingest_url: self.ingest_url.clone(),
            stream_key: self.stream_key.clone(),
            screen: self.screen.clone(),
            audio: AudioSelection {
                mode: self.audio_mode.as_str().to_string(),
                apps: self.selected_apps.clone(),
                mic: self.mic.clone(),
            },
            profile_id: self.profile_id.clone(),
            encoder_override: self.encoder_override.clone(),
            cursor: self.cursor,
            app_mix: self
                .app_mix
                .iter()
                .map(|(id, entry)| {
                    (
                        id.clone(),
                        SourceGain {
                            gain: entry.gain,
                            muted: entry.muted,
                        },
                    )
                })
                .collect(),
            hw_direct: false,
            direct_input: None,
        }
    }

    /// F-AU-04: live mixer update for the running stream.
    pub fn mix_update(&self) -> ezstreamer_core::ipc_types::AudioMixUpdate {
        ezstreamer_core::ipc_types::AudioMixUpdate {
            apps: self
                .app_mix
                .iter()
                .map(|(id, entry)| {
                    (
                        id.clone(),
                        SourceGain {
                            gain: entry.gain,
                            muted: entry.muted,
                        },
                    )
                })
                .collect(),
            mic: ezstreamer_core::ipc_types::MicUpdate {
                enabled: self.mic.enabled,
                muted: self.mic.muted,
                gain: self.mic.gain,
            },
        }
    }

    /// F-ST-01: `None` when valid, otherwise the i18n error key.
    pub fn key_error(&self) -> Option<&'static str> {
        if self.stream_key.is_empty() {
            return Some("stream.keyRequired");
        }
        if config::validate_stream_key(&self.stream_key).is_err() {
            return Some("stream.keyInvalid");
        }
        None
    }

    pub fn generic_key(&self) -> bool {
        ezstreamer_core::urls::is_generic_key(&self.stream_key)
    }

    pub fn playback_urls(&self) -> (String, String) {
        ezstreamer_core::urls::playback_urls(&self.ingest_url, &self.stream_key)
    }

    /// Built-ins first, then custom profiles.
    pub fn profile_ids(&self) -> Vec<String> {
        let Some(cfg) = &self.profiles else {
            return Vec::new();
        };
        let mut ids: Vec<String> = BUILTIN_PROFILE_IDS
            .iter()
            .filter(|id| cfg.profiles.contains_key(**id))
            .map(|id| (*id).to_string())
            .collect();
        ids.extend(
            cfg.profiles
                .keys()
                .filter(|id| !BUILTIN_PROFILE_IDS.contains(&id.as_str()))
                .cloned(),
        );
        ids
    }

    pub fn app_mix_entry(&self, id: &str) -> AppMixEntry {
        self.app_mix.get(id).copied().unwrap_or_default()
    }

    pub fn set_app_mix(&mut self, id: &str, entry: AppMixEntry) {
        self.app_mix.insert(id.to_string(), entry);
        self.mix_due = Some(Instant::now());
    }

    pub fn set_mic(&mut self, mic: MicSource) {
        self.mic = mic;
        self.mark_persist();
        self.mix_due = Some(Instant::now());
    }

    pub fn mark_persist(&mut self) {
        self.persist_due = Some(Instant::now());
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.mark_persist();
    }

    pub fn toast(&mut self, message: impl Into<String>, error: bool) {
        self.toast = Some((message.into(), Instant::now()));
        self.toast_error = error;
    }

    pub fn toggle_app(&mut self, id: &str) {
        if let Some(pos) = self.selected_apps.iter().position(|a| a == id) {
            self.selected_apps.remove(pos);
        } else {
            self.selected_apps.push(id.to_string());
        }
        self.mark_persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_keeps_selections() {
        let mut state = UiState::new(Locale::Ja);
        state.apply_config(ProfilesConfig::default());
        state.stream_key = "my-key-1".into();
        state.ingest_url = "rtmp://example.test/live".into();
        state.selected_apps = vec!["app:1".into()];
        state.audio_mode = AudioMode::Apps;
        state.cursor = false;
        let cfg = state.to_config().unwrap();
        assert_eq!(cfg.last_stream_key, "my-key-1");
        assert_eq!(cfg.ingest_url, "rtmp://example.test/live");
        assert_eq!(cfg.last_sources.include_apps, vec!["app:1".to_string()]);
        assert!(!cfg.last_sources.cursor);
        assert_eq!(cfg.locale, "ja");
    }

    #[test]
    fn key_error_matches_backend_rules() {
        let mut state = UiState::new(Locale::En);
        state.apply_config(ProfilesConfig::default());
        assert_eq!(state.key_error(), Some("stream.keyRequired"));
        state.stream_key = "ab".into();
        assert_eq!(state.key_error(), Some("stream.keyInvalid"));
        state.stream_key = "ok-key_1".into();
        assert_eq!(state.key_error(), None);
        assert!(!state.generic_key());
        state.stream_key = "test".into();
        assert!(state.generic_key());
    }

    #[test]
    fn builtin_profiles_come_first() {
        let mut state = UiState::new(Locale::En);
        let mut cfg = ProfilesConfig::default();
        cfg.profiles.insert(
            "custom1".into(),
            ezstreamer_core::config::Profile {
                name: "Custom 1".into(),
                w: 1280,
                h: 720,
                fps: 30,
                v_kbps: 1500,
                a_kbps: 192,
                encoder: "auto".into(),
                warn: None,
            },
        );
        state.apply_config(cfg);
        assert_eq!(
            state.profile_ids(),
            vec!["low", "mid", "high", "1080p", "custom1"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn playback_urls_use_current_key() {
        let mut state = UiState::new(Locale::En);
        state.apply_config(ProfilesConfig::default());
        state.stream_key = "abc123".into();
        let (pc, quest) = state.playback_urls();
        assert_eq!(pc, "rtspt://topaz.chat/live/abc123");
        assert_eq!(quest, "rtsp://topaz.chat/live/abc123");
    }
}
