//! Workflow runs list + per-run jobs/steps tree.

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, JobRow, RunRow};

/// (glyph, color) for a run/job/step given status + conclusion strings.
pub fn status_icon(status: &str, conclusion: Option<&str>) -> (&'static str, egui::Color32) {
    match conclusion.unwrap_or(status) {
        "success" => ("✓", theme::GREEN),
        "failure" | "timed_out" | "startup_failure" => ("×", theme::RED),
        "cancelled" | "canceled" => ("⊘", theme::DIM),
        "skipped" | "neutral" => ("−", theme::DIM),
        "in_progress" | "queued" | "pending" | "waiting" | "requested" => ("●", theme::YELLOW),
        _ => ("○", theme::DIM),
    }
}

/// Compact runs table for the sidebar. Clicking a run selects it.
#[allow(clippy::too_many_arguments)]
pub fn runs_list(
    ui: &mut Ui,
    runs: &[RunRow],
    sel: Option<u64>,
    loading: bool,
    can_dispatch: bool,
    on_select: &mut dyn FnMut(u64),
    on_refresh: &mut dyn FnMut(),
    on_dispatch: &mut dyn FnMut(),
) {
    ui.horizontal(|ui| {
        widgets::section_header(ui, "Workflow runs");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if can_dispatch && ui.small_button("▶ dispatch").clicked() {
                on_dispatch();
            }
            if ui.small_button("refresh").clicked() {
                on_refresh();
            }
        });
    });
    if loading && runs.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("loading workflow runs…").color(theme::DIM).size(11.5));
        });
        return;
    }
    if runs.is_empty() {
        ui.label(RichText::new("(no runs)").color(theme::DIM).size(11.5));
        return;
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 34.0, runs.len(), |ui, range| {
            for r in range {
                let run = &runs[r];
                let (icon, color) = status_icon(&run.status, run.conclusion.as_deref());
                let selected = sel == Some(run.id);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        2.0,
                        egui::Color32::WHITE.gamma_multiply(0.05),
                    );
                }
                if selected {
                    ui.painter().rect_filled(
                        rect,
                        2.0,
                        theme::ACCENT.gamma_multiply(0.35),
                    );
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.top() + 9.0),
                    egui::Align2::LEFT_CENTER,
                    icon,
                    egui::FontId::monospace(12.5),
                    color,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 24.0, rect.top() + 9.0),
                    egui::Align2::LEFT_CENTER,
                    truncate(&run.name, 26),
                    egui::FontId::monospace(12.0),
                    if selected { egui::Color32::WHITE } else { theme::TEXT },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 24.0, rect.top() + 24.0),
                    egui::Align2::LEFT_CENTER,
                    format!("#{} · {} · {}", run.id, truncate(&run.branch, 14), run.created),
                    egui::FontId::monospace(10.0),
                    theme::DIM,
                );
                if resp.clicked() {
                    on_select(run.id);
                }
            }
        });
}

/// Per-run jobs with collapsible per-step trees.
pub fn jobs_view(ui: &mut Ui, run_label: &str, jobs: &[JobRow], loading: bool) {
    widgets::section_header(ui, "Jobs");
    ui.label(
        RichText::new(run_label.to_string())
            .color(theme::DIM)
            .size(11.0),
    );
    if loading && jobs.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("loading jobs…").color(theme::DIM).size(11.5));
        });
        return;
    }
    if jobs.is_empty() {
        ui.label(RichText::new("(no jobs)").color(theme::DIM).size(11.5));
        return;
    }
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for job in jobs {
            let (icon, color) = status_icon(&job.status, job.conclusion.as_deref());
            let header = format!("{icon} {}{}", truncate(&job.name, 42), job.duration);
            egui::CollapsingHeader::new(
                RichText::new(header).color(color).size(12.5),
            )
            .id_salt(format!("job-{}", job.id))
            .default_open(true)
            .show(ui, |ui| {
                for step in &job.steps {
                    let (si, sc) = status_icon(&step.status, step.conclusion.as_deref());
                    ui.horizontal(|ui| {
                        ui.monospace(RichText::new(si).color(sc));
                        ui.label(RichText::new(step.name.clone()).size(11.5).color(theme::TEXT));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    RichText::new(step.duration.clone())
                                        .size(10.5)
                                        .color(theme::DIM),
                                );
                            },
                        );
                    });
                }
                if job.steps.is_empty() {
                    ui.label(
                        RichText::new("  (no step data)")
                            .size(11.0)
                            .color(theme::DIM),
                    );
                }
            });
        }
    });
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
