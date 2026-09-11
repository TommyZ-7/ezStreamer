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

pub const BG: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x13);
pub const PANEL: Color32 = Color32::from_rgb(0x15, 0x15, 0x1A);
pub const PANEL_2: Color32 = Color32::from_rgb(0x1B, 0x1B, 0x21);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x22, 0x22, 0x2A);
pub const LINE: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x32);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x3C, 0x3C, 0x46);
pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xEC);
pub const DIM: Color32 = Color32::from_rgb(0x95, 0x95, 0x9F);
pub const FAINT: Color32 = Color32::from_rgb(0x5B, 0x5B, 0x64);
pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8D, 0xFF);
pub const ACCENT_BG: Color32 = Color32::from_rgb(0x16, 0x22, 0x38);
pub const LIVE: Color32 = Color32::from_rgb(0xFF, 0x4D, 0x4D);
pub const LIVE_BG: Color32 = Color32::from_rgb(0x36, 0x16, 0x16);
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x20);
pub const OK: Color32 = Color32::from_rgb(0x3D, 0xC1, 0x72);
pub const METER_OFF: Color32 = Color32::from_rgb(0x25, 0x25, 0x2C);

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
pub fn apply(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = Visuals::dark();

    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL_2;
    visuals.faint_bg_color = PANEL_2;
    visuals.code_bg_color = PANEL_2;
    visuals.striped = false;
    visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.selection.bg_fill = ACCENT_BG;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = LIVE;
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, DIM);
    widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    widgets.inactive.bg_fill = PANEL_2;
    widgets.inactive.weak_bg_fill = PANEL_2;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
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
    widgets.open.bg_fill = PANEL_2;
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
