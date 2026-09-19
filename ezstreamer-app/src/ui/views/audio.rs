//! Step 2: audio sources, per-source VU/gain/mute (F-AU-01..06).
//! Shown in the middle column of the bottom bar (`views/bottom.rs`).
//! Narrow-column layout: every row stacks vertically — label/value line,
//! then a full-width VU, then a gain line. No row packs VU + slider +
//! value on one horizontal line (that always overflowed the 40% column).

use crate::backend::Shared;
use crate::ui::i18n::I18n;
use crate::ui::state::{AppMixEntry, AudioMode, UiState};
use crate::ui::theme::*;
use crate::ui::widgets::{checkbox, format_vu_db, section_header, small_hint, smooth_meter, vu_bar};
use egui::{RichText, Sense, Ui};
use ezstreamer_core::config::MicSource;
use std::sync::{Arc, Mutex};

pub fn show(ui: &mut Ui, state: &mut UiState, i18n: &I18n, shared: &Arc<Mutex<Shared>>) {
    let vu = shared.lock().unwrap().vu.clone();
    let is_live = shared.lock().unwrap().status.is_live;
    // Frame time for meter ballistics (clamped: tab-switch gaps must not
    // teleport the bars). Raw mixer levels update every ~10ms; the UI
    // repaints at ~20fps, so smooth here before display.
    let dt = ui.ctx().input(|i| i.stable_dt).clamp(0.0, 0.5);

    section_header(ui, &i18n.t("audio.title"), |_| {});

    // --- master: label + value line, VU full width below --------------------
    let master = smooth_meter(state.meters.master, vu.master.rms, dt);
    state.meters.master = master;
    ui.horizontal(|ui| {
        ui.label(RichText::new(i18n.t("audio.master")).size(12.5).color(DIM));
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(
                    RichText::new(format_vu_db(master))
                        .monospace()
                        .size(11.0)
                        .color(FAINT),
                );
            },
        );
    });
    vu_bar(ui, ui.available_width(), 12.0, master);
    ui.add_space(6.0);

    // --- mode ---------------------------------------------------------------
    let labels = vec![i18n.t("audio.system"), i18n.t("audio.apps")];
    let selected = if state.audio_mode == AudioMode::System {
        0
    } else {
        1
    };
    if let Some(idx) = crate::ui::widgets::segmented(ui, &labels, selected, true) {
        state.audio_mode = if idx == 0 {
            AudioMode::System
        } else {
            AudioMode::Apps
        };
        state.mark_persist();
    }
    ui.add_space(8.0);

    // --- per-app list: 1 app = name/mute line + VU line + gain line ---------
    if state.audio_mode == AudioMode::Apps {
        match &state.devices {
            None => small_hint(ui, &i18n.t("audio.noApps")),
            Some(devices) if devices.apps.is_empty() => small_hint(ui, &i18n.t("audio.noApps")),
            Some(devices) => {
                let apps = devices.apps.clone();
                // Drop smoothing state for apps that disappeared (device list churn).
                let live: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
                state.meters.apps.retain(|id, _| live.contains(id));
                let mute_label = i18n.t("audio.mute");
                for app in &apps {
                    let mut selected = state.selected_apps.iter().any(|a| a == &app.id);
                    ui.horizontal(|ui| {
                        if checkbox(ui, &mut selected, &app.label).clicked() {
                            state.toggle_app(&app.id);
                        }
                        if selected {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let entry = state.app_mix_entry(&app.id);
                                    let mut muted = entry.muted;
                                    if checkbox(ui, &mut muted, &mute_label).clicked() {
                                        state.set_app_mix(
                                            &app.id,
                                            AppMixEntry {
                                                gain: entry.gain,
                                                muted,
                                            },
                                        );
                                    }
                                },
                            );
                        }
                    });
                    if selected {
                        let entry = state.app_mix_entry(&app.id);
                        let raw = vu.apps.get(&app.id).map(|v| v.rms).unwrap_or(0.0);
                        let shown = smooth_meter(
                            state.meters.apps.get(&app.id).copied().unwrap_or(0.0),
                            raw,
                            dt,
                        );
                        state.meters.apps.insert(app.id.clone(), shown);
                        ui.add_space(2.0);
                        vu_bar(ui, ui.available_width(), 10.0, shown);
                        gain_row(ui, i18n, entry.gain, |gain| {
                            state.set_app_mix(
                                &app.id,
                                AppMixEntry {
                                    gain,
                                    muted: entry.muted,
                                },
                            );
                        });
                    }
                    ui.add_space(4.0);
                }
            }
        }
        ui.add_space(6.0);
    }

    // --- microphone: enable/mute line, device combo, VU, gain ---------------
    crate::ui::widgets::rule(ui);
    ui.add_space(6.0);
    let inputs = state
        .devices
        .as_ref()
        .map(|d| d.inputs.clone())
        .unwrap_or_default();
    let mic_snapshot = state.mic.clone();
    let mic_label = i18n.t("audio.mic");
    let mute_label = i18n.t("audio.mute");
    let gain_label = i18n.t("audio.gain");
    let default_mic_label = i18n.t("audio.defaultMic");
    ui.horizontal(|ui| {
        let mut enabled = mic_snapshot.enabled;
        if checkbox(ui, &mut enabled, &mic_label).clicked() {
            state.set_mic(MicSource {
                enabled,
                ..mic_snapshot.clone()
            });
        }
        if mic_snapshot.enabled {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut muted = mic_snapshot.muted;
                if checkbox(ui, &mut muted, &mute_label).clicked() {
                    state.set_mic(MicSource {
                        muted,
                        ..mic_snapshot.clone()
                    });
                }
            });
        }
    });
    ui.add_space(2.0);
    {
        let selected_label = if mic_snapshot.device == "default" {
            default_mic_label.clone()
        } else {
            inputs
                .iter()
                .find(|d| d.id == mic_snapshot.device)
                .map(|d| d.label.clone())
                .unwrap_or_else(|| mic_snapshot.device.clone())
        };
        let mut device = mic_snapshot.device.clone();
        egui::ComboBox::from_id_salt("mic-device")
            .width(ui.available_width())
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut device,
                    "default".to_string(),
                    default_mic_label.clone(),
                );
                for input in &inputs {
                    ui.selectable_value(&mut device, input.id.clone(), &input.label);
                }
            });
        if device != mic_snapshot.device {
            state.set_mic(MicSource {
                device,
                ..mic_snapshot.clone()
            });
        }
    }
    if mic_snapshot.enabled {
        ui.add_space(2.0);
        let mic = smooth_meter(
            state.meters.mic,
            vu.mic.as_ref().map(|m| m.rms).unwrap_or(0.0),
            dt,
        );
        state.meters.mic = mic;
        vu_bar(ui, ui.available_width(), 10.0, mic);
        let mic_gain = mic_snapshot.gain;
        gain_row(ui, i18n, mic_gain, |gain| {
            state.set_mic(MicSource {
                gain,
                ..mic_snapshot.clone()
            });
        });
        let _ = &gain_label;
    }
    if inputs_empty(&state.devices) {
        small_hint(ui, &i18n.t("audio.noInputs"));
    }
    if is_live {
        ui.add_space(4.0);
        small_hint(ui, &i18n.t("audio.liveHint"));
    }
    let _ = Sense::hover();
}

/// Gain line: small label + fixed slider + monospace value. The three parts
/// total ~200px and always fit the narrowest (~280px) column.
fn gain_row(ui: &mut Ui, i18n: &I18n, current: f32, on_change: impl FnOnce(f32)) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("audio.gain"))
                .size(11.5)
                .color(DIM),
        );
        let mut gain = current;
        if crate::ui::widgets::slider(ui, &mut gain, 0.0..=2.0).changed() {
            on_change(gain);
        }
        ui.label(
            RichText::new(format!("{:.2}", gain))
                .monospace()
                .size(11.0)
                .color(FAINT),
        );
    });
}

fn inputs_empty(devices: &Option<ezstreamer_core::ipc_types::AudioDevices>) -> bool {
    devices
        .as_ref()
        .map(|d| d.inputs.is_empty())
        .unwrap_or(false)
}
