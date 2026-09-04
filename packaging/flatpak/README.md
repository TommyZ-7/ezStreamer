# Linux / Flatpak builds

The Linux backend (Portal ScreenCast + PipeWire capture, GStreamer encode)
lives behind `cfg(target_os = "linux")`. At package time the GNOME runtime
provides GStreamer + PipeWire + WebKitGTK; on a dev machine install the
system equivalents (Ubuntu 24.04):

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libpipewire-0.3-dev libspa-0.2-dev libclang-dev
cargo test -p ezstreamer-core
cargo check -p ezstreamer --tests
pnpm build && pnpm test
```

Runtime needs a Wayland session with `xdg-desktop-portal` (+ a
compositor backend) and PipeWire. Screen selection goes through the OS
picker (`start_portal_picker`); there is no app-side window list.

## Flatpak bundle

```bash
pnpm install && pnpm build   # dist/ is tauri frontendDist
mkdir -p .cargo && cargo vendor >> .cargo/config.toml  # sandbox is offline
flatpak-builder --force-clean --repo=repo build-dir \
  packaging/flatpak/app.ezstreamer.desktop.yaml
flatpak build-bundle repo ezstreamer.flatpak app.ezstreamer.desktop
flatpak install --user ezstreamer.flatpak
```

Manifest pins `org.gnome.Platform//50`. Bump `runtime-version` (and the CI
SDK install in `release.yml`) together when moving to a newer GNOME branch.
