//! Bottom dock: ingest / key / playback URLs / big start-stop toggle.
//! Always visible (requirements §7: 配信開始ボタンは常時下部固定) and divided
//! from the content by a single hairline.

use crate::backend::{Backend, Busy, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{copy_row, small_hint, status_square};
use egui::{Context, Frame, Margin, RichText, Stroke, TopBottomPanel};
use std::sync::{Arc, Mutex};

pub fn show(
    ctx: &Context,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
) {
    let snapshot = {
        let shared = shared.lock().unwrap();
        (shared.status, shared.busy)
    };
    let (status, busy) = snapshot;
    let active = status.is_live || status.retrying.is_some();
    let busy_start = state.starting || busy == Some(Busy::Starting);
    let busy_stop = state.stopping || busy == Some(Busy::Stopping);

    TopBottomPanel::bottom("dock")
        .frame(
            Frame::NONE
                .fill(PANEL)
                .inner_margin(Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );

            // ingest + key (stacked full-width rows for breathing room)
            ui.horizontal(|ui| {
                ui.add_sized(
                    [96.0, ROW_H],
                    egui::Label::new(RichText::new(i18n.t("stream.ingest")).size(12.5).color(DIM)),
                );
                let width = (ui.available_width() - 4.0).max(160.0);
                let response = ui.add_sized(
                    [width, ROW_H],
                    egui::TextEdit::singleline(&mut state.ingest_url)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(width),
                );
                if response.changed() {
                    state.mark_persist();
                }
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [96.0, ROW_H],
                    egui::Label::new(RichText::new(i18n.t("stream.key")).size(12.5).color(DIM)),
                );
                let width = (ui.available_width() - 4.0).max(160.0);
                let response = ui.add_sized(
                    [width, ROW_H],
                    egui::TextEdit::singleline(&mut state.stream_key)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("my-event-123"),
                );
                if response.changed() {
                    state.mark_persist();
                }
            });

            ui.add_space(2.0);
            let key = if state.stream_key.is_empty() {
                "your-key"
            } else {
                state.stream_key.as_str()
            };
            let (pc, quest) = ezstreamer_core::urls::playback_urls(&state.ingest_url, key);
            ui.horizontal(|ui| {
                let half = (ui.available_width() - 16.0) / 2.0;
                ui.allocate_ui(egui::vec2(half, ROW_H), |ui| {
                    if copy_row(ui, &i18n.t("stream.copyPc"), &pc, &i18n.t("stream.copy")) {
                        ui.ctx().copy_text(pc.clone());
                        state.toast(i18n.t("stream.copied"), false);
                    }
                });
                ui.allocate_ui(egui::vec2(half, ROW_H), |ui| {
                    if copy_row(
                        ui,
                        &i18n.t("stream.copyQuest"),
                        &quest,
                        &i18n.t("stream.copy"),
                    ) {
                        ui.ctx().copy_text(quest.clone());
                        state.toast(i18n.t("stream.copied"), false);
                    }
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if let Some(retry) = status.retrying {
                    status_square(ui, WARN);
                    ui.label(
                        RichText::new(i18n.tf("stream.retrying", &[("n", &retry.to_string())]))
                            .size(12.0)
                            .color(WARN),
                    );
                } else if status.is_live {
                    status_square(ui, LIVE);
                    ui.label(
                        RichText::new(format!(
                            "{}  {} kbps  {} {}",
                            i18n.t("stream.live"),
                            status.bitrate_kbps.round() as u64,
                            i18n.t("stream.dropped"),
                            status.dropped_frames
                        ))
                        .size(12.0)
                        .color(DIM),
                    );
                } else if let Some(error) = &state.last_error {
                    ui.label(RichText::new(error).size(11.5).color(LIVE));
                } else if let Some(key_error) = state.key_error() {
                    ui.label(RichText::new(i18n.t(key_error)).size(11.5).color(LIVE));
                } else if state.generic_key() {
                    ui.label(
                        RichText::new(i18n.t("stream.keyGeneric"))
                            .size(11.5)
                            .color(WARN),
                    );
                } else if state.screen.id.is_empty() {
                    small_hint(ui, &i18n.t("stream.selectScreen"));
                } else {
                    small_hint(ui, &i18n.t("stream.stopped"));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if busy_stop {
                        i18n.t("stream.stopping")
                    } else if busy_start {
                        i18n.t("stream.starting")
                    } else if active {
                        i18n.t("stream.stop")
                    } else {
                        i18n.t("stream.start")
                    };
                    let enabled = if active {
                        !busy_stop && !busy_start
                    } else {
                        state.key_error().is_none() && !busy_start && !busy_stop
                    };
                    let widget = egui::Button::new(
                        RichText::new(label).size(15.0).strong().color(if enabled {
                            TEXT
                        } else {
                            FAINT
                        }),
                    )
                    .fill(if !enabled {
                        PANEL
                    } else if active {
                        LIVE_BG
                    } else {
                        ACCENT_BG
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if !enabled {
                            LINE
                        } else if active {
                            LIVE
                        } else {
                            ACCENT
                        },
                    ))
                    .corner_radius(0.0)
                    .min_size(egui::vec2(208.0, 38.0));
                    if ui.add_enabled(enabled, widget).clicked() {
                        if active {
                            state.stopping = true;
                            state.last_error = None;
                            backend.send(Command::StopStream);
                        } else {
                            state.starting = true;
                            state.last_error = None;
                            backend.send(Command::StartStream(Box::new(state.stream_config())));
                        }
                    }
                });
            });

            if cfg!(not(all(
                feature = "media",
                any(windows, target_os = "linux")
            ))) {
                ui.add_space(3.0);
                small_hint(ui, &i18n.t("stream.notAvailable"));
            }
        });
}
