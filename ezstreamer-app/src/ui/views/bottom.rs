//! Bottom configuration bar (OBS-style): three fixed columns — capture
//! source, quality/encoder, destination — sitting directly above the status
//! bar. Each column has a fixed height and scrolls only when its content
//! overflows. The central canvas owns everything above.

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

            // Three equal thirds; the automatic 8px item spacing forms the
            // gutters and the hairline dividers run through their middle.
            let third = (ui.available_width() - 16.0) / 3.0;
            let height = ui.available_height();
            ui.horizontal(|ui| {
                column(ui, third, height, "bottom-sources", |ui| {
                    crate::ui::views::screen::show(ui, state, i18n, backend, shared);
                });
                column(ui, third, height, "bottom-quality", |ui| {
                    crate::ui::views::output::show(ui, state, i18n, backend, shared);
                });
                column(ui, third, height, "bottom-destination", |ui| {
                    crate::ui::views::destination::show(ui, state, i18n);
                });
            });
            ui.painter().vline(
                rect.left() + third + 4.0,
                rect.y_range(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );
            ui.painter().vline(
                rect.left() + 2.0 * third + 12.0,
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
