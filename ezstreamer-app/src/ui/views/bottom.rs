//! Bottom configuration bar (OBS-style): three fixed columns — video
//! source (30%), volume (40%), encoder (30%) — sitting directly above the
//! status bar. The middle column is wider: the mixer needs room for
//! VU + gain rows. Each column has a fixed height and scrolls only when its
//! content overflows. The central canvas owns everything above.

use crate::backend::{Backend, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::BOTTOM_H;
use egui::{Align, Context, Frame, Layout, Margin, Stroke, TopBottomPanel, Ui};
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
        .resizable(false)
        .frame(Frame::NONE.fill(PANEL).inner_margin(Margin::same(10)))
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );

            // Column widths: source 30% / volume 40% / encoder 30%. The
            // automatic 8px item spacing forms the two gutters; hairline
            // dividers run through their middle.
            let inner = ui.available_width() - 16.0;
            let w_source = inner * 0.30;
            let w_volume = inner * 0.40;
            let w_encoder = inner * 0.30;
            let height = ui.available_height();
            // Top-align the three columns. `horizontal_top` keeps every
            // column at the panel top; the old centered `horizontal` pushed
            // short content to the middle of the 280px bar.
            ui.horizontal_top(|ui| {
                column(ui, w_source, height, "bottom-sources", |ui| {
                    crate::ui::views::screen::show(ui, state, i18n, backend, shared);
                });
                column(ui, w_volume, height, "bottom-audio", |ui| {
                    crate::ui::views::audio::show(ui, state, i18n, shared);
                });
                column(ui, w_encoder, height, "bottom-encoder", |ui| {
                    crate::ui::views::output::show(ui, state, i18n, backend, shared);
                });
            });
            ui.painter().vline(
                rect.left() + w_source + 4.0,
                rect.y_range(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );
            ui.painter().vline(
                rect.left() + w_source + 8.0 + w_volume + 4.0,
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
    // `allocate_ui` inherits the parent (horizontal) layout, which laid every
    // widget in the column left-to-right on one centered line and let narrow
    // rows bleed into the next column. Pin a vertical layout so each column
    // stacks its own rows from the top.
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        Layout::top_down(Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt(salt)
                .auto_shrink([false, false])
                .show(ui, |ui| add(ui));
        },
    );
}
