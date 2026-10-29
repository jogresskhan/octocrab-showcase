//! `octocrab-ui-egui` — a VSCode-dark-themed UI kit used by the
//! Octocrab IDE (`apps/octocrab-ide`).
//!
//! Everything is data-driven: panels take plain row structs + callbacks so
//! they can be rendered from cache, from live GitHub data or from the local
//! filesystem with zero code changes.

pub mod highlight;
pub mod panels;
pub mod theme;
pub mod widgets;

pub use highlight::{Lang, HlState, Span};
pub use panels::editor_tabs::OpenTab;
pub use panels::explorer::{ExplorerState, TreeItem, TreeKind};

// ------------------------------------------------------------- data rows

/// One workflow run (raw GitHub fields flattened to strings).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RunRow {
    pub id: u64,
    pub name: String,
    pub branch: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub created: String,
}

/// One step inside a workflow job.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StepRow {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub duration: String,
}

/// One workflow job with its steps.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JobRow {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started: String,
    pub duration: String,
    pub steps: Vec<StepRow>,
}

/// One release row for the releases table.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReleaseRow {
    pub tag: String,
    pub name: String,
    pub date: String,
    pub prerelease: bool,
    pub draft: bool,
}

/// One tag row for the tags table.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TagRow {
    pub name: String,
    pub sha: String,
}

/// A search hit (local fuzzy match or GitHub code search).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub detail: String,
    pub gh: bool,
}
