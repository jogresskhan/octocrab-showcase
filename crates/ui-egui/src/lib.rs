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

pub use highlight::{HlState, Lang, Span};
pub use panels::editor_tabs::OpenTab;
pub use panels::explorer::{ExplorerState, TreeItem, TreeKind};
pub use panels::pages::PagesForm;
pub use panels::repo::{RepoEdits, RepoForm};
pub use panels::templates::TemplatesForm;

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

/// Per-file push progress (template installs, site generation, scaffolds).
/// Updated live from the background thread through a shared mutex.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FileProgress {
    pub path: String,
    pub done: bool,
    pub ok: bool,
    /// html_url on success, error text on failure, empty while pending.
    pub detail: String,
}

/// GitHub Pages digest (info + latest build), rendered by the Pages panel.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PagesStatus {
    pub enabled: bool,
    pub html_url: String,
    pub build_type: String,
    /// Pages config status ("built" | "building" …).
    pub status: String,
    /// Latest build status ("built" | "building" | "errored" …).
    pub build_status: String,
    /// Set when Pages is disabled (404) or the info call failed.
    pub error: Option<String>,
}

/// Digest of GET /repos/{owner}/{repo} for the Repo panel.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RepoOverview {
    pub description: String,
    pub homepage: String,
    pub topics: Vec<String>,
    pub stars: u64,
    pub license: String,
    pub default_branch: String,
    pub html_url: String,
}

/// Digest of GET /repos/{owner}/{repo}/community/profile.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CommunityHealth {
    pub health_percent: u64,
    /// (pretty name, present) — fixed checklist order.
    pub files: Vec<(String, bool)>,
}
