//! `octocrab-tauri` — Octocrab Control Center (Tauri v2 desktop shell).
//!
//! The window is a static, dependency-free dashboard (`dist/`); all GitHub
//! access flows through the IPC commands in [`commands`], which read the
//! token exclusively from the encrypted vault in `octocrab-core`.

mod commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::auth_status,
            commands::modules,
            commands::default_target,
            commands::list_workflows,
            commands::list_runs,
            commands::run_details,
            commands::dispatch_workflow,
            commands::list_releases,
            commands::latest_release,
            commands::list_tags,
            commands::list_repos,
            commands::list_orgs,
            commands::org_details,
        ])
        .run(tauri::generate_context!())
        .expect("error while running octocrab-tauri");
}
