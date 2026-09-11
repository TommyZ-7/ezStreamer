//! Playback URL + stream key helpers (requirements F-URL-01/02, F-ST-01).
//!
//! The RTMP ingest URL maps to the TopazChat playback URLs:
//! `rtspt://` for PC (TCP interleaved RTSP, low latency) and `rtsp://` for
//! Quest. Pure string logic so it is testable on any host.

/// Build `(pc_url, quest_url)` from an ingest URL and stream key.
pub fn playback_urls(ingest_url: &str, key: &str) -> (String, String) {
    let base = ingest_url.strip_prefix("rtmp://").unwrap_or(ingest_url);
    let base = base.trim_end_matches('/');
    (
        format!("rtspt://{base}/{key}"),
        format!("rtsp://{base}/{key}"),
    )
}

/// Generic keys risk colliding with other people's streams (requirements §2.3).
pub const GENERIC_KEYS: &[&str] = &["test", "music", "live", "stream", "key", "vrchat"];

/// True when the key is one of the known collision-prone generic keys.
pub fn is_generic_key(key: &str) -> bool {
    GENERIC_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_urls_follow_topaz_scheme() {
        let (pc, quest) = playback_urls("rtmp://topaz.chat/live", "my-key");
        assert_eq!(pc, "rtspt://topaz.chat/live/my-key");
        assert_eq!(quest, "rtsp://topaz.chat/live/my-key");
    }

    #[test]
    fn playback_urls_tolerate_trailing_slash_and_plain_host() {
        let (pc, _) = playback_urls("rtmp://example.test/live/", "k");
        assert_eq!(pc, "rtspt://example.test/live/k");
        let (pc, _) = playback_urls("example.test/live", "k");
        assert_eq!(pc, "rtspt://example.test/live/k");
    }

    #[test]
    fn generic_keys_are_flagged_case_insensitively() {
        assert!(is_generic_key("test"));
        assert!(is_generic_key("VRChat"));
        assert!(!is_generic_key("my-event-123"));
    }
}
