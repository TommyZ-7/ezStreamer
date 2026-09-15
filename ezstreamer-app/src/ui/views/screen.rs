//! Capture source selection (F-SC-01/02/04). Shown in the left column of the
//! bottom bar (`views/bottom.rs`); the preview canvas lives in
//! `views/preview.rs`.

use crate::backend::{Backend, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, checkbox, section_header, small_hint, ButtonKind};
use egui::{pos2, vec2, Align2, FontId, RichText, Sense, Stroke, StrokeKind, Ui};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
) {
    let (busy_picking, busy_switching, live) = {
        let shared = shared.lock().unwrap();
        (
            shared.busy == Some(crate::backend::Busy::Picking),
            shared.busy == Some(crate::backend::Busy::Switching) || state.switching,
            shared.status.is_live || shared.status.retrying.is_some(),
        )
    };

    // --- section header: title left, reload right ----------------------------
    // カーソル切替は見出し行の外に独立行 (下部バーは区画幅が狭いため)。
    let title = i18n.t("screen.title");
    let cursor_label = i18n.t("screen.cursor");
    let reload_label = i18n.t("screen.reload");
    section_header(ui, &title, |ui| {
        if button(ui, &reload_label, ButtonKind::Normal, true).clicked() {
            backend.send(Command::RefreshSources);
        }
    });
    let mut cursor = state.cursor;
    if checkbox(ui, &mut cursor, &cursor_label).clicked() {
        let changed = cursor != state.cursor;
        state.cursor = cursor;
        state.mark_persist();
        // Windows embeds the cursor at capture start, so a live toggle
        // needs a source switch. On Linux the cursor mode is fixed at
        // pick time: only persist, the next picker applies it.
        if changed && live && !cfg!(target_os = "linux") {
            switch_live(state, backend, shared);
        } else {
            restart_preview_if_needed(state, shared);
        }
    }
    if live {
        small_hint(ui, &i18n.t("screen.liveHint"));
    }
    if busy_switching {
        small_hint(ui, &i18n.t("screen.switching"));
    }

    // --- source selection ---------------------------------------------------
    // Narrow column: stack rows vertically, never pack two variable-width
    // widgets on one line. Every control takes the full column width.
    if cfg!(target_os = "linux") {
        // Wayland/Portal: the OS picker is the selection surface.
        // Live switches go through PortalPicked -> SwitchScreen (ui/mod.rs).
        // Button on its own row, status below: the "button + long id" single
        // line always overflowed the 30% column.
        if button(
            ui,
            &i18n.t("screen.portalPicker"),
            ButtonKind::Primary,
            !busy_picking && !busy_switching,
        )
        .clicked()
        {
            backend.send(Command::PortalPicker {
                cursor: state.cursor,
            });
        }
        if state.screen.id.starts_with("portal:") {
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{} {}",
                        i18n.t("screen.portalSelected"),
                        state.screen.id
                    ))
                    .size(12.0)
                    .color(OK),
                )
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
        }
    } else {
        let labels = vec![i18n.t("screen.display"), i18n.t("screen.window")];
        let selected = if state.screen.kind == ezstreamer_core::config::ScreenTargetKind::Display {
            0
        } else {
            1
        };
        if let Some(idx) = crate::ui::widgets::segmented(ui, &labels, selected, !busy_switching) {
            let id = if idx == 0 {
                state
                    .displays
                    .first()
                    .map(|d| d.id.clone())
                    .unwrap_or_default()
            } else {
                state
                    .windows
                    .first()
                    .map(|w| w.id.clone())
                    .unwrap_or_default()
            };
            if !id.is_empty() {
                state.screen = ezstreamer_core::config::ScreenTarget {
                    kind: if idx == 0 {
                        ezstreamer_core::config::ScreenTargetKind::Display
                    } else {
                        ezstreamer_core::config::ScreenTargetKind::Window
                    },
                    id,
                };
                state.mark_persist();
                apply_source_change(state, backend, shared, live);
            }
        }
        ui.add_space(6.0);

        if selected == 0 {
            let displays = state.displays.clone();
            if displays.is_empty() {
                small_hint(ui, &i18n.t("screen.none"));
            } else {
                // Full-width vertical list: wrapped chips in a 30% column
                // produced ragged half-rows and clipped labels.
                ui.vertical(|ui| {
                    for display in &displays {
                        let is_selected = state.screen.id == display.id;
                        if choice_chip(
                            ui,
                            &format!("{}  {}x{}", display.label, display.w, display.h),
                            is_selected,
                        ) {
                            state.screen = ezstreamer_core::config::ScreenTarget {
                                kind: ezstreamer_core::config::ScreenTargetKind::Display,
                                id: display.id.clone(),
                            };
                            state.mark_persist();
                            apply_source_change(state, backend, shared, live);
                        }
                        ui.add_space(4.0);
                    }
                });
            }
        } else {
            let windows = state.windows.clone();
            if windows.is_empty() {
                small_hint(ui, &i18n.t("screen.none"));
            } else {
                let mut current = state.screen.id.clone();
                egui::ComboBox::from_id_salt("window-select")
                    .width(ui.available_width())
                    .selected_text(
                        windows
                            .iter()
                            .find(|w| w.id == current)
                            .map(|w| format!("{} ({})", w.title, w.app))
                            .unwrap_or_else(|| "--".to_string()),
                    )
                    .show_ui(ui, |ui| {
                        for window in &windows {
                            ui.selectable_value(
                                &mut current,
                                window.id.clone(),
                                format!("{} ({})", window.title, window.app),
                            );
                        }
                    });
                if current != state.screen.id && !current.is_empty() {
                    state.screen = ezstreamer_core::config::ScreenTarget {
                        kind: ezstreamer_core::config::ScreenTargetKind::Window,
                        id: current,
                    };
                    state.mark_persist();
                    apply_source_change(state, backend, shared, live);
                }
            }
        }
    }
    if state.displays.is_empty() && cfg!(not(target_os = "linux")) {
        ui.add_space(4.0);
        small_hint(ui, &i18n.t("screen.notAvailable"));
    }
}

/// Source change routing: live streams hot-swap via SwitchScreen, idle or
/// previewing restarts the 5fps preview capture (debounced).
fn apply_source_change(
    state: &mut UiState,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
    live: bool,
) {
    if live {
        switch_live(state, backend, shared);
    } else {
        restart_preview_if_needed(state, shared);
    }
}

/// Send the current selection to the running stream. The authoritative screen
/// comes back via `ScreenSwitched`; failures surface as `Error` toasts.
fn switch_live(state: &mut UiState, backend: &Backend, _shared: &Arc<Mutex<Shared>>) {
    state.preview_restart_due = None;
    state.switching = true;
    backend.send(Command::SwitchScreen {
        screen: state.screen.clone(),
        cursor: state.cursor,
    });
}

/// Screen switches while previewing restart the capture (debounced by the app
/// loop to 250ms so only the last selection reaches the backend).
fn restart_preview_if_needed(state: &mut UiState, shared: &Arc<Mutex<Shared>>) {
    if shared.lock().unwrap().previewing {
        state.preview_restart_due = Some(std::time::Instant::now());
    }
}

fn choice_chip(ui: &mut Ui, text: &str, selected: bool) -> bool {
    // Full column width: the old capped (240px) wrapped chips left
    // half-empty rows in the narrow source column.
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        ACCENT_BG
    } else if response.hovered() {
        ROW_HOVER
    } else {
        PANEL_2
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, if selected { ACCENT } else { LINE_STRONG }),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.5),
        if selected { TEXT } else { DIM },
    );
    response.clicked()
}
