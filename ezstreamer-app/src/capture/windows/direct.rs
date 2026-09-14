//! Compat shim: the FFmpeg generation had a `ddagrab` direct-input path
//! (`StreamConfig.direct_input`). GStreamer owns encode now and always takes
//! Rust-captured frames via `appsrc`, so there is no device-input leg.
//! The `ScreenHandle` enum is kept so call sites compile unchanged; the
//! `Direct` variant is never constructed (any `direct_input` request falls
//! back to WGC with a log line).

use super::screen::ScreenCapture;

/// What feeds the video leg of a running stream (always WGC now).
pub enum ScreenHandle {
    Wgc(ScreenCapture),
    /// Legacy compat, unused. Kept for call-site stability.
    Direct,
}

impl ScreenHandle {
    pub fn stop(&mut self) {
        match self {
            ScreenHandle::Wgc(s) => s.stop(),
            ScreenHandle::Direct => {}
        }
    }

    /// Stop the capture thread without tearing down the shared `VideoSink`.
    /// See `ScreenCapture::stop_source`.
    pub fn stop_source(&mut self) {
        match self {
            ScreenHandle::Wgc(s) => s.stop_source(),
            ScreenHandle::Direct => {}
        }
    }

    pub fn video_sink(&self) -> Option<ezstreamer_core::video::VideoSink> {
        match self {
            ScreenHandle::Wgc(s) => Some(s.video_sink()),
            ScreenHandle::Direct => None,
        }
    }
}

/// Wrap a running capture for storage in the backend session state.
pub fn make_handle(s: ScreenCapture) -> ScreenHandle {
    ScreenHandle::Wgc(s)
}
