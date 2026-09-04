//! Linux capture backend: Portal (ashpd) + PipeWire (design.md §3.1.2, §3.2.2).
//!
//! Screen/window goes through the xdg-desktop-portal ScreenCast picker;
//! audio uses PipeWire capture streams. Runtime needs a Wayland + PipeWire
//! session.

mod audio;
mod enumerate;
mod screen;

// Re-exported so submodules keep the single-file `super::{...}` paths
// (ezTopaz had one `capture/linux.rs`; the split must not churn call sites).
pub(super) use super::{CaptureError, Result};

pub use audio::{start_audio, AudioCapture};
pub use enumerate::{list_audio_devices, list_displays, list_windows};
pub use screen::{portal_picker, start_screen, ScreenCapture};

/// What feeds the video leg of a running stream (always Portal-picked).
pub enum ScreenHandle {
    Portal(ScreenCapture),
}

impl ScreenHandle {
    pub fn stop(&mut self) {
        match self {
            ScreenHandle::Portal(s) => s.stop(),
        }
    }
}
