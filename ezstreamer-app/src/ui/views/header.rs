//! Header bar: identity + live status, language toggle, settings.

use crate::backend::Shared;
use crate::ui::i18n::{I18n, Locale};
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, segmented_sized, status_square, ButtonKind};
use egui::{Align, Context, Frame, Layout, Margin, RichText, TopBottomPanel};
use std::sync::{Arc, Mutex};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n, shared: &Arc<Mutex<Shared>>) {
    let (is_live, retrying) = {
        let shared = shared.lock().unwrap();
        (shared.status.is_live, shared.status.retrying)
    };
    TopBottomPanel::top("header")
        .exact_height(44.0)
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
                // Live badge next to the title: status is visible from every
                // tab (and settings), not only near the dock CTA.
                if let Some(n) = retrying {
                    status_square(ui, WARN);
                    ui.label(
                        RichText::new(i18n.tf("stream.retrying", &[("n", &n.to_string())]))
                            .size(12.0)
                            .color(WARN),
                    );
                } else if is_live {
                    status_square(ui, LIVE);
                    ui.label(RichText::new(i18n.t("stream.live")).size(12.0).color(LIVE));
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
