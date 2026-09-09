//! Pipeline supervision: retry policy + runtime path checks (design.md §9).
//!
//! The live `gst::Pipeline` lives in the Tauri backend (Windows). This module
//! keeps the platform-independent policy: retry budget, backoff, and the
//! GStreamer-runtime locator used by CI diagnostics and error messages.

/// F-ST-04: max automatic reconnects after an abnormal bus EOS/ERROR.
pub const MAX_RETRIES: u32 = 3;

/// Exponential backoff 1s/2s/4s, capped at 16s (same as the FFmpeg generation).
pub fn retry_backoff_ms(retry: u32) -> u64 {
    1000u64 << retry.min(4)
}

/// Standard Windows install roots for the GStreamer MSVC runtime
/// (see `docs/design.md` §13.2). The backend probes these plus
/// `GSTREAMER_1_0_ROOT_MSVC_X86_64`.
pub fn runtime_search_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(env) = std::env::var_os("GSTREAMER_1_0_ROOT_MSVC_X86_64") {
        roots.push(std::path::PathBuf::from(env));
    }
    for base in [
        "C:\\gstreamer\\1.0\\msvc_x86_64",
        "C:\\Program Files\\gstreamer\\1.0\\msvc_x86_64",
    ] {
        roots.push(std::path::PathBuf::from(base));
    }
    roots
}

/// Bundled runtime layout (Release NSIS, design §13.2).
///
/// Tauri `$RESOURCE` is the exe dir on Windows. Array-notation
/// `resources/gstreamer/**/*` installs to `<exe>/resources/gstreamer/...`
/// (preview.01 layout); map-notation installs to `<exe>/gstreamer/...`.
/// Probe both so old installs and future bundles resolve.
fn bundled_candidate_dirs(resource_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![
        resource_dir.join("gstreamer"),
        resource_dir.join("resources").join("gstreamer"),
    ]
}

/// Bundled `bin/` dir if present (must contain the core DLL, otherwise the
/// staging is incomplete and we fall back to the system runtime).
pub fn bundled_bin_dir(resource_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    for root in bundled_candidate_dirs(resource_dir) {
        let bin = root.join("bin");
        if bin.join("gstreamer-1.0-0.dll").is_file() {
            return Some(bin);
        }
    }
    None
}

/// Bundled plugin dir (`lib/gstreamer-1.0`) if present.
pub fn bundled_plugin_dir(resource_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    for root in bundled_candidate_dirs(resource_dir) {
        let plugins = root.join("lib").join("gstreamer-1.0");
        if plugins.is_dir() {
            return Some(plugins);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_and_capped() {
        assert_eq!(retry_backoff_ms(0), 1000);
        assert_eq!(retry_backoff_ms(1), 2000);
        assert_eq!(retry_backoff_ms(2), 4000);
        assert_eq!(retry_backoff_ms(9), 16000);
    }

    #[test]
    fn env_root_is_preferred() {
        std::env::set_var("GSTREAMER_1_0_ROOT_MSVC_X86_64", "C:\\fake\\gst");
        let roots = runtime_search_roots();
        assert_eq!(roots[0], std::path::PathBuf::from("C:\\fake\\gst"));
        std::env::remove_var("GSTREAMER_1_0_ROOT_MSVC_X86_64");
    }

    #[test]
    fn bundled_layout_prefers_core_dll_and_supports_preview01_nesting() {
        let tmp = std::env::temp_dir().join(format!(
            "ezs-bundled-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // preview.01: <exe>/resources/gstreamer/bin + lib/...
        let bin = tmp.join("resources").join("gstreamer").join("bin");
        let plugins = tmp
            .join("resources")
            .join("gstreamer")
            .join("lib")
            .join("gstreamer-1.0");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&plugins).unwrap();
        assert_eq!(bundled_bin_dir(&tmp), None);
        std::fs::write(bin.join("gstreamer-1.0-0.dll"), b"fake").unwrap();
        assert_eq!(bundled_bin_dir(&tmp), Some(bin.clone()));
        assert_eq!(bundled_plugin_dir(&tmp), Some(plugins.clone()));
        // Future map-notation: <exe>/gstreamer/... takes precedence when complete.
        let flat_bin = tmp.join("gstreamer").join("bin");
        std::fs::create_dir_all(&flat_bin).unwrap();
        std::fs::write(flat_bin.join("gstreamer-1.0-0.dll"), b"fake").unwrap();
        assert_eq!(bundled_bin_dir(&tmp), Some(flat_bin));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn bundled_layout_absent_returns_none() {
        let tmp = std::env::temp_dir().join(format!(
            "ezs-bundled-empty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(bundled_bin_dir(&tmp), None);
        assert_eq!(bundled_plugin_dir(&tmp), None);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
