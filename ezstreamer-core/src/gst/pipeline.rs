//! GStreamer pipeline plan (design.md §4.3).
//!
//! The plan is pure data (no GStreamer runtime needed) so `cargo test` runs
//! anywhere, including Linux CI. The Tauri backend materializes it with
//! gstreamer-rs on Windows.
//!
//! Topology (all elements in one `gst::Pipeline`):
//!
//! ```text
//! video_src(appsrc BGRA w×h@fps) → videoconvert → videoscale → capsfilter
//!   → queue → videoconvert → [vulkanupload] → <encoder> → h264parse → mux.
//! audio_src(appsrc F32LE 48k stereo, Rust Mixer output) → audioconvert →
//!   audioresample → capsfilter → queue → audioconvert → voaacenc/avenc_aac → aacparse → mux.
//! mux(flvmux streamable) → rtmp2sink location=rtmp://…/{key}
//! ```
//!
//! The converters after each `queue` adapt the pinned pacer caps
//! (BGRA / F32LE) to the encoder's accepted subset (`vah264enc`: NV12;
//! `faac`/`fdkaacenc`: S16LE). Without them the queue→encoder pad link
//! itself fails because link checks live caps, not just templates.
//!
//! Vulkan (`vulkanh264enc`) only: `vulkanupload` sits between the tail
//! videoconvert and the encoder, because the encoder sink is
//! `video/x-raw(memory:VulkanImage),format=NV12` (verified with GStreamer
//! 1.28 `gst-inspect` + `gst-launch`: BGRA caps → queue → videoconvert →
//! vulkanupload → vulkanh264enc encodes cleanly).

use crate::config::{validate_bitrate, Profile};
use crate::error::{Error, Result};

/// UI-facing encoder ids. Unchanged from the FFmpeg generation so saved
/// profiles and the manual-select list keep working. Each maps to one or
/// more GStreamer elements (first available wins at runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderSpec {
    Nvenc,
    Qsv,
    Amf,
    Vaapi,
    Vulkan,
    X264,
}

