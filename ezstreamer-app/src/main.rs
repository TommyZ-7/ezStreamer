#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ezstreamer::ui::EzStreamerApp;

fn main() -> eframe::Result {
    // Must run before any GStreamer symbol is resolved (Windows NSIS bundle).
    #[cfg(all(windows, feature = "media"))]
    ezstreamer::gst_preload::preload_bundled_gstreamer_dlls();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ezStreamer")
            .with_inner_size([1020.0, 800.0])
            .with_min_inner_size([900.0, 660.0])
            .with_icon(EzStreamerApp::window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "ezStreamer",
        options,
        Box::new(|cc| Ok(Box::new(EzStreamerApp::new(cc)))),
    )
}
