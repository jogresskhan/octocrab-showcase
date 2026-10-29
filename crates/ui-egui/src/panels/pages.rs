//! GitHub Pages panel — status, enable, on-demand build and one-click
//! landing-site generation. Pure rendering: the caller owns state, gating
//! and the promise plumbing (see `apps/octocrab-ide/src/app.rs`).

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, FileProgress, PagesStatus};

/// Editable target-repo field for the pages panel.
#[derive(Clone, Debug, Default)]
pub struct PagesForm {
    /// `OWNER/REPO` target
    pub repo: String,
}

/// Render the pages panel. Buttons fire the `on_*` callbacks; the caller
/// decides which ones are permitted (module gates) via the `can_*` flags.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    form: &mut PagesForm,
    info: Option<&PagesStatus>,
    busy: Option<&str>,
    err: Option<&str>,
    generate_progress: &[FileProgress],
    can_admin: bool,
    can_write: bool,
    has_client: bool,
    on_status: &mut dyn FnMut(),
    on_enable: &mut dyn FnMut(),
    on_build: &mut dyn FnMut(),
    on_generate: &mut dyn FnMut(),
) {
    widgets::section_header(ui, "GitHub Pages");
    if let Some(e) = err {
        ui.label(RichText::new(e.to_string()).color(theme::RED).size(11.0));
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("repo").color(theme::DIM).size(11.0));
        ui.add(
            egui::TextEdit::singleline(&mut form.repo)
                .id_salt("pages-repo")
                .hint_text("OWNER/REPO")
                .desired_width(170.0),
        );
    });
    ui.add_space(2.0);

    // ---------------------------------------------------- action buttons
    let idle = busy.is_none() && has_client;
    ui.horizontal(|ui| {
        if ui.add_enabled(idle, egui::Button::new("Status")).clicked() {
            on_status();
        }
        let enable_label = if can_admin {
            "Enable Pages".to_string()
        } else {
            "🔒 Enable Pages".to_string()
        };
        if ui
            .add_enabled(idle && can_admin, egui::Button::new(enable_label))
            .clicked()
        {
            on_enable();
        }
        let build_label = if can_write {
            "Build now".to_string()
        } else {
            "🔒 Build now".to_string()
        };
        if ui
            .add_enabled(idle && can_write, egui::Button::new(build_label))
            .clicked()
        {
            on_build();
        }
    });
    ui.horizontal(|ui| {
        let gen_label = if can_write {
            "Generate site".to_string()
        } else {
            "🔒 Generate site".to_string()
        };
        if ui
            .add_enabled(idle && can_write, egui::Button::new(gen_label))
            .clicked()
        {
            on_generate();
        }
        if !can_write {
            ui.label(
                RichText::new("needs `repo:write` (renders + pushes site/)")
                    .size(10.0)
                    .color(theme::DIM),
            );
        }
        if let Some(b) = busy {
            ui.spinner();
            ui.label(
                RichText::new(format!("{b}…"))
                    .color(theme::YELLOW)
                    .size(11.0),
            );
        }
    });
    if !has_client {
        ui.label(
            RichText::new("offline — no GitHub client (no active token)")
                .color(theme::DIM)
                .size(11.0),
        );
    }

    // ---------------------------------------------------- status block
    widgets::section_header(ui, "Pages status");
    match info {
        None => {
            ui.label(
                RichText::new("(press Status to load pages info)")
                    .size(11.5)
                    .color(theme::DIM),
            );
        }
        Some(st) if !st.enabled => {
            egui::Frame::none()
                .fill(theme::YELLOW.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    theme::YELLOW.gamma_multiply(0.45),
                ))
                .inner_margin(egui::Margin::same(8.0))
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("🌐 Pages is NOT enabled for {}", form.repo.trim()))
                            .color(theme::YELLOW)
                            .size(12.0),
                    );
                    if let Some(e) = &st.error {
                        ui.label(RichText::new(e.clone()).color(theme::DIM).size(10.5));
                    }
                    ui.label(
                        RichText::new("use `Enable Pages` (needs repo:admin), then Generate site")
                            .size(10.5)
                            .color(theme::DIM),
                    );
                });
        }
        Some(st) => {
            widgets::badge(ui, "enabled", theme::GREEN);
            if !st.html_url.is_empty() {
                widgets::kv(ui, "url", &st.html_url, theme::ACCENT);
            }
            if !st.build_type.is_empty() {
                widgets::kv(ui, "build_type", &st.build_type, theme::TEXT);
            }
            if !st.status.is_empty() {
                widgets::kv(ui, "status", &st.status, theme::TEXT);
            }
            if !st.build_status.is_empty() {
                let (color, icon) = match st.build_status.as_str() {
                    "built" => (theme::GREEN, "✓"),
                    "building" | "queued" => (theme::YELLOW, "●"),
                    "errored" | "error" => (theme::RED, "×"),
                    _ => (theme::DIM, "○"),
                };
                ui.horizontal(|ui| {
                    ui.monospace(RichText::new(icon).color(color).size(12.0));
                    ui.label(
                        RichText::new(format!("latest build: {}", st.build_status))
                            .size(11.5)
                            .color(color),
                    );
                });
            }
        }
    }

    // ---------------------------------------------------- generation rows
    if !generate_progress.is_empty() {
        widgets::section_header(ui, "Site generation");
        ScrollArea::vertical()
            .id_salt("pages-progress")
            .max_height(120.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                crate::panels::templates::progress_rows(ui, generate_progress);
            });
    }
}
