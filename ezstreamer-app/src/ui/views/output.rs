//! Step 3: quality profile + encoder selection (F-EN-01..05).

use crate::backend::{Backend, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, rule, small_hint, status_square, ButtonKind};
use egui::{pos2, vec2, Align2, FontId, RichText, Sense, Stroke, StrokeKind, Ui};
use ezstreamer_core::config::{MAX_AUDIO_KBPS, MAX_VIDEO_KBPS};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    _shared: &Arc<Mutex<Shared>>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("output.title"))
                .size(14.0)
                .strong()
                .color(TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, &i18n.t("output.probe"), ButtonKind::Normal, true).clicked() {
                backend.send(Command::ProbeEncoders);
            }
        });
    });
    rule(ui);
    ui.add_space(8.0);

    // --- profile cards ------------------------------------------------------
    if let Some(cfg) = state.profiles.clone() {
        let ids = state.profile_ids();
        ui.horizontal_wrapped(|ui| {
            for id in &ids {
                let Some(profile) = cfg.profiles.get(id) else {
                    continue;
                };
                let selected = state.profile_id == *id;
                let label = if profile.name.starts_with("profile.") {
                    i18n.t(&profile.name)
                } else {
                    profile.name.clone()
                };
                let spec = format!("{}x{} {}fps", profile.w, profile.h, profile.fps);
                let rates = format!("{}k / {}k", profile.v_kbps, profile.a_kbps);
                let high_res = profile.w >= 1920 || profile.h >= 1080;
                if profile_card(ui, &label, &spec, &rates, selected, high_res) {
                    state.profile_id = id.clone();
                    state.mark_persist();
                }
            }
        });

        if let Some(profile) = cfg.profiles.get(&state.profile_id) {
            ui.add_space(6.0);
            if profile.warn.is_some() || profile.w >= 1920 || profile.h >= 1080 {
                ui.label(
                    RichText::new(i18n.t("output.warn1080p"))
                        .size(11.5)
                        .color(WARN),
                );
            }
            if profile.v_kbps > MAX_VIDEO_KBPS || profile.a_kbps > MAX_AUDIO_KBPS {
                ui.label(
                    RichText::new(i18n.t("output.overBitrate"))
                        .size(11.5)
                        .strong()
                        .color(LIVE),
                );
            }
            small_hint(
                ui,
                &format!(
                    "{}: {}x{} {}fps / {} kbps / {} kbps   {} {}",
                    i18n.t("output.details"),
                    profile.w,
                    profile.h,
                    profile.fps,
                    profile.v_kbps,
                    profile.a_kbps,
                    i18n.t("output.gop"),
                    profile.gop()
                ),
            );
        }
    } else {
        small_hint(ui, "…");
    }

    ui.add_space(10.0);
    rule(ui);
    ui.add_space(8.0);

    // --- encoder ------------------------------------------------------------
    ui.horizontal(|ui| {
        ui.add_sized(
            [88.0, ROW_H],
            egui::Label::new(
                RichText::new(i18n.t("output.encoder"))
                    .size(12.5)
                    .color(DIM),
            ),
        );

        let mut options: Vec<(String, String, bool)> =
            vec![("auto".to_string(), i18n.t("output.auto"), true)];
        for encoder in &state.encoders {
            options.push((encoder.name.clone(), encoder.name.clone(), encoder.usable));
        }
        for fallback in ["libx264", "h264_vulkan"] {
            if !state.encoders.iter().any(|e| e.name == fallback) {
                options.push((fallback.to_string(), fallback.to_string(), true));
            }
        }
        let selected_text = if state.encoder_override == "auto" {
            i18n.t("output.auto")
        } else {
            state.encoder_override.clone()
        };
        egui::ComboBox::from_id_salt("encoder-select")
            .width(240.0)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for (id, label, usable) in &options {
                    let text = if *usable {
                        label.clone()
                    } else {
                        format!("{} ({})", label, i18n.t("output.unusable"))
                    };
                    ui.selectable_value(&mut state.encoder_override, id.clone(), text);
                }
            });
        if state
            .encoders
            .iter()
            .any(|e| e.name == state.encoder_override && !e.usable)
        {
            ui.label(
                RichText::new(i18n.t("output.unusable"))
                    .size(11.5)
                    .color(WARN),
            );
        }
    });

    ui.add_space(6.0);
    if state.encoders.is_empty() {
        small_hint(ui, &i18n.t("output.checkingEncoders"));
    } else {
        for encoder in &state.encoders {
            ui.horizontal(|ui| {
                status_square(ui, if encoder.usable { OK } else { FAINT });
                ui.label(
                    RichText::new(&encoder.name)
                        .monospace()
                        .size(12.0)
                        .color(if encoder.usable { TEXT } else { DIM }),
                );
                if let Some(reason) = &encoder.reason {
                    ui.label(RichText::new(reason).size(11.0).color(FAINT));
                }
            });
        }
    }
}

fn profile_card(
    ui: &mut Ui,
    label: &str,
    spec: &str,
    rates: &str,
    selected: bool,
    high_res: bool,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(154.0, 54.0), Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    let fill = if selected {
        ACCENT_BG
    } else if response.hovered() {
        ROW_HOVER
    } else {
        PANEL_2
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, if selected { ACCENT } else { LINE_STRONG }),
        StrokeKind::Inside,
    );
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
            0.0,
            ACCENT,
        );
    }
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 7.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(13.5),
        if selected { TEXT } else { DIM },
    );
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 26.0),
        Align2::LEFT_TOP,
        spec,
        FontId::monospace(10.5),
        FAINT,
    );
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 39.0),
        Align2::LEFT_TOP,
        rates,
        FontId::monospace(10.5),
        FAINT,
    );
    if high_res {
        painter.rect_filled(
            egui::Rect::from_min_size(pos2(rect.right() - 14.0, rect.top() + 6.0), vec2(6.0, 6.0)),
            0.0,
            WARN,
        );
    }
    response.clicked()
}
