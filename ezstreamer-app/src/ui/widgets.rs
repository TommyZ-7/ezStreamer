//! Custom flat/linear widgets. egui defaults are themed, but the connected
//! segmented controls, segmented VU meters and full-row checkboxes are drawn
//! here so the composition stays hairline-based (no rounded pills, no fills
//! with rounded corners, no shadows).

use crate::ui::theme::*;
use egui::{
    pos2, vec2, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
};

/// Flat button variants: normal (panel), primary (accent), danger (live).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Normal,
    Primary,
    Danger,
}

pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let (fill, stroke, fg) = match kind {
        ButtonKind::Normal => (PANEL_2, LINE_STRONG, TEXT),
        ButtonKind::Primary => (ACCENT_BG, ACCENT, TEXT),
        ButtonKind::Danger => (LIVE_BG, LIVE, TEXT),
    };
    let widget = egui::Button::new(RichText::new(label).color(fg).size(13.0))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, stroke))
        .corner_radius(0.0)
        .min_size(vec2(0.0, ROW_H));
    if enabled {
        ui.add(widget)
    } else {
        ui.add_enabled(
            false,
            egui::Button::new(RichText::new(label).color(FAINT).size(13.0))
                .fill(PANEL)
                .stroke(Stroke::new(1.0_f32, LINE))
                .corner_radius(0.0)
                .min_size(vec2(0.0, ROW_H)),
        )
    }
}

pub fn icon_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let widget = egui::Button::new(RichText::new(label).color(DIM).size(12.0))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, LINE_STRONG))
        .corner_radius(0.0)
        .min_size(vec2(56.0, 22.0));
    ui.add_enabled(enabled, widget)
}

/// Connected segmented control: one outline, shared dividers, selected cell
/// gets an accent fill + accent underline. Click position selects the cell.
pub fn segmented(ui: &mut Ui, labels: &[String], selected: usize, enabled: bool) -> Option<usize> {
    let width = ui.available_width().min(520.0);
    segmented_sized(ui, width, labels, selected, enabled)
}

pub fn segmented_sized(
    ui: &mut Ui,
    width: f32,
    labels: &[String],
    selected: usize,
    enabled: bool,
) -> Option<usize> {
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, ROW_H),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let n = labels.len().max(1);
    let cell_w = rect.width() / n as f32;
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, PANEL_2);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, LINE_STRONG),
        StrokeKind::Inside,
    );
    for i in 1..n {
        let x = rect.left() + cell_w * i as f32;
        painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, LINE));
    }

    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(rect.left() + cell_w * i as f32, rect.top()),
            vec2(cell_w, rect.height()),
        );
        let is_selected = i == selected;
        let is_hovered = enabled && response.hover_pos().is_some_and(|p| cell.contains(p));
        if is_selected {
            painter.rect_filled(cell, 0.0, ACCENT_BG);
            painter.hline(
                cell.x_range(),
                cell.bottom() - 1.0,
                Stroke::new(2.0_f32, ACCENT),
            );
        } else if is_hovered {
            painter.rect_filled(cell, 0.0, ROW_HOVER);
        }
        let color = if !enabled {
            FAINT
        } else if is_selected {
            TEXT
        } else {
            DIM
        };
        painter.text(
            cell.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(13.0),
            color,
        );
    }
    if enabled && response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let idx = ((pos.x - rect.left()) / cell_w).floor() as usize;
            if idx < n {
                clicked = Some(idx);
            }
        }
    }
    clicked
}

/// Square checkbox sized to its label (no rounded checkbox glyph).
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let font = FontId::proportional(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), TEXT);
    let (rect, response) =
        ui.allocate_exact_size(vec2(14.0 + 6.0 + galley.size().x, ROW_H), Sense::click());
    let box_rect = Rect::from_min_size(pos2(rect.left(), rect.center().y - 7.0), vec2(14.0, 14.0));
    let painter = ui.painter();
    if *checked {
        painter.rect_filled(box_rect, 0.0, ACCENT);
        painter.line_segment(
            [
                pos2(box_rect.left() + 3.0, box_rect.center().y),
                pos2(box_rect.center().x, box_rect.bottom() - 3.5),
            ],
            Stroke::new(1.6_f32, BG),
        );
        painter.line_segment(
            [
                pos2(box_rect.center().x, box_rect.bottom() - 3.5),
                pos2(box_rect.right() - 3.0, box_rect.top() + 3.5),
            ],
            Stroke::new(1.6_f32, BG),
        );
    }
    painter.rect_stroke(
        box_rect,
        0.0,
        Stroke::new(
            1.0_f32,
            if *checked {
                ACCENT
            } else if response.hovered() {
                LINE_STRONG
            } else {
                LINE_STRONG
            },
        ),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(box_rect.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        TEXT,
    );
    if response.clicked() {
        *checked = !*checked;
    }
    response
}

