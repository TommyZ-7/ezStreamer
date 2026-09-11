//! Left navigation rail: plain section titles with a selection highlight.

use crate::ui::i18n::I18n;
use crate::ui::state::{Tab, UiState};
use crate::ui::theme::*;
use crate::ui::widgets::rule;
use egui::{pos2, vec2, Align2, Context, FontId, Frame, Margin, RichText, Sense, SidePanel};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n) {
    SidePanel::left("steps")
        .exact_width(128.0)
        .resizable(false)
        .frame(
            Frame::NONE
                .fill(PANEL)
                .inner_margin(Margin::symmetric(10, 12)),
        )
        .show(ctx, |ui| {
            for tab in Tab::ALL.iter() {
                if step(ui, state, i18n, *tab) {
                    state.tab = *tab;
                }
                ui.add_space(2.0);
            }

            ui.add_space(10.0);
            rule(ui);
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    i18n.t("app.version"),
                    env!("CARGO_PKG_VERSION")
                ))
                .size(11.0)
                .color(FAINT),
            );
        });
}

/// Returns true when the row was clicked.
fn step(ui: &mut egui::Ui, state: &UiState, i18n: &I18n, tab: Tab) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let selected = state.tab == tab;
    let painter = ui.painter();

    if selected {
        painter.rect_filled(rect, 0.0, ACCENT_BG);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            ACCENT,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, ROW_HOVER);
    }
    painter.text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        i18n.t(tab.title_key()),
        FontId::proportional(13.0),
        if selected { TEXT } else { DIM },
    );

    response.clicked()
}
