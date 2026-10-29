//! Repository panel — repo overview, editable settings (PATCH /repos +
//! PUT /topics), the community-health checklist (community/profile) and a
//! one-click community-files scaffold. Pure rendering; caller owns state.

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, CommunityHealth, FileProgress, RepoOverview};

/// Editable target-repo field for the repo panel.
#[derive(Clone, Debug, Default)]
pub struct RepoForm {
    /// `OWNER/REPO` target
    pub repo: String,
}

/// Editable settings fields (seeded from the loaded overview).
#[derive(Clone, Debug, Default)]
pub struct RepoEdits {
    pub description: String,
    pub homepage: String,
    /// comma-separated topic list
    pub topics: String,
}

/// Render the repository panel. `on_load` / `on_save` / `on_scaffold` are
/// the wired button callbacks; `busy` is Some while a promise is in flight.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    form: &mut RepoForm,
    overview: Option<&RepoOverview>,
    edits: &mut RepoEdits,
    community: Option<&CommunityHealth>,
    loading: bool,
    busy: Option<&str>,
    err: Option<&str>,
    scaffold_progress: &[FileProgress],
    can_admin: bool,
    can_write: bool,
    has_client: bool,
    on_load: &mut dyn FnMut(),
    on_save: &mut dyn FnMut(),
    on_scaffold: &mut dyn FnMut(),
) {
    widgets::section_header(ui, "Repository");
    if let Some(e) = err {
        ui.label(RichText::new(e.to_string()).color(theme::RED).size(11.0));
    }

    // ---------------------------------------------------- target + load
    ui.horizontal(|ui| {
        ui.label(RichText::new("repo").color(theme::DIM).size(11.0));
        ui.add(
            egui::TextEdit::singleline(&mut form.repo)
                .id_salt("repo-target")
                .hint_text("OWNER/REPO")
                .desired_width(150.0),
        );
        let idle = busy.is_none() && has_client;
        if ui
            .add_enabled(idle, egui::Button::new("Load"))
            .clicked()
        {
            on_load();
        }
        if loading {
            ui.spinner();
        }
    });
    if !has_client {
        ui.label(
            RichText::new("offline — no GitHub client (no active token)")
                .color(theme::DIM)
                .size(11.0),
        );
    }

    // ---------------------------------------------------- overview
    widgets::section_header(ui, "Overview");
    match (overview, loading) {
        (None, true) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("loading repository…").color(theme::DIM).size(11.5));
            });
        }
        (None, false) => {
            ui.label(
                RichText::new("(press Load to fetch repo info)")
                    .size(11.5)
                    .color(theme::DIM),
            );
        }
        (Some(ov), _) => {
            if !ov.description.is_empty() {
                ui.label(RichText::new(ov.description.clone()).size(11.5).color(theme::TEXT));
            }
            widgets::kv(ui, "homepage", if ov.homepage.is_empty() { "(none)" } else { &ov.homepage }, theme::ACCENT);
            widgets::kv(ui, "stars", &format!("⭐ {}", ov.stars), theme::TEXT);
            widgets::kv(ui, "license", &ov.license, theme::TEXT);
            widgets::kv(ui, "default branch", &ov.default_branch, theme::TEXT);
            if !ov.topics.is_empty() {
                widgets::kv(ui, "topics", &ov.topics.join(", "), theme::GREEN);
            }
        }
    }

    // ---------------------------------------------------- settings editor
    widgets::section_header(ui, "Settings");
    egui::Grid::new("repo-settings-grid")
        .num_columns(2)
        .min_col_width(76.0)
        .show(ui, |ui| {
            ui.label(RichText::new("description").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut edits.description)
                    .id_salt("repo-desc")
                    .hint_text("repo description")
                    .desired_width(160.0),
            );
            ui.end_row();

            ui.label(RichText::new("homepage").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut edits.homepage)
                    .id_salt("repo-home")
                    .hint_text("https://…")
                    .desired_width(160.0),
            );
            ui.end_row();

            ui.label(RichText::new("topics").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut edits.topics)
                    .id_salt("repo-topics")
                    .hint_text("comma, separated, topics")
                    .desired_width(160.0),
            );
            ui.end_row();
        });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let label = if can_admin {
            "💾 Save settings".to_string()
        } else {
            "🔒 Save settings".to_string()
        };
        let enabled = busy.is_none() && has_client && can_admin;
        if ui
            .add_enabled(enabled, egui::Button::new(label))
            .clicked()
        {
            on_save();
        }
        if !can_admin {
            ui.label(
                RichText::new("needs `repo:admin` (PATCH /repos + PUT /topics)")
                    .size(10.0)
                    .color(theme::DIM),
            );
        }
    });

    // ---------------------------------------------------- community health
    widgets::section_header(ui, "Community health");
    match community {
        None => {
            ui.label(
                RichText::new("(loaded together with the repo overview)")
                    .size(11.0)
                    .color(theme::DIM),
            );
        }
        Some(ch) => {
            let color = if ch.health_percent >= 80 {
                theme::GREEN
            } else if ch.health_percent >= 50 {
                theme::YELLOW
            } else {
                theme::RED
            };
            widgets::badge(ui, &format!("health {}/100", ch.health_percent), color);
            egui::Grid::new("community-grid")
                .num_columns(2)
                .min_col_width(120.0)
                .show(ui, |ui| {
                    for (name, present) in &ch.files {
                        ui.monospace(
                            RichText::new(if *present { "✓" } else { "−" })
                                .color(if *present { theme::GREEN } else { theme::RED })
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(name.clone())
                                .size(11.5)
                                .color(if *present { theme::TEXT } else { theme::DIM }),
                        );
                        ui.end_row();
                    }
                });
        }
    }

    // ---------------------------------------------------- scaffold
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let label = if can_write {
            "🧰 Scaffold community files".to_string()
        } else {
            "🔒 Scaffold community files".to_string()
        };
        let enabled = busy.is_none() && has_client && can_write;
        if ui
            .add_enabled(enabled, egui::Button::new(label))
            .clicked()
        {
            on_scaffold();
        }
    });
    if !can_write {
        ui.label(
            RichText::new("needs `repo:write` — renders the `community` template and pushes every file")
                .size(10.0)
                .color(theme::DIM),
        );
    }
    if !scaffold_progress.is_empty() {
        widgets::section_header(ui, "Scaffold progress");
        ScrollArea::vertical()
            .id_salt("repo-progress")
            .max_height(140.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                crate::panels::templates::progress_rows(ui, scaffold_progress);
            });
    }
}
