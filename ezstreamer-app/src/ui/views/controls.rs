//! Right-side controls panel: the single stream CTA plus watch URLs.
//! Drawn as the bottom section of the right panel (`ui/mod.rs` uses
//! `TopBottomPanel::show_inside`) so the mixer above flexes and the CTA
//! stays pinned at the bottom, OBS-style.

use crate::backend::{Backend, Busy, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{copy_row, primary_cta, rule};
use egui::{RichText, Ui};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
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

    // Single primary CTA (F-ST-01: only an invalid key blocks start; a
    // missing screen start is allowed and reported as an error toast).
    ui.vertical_centered(|ui| {
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

    // --- watch URLs (moved from the old output section) ----------------------
    ui.add_space(8.0);
    rule(ui);
    ui.add_space(4.0);
    ui.label(
        RichText::new(i18n.t("stream.playback")).size(12.5).color(DIM),
    );
    let key = if state.stream_key.is_empty() {
        "your-key"
    } else {
        state.stream_key.as_str()
    };
    let (pc, quest) = ezstreamer_core::urls::playback_urls(&state.ingest_url, key);
    if copy_row(ui, &i18n.t("stream.copyPc"), &pc, &i18n.t("stream.copy")) {
        ui.ctx().copy_text(pc.clone());
        state.toast(i18n.t("stream.copied"), false);
    }
    if copy_row(
        ui,
        &i18n.t("stream.copyQuest"),
        &quest,
        &i18n.t("stream.copy"),
    ) {
        ui.ctx().copy_text(quest.clone());
        state.toast(i18n.t("stream.copied"), false);
    }
}
