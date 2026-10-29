//! `octocrab-ide` — a fast, data-driven, VSCode-style IDE in pure Rust.
//!
//! All GitHub I/O happens on background threads (`poll_promise::Promise`
//! plus one shared tokio runtime); the UI renders instantly from the SQLite
//! TTL cache and swaps in fresh data when background refreshes land.

mod app;
mod fuzzy;
mod net;
mod selftest;

use std::sync::{Arc, Mutex};

use octocrab_core::db::SuiteDb;

fn open_db() -> Arc<Mutex<SuiteDb>> {
    match SuiteDb::open() {
        Ok(db) => return Arc::new(Mutex::new(db)),
        Err(e) => eprintln!("octocrab-ide: vault db failed to open: {e}"),
    }
    let fallback = std::env::temp_dir().join("com.octocrab.suite-fallback.db");
    match SuiteDb::open_at(&fallback) {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(e) => {
            eprintln!("octocrab-ide: fallback db failed: {e}");
            std::process::exit(1);
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let selftest = std::env::args().any(|a| a == "--selftest");

    if selftest {
        // 1) offline render checks: every new panel, canned data, no network
        if let Err(e) = selftest::run_offline_panel_checks() {
            eprintln!("octocrab-ide --selftest: offline panel checks FAILED: {e}");
            std::process::exit(2);
        }
        eprintln!("octocrab-ide --selftest: offline panel checks passed");
    }

    let db = open_db();

    // Create the shared tokio runtime up front so background promises can
    // block_on GitHub calls immediately (never blocking the UI thread).
    let _rt = net::global_runtime();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Octocrab IDE"),
        ..Default::default()
    };

    eframe::run_native(
        "Octocrab IDE",
        options,
        Box::new(move |cc| {
            octocrab_ui_egui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::IdeApp::new(db, selftest)) as Box<dyn eframe::App>)
        }),
    )
}
