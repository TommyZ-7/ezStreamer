#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
#[cfg(windows)]
mod gst_preload;
mod ipc;
mod logging;

use ipc::commands::AppState;

fn main() {
    #[cfg(windows)]
    gst_preload::preload_bundled_gstreamer_dlls();
    let app = tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            ipc::commands::ping,
            ipc::commands::get_displays,
            ipc::commands::get_windows,
            ipc::commands::start_portal_picker,
            ipc::commands::get_audio_devices,
            ipc::commands::get_profiles,
            ipc::commands::save_profiles,
            ipc::commands::probe_encoders,
            ipc::commands::start_stream,
            ipc::commands::stop_stream,
            ipc::commands::start_preview,
            ipc::commands::stop_preview,
            ipc::commands::get_status,
            ipc::commands::update_audio_mix,
            ipc::commands::get_vu,
            ipc::commands::copy_to_clipboard,
            ipc::commands::open_logs_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ezStreamer");
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            ipc::commands::shutdown(app_handle);
        }
    });
}
