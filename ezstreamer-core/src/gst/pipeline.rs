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
//!   → queue → videoconvert → <encoder> → h264parse → mux.
//! audio_src(appsrc F32LE 48k stereo, Rust Mixer output) → audioconvert →
//!   audioresample → capsfilter → queue → audioconvert → voaacenc/avenc_aac → aacparse → mux.
//! mux(flvmux streamable) → rtmp2sink location=rtmp://…/{key}
//! ```
//!
//! The converters after each `queue` adapt the pinned pacer caps
//! (BGRA / F32LE) to the encoder's accepted subset (`vah264enc`: NV12;
//! `faac`/`fdkaacenc`: S16LE). Without them the queue→encoder pad link
//! itself fails because link checks live caps, not just templates.

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
            // No stable Vulkan H.264 encoder element across Windows builds;
            // kept for UI parity, resolved at probe time (usually unavailable).
            Self::Vulkan => &["vulkanh264enc"],
            Self::X264 => &["x264enc", "openh264enc"],
        }
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
                ("preset".into(), "low-latency".into()),
                ("rc-mode".into(), "cbr".into()),
                ("bitrate".into(), bitrate),
                ("gop-size".into(), gop),
                ("bframes".into(), "0".into()),
            ],
            Self::Qsv => vec![
                ("bitrate".into(), bitrate),
                ("gop-size".into(), gop),
                ("bframes".into(), "0".into()),
                ("rate-control".into(), "cbr".into()),
            ],
            Self::Amf => vec![
                ("bitrate".into(), bitrate),
                ("gop-size".into(), gop),
                ("bframes".into(), "0".into()),
                ("rate-control".into(), "cbr".into()),
            ],
            Self::Vaapi | Self::Vulkan => vec![
                ("bitrate".into(), bitrate),
                ("keyframe-period".into(), gop),
                ("bframes".into(), "0".into()),
            ],
            // x264enc takes kbit/s; tune zerolatency is BANNED (Topaz gray-screen
            // regression) — use sliced-threads/sync-lookahead/scene-cut off only.
            Self::X264 => vec![
                ("bitrate".into(), bitrate),
                ("key-int-max".into(), gop),
                ("bframes".into(), "0".into()),
                (" cabac".trim().into(), "true".into()),
                (
                    "option-string".into(),
                    "sliced-threads=1:sync-lookahead=0:scenecut=0".into(),
                ),
            ],
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
    pub fn video_caps(&self) -> String {
        format!(
            "video/x-raw,format=BGRA,width={},height={},framerate={}/1",
            self.w, self.h, self.fps
        )
    }

    pub fn audio_caps(&self) -> String {
        "audio/x-raw,format=F32LE,rate=48000,channels=2".to_string()
    }
}

/// Resolve encoder id (`auto` or manual) to a spec. `available` lists usable
/// UI ids from [`crate::gst::probe_encoders`]; `auto` picks priority order.
pub fn resolve_encoder(encoder_override: &str, available: &[String]) -> Result<EncoderSpec> {
    if encoder_override == "auto" {
        for cand in crate::gst::AUTO_CANDIDATES {
            if available.iter().any(|a| a == cand) {
                return EncoderSpec::from_id(cand)
                    .ok_or_else(|| Error::EncoderNotAvailable(cand.to_string()));
            }
        }
        return Ok(EncoderSpec::X264); // software fallback always exists
    }
    let spec = EncoderSpec::from_id(encoder_override)
        .ok_or_else(|| Error::EncoderNotAvailable(encoder_override.to_string()))?;
    if available.iter().any(|a| a == spec.id()) || spec == EncoderSpec::X264 {
        Ok(spec)
    } else {
        Err(Error::EncoderNotAvailable(encoder_override.to_string()))
    }
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
    validate_bitrate(profile.v_kbps, profile.a_kbps)?;
    let key = stream_key.trim();
    if key.len() < 3 {
        return Err(Error::StreamKey("key must be at least 3 chars".into()));
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
        rtmp_url: format!("{}/{}", ingest_url.trim_end_matches('/'), key),
    })
}

/// `gst-launch-1.0`-compatible launch string for debugging / E2E
/// (`e2e-stream-test` skill). The app builds the same topology via
/// gstreamer-rs element APIs; this string is the readable reference.
pub fn build_launch_string(plan: &StreamPlan) -> String {
    let enc_props = plan
        .encoder
        .gst_props(&Profile {
            name: String::new(),
            w: plan.w,
            h: plan.h,
            fps: plan.fps,
            v_kbps: plan.v_kbps,
            a_kbps: plan.a_kbps,
            encoder: "auto".into(),
            warn: None,
        })
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "appsrc name=video_src caps=\"{vcaps}\" is-live=true format=time \
         ! videoconvert ! videoscale \
         ! \"video/x-raw,width={w},height={h},framerate={fps}/1\" \
         ! queue ! videoconvert ! {enc} {props} ! h264parse ! mux. \
         appsrc name=audio_src caps=\"{acaps}\" is-live=true format=time \
         ! audioconvert ! audioresample ! queue ! audioconvert ! voaacenc bitrate={abps} ! aacparse ! mux. \
         flvmux name=mux streamable=true \
         ! rtmp2sink location=\"{url}\"",
        vcaps = plan.video_caps(),
        acaps = plan.audio_caps(),
        w = plan.w,
        h = plan.h,
        fps = plan.fps,
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
    fn manual_unavailable_is_rejected() {
        let err =
            build_plan(&mid(), "h264_qsv", &avail(), "rtmp://topaz.chat/live", "abc", None).unwrap_err();
        assert!(matches!(err, Error::EncoderNotAvailable(_)));
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
    fn nvenc_props_have_no_bframes_and_cbr() {
        let props = EncoderSpec::Nvenc.gst_props(&mid());
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("bframes"), Some("0"));
        assert_eq!(get("rc-mode"), Some("cbr"));
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
        assert!(s.contains("F32LE,rate=48000"));
    }
}
