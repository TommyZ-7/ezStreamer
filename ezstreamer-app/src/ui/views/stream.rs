//! Right-side stream panel (中段右): stream key input, watch-URL copies and
//! the single start/stop CTA — nothing else. Split into two draw functions:
//! `settings` (upper, flexes) and `cta` (pinned to the panel bottom by
//! `ui/mod.rs` via `TopBottomPanel::show_inside`). The Ingest URL lives in
//! the settings pane (`views/settings.rs`).

use crate::backend::{Backend, Busy, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{copy_row, primary_cta, rule, section_header, small_hint};
use egui::{RichText, Ui};
use std::sync::{Arc, Mutex};

/// Upper section: stream key + watch URLs (scrollable when overflowing).
pub fn settings(ui: &mut Ui, state: &mut UiState, i18n: &I18n) {
    section_header(ui, &i18n.t("stream.title"), |_| {});

    // --- stream key ---------------------------------------------------------
    ui.label(
        RichText::new(i18n.t("stream.key")).size(12.5).color(DIM),
    );
    let width = (ui.available_width() - 4.0).max(120.0);
    let response = ui.add_sized(
        [width, ROW_H],
        egui::TextEdit::singleline(&mut state.stream_key)
            .font(egui::TextStyle::Monospace)
            .hint_text("my-event-123")
            .desired_width(width),
    );
    if response.changed() {
        state.mark_persist();
    }
    ui.add_space(2.0);
    if let Some(key_error) = state.key_error() {
        ui.label(RichText::new(i18n.t(key_error)).size(11.5).color(LIVE));
    } else if state.generic_key() {
        ui.label(
            RichText::new(i18n.t("stream.keyGeneric"))
                .size(11.5)
                .color(WARN),
        );
    }

    // --- watch URLs ---------------------------------------------------------
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
    small_hint(ui, &i18n.t("stream.ingestHint"));
}

/// Lower section: the single primary CTA (F-ST-01: only an invalid key blocks
/// start; a missing screen start is allowed and reported as an error toast).
pub fn cta(
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
}
