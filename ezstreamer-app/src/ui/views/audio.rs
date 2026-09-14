//! Step 2: audio sources, per-source VU/gain/mute (F-AU-01..06).

use crate::backend::Shared;
use crate::ui::i18n::I18n;
use crate::ui::state::{AppMixEntry, AudioMode, UiState};
use crate::ui::theme::*;
use crate::ui::widgets::{checkbox, label, section_header, small_hint, vu_bar};
use egui::{RichText, Sense, Ui};
use ezstreamer_core::config::MicSource;
use std::sync::{Arc, Mutex};

pub fn show(ui: &mut Ui, state: &mut UiState, i18n: &I18n, shared: &Arc<Mutex<Shared>>) {
    let vu = shared.lock().unwrap().vu.clone();
    let is_live = shared.lock().unwrap().status.is_live;

    section_header(ui, &i18n.t("audio.title"), |_| {});

    // F-ST-03: master VU over the mixed output.
    ui.horizontal(|ui| {
        label(ui, &i18n.t("audio.master"));
        vu_bar(ui, 280.0, 12.0, vu.master.rms);
        ui.label(
            RichText::new(format!("{:.2}", vu.master.rms))
                .monospace()
                .size(11.0)
                .color(FAINT),
        );
    });
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

    // --- per-app list: 1 app = 2 rows ------------------------------------
    // Row 1: [checkbox name ........ mute right]
    // Row 2: [indent VU + gain slider + value]
    // Single-horizontal詰め込みをやめ、幅に依らず折り返さない固定形にする。
    if state.audio_mode == AudioMode::Apps {
        match &state.devices {
            None => small_hint(ui, &i18n.t("audio.noApps")),
            Some(devices) if devices.apps.is_empty() => small_hint(ui, &i18n.t("audio.noApps")),
            Some(devices) => {
                let apps = devices.apps.clone();
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
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            vu_bar(
                                ui,
                                200.0,
                                10.0,
                                vu.apps.get(&app.id).map(|v| v.rms).unwrap_or(0.0),
                            );
                            let mut gain = entry.gain;
                            if crate::ui::widgets::slider(ui, &mut gain, 0.0..=2.0).changed() {
                                state.set_app_mix(
                                    &app.id,
                                    AppMixEntry {
                                        gain,
                                        muted: entry.muted,
                                    },
                                );
                            }
                            ui.label(
                                RichText::new(format!("{:.2}", gain))
                                    .monospace()
                                    .size(11.0)
                                    .color(FAINT),
                            );
                        });
                    }
                    ui.add_space(4.0);
                }
            }
        }
        ui.add_space(6.0);
    }

    // --- microphone: same 2-row shape as apps ------------------------------
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
    let default_mic_label = i18n.t("audio.defaultMic");
    ui.horizontal(|ui| {
        let mut enabled = mic_snapshot.enabled;
        if checkbox(ui, &mut enabled, &mic_label).clicked() {
            state.set_mic(MicSource {
                enabled,
                ..mic_snapshot.clone()
            });
        }
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
            .width(220.0)
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
    if mic_snapshot.enabled {
        let mic_gain = mic_snapshot.gain;
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            vu_bar(
                ui,
                200.0,
                10.0,
                vu.mic.as_ref().map(|m| m.rms).unwrap_or(0.0),
            );
            let mut gain = mic_gain;
            if crate::ui::widgets::slider(ui, &mut gain, 0.0..=2.0).changed() {
                state.set_mic(MicSource {
                    gain,
                    ..mic_snapshot.clone()
                });
            }
            ui.label(
                RichText::new(format!("{:.2}", gain))
                    .monospace()
                    .size(11.0)
                    .color(FAINT),
            );
        });
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

fn inputs_empty(devices: &Option<ezstreamer_core::ipc_types::AudioDevices>) -> bool {
    devices
        .as_ref()
        .map(|d| d.inputs.is_empty())
        .unwrap_or(false)
}
