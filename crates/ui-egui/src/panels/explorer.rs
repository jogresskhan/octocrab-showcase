//! Virtualized file-tree explorer.
//!
//! The tree is *data-driven*: a flat, sorted `Vec<TreeItem>` with a per-row
//! depth. Rendering goes through `ScrollArea::show_rows`, so a 100k-entry
//! tree costs the same as a 100-entry one — only visible rows get widgets.

use std::collections::HashSet;

use egui::{Align2, Color32, FontId, RichText, ScrollArea, Sense, Ui};
use serde::{Deserialize, Serialize};

use crate::theme;
use crate::widgets;

pub const ROW_H: f32 = 20.0;

/// `Dir` rows are collapsible; `File` rows open in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeKind {
    Dir,
    File,
}

/// One row of the (flat, depth-annotated) tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeItem {
    pub path: String,
    pub name: String,
    pub kind: TreeKind,
    pub depth: usize,
}

impl TreeItem {
    fn base(path: String, kind: TreeKind, depth: usize) -> Self {
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        Self { path, name, kind, depth }
    }

    /// Build flat depth-annotated rows from **sorted** relative paths,
    /// synthesizing intermediate directory rows as needed.
    pub fn build_from_paths(paths: &[(String, TreeKind)]) -> Vec<TreeItem> {
        let mut out: Vec<TreeItem> = Vec::with_capacity(paths.len() + 16);
        let mut seen_dirs: HashSet<String> = HashSet::new();
        for (p, kind) in paths {
            let mut off = 0usize;
            while let Some(idx) = p[off..].find('/') {
                let dir_end = off + idx;
                let dir = &p[..dir_end];
                if seen_dirs.insert(dir.to_string()) {
                    let depth = dir.matches('/').count();
                    out.push(Self::base(dir.to_string(), TreeKind::Dir, depth));
                }
                off = dir_end + 1;
            }
            let depth = p.matches('/').count();
            out.push(Self::base(p.clone(), *kind, depth));
        }
        out
    }

    /// Row indices that survive collapse-filtering (walk is O(n), cheap).
    pub fn visible_rows(items: &[TreeItem], collapsed: &HashSet<String>) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(items.len());
        let mut hide_from: Option<usize> = None;
        for (i, it) in items.iter().enumerate() {
            if let Some(d) = hide_from {
                if it.depth > d {
                    continue;
                }
                hide_from = None;
            }
            out.push(i);
            if it.kind == TreeKind::Dir && collapsed.contains(&it.path) {
                hide_from = Some(it.depth);
            }
        }
        out
    }

    pub fn file_count(items: &[TreeItem]) -> usize {
        items.iter().filter(|i| i.kind == TreeKind::File).count()
    }
}

/// UI state for one explorer instance.
#[derive(Default)]
pub struct ExplorerState {
    pub collapsed: HashSet<String>,
    pub selected: Option<String>,
}

/// The explorer panel. `on_open_file` is invoked with a path when a file row
/// is clicked (the caller turns that into an async open).
pub fn show(
    ui: &mut Ui,
    title: &str,
    items: &[TreeItem],
    state: &mut ExplorerState,
    on_open_file: &mut dyn FnMut(String),
) {
    widgets::section_header(ui, title);
    if items.is_empty() {
        ui.label(RichText::new("(no files)").color(theme::DIM).size(11.5));
        return;
    }
    let rows = TreeItem::visible_rows(items, &state.collapsed);
    let font = FontId::monospace(12.5);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
            for r in range {
                let it = &items[rows[r]];
                let indent = 6.0 + it.depth as f32 * 12.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
                let selected = state.selected.as_deref() == Some(it.path.as_str());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, 2.0, Color32::WHITE.gamma_multiply(0.05));
                }
                if selected {
                    ui.painter()
                        .rect_filled(rect, 2.0, theme::ACCENT.gamma_multiply(0.35));
                }
                let (glyph, color) = match it.kind {
                    TreeKind::Dir => {
                        let collapsed = state.collapsed.contains(&it.path);
                        (if collapsed { "▶" } else { "▼" }, theme::ACCENT)
                    }
                    TreeKind::File => glyph_for(&it.path),
                };
                ui.painter().text(
                    egui::pos2(rect.left() + indent, rect.center().y),
                    Align2::LEFT_CENTER,
                    glyph,
                    font.clone(),
                    color,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + indent + 16.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &it.name,
                    font.clone(),
                    if selected { Color32::WHITE } else { theme::TEXT },
                );
                if resp.clicked() {
                    match it.kind {
                        TreeKind::Dir => {
                            if state.collapsed.contains(&it.path) {
                                state.collapsed.remove(&it.path);
                            } else {
                                state.collapsed.insert(it.path.clone());
                            }
                        }
                        TreeKind::File => {
                            state.selected = Some(it.path.clone());
                            on_open_file(it.path.clone());
                        }
                    }
                }
            }
        });
}

/// Tiny colored letter per language (guaranteed glyph coverage).
fn glyph_for(path: &str) -> (&'static str, Color32) {
    crate::highlight::Lang::from_path(path).glyph()
}
