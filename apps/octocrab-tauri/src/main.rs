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
            commands::templates_list,
            commands::template_install,
            commands::pages_info,
            commands::pages_enable,
            commands::pages_build,
            commands::repo_get,
            commands::repo_edit,
            commands::community_scaffold,
            commands::site_generate,
        ])
        .run(tauri::generate_context!())
        .expect("error while running octocrab-tauri");
}
