//! `octocrab-core` — shared foundation for the Octocrab Suite binaries
//! (`octocrab-cli`, `octocrab-ide` [egui] and `octocrab-tauri`).
//!
//! Responsibilities:
//! * SQLite database living in the Tauri-style app-data directory
//!   (`$XDG_DATA_HOME/com.octocrab.suite/octocrab.db`).
//! * AES-256-GCM encrypted token vault — the raw token never leaves the
//!   database and is never accepted on the command line (no `curl`, no
//!   `--token`, no env leaks).
//! * Data-driven capability/module engine: a token is probed once and the
//!   resulting "granted modules" unlock features in every binary.
//! * Octocrab client builder (rustls based, token pulled from the vault).

pub mod cache;
pub mod caps;
pub mod client;
pub mod contents;
pub mod crypto;
pub mod db;
pub mod pages;
pub mod raw;
pub mod templates;

/// Tauri-style application identifier (also the data directory name).
pub const APP_ID: &str = "com.octocrab.suite";
pub const APP_NAME: &str = "Octocrab Suite";

/// Resolved app-data directory (same convention Tauri apps use:
/// `dirs::data_dir()/identifier` → `~/.local/share/com.octocrab.suite`).
pub fn data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_ID)
}

/// Default database path.
pub fn db_path() -> std::path::PathBuf {
    data_dir().join("octocrab.db")
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
