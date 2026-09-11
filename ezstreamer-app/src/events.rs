//! Events pushed from capture/pipeline threads to the egui UI thread, and the
//! cloneable sink handed to those threads.
//!
//! Replaces the Tauri `emit`/`listen` bridge (design v0.3 §5): one bounded-ish
//! `mpsc` channel owned by the UI. Preview frames travel as raw RGBA (the UI
//! uploads them to an egui texture); PNG/base64 encoding is gone.

use ezstreamer_core::config::ProfilesConfig;
use ezstreamer_core::ipc_types::{AudioDevices, Display, EncoderInfo, ScreenTarget, WindowInfo};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone, Debug)]
pub enum UiEvent {
    Config(Box<ProfilesConfig>),
    Displays(Vec<Display>),
    Windows(Vec<WindowInfo>),
    AudioDevices(Box<AudioDevices>),
    Encoders(Vec<EncoderInfo>),
    Preview { rgba: Arc<Vec<u8>>, w: u32, h: u32 },
    StreamStarted,
    StreamStopped,
    PreviewStarted,
    PreviewStopped,
    PortalPicked(ScreenTarget),
    Toast(String),
    Error(String),
}

/// Cloneable, `Send + Sync` sink. Capture callbacks run on RT-ish threads:
/// failures must never panic there, so every send is best-effort.
#[derive(Clone)]
pub struct UiSink {
    tx: Arc<Mutex<mpsc::Sender<UiEvent>>>,
}

impl UiSink {
    pub fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self {
            tx: Arc::new(Mutex::new(tx)),
        }
    }

    pub fn send(&self, event: UiEvent) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(event);
        }
    }

    pub fn preview(&self, rgba: Vec<u8>, w: u32, h: u32) {
        self.send(UiEvent::Preview {
            rgba: Arc::new(rgba),
            w,
            h,
        });
    }

    pub fn error(&self, code: &str, msg: impl std::fmt::Display) {
        self.send(UiEvent::Error(format!("{code}: {msg}")));
    }

    pub fn toast(&self, msg: impl Into<String>) {
        self.send(UiEvent::Toast(msg.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UiSink>();
    }

    #[test]
    fn sends_preview_as_raw_rgba() {
        let (tx, rx) = mpsc::channel();
        let sink = UiSink::new(tx);
        sink.preview(vec![1, 2, 3, 4], 1, 1);
        match rx.try_recv().unwrap() {
            UiEvent::Preview { rgba, w, h } => {
                assert_eq!(*rgba, vec![1, 2, 3, 4]);
                assert_eq!((w, h), (1, 1));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
