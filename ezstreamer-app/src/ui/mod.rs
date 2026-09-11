//! egui application shell: event pump, debounced persistence, painting.

pub mod i18n;
pub mod state;
pub mod theme;
pub mod views;
pub mod widgets;

use crate::backend::{Backend, Command, Shared};
use crate::events::UiEvent;
use crate::logging;
use egui::{Align2, Context, Frame, Margin, Order, RichText, Stroke};
use i18n::{I18n, Locale};
use state::{Tab, UiState};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct EzStreamerApp {
    backend: Backend,
    shared: Arc<Mutex<Shared>>,
    i18n: I18n,
    state: UiState,
    preview_texture: Option<egui::TextureHandle>,
}

impl EzStreamerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);

        let locale = os_locale();
        let backend = Backend::start();
        backend.send(Command::LoadAll);
        Self {
            shared: backend.shared(),
            backend,
            i18n: I18n::new(locale),
            state: UiState::new(locale),
            preview_texture: None,
        }
    }

    /// Window icon for the OS taskbar (decoded through eframe's `image` dep).
    pub fn window_icon() -> egui::IconData {
        eframe::icon_data::from_png_bytes(include_bytes!("../../icons/icon.png"))
            .unwrap_or_default()
    }

    fn drain_events(&mut self, ctx: &Context) {
        while let Some(event) = self.backend.try_recv_event() {
            match event {
                UiEvent::Config(cfg) => {
                    self.state.apply_config(*cfg);
                    self.i18n.set_locale(self.state.locale);
                }
                UiEvent::Displays(displays) => self.state.displays = displays,
                UiEvent::Windows(windows) => self.state.windows = windows,
                UiEvent::AudioDevices(devices) => self.state.devices = Some(*devices),
                UiEvent::Encoders(encoders) => self.state.encoders = encoders,
                UiEvent::Preview { rgba, w, h } => {
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    match &mut self.preview_texture {
                        Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                        None => {
                            self.preview_texture = Some(ctx.load_texture(
                                "preview",
                                image,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                    }
                }
                UiEvent::StreamStarted => {
                    self.state.starting = false;
                    self.state.stopping = false;
                    self.state.last_error = None;
                    self.state.toast(self.i18n.t("stream.live"), false);
                }
                UiEvent::StreamStopped => {
                    self.state.starting = false;
                    self.state.stopping = false;
                    self.preview_texture = None;
                }
                UiEvent::PreviewStarted => {}
                UiEvent::PreviewStopped => self.preview_texture = None,
                UiEvent::PortalPicked(target) => {
                    self.state.screen = target;
                    self.state.mark_persist();
                }
                UiEvent::Toast(message) => self.state.toast(message, false),
                UiEvent::Error(message) => {
                    logging::error(&format!("backend: {message}"));
                    self.state.starting = false;
                    self.state.stopping = false;
                    self.state.last_error = Some(message.clone());
                    self.state.toast(message, true);
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let text = match (&file.bytes, &file.path) {
                (Some(bytes), _) => String::from_utf8_lossy(bytes).to_string(),
                (None, Some(path)) => match std::fs::read_to_string(path) {
                    Ok(text) => text,
                    Err(e) => {
                        self.state.toast(
                            format!("{}: {e}", self.i18n.t("settings.importError")),
                            true,
                        );
                        continue;
                    }
                },
                (None, None) => continue,
            };
            match serde_json::from_str::<ezstreamer_core::config::ProfilesConfig>(&text) {
                Ok(cfg) if !cfg.profiles.is_empty() => {
                    self.state.pending_import = Some(cfg);
                    self.state.settings_open = true;
                    self.state.settings_draft = None;
                }
                Ok(_) => self.state.toast(self.i18n.t("settings.importError"), true),
                Err(e) => self.state.toast(
                    format!("{}: {e}", self.i18n.t("settings.importError")),
                    true,
                ),
            }
        }
    }

    fn tick_timers(&mut self) {
        // F-CF-02: debounce selection changes (400ms) into one atomic save.
        if let Some(due) = self.state.persist_due {
            if due.elapsed() >= Duration::from_millis(400) {
                self.state.persist_due = None;
                if let Some(cfg) = self.state.to_config() {
                    self.backend.send(Command::SaveConfig(Box::new(cfg)));
                }
            }
        }
        // F-AU-04: live gain/mute debounce (100ms).
        if let Some(due) = self.state.mix_due {
            if due.elapsed() >= Duration::from_millis(100) {
                self.state.mix_due = None;
                if self.shared.lock().unwrap().status.is_live {
                    self.backend
                        .send(Command::UpdateMix(Box::new(self.state.mix_update())));
                }
            }
        }
        // Screen switch while previewing: restart capture with the new target.
        if let Some(due) = self.state.preview_restart_due {
            if due.elapsed() >= Duration::from_millis(250) {
                self.state.preview_restart_due = None;
                self.backend.send(Command::StopPreview);
                self.backend
                    .send(Command::StartPreview(Box::new(self.state.stream_config())));
            }
        }
        // Toasts expire on their own.
        let expired = self
            .state
            .toast
            .as_ref()
            .map(|(_, at)| at.elapsed() > Duration::from_millis(2600))
            .unwrap_or(false);
        if expired {
            self.state.toast = None;
        }
    }

    fn draw(&mut self, ctx: &Context) {
        if !self.state.booted {
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(theme::BG))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(120.0);
                            ui.label(
                                RichText::new(self.i18n.t("app.title"))
                                    .size(20.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(self.i18n.t("app.loading"))
                                    .size(12.0)
                                    .color(theme::DIM),
                            );
                        });
                    });
                });
            draw_toast(ctx, &mut self.state);
            return;
        }

        let Self {
            backend,
            shared,
            i18n,
            state,
            preview_texture,
            ..
        } = self;

        views::header::show(ctx, state, i18n, shared);

        if state.settings_open {
            views::settings::show(ctx, state, i18n, backend);
            draw_toast(ctx, state);
            return;
        }

        views::dock::show(ctx, state, i18n, backend, shared);
        views::steps::show(ctx, state, i18n, shared);

        egui::CentralPanel::default()
            .frame(
                Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ctx, |ui| match state.tab {
                Tab::Screen => {
                    views::screen::show(ui, state, i18n, backend, shared, preview_texture)
                }
                Tab::Audio => views::audio::show(ui, state, i18n, shared),
                Tab::Output => views::output::show(ui, state, i18n, backend, shared),
            });

        draw_toast(ctx, state);
    }
}