impl EncoderSpec {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "h264_nvenc" => Some(Self::Nvenc),
            "h264_qsv" => Some(Self::Qsv),
            "h264_amf" => Some(Self::Amf),
            "h264_vaapi" => Some(Self::Vaapi),
            "h264_vulkan" => Some(Self::Vulkan),
            "libx264" => Some(Self::X264),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::Qsv => "h264_qsv",
            Self::Amf => "h264_amf",
            Self::Vaapi => "h264_vaapi",
            Self::Vulkan => "h264_vulkan",
            Self::X264 => "libx264",
        }
    }

    /// Candidate GStreamer encoder elements, best first.
    pub fn gst_elements(self) -> &'static [&'static str] {
        match self {
            // NVIDIA: proprietary plugin name differs by Gst build; try both.
            Self::Nvenc => &["nvh264enc", "nvenc_h264enc"],
            Self::Qsv => &["qsvh264enc"],
            Self::Amf => &["amfh264enc"],
            Self::Vaapi => &["vah264enc", "vaapih264enc"],
            // Vulkan Video encode (GStreamer 1.28+, gst-plugins-bad `vulkan`).
            Self::Vulkan => &["vulkanh264enc"],
            Self::X264 => &["x264enc", "openh264enc"],
        }
    }

    /// True for encoders whose sink needs Vulkan device memory.
    /// The backend inserts `vulkanupload` between the tail videoconvert and
    /// the encoder (see topology note).
    pub fn needs_vulkan_upload(self) -> bool {
        matches!(self, Self::Vulkan)
    }

    /// Encoder element properties for Topaz-safe low latency:
    /// B-frames 0, strict 2s GOP, CBR at profile bitrate, high profile.
    /// Returned as (name, value) string pairs for `gst::Element::set_property_from_str`
    /// or `gst_parse_launch` fragments.
    pub fn gst_props(self, profile: &Profile) -> Vec<(String, String)> {
        let gop = profile.gop().to_string();
        let bitrate = profile.v_kbps.to_string();
        match self {
            Self::Nvenc => vec![
                // Requirement §2.2: Booth's "Max Performance" tuning. The
                // real GstNvEncoderPreset nick is `hp` (High Performance);
                // there is no `high-performance` member, and feeding that
                // string to `set_property_from_str` panicked at runtime
                // (review 2026-09-10). `hp` is deprecated since 1.22 in
                // favor of p1~7 + tune but still present in 1.28. The
                // `low-latency*` presets stay forbidden (VRChat gray-screen
                // regression).
                ("preset".into(), "hp".into()),
                ("rc-mode".into(), "cbr".into()),
                ("bitrate".into(), bitrate),
                ("gop-size".into(), gop),
                ("bframes".into(), "0".into()),
                // Booth low-latency tuning: look-ahead OFF, zerolatency off.
                ("rc-lookahead".into(), "0".into()),
                ("zerolatency".into(), "false".into()),
            ],
            // `b-frames` (hyphenated): `bframes` does not exist on
            // qsvh264enc/amfh264enc and was silently dropped by the
            // `has_property` guard.
            Self::Qsv | Self::Amf => vec![
                ("bitrate".into(), bitrate),
                ("gop-size".into(), gop),
                ("b-frames".into(), "0".into()),
                ("rate-control".into(), "cbr".into()),
            ],
            // Current `vah264enc` (1.28) uses the canonical names. The removed
            // `vaapih264enc` dialect is handled in `gst_props_for` below.
            Self::Vaapi => vec![
                ("bitrate".into(), bitrate),
                ("rate-control".into(), "cbr".into()),
                ("key-int-max".into(), gop),
                ("b-frames".into(), "0".into()),
            ],
            // `vulkanh264enc` sink is NV12 VulkanImage (see topology note).
            // It inherits `idr-period`/`b-frames` from `GstH264Encoder`
            // (GStreamer 1.28, not listed on the element doc page); it has no
            // `keyframe-period`/`gop-size`. `rate-control` defaults to `cqp`,
            // so CBR must be set explicitly for `bitrate` to apply.
            Self::Vulkan => vec![
                ("bitrate".into(), bitrate),
                ("rate-control".into(), "cbr".into()),
                ("idr-period".into(), gop),
                ("b-frames".into(), "0".into()),
            ],
            // x264enc takes kbit/s; tune zerolatency is BANNED (Topaz gray-screen
            // regression) — use sliced-threads/sync-lookahead/scene-cut off only.
            Self::X264 => vec![
                ("bitrate".into(), bitrate),
                ("key-int-max".into(), gop),
                ("bframes".into(), "0".into()),
                ("cabac".into(), "true".into()),
                ("pass".into(), "cbr".into()),
                (
                    "option-string".into(),
                    "sliced-threads=1:sync-lookahead=0:scenecut=0".into(),
                ),
            ],
        }
    }

    /// Encoder properties for a concrete element factory. The generic set
    /// above targets the primary element of each spec; dialects with
    /// different names/units are mapped here so `build_launch_string` stays
    /// runnable with `gst-launch-1.0` and the runtime `has_property` guard
    /// only skips genuinely absent extras:
    ///
    /// - `openh264enc`: bitrate is **bits/s** (not kbit/s) and the GOP
    ///   property is `gop-size` (feeding x264enc's kbit/s value there gave
    ///   ~1.5 kbps).
    /// - `vaapih264enc` (removed in GStreamer 1.26): `keyframe-period`/`bframes`.
    ///
    /// NOTE: none of the H.264 encoders in use exposes a `profile` property
    /// (it is a caps field, not an element property), so profiles are left to
    /// the encoder defaults negotiated by caps.
    pub fn gst_props_for(self, element: &str, profile: &Profile) -> Vec<(String, String)> {
        match (self, element) {
            (Self::X264, "openh264enc") => vec![
                ("bitrate".into(), (profile.v_kbps * 1000).to_string()),
                ("rate-control".into(), "bitrate".into()),
                ("gop-size".into(), profile.gop().to_string()),
            ],
            (Self::Vaapi, "vaapih264enc") => vec![
                ("bitrate".into(), profile.v_kbps.to_string()),
                ("keyframe-period".into(), profile.gop().to_string()),
                ("bframes".into(), "0".into()),
                ("rate-control".into(), "cbr".into()),
            ],
            _ => self.gst_props(profile),
        }
    }
}

