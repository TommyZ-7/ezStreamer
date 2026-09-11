//! Flat, linear design system.
//!
//! Rules (user requirement):
//! - straight, connected composition: 1px hairlines, square corners, no
//!   floating cards, sections share edges instead of adding shadows;
//! - no gradients, no emoji, no rounded corners, no drop shadows;
//! - one accent color (blue); red = live, amber = warning, green = signal.
//!
//! Everything is painted from these tokens so the whole app stays consistent.

use egui::{
    epaint::Shadow, Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x23, 0x24, 0x2C);
pub const PANEL: Color32 = Color32::from_rgb(0x2A, 0x2B, 0x34);
pub const PANEL_2: Color32 = Color32::from_rgb(0x32, 0x33, 0x3E);
pub const INPUT_BG: Color32 = Color32::from_rgb(0x3A, 0x3B, 0x48);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x40, 0x41, 0x50);
pub const LINE: Color32 = Color32::from_rgb(0x46, 0x47, 0x57);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x5E, 0x5F, 0x72);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF4);
pub const DIM: Color32 = Color32::from_rgb(0xB8, 0xB9, 0xC2);
pub const FAINT: Color32 = Color32::from_rgb(0x8B, 0x8C, 0x98);
pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8D, 0xFF);
pub const ACCENT_BG: Color32 = Color32::from_rgb(0x1E, 0x2E, 0x52);
pub const LIVE: Color32 = Color32::from_rgb(0xFF, 0x60, 0x60);
pub const LIVE_BG: Color32 = Color32::from_rgb(0x42, 0x22, 0x22);
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xB8, 0x30);
pub const OK: Color32 = Color32::from_rgb(0x45, 0xC7, 0x7A);
pub const METER_OFF: Color32 = Color32::from_rgb(0x36, 0x37, 0x43);

pub const ROW_H: f32 = 26.0;
pub const PAD: f32 = 14.0;

/// Fonts: Latin from egui defaults, CJK from the bundled Noto Sans JP.
/// The emoji fonts are removed on purpose (design rule).
pub fn install_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.remove("NotoEmoji-Regular");
    fonts.font_data.remove("emoji-icon-font");
    for family in fonts.families.values_mut() {
        family.retain(|name| name != "NotoEmoji-Regular" && name != "emoji-icon-font");
    }
    fonts.font_data.insert(
        "noto-sans-jp".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansJP-Regular.otf"
        ))),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("proportional family exists")
        .push("noto-sans-jp".into());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .expect("monospace family exists")
        .push("noto-sans-jp".into());
    ctx.set_fonts(fonts);
}

/// Square corners, hairlines, flat fills, no shadows anywhere.
///
/// Palette intent: soft charcoal grays (no pure black), inputs in the same
/// dark family as the background (no white boxes), secondary text bright
/// enough for WCAG AA on `BG`.
pub fn apply(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = Visuals::dark();

    visuals.panel_fill = BG;
    visuals.window_fill = PANEL_2;
    visuals.extreme_bg_color = INPUT_BG;
    visuals.text_edit_bg_color = Some(INPUT_BG);
    visuals.faint_bg_color = PANEL_2;
    visuals.code_bg_color = PANEL_2;
    visuals.striped = false;
    visuals.window_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.selection.bg_fill = ACCENT_BG;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = LIVE;
    visuals.weak_text_color = Some(DIM);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;

    let widgets = &mut visuals.widgets;
    // Default body text must be `TEXT`; `DIM`/`FAINT` are opt-in secondary.
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    // Inputs (TextEdit / ComboBox / DragValue): dark fill from the same
    // family as `BG` with a visible outline even when unfocused. Focus uses
    // `selection.stroke` (accent) via TextEdit's own frame painting.
    widgets.inactive.bg_fill = INPUT_BG;
    widgets.inactive.weak_bg_fill = INPUT_BG;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.inactive.corner_radius = CornerRadius::ZERO;
    widgets.hovered.bg_fill = ROW_HOVER;
    widgets.hovered.weak_bg_fill = ROW_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.hovered.corner_radius = CornerRadius::ZERO;
    widgets.active.bg_fill = ACCENT_BG;
    widgets.active.weak_bg_fill = ACCENT_BG;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.active.corner_radius = CornerRadius::ZERO;
    widgets.open.bg_fill = ROW_HOVER;
    widgets.open.weak_bg_fill = ROW_HOVER;
    widgets.open.bg_stroke = Stroke::new(1.0_f32, LINE_STRONG);
    widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.open.corner_radius = CornerRadius::ZERO;

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 3.0);
    style.spacing.slider_width = 110.0;
    style.spacing.interact_size.y = ROW_H;
    style.spacing.scroll.bar_width = 8.0;
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style(style);
}

/// Discrete color for a VU segment index (no gradient: hard steps).
pub fn meter_color(index: usize, total: usize) -> Color32 {
    let ratio = (index + 1) as f32 / total.max(1) as f32;
    if ratio > 0.9 {
        LIVE
    } else if ratio > 0.72 {
        WARN
    } else {
        OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Color32) -> f32 {
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (hi, lo) = {
            let (x, y) = (luminance(a), luminance(b));
            if x > y { (x, y) } else { (y, x) }
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn body_text_has_strong_contrast_on_bg() {
        assert!(contrast(TEXT, BG) >= 7.0, "TEXT on BG must be AAA");
        assert!(contrast(DIM, BG) >= 4.5, "DIM on BG must be AA");
        assert!(contrast(FAINT, BG) >= 4.0, "hints must stay readable");
    }

    #[test]
    fn inputs_stay_in_dark_family() {
        // Regression guard for "white input boxes": inputs must be dark grays
        // from the same family as BG, never near-white.
        for c in [INPUT_BG, PANEL, PANEL_2] {
            assert!(luminance(c) < 0.10, "surface {c:?} must stay dark");
            assert!(c.r() < 0x80 && c.g() < 0x80 && c.b() < 0x90);
        }
        assert!(luminance(BG) > 0.012, "BG must not be pure black");
    }

    #[test]
    fn hairlines_stay_visible_on_bg() {
        assert!(contrast(LINE_STRONG, BG) >= 2.0);
        assert!(contrast(LINE, BG) >= 1.5);
    }

    #[test]
    fn apply_wires_inputs_to_dark_fill() {
        let ctx = Context::default();
        apply(&ctx);
        let visuals = ctx.style().visuals.clone();
        assert_eq!(visuals.text_edit_bg_color(), INPUT_BG);
        assert_eq!(visuals.extreme_bg_color, INPUT_BG);
        assert_eq!(visuals.widgets.inactive.bg_fill, INPUT_BG);
        assert_eq!(visuals.widgets.inactive.text_color(), TEXT);
        assert_eq!(visuals.text_color(), TEXT);
    }
}
