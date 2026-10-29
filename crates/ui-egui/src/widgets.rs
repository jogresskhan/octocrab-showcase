//! Small reusable widgets: icon buttons, badges, section headers and the
//! module-gating `locked_panel`.

use egui::{Color32, RichText, Ui};

use crate::theme;

/// A compact square button showing a glyph/emoji, with hover tooltip.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(15.0).color(theme::TEXT))
            .fill(Color32::TRANSPARENT)
            .min_size(egui::vec2(28.0, 26.0)),
    )
    .on_hover_text(tooltip)
}

/// A rounded pill badge (granted / denied / pre / draft / token kind …).
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.22))
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .rounding(3.0)
        .show(ui, |ui| {
            ui.label(RichText::new(text.to_string()).color(color).size(10.5));
        });
}

/// Uppercase dim section title with a little breathing room.
pub fn section_header(ui: &mut Ui, text: &str) {
    ui.add_space(3.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .color(theme::DIM)
            .strong(),
    );
    ui.add_space(1.0);
}

/// Rendered wherever a feature needs a capability module that is not granted:
///
/// `🔒 locked — module `workflow:read` is required (run `octocrab-cli auth check`)`
pub fn locked_panel(ui: &mut Ui, module_key: &str, desc: &str) {
    egui::Frame::none()
        .fill(Color32::from_rgb(0x2a, 0x22, 0x1c))
        .stroke(egui::Stroke::new(1.0, theme::YELLOW.gamma_multiply(0.45)))
        .inner_margin(egui::Margin::same(10.0))
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("🔒 locked — module `{module_key}` is required"))
                    .color(theme::YELLOW)
                    .size(12.5),
            );
            ui.label(
                RichText::new(format!("(run `octocrab-cli auth check`) — {desc}"))
                    .color(theme::DIM)
                    .size(11.0),
            );
        });
}