/// Resolved, validated stream plan (bitrate guard applied).
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPlan {
    pub encoder: EncoderSpec,
    pub encoder_element: String,
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub v_kbps: u32,
    pub a_kbps: u32,
    pub gop: u32,
    pub rtmp_url: String,
}

impl StreamPlan {
    /// appsrc caps: always BGRA — the Rust FramePacer normalizes to packed
    /// BGRA; the tail videoconvert downstream converts (e.g. NV12 for
    /// Vulkan/VAAPI) before the encoder.
    pub fn video_caps(&self) -> String {
        format!(
            "video/x-raw,format=BGRA,width={},height={},framerate={}/1",
            self.w, self.h, self.fps
        )
    }

    pub fn audio_caps(&self) -> String {
        "audio/x-raw,format=F32LE,layout=interleaved,rate=48000,channels=2".to_string()
    }
}

/// Resolve encoder id (`auto`, empty/whitespace, or manual) to a spec.
/// `available` lists usable UI ids from [`crate::gst::probe_encoders`];
/// `auto` picks priority order.
///
/// Empty/whitespace means `auto` too: the backend's usable-list branch
/// already treated it that way, but `build_plan` used to reject it with
/// `EncoderNotAvailable("")` (review 2026-09-10, Low #3).
///
/// Manual selection is used as-is (design §8.1): only the id is validated,
/// registry presence is NOT gated here. A missing element fails later at
/// pipeline build with an actionable `no GStreamer element for …` message.
pub fn resolve_encoder(encoder_override: &str, available: &[String]) -> Result<EncoderSpec> {
    let id = encoder_override.trim();
    if id.is_empty() || id == "auto" {
        for cand in crate::gst::AUTO_CANDIDATES {
            if available.iter().any(|a| a == cand) {
                return EncoderSpec::from_id(cand)
                    .ok_or_else(|| Error::EncoderNotAvailable(cand.to_string()));
            }
        }
        return Ok(EncoderSpec::X264); // software fallback always exists
    }
    EncoderSpec::from_id(id).ok_or_else(|| Error::EncoderNotAvailable(id.to_string()))
}

/// Common lower bound accepted by every hardware encoder in use
/// (nvenc 160x64, amf 128x128, qsv 16x16): 160x128.
const MIN_HW_W: u32 = 160;
const MIN_HW_H: u32 = 128;

