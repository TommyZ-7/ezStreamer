# Linux / Flatpak builds

The Linux backend (Portal ScreenCast + PipeWire capture, GStreamer encode)
lives behind `cfg(target_os = "linux")` + the default `media` feature. At
package time the GNOME runtime provides GStreamer + PipeWire + GL/EGL; on a
dev machine install the system equivalents (Ubuntu 24.04):

```bash
sudo apt-get install -y \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  pipewire libpipewire-0.3-dev libspa-0.2-dev libclang-dev \
  libxkbcommon-dev libwayland-dev
cargo test -p ezstreamer-core
cargo check -p ezstreamer --tests
cargo test -p ezstreamer --no-default-features  # UI-only, no media stack
```

The egui UI itself needs no Node/pnpm toolchain; `cargo check -p ezstreamer
--no-default-features` type-checks it on hosts without GStreamer headers.

Runtime needs a Wayland session with `xdg-desktop-portal` (+ a
compositor backend) and PipeWire. Screen selection goes through the OS
picker (Portal); there is no app-side window list.

## Flatpak bundle

```bash
mkdir -p .cargo && cargo vendor >> .cargo/config.toml  # sandbox is offline
flatpak-builder --force-clean --repo=repo build-dir \
  packaging/flatpak/app.ezstreamer.desktop.yaml
flatpak build-bundle repo ezstreamer.flatpak app.ezstreamer.desktop
flatpak install --user ezstreamer.flatpak
```

Manifest pins `org.gnome.Platform//50`. Bump `runtime-version` (and the CI
SDK install in `release.yml`) together when moving to a newer GNOME branch.
