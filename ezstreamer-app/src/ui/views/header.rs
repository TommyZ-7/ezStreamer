//! Header bar: identity, live status, language toggle, settings.
//! Continuous 1px underline connects it to the rest of the window.

use crate::backend::Shared;
use crate::ui::i18n::{I18n, Locale};
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, segmented_sized, status_square, ButtonKind};
use egui::{Align, Context, Frame, Layout, Margin, RichText, Sense, TopBottomPanel};
use std::sync::{Arc, Mutex};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n, shared: &Arc<Mutex<Shared>>) {
    let status = shared.lock().unwrap().status;
    TopBottomPanel::top("header")
        .exact_height(46.0)
        .frame(
            Frame::NONE
                .fill(PANEL)
                .inner_margin(Margin::symmetric(14, 0)),
        )
        .show(ctx, |ui| {
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.max_rect().bottom(),
                egui::Stroke::new(1.0_f32, LINE_STRONG),
            );
            ui.horizontal_centered(|ui| {
                ui.label(
                    RichText::new(i18n.t("app.title"))
                        .size(15.0)
                        .strong()
                        .color(TEXT),
                );
                ui.add_space(4.0);
                let (sep, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), Sense::hover());
                ui.painter().rect_filled(sep, 0.0, LINE);
                ui.add_space(4.0);

                if let Some(retry) = status.retrying {
                    status_square(ui, WARN);
                    ui.label(
                        RichText::new(i18n.tf("stream.retrying", &[("n", &retry.to_string())]))
                            .size(12.5)
                            .color(WARN),
                    );
                } else if status.is_live {
                    status_square(ui, LIVE);
                    ui.label(
                        RichText::new(format!(
                            "{} {:02}:{:02}",
                            i18n.t("stream.live"),
                            status.duration_sec / 60,
                            status.duration_sec % 60
                        ))
                        .size(12.5)
                        .strong()
                        .color(LIVE),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!(
                            "{} {} kbps",
                            i18n.t("stream.bitrate"),
                            status.bitrate_kbps.round() as u64
                        ))
                        .size(12.0)
                        .color(DIM),
                    );
                } else {
                    status_square(ui, FAINT);
                    ui.label(
                        RichText::new(i18n.t("stream.stopped"))
                            .size(12.5)
                            .color(DIM),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, &i18n.t("app.settings"), ButtonKind::Normal, true).clicked() {
                        state.settings_open = true;
                        state.settings_draft = state.profiles.clone();
                        state.import_path.clear();
                    }
                    ui.add_space(6.0);
                    let labels = vec!["ja".to_string(), "en".to_string()];
                    let selected = if state.locale == Locale::Ja { 0 } else { 1 };
                    if let Some(idx) = segmented_sized(ui, 92.0, &labels, selected, true) {
                        state.set_locale(if idx == 0 { Locale::Ja } else { Locale::En });
                    }
                });
            });
        });
}
