//! Central preview canvas (OBS-style): a large always-visible canvas fed by
//! the 1fps capture preview, or by the live capture itself during a stream
//! (F-SC-03). The capture source picker lives in `views/screen.rs`, shown in
//! the bottom bar (`views/bottom.rs`).

use crate::backend::{Backend, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, small_hint, ButtonKind};
use egui::{
    pos2, vec2, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, TextureHandle, Ui,
};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
    preview_texture: &Option<TextureHandle>,
) {
    let (live, mut previewing) = {
        let shared = shared.lock().unwrap();
        (
            shared.status.is_live || shared.status.retrying.is_some(),
            shared.previewing,
        )
    };
    // While live the capture feeds this texture at 1fps; treat it as
    // previewing so the control strip only shows the LIVE marker.
    if live {
        previewing = true;
    }

    // Canvas: fill the width but keep 16:9, leaving room for the controls.
    let avail_w = ui.available_width();
    let avail_h = (ui.available_height() - 58.0).max(220.0);
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

    // Control strip: start/stop the idle preview. While live the capture is
    // already up, so there is nothing to start/stop.
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("screen.preview"))
                .size(12.5)
                .color(DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if live {
                ui.label(RichText::new(i18n.t("stream.live")).size(12.0).color(LIVE));
                return;
            }
            let has_source = !state.screen.id.is_empty();
            let label = if previewing {
                i18n.t("screen.previewStop")
            } else {
                i18n.t("screen.previewStart")
            };
            let kind = if previewing {
                ButtonKind::Danger
            } else {
                ButtonKind::Normal
            };
            let enabled = has_source || previewing;
            if button(ui, &label, kind, enabled).clicked() {
                if previewing {
                    backend.send(Command::StopPreview);
                } else {
                    state.preview_restart_due = None;
                    backend.send(Command::StartPreview(Box::new(state.stream_config())));
                }
            }
        });
    });
    ui.add_space(4.0);
    if live {
        small_hint(ui, &i18n.t("screen.livePreviewHint"));
    } else {
        small_hint(ui, &i18n.t("screen.previewHint"));
    }
}
