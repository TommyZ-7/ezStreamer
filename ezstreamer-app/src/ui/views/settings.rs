//! Settings screen: profile CRUD, JSON import/export, encoders, logs,
//! licenses. Full-pane (not a floating modal) to keep the linear composition.

use crate::backend::{Backend, Command};
use crate::ui::i18n::I18n;
use crate::ui::state::{UiState, BUILTIN_PROFILE_IDS};
use crate::ui::theme::*;
use crate::ui::widgets::{button, rule, small_hint, status_square, ButtonKind};
use egui::{
    Align, CentralPanel, Context, Frame, Margin, RichText, ScrollArea, Stroke, StrokeKind, Ui,
};
use ezstreamer_core::config::{self, Profile, ProfilesConfig, MAX_AUDIO_KBPS, MAX_VIDEO_KBPS};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n, backend: &Backend) {
    CentralPanel::default()
        .frame(Frame::NONE.fill(BG).inner_margin(Margin::same(14)))
        .show(ctx, |ui| {
            if let Some(imported) = state.pending_import.take() {
                state.settings_draft = Some(imported);
                state.toast(i18n.t("settings.importLoaded"), false);
            }

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(i18n.t("settings.title"))
                        .size(15.0)
                        .strong()
                        .color(TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, &i18n.t("app.back"), ButtonKind::Normal, true).clicked() {
                        state.settings_open = false;
                        state.settings_draft = None;
                    }
                });
            });
            rule(ui);
            ui.add_space(4.0);

            ScrollArea::vertical()
                .id_salt("settings-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    profiles_section(ui, state, i18n, backend);
                    ui.add_space(14.0);
                    rule(ui);
                    ui.add_space(10.0);
                    encoders_section(ui, state, i18n, backend);
                    ui.add_space(14.0);
                    rule(ui);
                    ui.add_space(10.0);
                    io_section(ui, state, i18n, backend);
                    ui.add_space(14.0);
                    rule(ui);
                    ui.add_space(10.0);
                    footer_section(ui, i18n);
                });
        });
}

fn profiles_section(ui: &mut Ui, state: &mut UiState, i18n: &I18n, backend: &Backend) {
    let Some(draft) = state.settings_draft.as_mut() else {
        return;
    };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("settings.profiles"))
                .size(13.0)
                .strong()
                .color(TEXT),
        );
        if button(ui, &i18n.t("settings.add"), ButtonKind::Normal, true).clicked() {
            let mut n = 1;
            while draft.profiles.contains_key(&format!("custom{n}")) {
                n += 1;
            }
            draft.profiles.insert(
                format!("custom{n}"),
                Profile {
                    name: format!("Custom {n}"),
                    w: 1280,
                    h: 720,
                    fps: 30,
                    v_kbps: 1500,
                    a_kbps: 192,
                    encoder: "auto".into(),
                    warn: None,
                },
            );
        }
    });
    ui.add_space(4.0);

    let ids: Vec<String> = BUILTIN_PROFILE_IDS
        .iter()
        .filter(|id| draft.profiles.contains_key(**id))
        .map(|id| (*id).to_string())
        .chain(
            draft
                .profiles
                .keys()
                .filter(|id| !BUILTIN_PROFILE_IDS.contains(&id.as_str()))
                .cloned(),
        )
        .collect();

    let mut remove: Option<String> = None;
    let mut duplicate: Option<String> = None;

    egui::Grid::new("profiles-grid")
        .num_columns(7)
        .spacing([10.0, 4.0])
        .striped(false)
        .show(ui, |ui| {
            for header in [
                i18n.t("settings.name"),
                i18n.t("settings.resolution"),
                i18n.t("settings.fps"),
                i18n.t("settings.vKbps"),
                i18n.t("settings.aKbps"),
                String::new(),
                String::new(),
            ] {
                ui.label(RichText::new(header).size(11.5).color(FAINT));
            }
            ui.end_row();

            for id in &ids {
                let builtin = BUILTIN_PROFILE_IDS.contains(&id.as_str());
                let Some(profile) = draft.profiles.get_mut(id) else {
                    continue;
                };

                if builtin {
                    let label = if profile.name.starts_with("profile.") {
                        i18n.t(&profile.name)
                    } else {
                        profile.name.clone()
                    };
                    ui.label(RichText::new(label).size(12.5).color(TEXT));
                } else {
                    ui.add_sized(
                        [140.0, ROW_H],
                        egui::TextEdit::singleline(&mut profile.name).desired_width(140.0),
                    );
                }

                ui.horizontal(|ui| {
                    number(ui, &mut profile.w, 16..=7680, false);
                    ui.label(RichText::new("x").color(FAINT).size(11.0));
                    number(ui, &mut profile.h, 16..=4320, false);
                });
                number(ui, &mut profile.fps, 1..=240, false);
                let v_over = profile.v_kbps > MAX_VIDEO_KBPS;
                let a_over = profile.a_kbps > MAX_AUDIO_KBPS;
                number(ui, &mut profile.v_kbps, 1..=u32::MAX, v_over);
                number(ui, &mut profile.a_kbps, 1..=u32::MAX, a_over);

                if button(ui, &i18n.t("settings.duplicate"), ButtonKind::Normal, true).clicked() {
                    duplicate = Some(id.clone());
                }
                if button(ui, &i18n.t("settings.delete"), ButtonKind::Normal, !builtin).clicked() {
                    remove = Some(id.clone());
                }
                ui.end_row();
            }
        });

    if let Some(id) = duplicate {
        if let Some(profile) = draft.profiles.get(&id).cloned() {
            let mut n = 1;
            while draft.profiles.contains_key(&format!("{id}-copy{n}")) {
                n += 1;
            }
            let mut copy = profile;
            copy.name = format!("{}-copy{n}", copy.name);
            draft.profiles.insert(format!("{id}-copy{n}"), copy);
        }
    }
    if let Some(id) = remove {
        draft.profiles.remove(&id);
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if button(ui, &i18n.t("app.save"), ButtonKind::Primary, true).clicked() {
            if let Some(mut to_save) = state.settings_draft.clone() {
                // Keep screen/selections from the other panels in the same
                // write so saving settings never reverts them.
                to_save.locale = state.locale.code().into();
                to_save.ingest_url = state.ingest_url.clone();
                to_save.active_profile = state.profile_id.clone();
                to_save.encoder_override = state.encoder_override.clone();
                to_save.last_stream_key = state.stream_key.clone();
                to_save.last_sources = ezstreamer_core::config::LastSources {
                    screen: state.screen.clone(),
                    include_apps: state.selected_apps.clone(),
                    mic: state.mic.clone(),
                    cursor: state.cursor,
                };
                state.profiles = Some(to_save.clone());
                state.settings_draft = None;
                state.toast(i18n.t("app.saved"), false);
                backend.send(Command::SaveConfig(Box::new(to_save)));
            }
        }
        if button(ui, &i18n.t("app.discard"), ButtonKind::Normal, true).clicked() {
            state.settings_draft = state.profiles.clone();
        }
    });
}

