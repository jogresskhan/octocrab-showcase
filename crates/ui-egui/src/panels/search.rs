//! Quick-open / search panel: live fuzzy filter over tree paths plus an
//! optional GitHub code search (requires the `repo:read` module).

use egui::{RichText, ScrollArea, Ui};

use crate::{theme, widgets, SearchHit};

/// `hits` are pre-ranked by the caller (local + gh merged). `on_pick`
/// receives the index of a clicked row; `on_gh_search` starts a GitHub
/// code search when the button is pressed.
pub fn show(
    ui: &mut Ui,
    query: &mut String,
    hits: &[SearchHit],
    on_pick: &mut dyn FnMut(usize),
    gh_ready: bool,
    gh_busy: bool,
    gh_error: Option<&str>,
    on_gh_search: &mut dyn FnMut(),
) {
    widgets::section_header(ui, "Search");
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text("filter files · search code…")
            .desired_width(ui.available_width()),
    );
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !hits.is_empty() {
        on_pick(0);
    }

    ui.horizontal(|ui| {
        if gh_ready {
            if ui
                .add_enabled(!gh_busy, egui::Button::new("Search GitHub"))
                .clicked()
            {
                on_gh_search();
            }
        } else {
            ui.label(
                RichText::new("GitHub code search needs `repo:read`")
                    .size(10.5)
                    .color(theme::DIM),
            );
        }
        if gh_busy {
            ui.spinner();
        }
    });
    if let Some(e) = gh_error {
        ui.label(RichText::new(e).color(theme::RED).size(11.0));
    }

    ui.add_space(2.0);
    widgets::section_header(ui, &format!("results ({})", hits.len()));
    if hits.is_empty() {
        ui.label(
            RichText::new("type to filter the open tree")
                .size(11.5)
                .color(theme::DIM),
        );
        return;
    }
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 18.0, hits.len(), |ui, range| {
            for i in range {
                let h = &hits[i];
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 18.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::WHITE.gamma_multiply(0.05));
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    h.path.clone(),
                    egui::FontId::monospace(11.5),
                    if h.gh { theme::GREEN } else { theme::TEXT },
                );
                if !h.detail.is_empty() {
                    ui.painter().text(
                        egui::pos2(rect.right() - 6.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        h.detail.clone(),
                        egui::FontId::monospace(10.0),
                        theme::DIM,
                    );
                }
                if resp.clicked() {
                    on_pick(i);
                }
            }
        });
}
