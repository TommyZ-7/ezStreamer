//! Stub pipeline for builds without the platform media stack
//! (`--no-default-features` / unsupported hosts). Same API surface as the
//! real `gst_stream` module so [`super`] compiles unchanged.

use ezstreamer_core::gst::StreamPlan;
use ezstreamer_core::ipc_types::{EncoderInfo, StreamStatus};

pub struct ExitStatus {
    success: bool,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.success
    }
}

pub struct GstStream {
    pub retry_count: u32,
}

impl GstStream {
    pub fn status(&self) -> StreamStatus {
        StreamStatus::default()
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        None
    }

    pub fn take_result(&mut self) -> Option<bool> {
        None
    }

    pub fn stop(&mut self) {}

    pub fn mark_retrying(&self, _n: u32) {}
}

pub fn spawn_pipeline(
    _plan: StreamPlan,
    _video_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    _audio_rx: std::sync::mpsc::Receiver<Vec<f32>>,
    _retry: u32,
) -> Result<GstStream, String> {
    Err("this build has no media backend (feature `media` is disabled)".into())
}

/// No bundled runtime to point at in a stub build.
pub fn ensure_bundled_runtime() {}

/// Stub builds have no registry: software fallback is reported unusable.
pub fn probe_encoder_infos() -> Vec<EncoderInfo> {
    vec![EncoderInfo {
        name: "libx264".into(),
        usable: false,
        reason: Some("this build has no media backend".into()),
    }]
}
