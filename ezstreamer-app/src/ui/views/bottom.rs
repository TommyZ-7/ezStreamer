//! Bottom configuration bar (OBS-style): two fixed columns — capture source
//! on the left, quality/encoder/destination on the right — sitting directly
//! above the status dock. The central canvas owns everything above.

use crate::backend::{Backend, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::BOTTOM_H;
use egui::{Context, Frame, Margin, Stroke, TopBottomPanel, Ui};
use std::sync::{Arc, Mutex};

pub fn show(
    ctx: &Context,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
) {
    TopBottomPanel::bottom("bottom")
        .exact_height(BOTTOM_H)
        .frame(Frame::NONE.fill(PANEL).inner_margin(Margin::same(10)))
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );

            // Two halves; the automatic 8px item spacing is the gutter and the
            // hairline divider is painted through its middle.
            let half = (ui.available_width() - 8.0) / 2.0;
            let height = ui.available_height();
            ui.horizontal(|ui| {
                column(ui, half, height, "bottom-sources", |ui| {
                    crate::ui::views::screen::show(ui, state, i18n, backend, shared);
                });
                column(ui, half, height, "bottom-output", |ui| {
                    crate::ui::views::output::show(ui, state, i18n, backend, shared);
                });
            });
            ui.painter().vline(
                rect.center().x,
                rect.y_range(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );
        });
}

fn column(
    ui: &mut Ui,
    width: f32,
    height: f32,
    salt: &'static str,
    add: impl FnOnce(&mut Ui),
) {
    ui.allocate_ui(egui::vec2(width, height), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(salt)
            .auto_shrink([false, false])
            .show(ui, |ui| add(ui));
    });
}
