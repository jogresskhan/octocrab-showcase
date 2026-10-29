//! Templates gallery — lists the six built-in repository templates from
//! `octocrab_core::templates`, edits render parameters and kicks off a
//! background install (Contents API pushes with per-file progress).
//!
//! Data-driven like every panel: the caller owns the form state and the
//! promise plumbing; this module only renders plain data + callbacks.

use egui::{RichText, ScrollArea, Ui};
use octocrab_core::templates::{TemplateParams, TEMPLATES};

use crate::{theme, widgets, FileProgress};

/// Editable parameter form for the selected template.
#[derive(Clone, Debug, Default)]
pub struct TemplatesForm {
    /// index into `TEMPLATES`
    pub selected: usize,
    /// `OWNER/REPO` destination
    pub repo: String,
    pub project: String,
    pub tagline: String,
    /// comma-separated binary names
    pub bins: String,
    /// comma-separated crates excluded from CI
    pub exclude: String,
}

/// Clamp + resolve the currently selected `TemplateDef`.
pub fn selected_def(form: &TemplatesForm) -> &'static octocrab_core::templates::TemplateDef {
    &TEMPLATES[form.selected.min(TEMPLATES.len() - 1)]
}

/// Build render params from the form (+ fallbacks from repo context).
pub fn build_params(
    form: &TemplatesForm,
    owner: &str,
    repo: &str,
    description: &str,
    license: &str,
    contact: &str,
    branch: &str,
) -> TemplateParams {
    TemplateParams {
        project: if form.project.trim().is_empty() {
            repo.to_string()
        } else {
            form.project.trim().to_string()
        },
        owner: owner.to_string(),
        repo: repo.to_string(),
        description: description.to_string(),
        tagline: form.tagline.trim().to_string(),
        bins: widgets::parse_list(&form.bins),
        branches: vec![branch.to_string()],
        exclude: widgets::parse_list(&form.exclude),
        license: if license.is_empty() {
            "MIT".to_string()
        } else {
            license.to_string()
        },
        contact: contact.to_string(),
    }
}

