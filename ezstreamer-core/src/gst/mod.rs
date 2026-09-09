//! GStreamer pipeline construction / probe / supervision (design.md §4).
//!
//! Capture stays in Rust (WGC screen + WASAPI audio + Rust Mixer/FramePacer,
//! same as before). GStreamer replaces the old FFmpeg sidecar and owns only:
//! encode (H.264) + mux (FLV) + RTMP send. Feeds enter via `appsrc`:
//! video `BGRA` (profile-normalized) and audio `F32LE 48kHz stereo`.

pub mod pipeline;
pub mod probe;
pub mod supervisor;

pub use pipeline::{build_launch_string, build_plan, EncoderSpec, StreamPlan};
pub use probe::{pick_best, probe_encoders, probe_with, AUTO_CANDIDATES, MANUAL_ENCODERS};
pub use supervisor::{
    bundled_bin_dir, bundled_plugin_dir, retry_backoff_ms, runtime_search_roots, MAX_RETRIES,
};
