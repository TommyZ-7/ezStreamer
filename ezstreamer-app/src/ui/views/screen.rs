//! Step 1: capture source + preview (F-SC-01/02/03/04).

use crate::backend::{Backend, Command, Shared};
use crate::ui::i18n::I18n;
use crate::ui::state::UiState;
use crate::ui::theme::*;
use crate::ui::widgets::{button, section_header, small_hint, ButtonKind};
use egui::{
    pos2, vec2, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, TextureHandle, Ui,
};
use std::sync::{Arc, Mutex};

pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    i18n: &I18n,
    backend: &Backend,
    shared: &Arc<Mutex<Shared>>,
    preview_texture: &Option<TextureHandle>,
) {
    let (busy_picking, busy_switching, previewing, live) = {
        let shared = shared.lock().unwrap();
        (
            shared.busy == Some(crate::backend::Busy::Picking),
            shared.busy == Some(crate::backend::Busy::Switching) || state.switching,
            shared.previewing,
            shared.status.is_live || shared.status.retrying.is_some(),
        )
    };

    // --- section header: title left, cursor + reload right (max 2) --------
    let title = i18n.t("screen.title");
    let cursor_label = i18n.t("screen.cursor");
    let reload_label = i18n.t("screen.reload");
    section_header(ui, &title, |ui| {
        if button(ui, &reload_label, ButtonKind::Normal, true).clicked() {
            backend.send(Command::RefreshSources);
        }
        ui.add_space(6.0);
        let mut cursor = state.cursor;
        if crate::ui::widgets::checkbox(ui, &mut cursor, &cursor_label).clicked() {
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
    });
    if live {
        small_hint(ui, &i18n.t("screen.liveHint"));
    }
    if busy_switching {
        small_hint(ui, &i18n.t("screen.switching"));
    }

    // --- source selection ---------------------------------------------------
    if cfg!(target_os = "linux") {
        // Wayland/Portal: the OS picker is the selection surface.
        // Live switches go through PortalPicked -> SwitchScreen (ui/mod.rs).
        ui.horizontal(|ui| {
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
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        i18n.t("screen.portalSelected"),
                        state.screen.id
                    ))
                    .size(12.0)
                    .color(OK),
                );
            }
        });
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
                ui.horizontal_wrapped(|ui| {
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
                    .width((ui.available_width() - 8.0).min(520.0))
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
    ui.add_space(8.0);

    // --- preview (F-SC-03) --------------------------------------------------
    // During a live stream the capture already feeds this texture at 1fps,
    // so there is no separate preview to start/stop: switching happens from
    // the selection above.
    let previewing = previewing || shared.lock().unwrap().previewing;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(i18n.t("screen.preview"))
                .size(12.5)
                .color(DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if live {
                ui.label(RichText::new(i18n.t("stream.live")).size(12.0).color(LIVE));
                return;
            }
            let has_source = !state.screen.id.is_empty();
            let label = if previewing {
                i18n.t("screen.previewStop")
            } else {
                i18n.t("screen.previewStart")
            };
            let kind = if previewing {
                ButtonKind::Danger
            } else {
                ButtonKind::Normal
            };
            let enabled = has_source || previewing;
            if button(ui, &label, kind, enabled).clicked() {
                if previewing {
                    backend.send(Command::StopPreview);
                } else {
                    state.preview_restart_due = None;
                    backend.send(Command::StartPreview(Box::new(state.stream_config())));
                }
            }
        });
    });

    let width = ui.available_width().min(520.0);
    let height = (width * 9.0 / 16.0).max(120.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, LINE_STRONG),
        StrokeKind::Inside,
    );
    if let Some(texture) = preview_texture {
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            i18n.t("screen.previewEmpty"),
            FontId::proportional(12.0),
            FAINT,
        );
    }
    ui.add_space(4.0);
    if live {
        small_hint(ui, &i18n.t("screen.livePreviewHint"));
    } else {
        small_hint(ui, &i18n.t("screen.previewHint"));
    }

    if state.displays.is_empty() && cfg!(not(target_os = "linux")) {
        ui.add_space(4.0);
        small_hint(ui, &i18n.t("screen.notAvailable"));
    }
}

/// Source change routing: live streams hot-swap via SwitchScreen, idle or
/// previewing restarts the 1fps preview capture (debounced).
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
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width().min(240.0), ROW_H), Sense::click());
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