/// Render the templates gallery. `on_select(i)` fires on template pick,
/// `on_install()` on Install press (caller gates + spawns the promise).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    form: &mut TemplatesForm,
    progress: &[FileProgress],
    installing: bool,
    has_client: bool,
    gate_ok: bool,
    err: Option<&str>,
    on_select: &mut dyn FnMut(usize),
    on_install: &mut dyn FnMut(),
) {
    widgets::section_header(ui, "Repository templates");
    if let Some(e) = err {
        ui.label(RichText::new(e.to_string()).color(theme::RED).size(11.0));
    }

    // ---------------------------------------------------- template list
    let mut picked: Option<usize> = None;
    ScrollArea::vertical()
        .id_salt("tpl-list")
        .max_height(158.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, t) in TEMPLATES.iter().enumerate() {
                let selected = i == form.selected.min(TEMPLATES.len() - 1);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 34.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, 3.0, egui::Color32::WHITE.gamma_multiply(0.05));
                }
                if selected {
                    ui.painter()
                        .rect_filled(rect, 3.0, theme::ACCENT.gamma_multiply(0.35));
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.top() + 9.0),
                    egui::Align2::LEFT_CENTER,
                    t.name,
                    egui::FontId::monospace(12.5),
                    if selected { egui::Color32::WHITE } else { theme::TEXT },
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 6.0, rect.top() + 9.0),
                    egui::Align2::RIGHT_CENTER,
                    t.gate,
                    egui::FontId::monospace(10.0),
                    if t.gate == "repo:write" { theme::GREEN } else { theme::YELLOW },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.top() + 25.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{} · {} file(s)", truncate(t.desc, 46), t.files.len()),
                    egui::FontId::monospace(10.0),
                    theme::DIM,
                );
                if resp.clicked() {
                    picked = Some(i);
                }
            }
        });
    if let Some(i) = picked {
        on_select(i);
    }

    // ---------------------------------------------------- params form
    let active = picked.unwrap_or_else(|| form.selected.min(TEMPLATES.len() - 1));
    let def = &TEMPLATES[active];
    widgets::section_header(ui, &format!("Parameters — {}", def.name));
    ui.label(
        RichText::new(def.desc.to_string())
            .size(11.0)
            .color(theme::TEXT),
    );
    ui.label(
        RichText::new(format!("writes: {}", def.files.join("  ·  ")))
            .size(10.5)
            .color(theme::DIM),
    );
    ui.add_space(2.0);

    egui::Grid::new("tpl-form-grid")
        .num_columns(2)
        .min_col_width(64.0)
        .show(ui, |ui| {
            ui.label(RichText::new("repo").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut form.repo)
                    .id_salt("tpl-repo")
                    .hint_text("OWNER/REPO")
                    .desired_width(170.0),
            );
            ui.end_row();

            ui.label(RichText::new("project").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut form.project)
                    .id_salt("tpl-project")
                    .hint_text("display name")
                    .desired_width(170.0),
            );
            ui.end_row();

            ui.label(RichText::new("tagline").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut form.tagline)
                    .id_salt("tpl-tagline")
                    .hint_text("one-liner for README / site")
                    .desired_width(170.0),
            );
            ui.end_row();

            ui.label(RichText::new("bins").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut form.bins)
                    .id_salt("tpl-bins")
                    .hint_text("comma, separated, bins")
                    .desired_width(170.0),
            );
            ui.end_row();

            ui.label(RichText::new("exclude").color(theme::DIM).size(11.0));
            ui.add(
                egui::TextEdit::singleline(&mut form.exclude)
                    .id_salt("tpl-exclude")
                    .hint_text("comma, separated, crates")
                    .desired_width(170.0),
            );
            ui.end_row();
        });

    // ---------------------------------------------------- install action
    ui.add_space(4.0);
    if installing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new(format!("installing {} files…", def.files.len()))
                    .color(theme::YELLOW)
                    .size(11.5),
            );
        });
    } else if !has_client {
        ui.label(
            RichText::new("offline — no GitHub client (no active token)")
                .color(theme::DIM)
                .size(11.5),
        );
    } else if !gate_ok {
        widgets::locked_panel(ui, def.gate, &format!("install the {} template", def.name));
    } else {
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(
                    RichText::new(format!("⬇ Install {}", def.name)).size(12.5),
                ))
                .clicked()
            {
                on_install();
            }
            ui.label(
                RichText::new(format!("pushes {} file(s) to {}", def.files.len(), def.key))
                    .size(10.5)
                    .color(theme::DIM),
            );
        });
    }

    // ---------------------------------------------------- progress rows
    if !progress.is_empty() {
        widgets::section_header(ui, "Install progress");
        ScrollArea::vertical()
            .id_salt("tpl-progress")
            .max_height(150.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                progress_rows(ui, progress);
            });
    }
}

/// Shared per-file progress rows (✓ pushed / × failed / ● pending).
pub fn progress_rows(ui: &mut Ui, progress: &[FileProgress]) {
    for row in progress {
        let (icon, color) = if !row.done {
            ("●", theme::YELLOW)
        } else if row.ok {
            ("✓", theme::GREEN)
        } else {
            ("×", theme::RED)
        };
        ui.horizontal(|ui| {
            ui.monospace(RichText::new(icon).color(color).size(12.0));
            ui.label(
                RichText::new(row.path.clone())
                    .size(11.5)
                    .color(if row.done && row.ok { theme::TEXT } else if row.done { theme::RED } else { theme::DIM }),
            );
        });
        if !row.detail.is_empty() {
            ui.label(
                RichText::new(format!("    {}", truncate(&row.detail, 52)))
                    .size(10.0)
                    .color(if row.ok { theme::GREEN } else { theme::RED }),
            );
        }
        ui.add_space(1.0);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
