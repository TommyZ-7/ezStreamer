//! Header bar: identity, language toggle, settings.

use crate::ui::i18n::{I18n, Locale};
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, segmented_sized, ButtonKind};
use egui::{Align, Context, Frame, Layout, Margin, RichText, TopBottomPanel};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n) {
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