/// Reject unusable profile geometry before a pipeline is built. Without
/// this, unchecked UI values (fps=0, odd/1px sizes) surfaced as
/// `not-negotiated` / infinite GOP only after the stream started, then as
/// three retries before giving up; `Error::Config` is the actionable
/// failure (review 2026-09-10, Medium).
fn validate_profile(profile: &Profile) -> Result<()> {
    if profile.fps < 1 {
        return Err(Error::Config(format!(
            "profile fps must be >= 1 (got {})",
            profile.fps
        )));
    }
    if profile.w == 0
        || profile.h == 0
        || !profile.w.is_multiple_of(2)
        || !profile.h.is_multiple_of(2)
    {
        return Err(Error::Config(format!(
            "profile dimensions must be positive even numbers (got {}x{})",
            profile.w, profile.h
        )));
    }
    if profile.w < MIN_HW_W || profile.h < MIN_HW_H {
        return Err(Error::Config(format!(
            "profile {}x{} is below the minimum encoder size {}x{}",
            profile.w, profile.h, MIN_HW_W, MIN_HW_H
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn build_plan(
    profile: &Profile,
    encoder_override: &str,
    available: &[String],
    ingest_url: &str,
    stream_key: &str,
    preferred_element: Option<&str>,
) -> Result<StreamPlan> {
    validate_profile(profile)?;
    validate_bitrate(profile.v_kbps, profile.a_kbps)?;
    let key = stream_key.trim();
    // F-ST-01 full validation (charset + 3..=64) also protects the RTMP URL.
    crate::config::validate_stream_key(key)?;
    let ingest = ingest_url.trim().trim_end_matches('/');
    if !(ingest.starts_with("rtmp://") || ingest.starts_with("rtmps://")) {
        return Err(Error::Config(format!(
            "ingest URL must start with rtmp:// or rtmps://: {ingest_url}"
        )));
    }
    let spec = resolve_encoder(encoder_override, available)?;
    let element = preferred_element
        .map(|s| s.to_string())
        .or_else(|| spec.gst_elements().first().map(|s| s.to_string()))
        .unwrap_or_else(|| "x264enc".to_string());
    Ok(StreamPlan {
        encoder: spec,
        encoder_element: element,
        w: profile.w,
        h: profile.h,
        fps: profile.fps,
        v_kbps: profile.v_kbps,
        a_kbps: profile.a_kbps,
        gop: profile.gop(),
        rtmp_url: format!("{ingest}/{key}"),
    })
}

/// `gst-launch-1.0`-compatible launch string for debugging / E2E
/// (`e2e-stream-test` skill). The app builds the same topology via
/// gstreamer-rs element APIs; this string is the readable reference.
pub fn build_launch_string(plan: &StreamPlan) -> String {
    let enc_props = plan
        .encoder
        .gst_props_for(
            &plan.encoder_element,
            &Profile {
                name: String::new(),
                w: plan.w,
                h: plan.h,
                fps: plan.fps,
                v_kbps: plan.v_kbps,
                a_kbps: plan.a_kbps,
                encoder: "auto".into(),
                warn: None,
            },
        )
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    // Vulkan inserts `vulkanupload` between the tail videoconvert and the
    // encoder (see topology note). Verified working:
    // `... queue ! videoconvert ! vulkanupload ! vulkanh264enc ...`.
    let upload = if plan.encoder.needs_vulkan_upload() {
        " ! vulkanupload"
    } else {
        ""
    };
    format!(
        "appsrc name=video_src caps=\"{vcaps}\" is-live=true format=time \
         ! videoconvert ! videoscale \
         ! \"video/x-raw,width={w},height={h},framerate={fps}/1\" \
         ! queue ! videoconvert{upload} ! {enc} {props} ! h264parse ! mux. \
         appsrc name=audio_src caps=\"{acaps}\" is-live=true format=time \
         ! audioconvert ! audioresample ! queue ! audioconvert ! voaacenc bitrate={abps} ! aacparse ! mux. \
         flvmux name=mux streamable=true \
         ! rtmp2sink location=\"{url}\"",
        vcaps = plan.video_caps(),
        acaps = plan.audio_caps(),
        w = plan.w,
        h = plan.h,
        fps = plan.fps,
        upload = upload,
        enc = plan.encoder_element,
        props = enc_props,
        abps = plan.a_kbps * 1000,
        url = plan.rtmp_url,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;

    fn mid() -> Profile {
        Profile {
            name: "mid".into(),
            w: 1280,
            h: 720,
            fps: 30,
            v_kbps: 1500,
            a_kbps: 192,
            encoder: "auto".into(),
            warn: None,
        }
    }

    fn avail() -> Vec<String> {
        vec!["h264_nvenc".into(), "libx264".into()]
    }

    #[test]
    fn auto_prefers_nvenc_over_x264() {
        let p = build_plan(&mid(), "auto", &avail(), "rtmp://topaz.chat/live", "abc", None).unwrap();
        assert_eq!(p.encoder, EncoderSpec::Nvenc);
        assert_eq!(p.gop, 60);
        assert!(p.rtmp_url.ends_with("/abc"));
    }

    #[test]
    fn manual_selection_is_used_as_is_per_design() {
        // Design §8.1: manual bypasses the registry gate; a missing element
        // fails later at pipeline build, not here. Regression test for
        // `encoder not available: h264_vulkan` on manual select.
        let p = build_plan(&mid(), "h264_vulkan", &[], "rtmp://topaz.chat/live", "abc", None)
            .unwrap();
        assert_eq!(p.encoder, EncoderSpec::Vulkan);
        let p = build_plan(&mid(), "h264_qsv", &avail(), "rtmp://topaz.chat/live", "abc", None)
            .unwrap();
        assert_eq!(p.encoder, EncoderSpec::Qsv);
    }

    #[test]
    fn unknown_manual_id_is_rejected() {
        let err = build_plan(
            &mid(),
            "h264_nope",
            &avail(),
            "rtmp://topaz.chat/live",
            "abc",
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::EncoderNotAvailable(_)));
    }

    #[test]
    fn empty_or_blank_override_means_auto() {
        // Review Low #3: `""` used to reach `EncoderSpec::from_id` and fail
        // with `EncoderNotAvailable("")` while `start_stream` had built the
        // usable list as if the override were auto.
        for ov in ["", "   "] {
            let p = build_plan(&mid(), ov, &avail(), "rtmp://topaz.chat/live", "abc", None)
                .unwrap();
            assert_eq!(p.encoder, EncoderSpec::Nvenc, "override {ov:?}");
        }
    }

    #[test]
    fn invalid_profile_geometry_is_rejected_before_gst() {
        for bad in [
            Profile { fps: 0, ..mid() },
            Profile { w: 0, ..mid() },
            Profile { w: 1279, ..mid() },
            Profile { h: 721, ..mid() },
            Profile { w: 159, ..mid() }, // odd and below the HW floor
            Profile { h: 126, ..mid() }, // even but below the AMF floor
        ] {
            let err = build_plan(&bad, "auto", &avail(), "rtmp://topaz.chat/live", "abc", None)
                .unwrap_err();
            assert!(matches!(err, Error::Config(_)), "{bad:?} -> {err:?}");
        }
        // The common hardware lower bound is accepted.
        let min = Profile { w: 160, h: 128, ..mid() };
        assert!(build_plan(&min, "auto", &avail(), "rtmp://topaz.chat/live", "abc", None).is_ok());
    }

    #[test]
    fn builtin_profiles_pass_geometry_validation() {
        for p in crate::config::default_profiles().values() {
            build_plan(p, "auto", &avail(), "rtmp://topaz.chat/live", "abc", None)
                .unwrap_or_else(|e| panic!("builtin {}: {e}", p.name));
        }
    }

    #[test]
    fn bitrate_guard_fires_before_gst() {
        let mut bad = mid();
        bad.v_kbps = 2500;
        let err =
            build_plan(&bad, "auto", &avail(), "rtmp://topaz.chat/live", "abc", None).unwrap_err();
        assert!(matches!(err, Error::BitrateOver { .. }));
    }

    #[test]
    fn short_key_rejected() {
        let err =
            build_plan(&mid(), "auto", &avail(), "rtmp://topaz.chat/live", "ab", None).unwrap_err();
        assert!(matches!(err, Error::StreamKey(_)));
    }

    #[test]
    fn invalid_chars_and_bad_ingest_rejected() {
        let err = build_plan(
            &mid(),
            "auto",
            &avail(),
            "rtmp://topaz.chat/live",
            "bad key!",
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::StreamKey(_)));
        let err = build_plan(&mid(), "auto", &avail(), "http://x/live", "abc", None).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn nvenc_props_have_no_bframes_and_cbr() {
        let props = EncoderSpec::Nvenc.gst_props(&mid());
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("bframes"), Some("0"));
        assert_eq!(get("rc-mode"), Some("cbr"));
        // Requirements §2.2: "Low Latency" preset is forbidden (gray screen)
        // and the value must be a real GstNvEncoderPreset nick (`hp`, not the
        // non-existent `high-performance`; review 2026-09-10).
        assert_eq!(get("preset"), Some("hp"));
        assert_ne!(get("preset"), Some("low-latency"));
        assert_ne!(get("preset"), Some("high-performance"));
        assert_eq!(get("rc-lookahead"), Some("0"));
        assert_eq!(get("zerolatency"), Some("false"));
    }

    #[test]
    fn nvenc_preset_value_is_a_documented_nick() {
        // Guard against inventing a preset name again (review 2026-09-10:
        // `high-performance` is not a GstNvEncoderPreset member; the backend
        // skips unknown values at runtime, but silently losing the preset is
        // still a behavior bug). Keep this in sync with the published enum.
        const NVENC_PRESETS: &[&str] = &[
            "default",
            "hp",
            "hq",
            "low-latency",
            "low-latency-hq",
            "low-latency-hp",
            "lossless",
            "lossless-hp",
            "p1",
            "p2",
            "p3",
            "p4",
            "p5",
            "p6",
            "p7",
        ];
        let props = EncoderSpec::Nvenc.gst_props(&mid());
        let preset = props
            .iter()
            .find(|(k, _)| k == "preset")
            .map(|(_, v)| v.as_str())
            .expect("nvenc props carry a preset");
        assert!(
            NVENC_PRESETS.contains(&preset),
            "unknown GstNvEncoderPreset nick: {preset}"
        );
    }

    #[test]
    fn openh264enc_gets_bps_bitrate_and_gop() {
        let props = EncoderSpec::X264.gst_props_for("openh264enc", &mid());
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        // openh264enc bitrate is bits/s, not the kbit/s x264enc takes.
        assert_eq!(get("bitrate"), Some("1500000"));
        assert_eq!(get("rate-control"), Some("bitrate"));
        assert_eq!(get("gop-size"), Some("60"));
        // x264-only properties must not leak into the openh264 element.
        assert_eq!(get("key-int-max"), None);
        assert_eq!(get("option-string"), None);
        // x264enc keeps kbit/s and the tuned option string.
        let x264 = EncoderSpec::X264.gst_props_for("x264enc", &mid());
        assert!(x264.iter().any(|(k, v)| k == "bitrate" && v == "1500"));
        assert!(x264.iter().any(|(k, _)| k == "pass"));
    }

    #[test]
    fn qsv_and_amf_use_hyphenated_b_frames() {
        // Regression: `bframes` is not a qsvh264enc/amfh264enc property and
        // was silently dropped by `has_property` (official docs: `b-frames`).
        for (spec, element) in [(EncoderSpec::Qsv, "qsvh264enc"), (EncoderSpec::Amf, "amfh264enc")] {
            let props = spec.gst_props_for(element, &mid());
            let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            assert_eq!(get("b-frames"), Some("0"), "{element}");
            assert_eq!(get("bframes"), None, "{element} has no bframes property");
            assert_eq!(get("rate-control"), Some("cbr"), "{element}");
        }
    }

    #[test]
    fn vaapi_legacy_element_gets_legacy_dialect() {
        // `vaapih264enc` (removed in 1.26) uses keyframe-period/bframes;
        // `has_property` lets the same plan run on old and new runtimes.
        let legacy = EncoderSpec::Vaapi.gst_props_for("vaapih264enc", &mid());
        let get = |k: &str| legacy.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("keyframe-period"), Some("60"));
        assert_eq!(get("bframes"), Some("0"));
        assert_eq!(get("key-int-max"), None);
        let modern = EncoderSpec::Vaapi.gst_props_for("vah264enc", &mid());
        let get = |k: &str| modern.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("key-int-max"), Some("60"));
        assert_eq!(get("b-frames"), Some("0"));
        assert_eq!(get("keyframe-period"), None);
    }

    #[test]
    fn launch_string_props_are_valid_for_the_named_element() {
        // `keyframe-period`/`bframes` only exist on the removed vaapih264enc;
        // QSV/AMF/VAAPI/Vulkan must not emit them.
        for id in ["h264_qsv", "h264_amf", "h264_vaapi", "h264_vulkan"] {
            let p = build_plan(&mid(), id, &[], "rtmp://topaz.chat/live", "k123", None).unwrap();
            let s = build_launch_string(&p);
            assert!(!s.contains("keyframe-period"), "{id}: {s}");
            assert!(!s.contains("bframes="), "{id}: {s}");
            assert!(s.contains("b-frames=0"), "{id}: {s}");
        }
        // No GStreamer H.264 encoder exposes `profile` as a property (it is a
        // caps field): pinning it here would break gst-launch.
        for id in ["libx264", "h264_nvenc", "h264_qsv", "h264_amf", "h264_vaapi", "h264_vulkan"] {
            let p = build_plan(&mid(), id, &[], "rtmp://topaz.chat/live", "k123", None).unwrap();
            let s = build_launch_string(&p);
            assert!(!s.contains("profile="), "{id}: {s}");
        }
        let p = build_plan(&mid(), "h264_vulkan", &[], "rtmp://topaz.chat/live", "k123", None).unwrap();
        let s = build_launch_string(&p);
        assert!(s.contains("idr-period=60"), "{s}");
        let p = build_plan(&mid(), "h264_vaapi", &[], "rtmp://topaz.chat/live", "k123", None).unwrap();
        let s = build_launch_string(&p);
        assert!(s.contains("key-int-max=60"), "{s}");
    }

    #[test]
    fn x264_props_avoid_zerolatency_tune() {
        let s = build_launch_string(
            &build_plan(&mid(), "libx264", &["libx264".into()], "rtmp://topaz.chat/live", "k123", None)
                .unwrap(),
        );
        assert!(!s.contains("zerolatency"), "Topaz gray-screen regression");
        assert!(s.contains("scenecut=0"));
        assert!(s.contains("flvmux"));
        assert!(s.contains("rtmp2sink"));
    }

    #[test]
    fn launch_string_pins_profile_geometry() {
        let p =
            build_plan(&mid(), "auto", &avail(), "rtmp://topaz.chat/live", "k123", None).unwrap();
        let s = build_launch_string(&p);
        assert!(s.contains("width=1280"));
        assert!(s.contains("framerate=30/1"));
        assert!(s.contains("F32LE,layout=interleaved,rate=48000"));
    }

    #[test]
    fn vulkan_props_request_cbr_with_idr_gop() {
        let props = EncoderSpec::Vulkan.gst_props(&mid());
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("bitrate"), Some("1500"));
        assert_eq!(get("rate-control"), Some("cbr"));
        assert_eq!(get("idr-period"), Some("60"));
        assert_eq!(get("b-frames"), Some("0"));
    }

    #[test]
    fn vulkan_launch_string_inserts_upload_after_tail_convert() {
        let p = build_plan(
            &mid(),
            "h264_vulkan",
            &[],
            "rtmp://topaz.chat/live",
            "k123",
            None,
        )
        .unwrap();
        // appsrc stays BGRA (FramePacer output); the tail videoconvert +
        // vulkanupload adapt to NV12 VulkanImage before the encoder.
        assert!(p.video_caps().contains("format=BGRA"));
        let s = build_launch_string(&p);
        assert!(s.contains("queue ! videoconvert ! vulkanupload ! vulkanh264enc"));
    }

    #[test]
    fn non_vulkan_launch_string_has_no_upload() {
        let p =
            build_plan(&mid(), "auto", &avail(), "rtmp://topaz.chat/live", "k123", None).unwrap();
        let s = build_launch_string(&p);
        assert!(!s.contains("vulkanupload"));
        assert!(s.contains("queue ! videoconvert !"));
    }
}
