//! Left step rail: numbered squares connected by a vertical hairline. This is
//! the structural backbone of the "straight / connected" layout.

use crate::backend::Shared;
use crate::ui::i18n::I18n;
use crate::ui::state::{Tab, UiState};
use crate::ui::theme::*;
use crate::ui::widgets::rule;
use egui::{
    pos2, vec2, Align2, Context, FontId, Frame, Margin, RichText, Sense, SidePanel, Stroke,
    StrokeKind,
};
use std::sync::{Arc, Mutex};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n, _shared: &Arc<Mutex<Shared>>) {
    SidePanel::left("steps")
        .exact_width(186.0)
        .resizable(false)
        .frame(
            Frame::NONE
                .fill(PANEL)
                .inner_margin(Margin::symmetric(14, 16)),
        )
        .show(ctx, |ui| {
            let mut prev_bottom: Option<f32> = None;
            for (index, tab) in Tab::ALL.iter().enumerate() {
                if step(ui, state, i18n, *tab, index, &mut prev_bottom) {
                    state.tab = *tab;
                }
            }

            ui.add_space(14.0);
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

/// Returns true when the step was clicked. `prev_bottom` carries the previous
/// marker's bottom edge so the connector line stays continuous.
fn step(
    ui: &mut egui::Ui,
    state: &UiState,
    i18n: &I18n,
    tab: Tab,
    index: usize,
    prev_bottom: &mut Option<f32>,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    let marker = egui::Rect::from_min_size(pos2(rect.left(), rect.top() + 4.0), vec2(22.0, 22.0));
    let painter = ui.painter();

    if let Some(prev) = *prev_bottom {
        painter.vline(
            marker.center().x,
            prev..=marker.top(),
            Stroke::new(1.0_f32, LINE),
        );
    }
    let selected = state.tab == tab;
    if selected {
        painter.rect_filled(marker, 0.0, ACCENT);
    } else if response.hovered() {
        painter.rect_filled(marker, 0.0, ROW_HOVER);
    }
    painter.rect_stroke(
        marker,
        0.0,
        Stroke::new(1.0_f32, if selected { ACCENT } else { LINE_STRONG }),
        StrokeKind::Inside,
    );
    painter.text(
        marker.center(),
        Align2::CENTER_CENTER,
        format!("{:02}", index + 1),
        FontId::monospace(11.0),
        if selected { BG } else { DIM },
    );
    painter.text(
        pos2(marker.right() + 10.0, rect.top() + 5.0),
        Align2::LEFT_TOP,
        i18n.t(tab.title_key()),
        FontId::proportional(13.5),
        if selected { TEXT } else { DIM },
    );
    painter.text(
        pos2(marker.right() + 10.0, rect.top() + 24.0),
        Align2::LEFT_TOP,
        i18n.t(tab.hint_key()),
        FontId::proportional(10.5),
        FAINT,
    );

    *prev_bottom = Some(marker.bottom());
    response.clicked()
}
