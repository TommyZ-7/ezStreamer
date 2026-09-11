---
name: egui UI Debug
description: Diagnose ezStreamer egui UI failures (blank window, missing preview frames, tofu Japanese text). Use when the native window misbehaves on any platform.
---

## Symptom A: blank / black window, app process alive

Cause: no usable OpenGL context (eframe `glow` backend). Common on Linux
without GPU access, in VMs, or in Flatpak without `--device=dri`.

- Try software rendering: `LIBGL_ALWAYS_SOFTWARE=1 cargo run -p ezstreamer`
- Flatpak: confirm the manifest still carries `--device=dri`.
- Wayland/X11: `cargo run -p ezstreamer` on the developer machine should
  print the winit/glutin error; `EGUI_…` env vars from eframe/egui can add
  logs (`RUST_LOG=debug` is not wired, use `eprintln!`/`logging`).
- `cargo check -p ezstreamer --no-default-features` isolates whether the
  failure is in the UI build vs the media stack.

## Symptom B: Japanese text renders as boxes (tofu)

Cause: the CJK fallback font failed to load.

- Check `ezstreamer-app/assets/fonts/NotoSansJP-Regular.otf` exists and is
  embedded by `include_bytes!` in `ui/theme.rs::install_fonts`.
- The emoji fonts are removed on purpose (design rule); do not re-add them
  to "fix" missing glyphs.
- Verify with `cargo test -p ezstreamer --no-default-features` (i18n catalog
  test also rejects emoji strings).

## Symptom C: capture session runs (OS capture indicator visible) but the preview stays empty

Cause: capture backends push preview RGBA through `UiSink` →
`UiEvent::Preview`; the UI uploads it as an egui texture.

- Errors from capture threads arrive as `UiEvent::Error` and surface as a
  red toast plus `state.last_error` in the dock; check the toast first.
- Preview is 1 fps by design (F-SC-03) — a static image is expected.
- Check `~/.config/ezStreamer/logs/` (Linux) or
  `%APPDATA%/ezStreamer/logs/` (Windows) for `pw video` / `wgc capture`
  lines.
- Linux: `start_portal_picker` must succeed before `start_preview`
  (`PORTAL` session is reused); re-pick after changing cursor mode.

## General

- All backend communication is one `mpsc` channel drained by
  `EzStreamerApp::drain_events`; if an error is invisible, check that the
  event variant is handled and not dropped in `backend/mod.rs`.
- The `media` feature is on by default; without it every capture/pipeline
  action returns a clear "this build has no media backend" error instead of
  failing silently.
