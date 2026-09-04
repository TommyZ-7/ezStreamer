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
}
