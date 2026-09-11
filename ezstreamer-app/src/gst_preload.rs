//! Early GStreamer DLL preload (Windows; design §4.4).
//!
//! `gstreamer-rs` links its `-sys` crates at load time. The bundled runtime
//! lives in `resources/gstreamer/{bin,lib/gstreamer-1.0}` beside the exe
//! (Tauri `$RESOURCE` == exe dir on Windows), which the loader does NOT
//! search before `main()`. `build.rs` marks those imports `/DELAYLOAD`, and
//! this module adds the bundled `bin/` to the DLL search path + preloads the
//! core DLLs with full paths before the first `gst::init()`.
//!
//! Must run as the first statement in `main()`. No-op on non-Windows, in dev
//! (no bundle layout → system runtime), and when already loaded.

#[cfg(all(windows, feature = "media"))]
pub fn preload_bundled_gstreamer_dlls() {
    let Some((bin, plugins)) = find_bundled_dirs() else {
        return;
    };

    unsafe {
        use windows::core::HSTRING;
        use windows::Win32::System::LibraryLoader::{
            AddDllDirectory, LoadLibraryW, SetDefaultDllDirectories, SetDllDirectoryW,
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        };

        // Make AddDllDirectory entries effective for subsequent loads.
        let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);

        // Legacy search path (PATH + SetDllDirectory) for the plugin scanner
        // child process + older loaders; AddDllDirectory for this process.
        // HSTRING::from(&Path) exists in windows-core 0.52; &HSTRING
        // implements IntoParam<PCWSTR>.
        let bin_str = HSTRING::from(bin.as_path());
        let _ = SetDllDirectoryW(&bin_str);
        AddDllDirectory(&bin_str);

        // PATH prepend so `gst-plugin-scanner` + transitive loads find bin/.
        let mut paths = vec![bin.clone()];
        if let Some(p) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&p));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            std::env::set_var("PATH", joined);
        }
        std::env::set_var("GST_PLUGIN_PATH", &plugins);

        // Preload in dependency order with explicit full paths so the loader
        // binds from the bundle even if a stray system copy exists. Failures
        // are ignored here — the later `gst::init()` returns the actionable
        // error to the UI.
        for name in [
            "glib-2.0-0.dll",
            "gthread-2.0-0.dll",
            "gmodule-2.0-0.dll",
            "gobject-2.0-0.dll",
            "gio-2.0-0.dll",
            "gstreamer-1.0-0.dll",
            "gstbase-1.0-0.dll",
            "gstvideo-1.0-0.dll",
            "gstaudio-1.0-0.dll",
            "gstapp-1.0-0.dll",
        ] {
            let full = bin.join(name);
            if full.is_file() {
                let s = HSTRING::from(full.as_path());
                let _ = LoadLibraryW(&s);
            }
        }
    }
}

/// Locate the bundled runtime from the exe directory (the NSIS installer
/// places `gstreamer/` next to the exe). Checks both the flat
/// `<exe>/gstreamer/...` and preview.01 `<exe>/resources/gstreamer/...`
/// layouts via `ezstreamer-core` (tested there).
#[cfg(all(windows, feature = "media"))]
fn find_bundled_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let bin = ezstreamer_core::gst::bundled_bin_dir(exe_dir)?;
    let plugins = ezstreamer_core::gst::bundled_plugin_dir(exe_dir)?;
    Some((bin, plugins))
}

#[cfg(not(all(windows, feature = "media")))]
pub fn preload_bundled_gstreamer_dlls() {}