/// Flat gain slider: hairline track + square handle (no circular knob, no
/// gradient). Returns the usual `Response`; `.changed()` reports edits.
pub fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(110.0, ROW_H), Sense::click_and_drag());
    let (min, max) = (*range.start(), *range.end());
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let track_h = 4.0;
    let track = Rect::from_min_size(
        pos2(rect.left(), rect.center().y - track_h / 2.0),
        vec2(rect.width(), track_h),
    );
    let painter = ui.painter();
    painter.rect_filled(track, 0.0, PANEL);
    painter.rect_stroke(
        track,
        0.0,
        Stroke::new(1.0_f32, LINE_STRONG),
        StrokeKind::Inside,
    );
    let filled_w = track.width() * t;
    painter.rect_filled(
        Rect::from_min_size(track.min, vec2(filled_w, track_h)),
        0.0,
        ACCENT,
    );
    let knob = Rect::from_center_size(
        pos2(track.left() + filled_w, track.center().y),
        vec2(8.0, 16.0),
    );
    painter.rect_filled(knob, 0.0, if response.hovered() { TEXT } else { DIM });
    painter.rect_stroke(
        knob,
        0.0,
        Stroke::new(1.0_f32, LINE_STRONG),
        StrokeKind::Inside,
    );
    if (response.dragged() || response.clicked()) && track.width() > 0.0 {
        if let Some(pos) = response.interact_pointer_pos() {
            let nt = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let new_value = min + nt * (max - min);
            if (new_value - *value).abs() > f32::EPSILON {
                *value = new_value;
                response.mark_changed();
            }
        }
    }
    response
}

/// Segmented VU meter: discrete blocks, hard color steps (no gradient).
pub fn vu_bar(ui: &mut Ui, width: f32, height: f32, level: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, PANEL);
    let segments = ((rect.width() / 5.0) as usize).clamp(8, 40);
    let gap = 1.0;
    let seg_w = (rect.width() - gap * (segments.saturating_sub(1)) as f32) / segments as f32;
    let lit = (level.clamp(0.0, 1.0) * segments as f32).round() as usize;
    for i in 0..segments {
        let x = rect.left() + i as f32 * (seg_w + gap);
        let seg = Rect::from_min_size(
            pos2(x, rect.top() + 1.0),
            vec2(seg_w.max(1.0), rect.height() - 2.0),
        );
        let color = if i < lit {
            meter_color(i, segments)
        } else {
            METER_OFF
        };
        painter.rect_filled(seg, 0.0, color);
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, LINE_STRONG),
        StrokeKind::Inside,
    );
}

/// Fixed-width dim label used to align form rows.
pub fn label(ui: &mut Ui, text: &str) {
    ui.add_sized(
        vec2(88.0, ROW_H),
        egui::Label::new(RichText::new(text).color(DIM).size(12.5))
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}

pub fn small_hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(FAINT).size(11.5));
}

/// Full-width 1px rule.
pub fn rule(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, LINE);
}

/// Small filled square used as a status marker (live / usable).
pub fn status_square(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// Monospace copy row: label | URL | copy button.
pub fn copy_row(ui: &mut Ui, label_text: &str, url: &str, copy_label: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        label(ui, label_text);
        let width = (ui.available_width() - 64.0).max(60.0);
        ui.add_sized(
            vec2(width, ROW_H),
            egui::Label::new(RichText::new(url).monospace().color(TEXT).size(12.0))
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
        if icon_button(ui, copy_label, true).clicked() {
            clicked = true;
        }
    });
    clicked
}
