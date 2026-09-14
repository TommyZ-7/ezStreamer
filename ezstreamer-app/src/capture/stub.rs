//! Stub capture backend for builds without the platform media stack
//! (`--no-default-features` / unsupported hosts). Same API as the real
//! `windows`/`linux` modules so [`crate::backend`] compiles unchanged.

use super::{CaptureError, Result};
use crate::events::UiSink;
use ezstreamer_core::audio::AudioSink;
use ezstreamer_core::config::{Profile, ScreenTarget};
use ezstreamer_core::ipc_types::{AudioDevices, AudioSelection, Display, WindowInfo};
use ezstreamer_core::video::VideoSink;

pub struct ScreenCapture {
    pub sink: VideoSink,
}

impl ScreenCapture {
    pub fn stop(&mut self) {
        self.stop_source();
        self.sink.stop();
    }

    pub fn stop_source(&mut self) {}

    pub fn video_sink(&self) -> VideoSink {
        self.sink.clone()
    }
}

pub struct AudioCapture {
    pub sink: Option<AudioSink>,
}

impl AudioCapture {
    pub fn stop(&mut self) {
        if let Some(s) = self.sink.as_mut() {
            s.stop();
        }
    }
}

pub enum ScreenHandle {
    Stub(ScreenCapture),
}

impl ScreenHandle {
    pub fn stop(&mut self) {
        match self {
            ScreenHandle::Stub(s) => s.stop(),
        }
    }

    pub fn stop_source(&mut self) {
        match self {
            ScreenHandle::Stub(s) => s.stop_source(),
        }
    }

    pub fn video_sink(&self) -> Option<VideoSink> {
        match self {
            ScreenHandle::Stub(s) => Some(s.video_sink()),
        }
    }
}

pub fn make_handle(s: ScreenCapture) -> ScreenHandle {
    ScreenHandle::Stub(s)
}

pub fn list_displays() -> Result<Vec<Display>> {
    Err(CaptureError::NotAvailable)
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    Err(CaptureError::NotAvailable)
}

pub fn list_audio_devices() -> Result<AudioDevices> {
    Err(CaptureError::NotAvailable)
}

pub fn start_screen(
    _ui: UiSink,
    _target: &ScreenTarget,
    _profile: &Profile,
    _sink: VideoSink,
    _cursor: bool,
) -> Result<ScreenCapture> {
    Err(CaptureError::NotAvailable)
}

pub fn start_audio(
    _selection: &AudioSelection,
    _sink: AudioSink,
    _ui: Option<UiSink>,
) -> Result<AudioCapture> {
    Err(CaptureError::NotAvailable)
}

pub async fn portal_picker(_cursor: bool) -> Result<ScreenTarget> {
    Err(CaptureError::NotAvailable)
}
