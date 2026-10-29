//! Editor chrome: tab bar with close buttons, breadcrumbs and the
//! virtualized code view (line-number gutter + per-line colored spans).

use egui::{Align2, Color32, FontId, RichText, ScrollArea, Sense, Ui};

use crate::highlight::{self, Lang, Span};
use crate::theme;

const CODE_FONT_SIZE: f32 = 12.5;

/// One open editor tab.
#[derive(Clone, Debug)]
pub struct OpenTab {
    pub path: String,
    pub title: String,
    pub lang: Lang,
    pub loading: bool,
}

/// Horizontal tab strip with per-tab close buttons and an accent underline
/// for the active tab.
pub fn tab_bar(
    ui: &mut Ui,
    tabs: &[OpenTab],
    active: Option<usize>,
    on_select: &mut dyn FnMut(usize),
    on_close: &mut dyn FnMut(usize),
) {
    egui::Frame::none()
        .fill(Color32::from_rgb(0x2d, 0x2d, 0x2d))
        .inner_margin(egui::Margin {
            left: 4.0,
            right: 4.0,
            top: 0.0,
            bottom: 0.0,
        })
        .show(ui, |ui| {
            ui.set_min_height(28.0);
            ui.spacing_mut().item_spacing.x = 0.0;
            ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, tab) in tabs.iter().enumerate() {
                        let sel = active == Some(i);
                        let fill = if sel { theme::PANEL } else { Color32::TRANSPARENT };
                        let resp = egui::Frame::none()
                            .fill(fill)
                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if tab.loading {
                                        ui.spinner();
                                    }
                                    let t = ui.selectable_label(
                                        sel,
                                        RichText::new(tab.title.clone()).size(12.0).color(
                                            if sel { Color32::WHITE } else { theme::DIM },
                                        ),
                                    );
                                    if t.clicked() {
                                        on_select(i);
                                    }
                                    let x = ui.add(
                                        egui::Button::new(
                                            RichText::new("×").size(11.0).color(theme::DIM),
                                        )
                                        .frame(false)
                                        .min_size(egui::vec2(14.0, 14.0)),
                                    );
                                    if x.clicked() {
                                        on_close(i);
                                    }
                                });
                            })
                            .response;
                        if sel {
                            let r = resp.rect;
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top() + 2.0)),
                                0.0,
                                theme::ACCENT,
                            );
                        }
                    }
                });
        });
}

/// owner / repo / dir / file breadcrumb trail.
pub fn breadcrumbs(ui: &mut Ui, owner: Option<&str>, repo: Option<&str>, path: &str) {
    ui.horizontal(|ui| {
        let mut parts: Vec<String> = Vec::new();
        if let Some(o) = owner {
            parts.push(o.to_string());
        }
        if let Some(r) = repo {
            parts.push(r.to_string());
        }
        parts.extend(path.split('/').map(|s| s.to_string()));
        let last = parts.len().saturating_sub(1);
        for (i, p) in parts.iter().enumerate() {
            let is_last = i == last;
            ui.label(
                RichText::new(p.clone())
                    .size(11.5)
                    .color(if is_last { theme::TEXT } else { theme::DIM }),
            );
            if !is_last {
                ui.label(RichText::new("/").size(11.5).color(theme::DIM));
            }
        }
    });
}

/// Virtualized code view with gutter + colored spans. Rows are painted by
/// hand (painter primitives, no widgets) so huge files scroll smoothly.
pub fn code_view_spans(ui: &mut Ui, lines: &[Vec<Span>]) {
    let font = FontId::monospace(CODE_FONT_SIZE);
    let row_h = ui.fonts(|f| f.row_height(&font)) + 2.0;
    ui.spacing_mut().item_spacing.y = 0.0;

    let digits = lines.len().to_string().len().max(2);
    let gutter_text_w = ui
        .painter()
        .layout_no_wrap("0".repeat(digits), font.clone(), Color32::WHITE)
        .size()
        .x;
    let gutter_w = gutter_text_w + 24.0;
    let total = lines.len().max(1);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, total, |ui, range| {
            for i in range {
                let (rect, _resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    Sense::hover(),
                );
                // subtle gutter band
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top()), egui::pos2(rect.left() + gutter_w, rect.bottom())),
                    0.0,
                    Color32::WHITE.gamma_multiply(0.02),
                );
                ui.painter().text(
                    egui::pos2(rect.left() + gutter_w - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    font.clone(),
                    theme::DIM,
                );
                let mut job = egui::text::LayoutJob::default();
                let spans = lines.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
                if spans.is_empty() {
                    job.append(" ", 0.0, egui::TextFormat::simple(font.clone(), theme::TEXT));
                } else {
                    for s in spans {
                        job.append(&s.text, 0.0, egui::TextFormat::simple(font.clone(), s.color));
                    }
                }
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(
                    egui::pos2(rect.left() + gutter_w, rect.top() + 1.0),
                    galley,
                    theme::TEXT,
                );
            }
        });
}

/// Convenience: highlight + render in one shot (fine for small files).
pub fn code_view(ui: &mut Ui, lang: Lang, content: &str) {
    let lines = highlight::precompute(lang, content);
    code_view_spans(ui, &lines);
}

/// Welcome screen shown when no tab is open.
pub fn welcome(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.2);
        ui.label(RichText::new("Octocrab IDE").size(30.0).color(theme::DIM).strong());
        ui.add_space(10.0);
        ui.label(
            RichText::new("Ctrl+P quick open · Ctrl+Shift+P commands · Ctrl+B sidebar")
                .size(12.5)
                .color(theme::DIM),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("pick a file in the explorer to start")
                .size(12.0)
                .color(theme::DIM),
        );
    });
}
