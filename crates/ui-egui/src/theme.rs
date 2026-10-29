//! VSCode "Dark+" inspired theme constants + a one-call [`apply`] that
//! installs the palette, monospace body fonts and dark visuals on a context.

use egui::{Color32, Context, FontId, Style, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x25, 0x25, 0x26);
pub const PANEL: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x7a, 0xcc);
pub const TEXT: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
pub const DIM: Color32 = Color32::from_rgb(0x85, 0x85, 0x85);
pub const GREEN: Color32 = Color32::from_rgb(0x4e, 0xc9, 0xb0);
pub const YELLOW: Color32 = Color32::from_rgb(0xdc, 0xdc, 0xaa);
pub const RED: Color32 = Color32::from_rgb(0xf4, 0x47, 0x47);
pub const BORDER: Color32 = Color32::from_rgb(0x3c, 0x3c, 0x3c);
pub const WHITE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);

pub const MONO_SIZE: f32 = 13.0;

/// Monospace font at the given size (the IDE is monospace-first).
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Install the VSCode-dark look on `ctx`. Cheap enough to call every frame.
pub fn apply(ctx: &Context) {
    let mut style: Style = (*ctx.style()).clone();

    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = Color32::from_rgb(0x14, 0x14, 0x14);
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.40);
    v.selection.stroke = egui::Stroke::new(0.5_f32, ACCENT);
    v.window_stroke = egui::Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.bg_fill = SIDEBAR;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x2d, 0x2e);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, WHITE);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.55);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, WHITE);
    style.visuals = v;

    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);

    let body = mono(MONO_SIZE);
    style.text_styles = [
        (TextStyle::Body, body.clone()),
        (TextStyle::Button, body.clone()),
        (TextStyle::Monospace, body.clone()),
        (TextStyle::Small, mono(11.0)),
        (TextStyle::Heading, mono(18.0)),
    ]
    .into();

    ctx.set_style(style);
}
