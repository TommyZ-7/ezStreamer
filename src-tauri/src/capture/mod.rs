//! Capture backends (design.md §3).
//!
//! Screen/window: WGC (Windows) or Portal ScreenCast + PipeWire (Linux) →
//! BGRA → FramePacer (profile-normalized) → GStreamer `appsrc`.
//! Audio: WASAPI (Windows) or PipeWire (Linux) → Rust Mixer → GStreamer
//! `appsrc` (F32LE 48kHz stereo).
//! Other hosts expose only the stub error so `cargo test` / `cargo check`
//! pass anywhere; CI builds Windows on `windows-latest` and Linux on
//! `ubuntu-latest` (Flatpak runtime at package time).

#![allow(dead_code)]

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux as platform;
#[cfg(windows)]
pub use windows as platform;

#[cfg(any(windows, target_os = "linux"))]
pub use platform::{AudioCapture, ScreenCapture};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture backend requires Windows (this build is a stub)")]
    NotAvailable,
    #[error("capture failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// Encode raw RGBA pixels as PNG (preview pipeline, design §6.4).
#[cfg(any(windows, target_os = "linux"))]
pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba.to_vec())?
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// A sink writer that swallows frames (preview mode: the capture backend feeds
/// `stream://preview` itself; no pipeline involved).
#[cfg(any(windows, target_os = "linux"))]
pub fn null_file() -> std::fs::File {
    #[cfg(windows)]
    return std::fs::File::open("NUL").expect("NUL is always openable");
    #[cfg(target_os = "linux")]
    return std::fs::File::open("/dev/null").expect("/dev/null is always openable");
}
