//! Simple releases table (tag, name, date, prerelease/draft badges).

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, ReleaseRow};

pub fn show(ui: &mut Ui, rows: &[ReleaseRow], loading: bool) {
    ui.horizontal(|ui| {
        widgets::section_header(ui, "Releases");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} releases", rows.len()))
                    .size(10.5)
                    .color(theme::DIM),
            );
        });
    });
    if loading && rows.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("loading releases…")
                    .color(theme::DIM)
                    .size(11.5),
            );
        });
        return;
    }
    if rows.is_empty() {
        ui.label(RichText::new("(no releases)").color(theme::DIM).size(11.5));
        return;
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in rows {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(r.tag.clone()).color(theme::ACCENT).size(12.5));
                    if r.draft {
                        widgets::badge(ui, "draft", theme::DIM);
                    }
                    if r.prerelease {
                        widgets::badge(ui, "pre", theme::YELLOW);
                    }
                });
                ui.label(
                    RichText::new(format!("  {}", r.name))
                        .size(11.5)
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new(format!("  {}", r.date))
                        .size(10.5)
                        .color(theme::DIM),
                );
                ui.add_space(3.0);
            }
        });
}
