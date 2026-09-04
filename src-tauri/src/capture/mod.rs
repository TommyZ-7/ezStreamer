//! Windows-only capture backend (design.md §3).
//!
//! Screen/window: WGC → BGRA → FramePacer (profile-normalized) → GStreamer
//! `appsrc`. Audio: WASAPI (system loopback / per-app process loopback / mic)
//! → Rust Mixer → GStreamer `appsrc` (F32LE 48kHz stereo).
//! On non-Windows hosts this module exposes only the stub error so
//! `cargo test` / `cargo check` pass anywhere; CI builds Windows on
//! `windows-latest`.

#![allow(dead_code)]

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::{AudioCapture, ScreenCapture};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture backend requires Windows (this build is a stub)")]
    NotAvailable,
    #[error("capture failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

/// Encode raw RGBA pixels as PNG (preview pipeline, design §6.4).
#[cfg(windows)]
pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba.to_vec())?
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// A sink writer that swallows frames (preview mode: the capture backend feeds
/// `stream://preview` itself; no pipeline involved).
#[cfg(windows)]
pub fn null_file() -> std::fs::File {
    std::fs::File::open("NUL").expect("NUL is always openable")
}
