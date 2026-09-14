//! Bottom dock: status + single primary CTA (Start/Stop).
//! Always visible (requirements §7: 配信開始ボタンは常時下部固定) and divided
//! from the content by a single hairline.
//!
//! 入力系 (ingest/key/視聴URL) は `output` タブの「配信先」に集約し、
//! ここは状態表示と開始/停止の1ボタンに専念させる。

use crate::backend::{Backend, Busy, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{primary_cta, small_hint, status_square};
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
                        // 画面未選択でも開始押下自体は許可し、不足は
                        // backend 側エラー + 出力タブの field hint で案内する。
                        // ここではキー不正のみを開始不可にする。
                        state.key_error().is_none() && !busy_start && !busy_stop
                    };
                    if primary_cta(ui, &label, active, enabled).clicked() {
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