impl eframe::App for EzStreamerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        self.handle_dropped_files(ctx);
        self.tick_timers();
        self.draw(ctx);
        // Poll backend events/status at ~20fps even when idle: VU meters and
        // the status tick need it, and the cost is a couple of locks.
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn draw_toast(ctx: &Context, state: &mut UiState) {
    let expired = state
        .toast
        .as_ref()
        .map(|(_, at)| at.elapsed() > Duration::from_millis(2600))
        .unwrap_or(false);
    if expired {
        state.toast = None;
        return;
    }
    let Some((message, _)) = state.toast.as_ref() else {
        return;
    };
    let (fill, stroke) = if state.toast_error {
        (theme::LIVE_BG, theme::LIVE)
    } else {
        (theme::PANEL_2, theme::LINE_STRONG)
    };
    egui::Area::new(egui::Id::new("toast"))
        .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -220.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(fill)
                .stroke(Stroke::new(1.0_f32, stroke))
                .inner_margin(Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new(message).size(12.5).color(theme::TEXT));
                });
        });
}

/// F-CF-05: explicit saved locale wins; otherwise follow the OS language.
fn os_locale() -> Locale {
    #[cfg(all(windows, feature = "media"))]
    {
        let mut buffer = [0u16; 85];
        let len = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buffer) };
        if len > 1 {
            let name = String::from_utf16_lossy(&buffer[..(len - 1) as usize]);
            return if name.to_ascii_lowercase().starts_with("ja") {
                Locale::Ja
            } else {
                Locale::En
            };
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.to_ascii_lowercase();
            if value.starts_with("ja") {
                return Locale::Ja;
            }
            if !value.is_empty() {
                return Locale::En;
            }
        }
    }
    Locale::En
}
