//! Encoder column (下段右): profile selection + encoder choice/usage
//! (F-EN-01..05). Shown in the bottom bar (`views/bottom.rs`). Redesigned for
//! a narrow fixed column: the profile cards became a ComboBox with a detail
//! line; the Ingest/Key inputs moved to the right panel & settings pane.

use crate::backend::{Backend, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, section_header, small_hint, status_square, ButtonKind};
use egui::{RichText, Ui};
use ezstreamer_core::config::{MAX_AUDIO_KBPS, MAX_VIDEO_KBPS};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    _shared: &Arc<Mutex<Shared>>,
) {
    let title = i18n.t("output.title");
    let probe_label = i18n.t("output.probe");
    section_header(ui, &title, |ui| {
        if button(ui, &probe_label, ButtonKind::Normal, true).clicked() {
            backend.send(Command::ProbeEncoders);
        }
    });

    // --- profile (quality preset) --------------------------------------------
    ui.label(
        RichText::new(i18n.t("output.profile")).size(12.5).color(DIM),
    );
    if let Some(cfg) = state.profiles.clone() {
        let ids = state.profile_ids();
        let selected_label = {
            let profile = cfg.profiles.get(&state.profile_id);
            match profile {
                Some(p) => {
                    let name = if p.name.starts_with("profile.") {
                        i18n.t(&p.name)
                    } else {
                        p.name.clone()
                    };
                    format!("{}  {}x{} {}fps", name, p.w, p.h, p.fps)
                }
                None => state.profile_id.clone(),
            }
        };
        egui::ComboBox::from_id_salt("profile-select")
            .width(ui.available_width())
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for id in &ids {
                    let Some(p) = cfg.profiles.get(id) else {
                        continue;
                    };
                    let name = if p.name.starts_with("profile.") {
                        i18n.t(&p.name)
                    } else {
                        p.name.clone()
                    };
                    ui.selectable_value(
                        &mut state.profile_id,
                        id.clone(),
                        format!("{}  {}x{} {}fps", name, p.w, p.h, p.fps),
                    );
                }
            });
        if let Some(profile) = cfg.profiles.get(&state.profile_id) {
            ui.add_space(2.0);
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
                    "{} {} kbps / {} kbps   {} {}",
                    i18n.t("output.details"),
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

    ui.add_space(8.0);
    crate::ui::widgets::rule(ui);
    ui.add_space(6.0);

    // --- encoder -------------------------------------------------------------
    // Label above, combo full width below: the old single-line
    // "96px label + combo" always overflowed the 30% column.
    ui.label(
        RichText::new(i18n.t("output.encoder")).size(12.5).color(DIM),
    );
    ui.add_space(2.0);
    {
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
            .width(ui.available_width())
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
    }
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

    ui.add_space(6.0);
    if state.encoders.is_empty() {
        small_hint(ui, &i18n.t("output.checkingEncoders"));
    } else {
        for encoder in &state.encoders {
            ui.horizontal(|ui| {
                status_square(ui, if encoder.usable { OK } else { FAINT });
                ui.add(
                    egui::Label::new(
                        RichText::new(&encoder.name)
                            .monospace()
                            .size(12.0)
                            .color(if encoder.usable { TEXT } else { DIM }),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
            if let Some(reason) = &encoder.reason {
                small_hint(ui, &format!("   {reason}"));
            }
        }
    }
}
