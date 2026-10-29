//! `me`, `db`, `status` — identity, vault info and the machine-readable
//! summary consumed by the web control center.

use crate::cmd::auth::open_client;
use crate::util::*;
use anyhow::Result;
use serde_json::json;

pub async fn me(json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    let user = octo
        .get::<serde_json::Value, _, _>("/user", None::<&[(&str, &str)]>)
        .await?;
    db.save_account(
        user.get("login").and_then(|v| v.as_str()).unwrap_or("?"),
        &user.to_string(),
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&user)?);
        return Ok(());
    }
    let g = |k: &str| user.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let s = |k: &str| g(k).as_str().unwrap_or("—").to_string();
    println!("{}  {}", bold(&s("login")), dim(&s("name")));
    if !g("bio").is_null() {
        println!("bio: {}", s("bio"));
    }
    println!(
        "repos: {} • gists: {} • followers: {} • following: {}",
        g("public_repos"),
        g("public_gists"),
        g("followers"),
        g("following")
    );
    println!("created: {}", s("created_at"));
    println!("url: {}", s("html_url"));
    Ok(())
}

pub async fn db_info(path: bool) -> Result<()> {
    let db = octocrab_core::db::SuiteDb::open()?;
    if path {
        println!("{}", db.path().display());
        return Ok(());
    }
    println!("{}", db.info_summary()?);
    println!("recent events:");
    for (ts, kind, detail) in db.events(10) {
        println!("  {} {} {}", dim(&fmt_ts(ts)), cyan(&kind), detail);
    }
    Ok(())
}

/// Machine-readable suite summary — consumed by the web control center.
/// Contains **no token material**.
pub async fn status(json: bool) -> Result<()> {
    let db = octocrab_core::db::SuiteDb::open()?;
    if !json {
        println!("{}", db.info_summary()?);
        return Ok(());
    }
    let modules = db.modules();
    let orgs: Vec<serde_json::Value> = db
        .orgs()
        .into_iter()
        .map(|(login, role, read, write, admin)| {
            json!({"login": login, "role": role, "read": read, "write": write, "admin": admin})
        })
        .collect();
    let v = json!({
        "app": {
            "name": octocrab_core::APP_NAME,
            "id": octocrab_core::APP_ID,
        },
        "db": db.path().display().to_string(),
        "token": {
            "stored": db.list_tokens().iter().find(|t| t.active).is_some(),
            "label": db.active_label(),
            "kind": db.get_meta("auth_kind"),
            "auth_state": db.get_meta("auth_state"),
        },
        "identity": {
            "login": db.get_meta("auth_login"),
            "scopes": db.get_meta("auth_scopes").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),
        },
        "modules": modules,
        "granted": modules.iter().filter(|m| m.enabled).count(),
        "orgs": orgs,
        "checked_at": db.get_meta("caps_checked_at"),
        "binaries": ["octocrab-cli", "octocrab-ide", "octocrab-tauri"],
    });
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

/// Raw GET passthrough for any API route (`api user/repos`, `api /rate_limit`).
pub async fn api_raw(route: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let route = if route.starts_with('/') { route } else { format!("/{route}") };
    let v = octo
        .get::<serde_json::Value, _, _>(route.as_str(), None::<&[(&str, &str)]>)
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}
