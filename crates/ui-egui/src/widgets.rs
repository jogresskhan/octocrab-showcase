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
        .stroke(egui::Stroke::new(1.0_f32, theme::YELLOW.gamma_multiply(0.45)))
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

/// Parse an `OWNER/REPO` text field into `(owner, repo)`.
pub fn parse_repo(s: &str) -> Option<(String, String)> {
    let (o, r) = s.trim().split_once('/')?;
    let (o, r) = (o.trim(), r.trim());
    if o.is_empty() || r.is_empty() {
        None
    } else {
        Some((o.to_string(), r.to_string()))
    }
}

/// Parse a comma-separated list field ("a, b, c") into trimmed, non-empty items.
pub fn parse_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// A dim key + coloured value row ("license:  MIT").
pub fn kv(ui: &mut Ui, key: &str, value: &str, value_color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{key}:")).color(theme::DIM).size(11.0));
        ui.label(RichText::new(value.to_string()).color(value_color).size(11.5));
    });
}
