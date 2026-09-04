---
name: E2E Stream Test
description: Manually verify ezStreamer end-to-end on a real Windows machine (capture, streaming, output quality). Use when validating a preview build before or after release.
---

## Prerequisites

- Windows 10 2004+ / 11 (only supported target).
- GStreamer MSVC runtime installed (dev machine) or the NSIS bundle (AC-11 clean-machine path).
- `ffprobe` available for output verification (any ffmpeg distribution).

## Workflow

1. Optional pre-flight without the app: print the planned pipeline
   (`build_launch_string` in `ezstreamer-core/src/gst/pipeline.rs`) and dry-run
   the fixed part with `gst-launch-1.0`:
   `videotestsrc ! x264enc ... ! flvmux ! rtmp2sink location=...`
2. Launch the app → screen picker → start streaming.
3. Verify the output stream: `ffprobe rtspt://topaz.chat/live/<key>`
4. Acceptance (requirements AC-04/05/06/08/11, "high" profile):
   video 2000kbps ±10%, audio 320kbps, 60fps, yuv420p, GOP 2s.
   AC-11: repeat step 2 on a clean VM with only the NSIS installer
   (bundled `resources/gstreamer/` must suffice, no system runtime).
5. On failure, inspect `%APPDATA%/ezStreamer/logs/ezStreamer-YYYY-MM-DD.log`
   (GStreamer bus ERROR/EOS and retry history are logged there).

## References

- `docs/impl-report.md` §4 (remaining tasks), `docs/design.md` §9 (errors),
  `docs/requirements.md` §10 (AC-01〜11).
