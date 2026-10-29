//! Simple tags table (tag, commit sha).

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, TagRow};

pub fn show(ui: &mut Ui, rows: &[TagRow], loading: bool) {
    widgets::section_header(ui, "Tags");
    if loading && rows.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("loading tags…").color(theme::DIM).size(11.5));
        });
        return;
    }
    if rows.is_empty() {
        ui.label(RichText::new("(no tags)").color(theme::DIM).size(11.5));
        return;
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 18.0, rows.len(), |ui, range| {
            for r in range {
                let tag = &rows[r];
                let (rect, _resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 18.0),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    tag.name.clone(),
                    egui::FontId::monospace(12.0),
                    theme::ACCENT,
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    tag.sha.clone(),
                    egui::FontId::monospace(11.0),
                    theme::DIM,
                );
            }
        });
}
