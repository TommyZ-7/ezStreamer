fn main() {
    tauri_build::build();

    // Windows MSVC: delay-load GStreamer/GLib DLLs so the bundled runtime in
    // resources/gstreamer/bin can be added to the DLL search path before the
    // first call. Without this the loader resolves imports at process startup
    // (before main/PATH patching) and clean machines fail with
    // "gstreamer-1.0-0.dll not found" (preview.01).
    //
    // Only direct Rust-linked imports need DELAYLOAD; their same-dir
    // dependencies (intl/pcre2/ffi/zlib/ssl/...) resolve from the loading
    // DLL's directory once the core DLLs are preloaded via LoadLibraryW.
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
            "gstvideo-1.0-0.dll",
            "gstaudio-1.0-0.dll",
            "gsttag-1.0-0.dll",
            "gstpbutils-1.0-0.dll",
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
