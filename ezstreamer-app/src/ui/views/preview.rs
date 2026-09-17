//! Central preview canvas (OBS-style): a large always-visible canvas fed by
//! the 5fps capture preview, or by the live capture itself during a stream
//! (F-SC-03). The capture source picker lives in `views/screen.rs`, shown in
//! the bottom bar (`views/bottom.rs`). Preview start/stop lives in the stream
//! panel above the stream CTA (`views/stream.rs`).

use crate::ui::i18n::I18n;
use crate::ui::theme::*;
use egui::{pos2, vec2, Align2, Color32, FontId, Sense, Stroke, StrokeKind, TextureHandle, Ui};

pub fn show(ui: &mut Ui, i18n: &I18n, preview_texture: &Option<TextureHandle>) {
    // Canvas: fill the width but keep 16:9.
    let avail_w = ui.available_width();
    let avail_h = ui.available_height().max(220.0);
    let width = avail_w.min(avail_h * 16.0 / 9.0);
    let height = width * 9.0 / 16.0;
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0_f32, LINE_STRONG),
            StrokeKind::Inside,
        );
        if let Some(texture) = preview_texture {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                i18n.t("screen.previewEmpty"),
                FontId::proportional(12.0),
                FAINT,
            );
        }
    });
}
