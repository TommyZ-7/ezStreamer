//! Windows MSVC: delay-load the GStreamer/GLib DLLs that gstreamer-rs links.
//!
//! The bundled runtime lives in `resources/gstreamer/bin` beside the exe
//! (NSIS layout), which the Windows loader does not search at process startup.
//! Without `/DELAYLOAD` the exe fails before `main()` on machines without a
//! system GStreamer install ("gobject-2.0-0.dll not found"),
//! so `gst_preload::preload_bundled_gstreamer_dlls()` never gets the chance to
//! add the bundled dir to the search path (design §4.4/§13.2).
//!
//! This file was lost in the Tauri→egui migration (preview.10 regression:
//! previously `src-tauri/build.rs`). The release workflow now verifies with
//! `dumpbin /dependents` that these DLLs are not static imports.
//!
//! Only direct Rust-linked imports need DELAYLOAD; their same-directory
//! dependencies (intl/pcre2/ffi/zlib/ssl/orc/...) resolve from the loading
//! DLL's directory once the core DLLs are preloaded via LoadLibraryW.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        for dll in [
            "glib-2.0-0.dll",
            "gthread-2.0-0.dll",
            "gmodule-2.0-0.dll",
            "gobject-2.0-0.dll",
            "gio-2.0-0.dll",
            "gstreamer-1.0-0.dll",
            "gstbase-1.0-0.dll",
            "gstapp-1.0-0.dll",
        ] {
            println!("cargo:rustc-link-arg=/DELAYLOAD:{dll}");
        }
        // delayimp.lib provides __delayLoadHelper2. Both forms: link-lib
        // (no kind — `dylib=` is dropped by cargo for the bin target) and
        // an explicit link-arg passthrough so link.exe always sees it.
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }
}
