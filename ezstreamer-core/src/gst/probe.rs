//! Encoder discovery via the GStreamer registry (design.md §8.1).
//!
//! The old FFmpeg generation spawned `ffmpeg -encoders` + 1-frame test
//! encodes. Here discovery is a registry lookup (`gst::Registry::find_feature`)
//! with no child processes, so it is fast enough to run at every launch
//! while still cached next to `profiles.json`.

use crate::gst::pipeline::EncoderSpec;
use crate::ipc_types::EncoderInfo;

/// Auto-select priority (Windows-only): NVENC > QSV > AMF > VAAPI > x264.
/// `h264_vulkan` stays manual-only (requirements F-EN-03).
pub const AUTO_CANDIDATES: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf", "h264_vaapi"];

/// Manual-select list shown in the UI (vulkan included).
pub const MANUAL_ENCODERS: &[&str] = &[
    "auto",
    "libx264",
    "h264_nvenc",
    "h264_qsv",
    "h264_amf",
    "h264_vaapi",
    "h264_vulkan",
];

/// Map a UI id to usable info given the set of element factories present in
/// the registry. Pure function — the Tauri backend feeds it with
/// `gst::Registry::get().get_feature_list(...)` names; tests feed fixtures.
pub fn probe_with_elements(element_names: &[&str]) -> Vec<EncoderInfo> {
    let has = |names: &[&str]| names.iter().any(|n| element_names.contains(n));
    let mut out = Vec::new();
    for id in ["h264_nvenc", "h264_qsv", "h264_amf", "h264_vaapi", "h264_vulkan"] {
        let spec = EncoderSpec::from_id(id).expect("known id");
        if has(spec.gst_elements()) {
            out.push(EncoderInfo { name: id.into(), usable: true, reason: None });
        } else {
            out.push(EncoderInfo {
                name: id.into(),
                usable: false,
                reason: Some("GStreamer plugin not installed".into()),
            });
        }
    }
    out.push(EncoderInfo { name: "libx264".into(), usable: true, reason: None });
    out
}

/// Probe via a presence predicate (the Tauri backend passes a registry
/// lookup; tests pass fixtures). Software encoders are always assumed
/// present so the `libx264` fallback never disappears.
pub fn probe_with(check: impl Fn(&str) -> bool) -> Vec<EncoderInfo> {
    let mut elements = vec!["x264enc", "openh264enc"];
    for id in MANUAL_ENCODERS.iter().filter(|id| **id != "auto") {
        if let Some(spec) = EncoderSpec::from_id(id) {
            for e in spec.gst_elements() {
                if check(e) {
                    elements.push(e);
                }
            }
        }
    }
    probe_with_elements(&elements)
}
pub fn usable_ids(infos: &[EncoderInfo]) -> Vec<String> {
    let mut ids: Vec<String> = infos.iter().filter(|i| i.usable).map(|i| i.name.clone()).collect();
    ids.sort_by_key(|id| AUTO_CANDIDATES.iter().position(|c| c == id).unwrap_or(usize::MAX));
    ids
}

/// Best encoder id for `auto` (software fallback last).
pub fn pick_best(infos: &[EncoderInfo]) -> String {
    for cand in AUTO_CANDIDATES {
        if infos.iter().any(|i| i.name == *cand && i.usable) {
            return cand.to_string();
        }
    }
    "libx264".to_string()
}

/// Default probe result when the GStreamer runtime is absent (non-Windows
/// dev builds, CI frontend job): software only, no failure.
pub fn probe_encoders() -> Vec<EncoderInfo> {
    probe_with_elements(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_means_software_only() {
        let infos = probe_with_elements(&[]);
        assert_eq!(pick_best(&infos), "libx264");
        assert!(infos.iter().find(|i| i.name == "libx264").unwrap().usable);
        assert!(!infos.iter().find(|i| i.name == "h264_nvenc").unwrap().usable);
    }

    #[test]
    fn nvenc_wins_when_present() {
        let infos = probe_with_elements(&["nvh264enc", "x264enc", "vah264enc"]);
        assert_eq!(pick_best(&infos), "h264_nvenc");
        let ids = usable_ids(&infos);
        assert_eq!(ids[0], "h264_nvenc");
    }

    #[test]
    fn qsv_beats_amf() {
        let infos = probe_with_elements(&["qsvh264enc", "amfh264enc", "x264enc"]);
        assert_eq!(pick_best(&infos), "h264_qsv");
    }

    #[test]
    fn manual_list_contains_vulkan_but_auto_skips_it() {
        assert!(MANUAL_ENCODERS.contains(&"h264_vulkan"));
        assert!(!AUTO_CANDIDATES.contains(&"h264_vulkan"));
        let infos = probe_with_elements(&["vulkanh264enc"]);
        assert_eq!(pick_best(&infos), "libx264");
    }

    #[test]
    fn probe_with_predicate_maps_elements_to_ui_ids() {
        let infos = probe_with(|e| e == "nvh264enc" || e == "x264enc");
        assert!(infos.iter().find(|i| i.name == "h264_nvenc").unwrap().usable);
        assert!(!infos.iter().find(|i| i.name == "h264_qsv").unwrap().usable);
        assert_eq!(pick_best(&infos), "h264_nvenc");
    }

    #[test]
    fn probe_with_empty_predicate_still_offers_software() {
        let infos = probe_with(|_| false);
        assert_eq!(pick_best(&infos), "libx264");
    }
}
