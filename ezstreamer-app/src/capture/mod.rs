//! Capture backends (design.md §3).
//!
//! Screen/window: WGC (Windows) or Portal ScreenCast + PipeWire (Linux) →
//! BGRA → FramePacer (profile-normalized) → GStreamer `appsrc`.
//! Audio: WASAPI (Windows) or PipeWire (Linux) → Rust Mixer → GStreamer
//! `appsrc` (F32LE 48kHz stereo).
//!
//! The platform backends need the `media` feature plus Windows/Linux. Every
//! other combination (e.g. `cargo check --no-default-features` on a dev box
//! without GStreamer headers) compiles the stub module instead, which exposes
//! the same API and fails with a clear error at runtime.

#![allow(dead_code)]

#[cfg(all(feature = "media", target_os = "linux"))]
pub mod linux;
#[cfg(all(feature = "media", windows))]
pub mod windows;

#[cfg(all(feature = "media", target_os = "linux"))]
pub use linux as platform;
#[cfg(all(feature = "media", windows))]
pub use windows as platform;

#[cfg(not(all(feature = "media", any(windows, target_os = "linux"))))]
pub mod stub;
#[cfg(not(all(feature = "media", any(windows, target_os = "linux"))))]
pub use stub as platform;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture backend requires Windows or Linux (this build is a stub)")]
    NotAvailable,
    #[error("capture failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// A sink writer that swallows frames (preview mode: the capture backend feeds
/// the UI itself; no pipeline involved).
pub fn null_file() -> std::fs::File {
    #[cfg(windows)]
    return std::fs::File::open("NUL").expect("NUL is always openable");
    #[cfg(not(windows))]
    return std::fs::File::open("/dev/null").expect("/dev/null is always openable");
}
