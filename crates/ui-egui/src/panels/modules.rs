//! The capability grid: every module from `octocrab_core::caps::MODULES`
//! with granted / denied / unknown badge + evidence, plus the org access
//! table (read / write / admin marks).

use egui::{RichText, ScrollArea, Ui};
use octocrab_core::caps;
use octocrab_core::db::ModuleState;

use crate::theme;
use crate::widgets;

pub fn show(ui: &mut Ui, mods: &[ModuleState], orgs: &[(String, String, bool, bool, bool)]) {
    widgets::section_header(ui, "Capability modules");
    let granted = mods.iter().filter(|m| m.enabled).count();
    ui.label(
        RichText::new(format!("{granted}/{} modules granted", mods.len()))
            .color(if granted > 0 { theme::GREEN } else { theme::DIM })
            .size(12.0),
    );
    ui.add_space(4.0);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for def in caps::MODULES {
                let st = mods.iter().find(|m| m.key == def.key);
                let (state_str, color) = match st.map(|s| s.state.as_str()) {
                    Some("granted") => ("granted", theme::GREEN),
                    Some("denied") => ("denied", theme::RED),
                    _ => ("unknown", theme::DIM),
                };
                ui.horizontal(|ui| {
                    widgets::badge(ui, state_str, color);
                    ui.label(RichText::new(def.key).size(12.5).color(color));
                    ui.label(
                        RichText::new(def.desc)
                            .size(11.0)
                            .color(theme::DIM),
                    );
                });
                let evidence = st.map(|s| s.evidence.as_str()).unwrap_or("");
                if !evidence.is_empty() {
                    ui.label(
                        RichText::new(format!("      {evidence}"))
                            .size(10.5)
                            .color(theme::DIM),
                    );
                }
                ui.add_space(2.0);
            }

            ui.add_space(10.0);
            widgets::section_header(ui, "Organization access");
            if orgs.is_empty() {
                ui.label(
                    RichText::new("(no organizations on record)")
                        .size(11.5)
                        .color(theme::DIM),
                );
            } else {
                egui::Grid::new("org-access-grid")
                    .num_columns(5)
                    .striped(true)
                    .min_col_width(64.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("org").color(theme::DIM).size(11.0));
                        ui.label(RichText::new("role").color(theme::DIM).size(11.0));
                        ui.label(RichText::new("read").color(theme::DIM).size(11.0));
                        ui.label(RichText::new("write").color(theme::DIM).size(11.0));
                        ui.label(RichText::new("admin").color(theme::DIM).size(11.0));
                        ui.end_row();
                        for (login, role, read, write, admin) in orgs {
                            ui.label(RichText::new(login.clone()).color(theme::TEXT).size(12.0));
                            ui.label(RichText::new(role.clone()).color(theme::DIM).size(11.5));
                            mark(ui, *read);
                            mark(ui, *write);
                            mark(ui, *admin);
                            ui.end_row();
                        }
                    });
            }
        });
}

fn mark(ui: &mut Ui, on: bool) {
    ui.label(
        RichText::new(if on { "✓" } else { "−" })
            .size(12.0)
            .color(if on { theme::GREEN } else { theme::DIM }),
    );
}
