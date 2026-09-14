//! Destination inputs: ingest URL + stream key (F-URL-01/02). Shown in the
//! right column of the bottom bar (`views/bottom.rs`). The watch-URL copy
//! rows live in the right-side controls panel (`views/controls.rs`).

use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{label, section_header};
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, state: &mut UiState, i18n: &I18n) {
    section_header(ui, &i18n.t("output.destination"), |_| {});

    ui.horizontal(|ui| {
        label(ui, &i18n.t("stream.ingest"));
        let width = (ui.available_width() - 4.0).max(100.0);
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
        label(ui, &i18n.t("stream.key"));
        let width = (ui.available_width() - 4.0).max(100.0);
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
    if let Some(key_error) = state.key_error() {
        ui.label(RichText::new(i18n.t(key_error)).size(11.5).color(LIVE));
    } else if state.generic_key() {
        ui.label(
            RichText::new(i18n.t("stream.keyGeneric"))
                .size(11.5)
                .color(WARN),
        );
    }
}
