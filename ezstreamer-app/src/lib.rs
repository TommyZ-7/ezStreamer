//! ezStreamer: native egui UI + capture/GStreamer backend.
//!
//! Layout:
//! - [`backend`] owns capture, the GStreamer pipeline and persisted config;
//! - [`ui`] is the immediate-mode egui application;
//! - [`capture`] holds the WGC/Portal/WASAPI/PipeWire backends (media feature);
//! - [`events`] is the thread-to-UI event channel replacing Tauri IPC.

pub mod backend;
pub mod capture;
pub mod events;
pub mod gst_preload;
pub mod logging;
pub mod ui;