fn encoders_section(ui: &mut Ui, state: &UiState, i18n: &I18n, backend: &Backend) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("settings.encoders"))
                .size(13.0)
                .strong()
                .color(TEXT),
        );
        if button(ui, &i18n.t("output.probe"), ButtonKind::Normal, true).clicked() {
            backend.send(Command::ProbeEncoders);
        }
    });
    ui.add_space(4.0);
    if state.encoders.is_empty() {
        small_hint(ui, &i18n.t("output.checkingEncoders"));
        return;
    }
    for encoder in &state.encoders {
        ui.horizontal(|ui| {
            status_square(ui, if encoder.usable { OK } else { FAINT });
            ui.label(
                RichText::new(&encoder.name)
                    .monospace()
                    .size(12.0)
                    .color(if encoder.usable { TEXT } else { DIM }),
            );
            ui.label(
                RichText::new(if encoder.usable {
                    i18n.t("output.usable")
                } else {
                    i18n.t("output.unusable")
                })
                .size(11.0)
                .color(if encoder.usable { OK } else { FAINT }),
            );
            if let Some(reason) = &encoder.reason {
                ui.label(RichText::new(reason).size(11.0).color(FAINT));
            }
        });
    }
}

fn io_section(ui: &mut Ui, state: &mut UiState, i18n: &I18n, backend: &Backend) {
    ui.label(
        RichText::new(i18n.t("settings.export"))
            .size(13.0)
            .strong()
            .color(TEXT),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if button(ui, &i18n.t("settings.export"), ButtonKind::Normal, true).clicked() {
            let cfg = state
                .settings_draft
                .clone()
                .or_else(|| state.profiles.clone());
            if let Some(cfg) = cfg {
                match export_profiles(&cfg) {
                    Ok(path) => state.toast(
                        format!("{}: {}", i18n.t("settings.exportDone"), path.display()),
                        false,
                    ),
                    Err(e) => state.toast(format!("{}: {e}", i18n.t("settings.exportDone")), true),
                }
            }
        }
        if button(ui, &i18n.t("settings.logs"), ButtonKind::Normal, true).clicked() {
            backend.send(Command::OpenDir(config::config_dir().join("logs")));
        }
    });
    ui.add_space(8.0);

    ui.label(
        RichText::new(i18n.t("settings.import"))
            .size(13.0)
            .strong()
            .color(TEXT),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_sized(
            [420.0, ROW_H],
            egui::TextEdit::singleline(&mut state.import_path)
                .hint_text(i18n.t("settings.importPath"))
                .font(egui::TextStyle::Monospace),
        );
        if button(ui, &i18n.t("settings.import"), ButtonKind::Normal, true).clicked() {
            match import_profiles(&state.import_path) {
                Ok(cfg) => {
                    state.settings_draft = Some(cfg);
                    state.toast(i18n.t("settings.importLoaded"), false);
                }
                Err(e) => state.toast(format!("{}: {e}", i18n.t("settings.importError")), true),
            }
        }
    });
    small_hint(ui, &i18n.t("settings.dropHint"));
}

fn footer_section(ui: &mut Ui, i18n: &I18n) {
    small_hint(ui, &i18n.t("settings.licenseText"));
    ui.label(
        RichText::new(i18n.t("settings.sources"))
            .size(11.0)
            .color(FAINT),
    );
}

fn number(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>, over_limit: bool) {
    let color = if over_limit { LIVE } else { TEXT };
    let response = ui
        .scope(|ui| {
            ui.style_mut().visuals.override_text_color = Some(color);
            ui.add_sized(
                [64.0, ROW_H],
                egui::DragValue::new(value)
                    .range(range)
                    .speed(1.0_f32)
                    .update_while_editing(false),
            )
        })
        .inner;
    if over_limit {
        ui.painter().rect_stroke(
            response.rect,
            0.0,
            Stroke::new(1.0_f32, LIVE),
            StrokeKind::Inside,
        );
    }
}

fn export_profiles(cfg: &ProfilesConfig) -> Result<std::path::PathBuf, String> {
    let dir = config::config_dir().join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!(
        "ezstreamer-profiles-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = dir.join(name);
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}

fn import_profiles(path: &str) -> Result<ProfilesConfig, String> {
    let text = std::fs::read_to_string(path.trim()).map_err(|e| e.to_string())?;
    let cfg: ProfilesConfig = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if cfg.profiles.is_empty() {
        return Err("profiles field missing".into());
    }
    Ok(cfg)
}
