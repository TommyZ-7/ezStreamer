//! Bottom status bar: stream state at a glance, always visible.
//! The start/stop CTA lives in the right-side controls panel
//! (`views/controls.rs`); this bar only reports.

use crate::backend::{Busy, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{small_hint, status_square};
use egui::{Align, Context, Frame, Layout, Margin, RichText, Stroke, TopBottomPanel};
use std::sync::{Arc, Mutex};

pub fn show(ctx: &Context, state: &mut UiState, i18n: &I18n, shared: &Arc<Mutex<Shared>>) {
    let snapshot = {
        let shared = shared.lock().unwrap();
        (shared.status, shared.busy)
    };
    let (status, busy) = snapshot;
    let busy_start = state.starting || busy == Some(Busy::Starting);
    let busy_stop = state.stopping || busy == Some(Busy::Stopping);

    TopBottomPanel::bottom("status")
        .exact_height(30.0)
        .frame(
            Frame::NONE
                .fill(PANEL)
                .inner_margin(Margin::symmetric(16, 4)),
        )
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.top(),
                Stroke::new(1.0_f32, LINE_STRONG),
            );

            ui.horizontal_centered(|ui| {
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
                } else if busy_stop {
                    status_square(ui, FAINT);
                    ui.label(
                        RichText::new(i18n.t("stream.stopping"))
                            .size(12.0)
                            .color(DIM),
                    );
                } else if busy_start {
                    status_square(ui, FAINT);
                    ui.label(
                        RichText::new(i18n.t("stream.starting"))
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

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            i18n.t("app.version"),
                            env!("CARGO_PKG_VERSION")
                        ))
                        .size(11.0)
                        .color(FAINT),
                    );
                    if cfg!(not(all(
                        feature = "media",
                        any(windows, target_os = "linux")
                    ))) {
                        small_hint(ui, &i18n.t("stream.notAvailable"));
                    }
                });
            });
        });
}
