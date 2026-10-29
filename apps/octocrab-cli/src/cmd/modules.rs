//! `modules` — inspect the capability modules unlocked for the session.

use crate::cmd::auth::open_client;
use crate::util::*;
use anyhow::Result;
use octocrab_core::caps::{module_def, MODULES};

pub async fn list(json: bool) -> Result<()> {
    let (db, _octo) = open_client()?;
    let modules = db.modules();
    if modules.is_empty() {
        println!("{}", warn("no capability data yet — run: octocrab-cli auth check"));
        return Ok(());
    }
    if json {
        let v = serde_json::json!({
            "modules": modules,
            "orgs": db.orgs().into_iter().map(|(login, role, read, write, admin)| {
                serde_json::json!({"login": login, "role": role, "read": read, "write": write, "admin": admin})
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let t = Table::new(&[18, 10, 46]);
    t.row(&["MODULE", "STATE", "DESCRIPTION"]);
    for def in MODULES {
        let m = modules.iter().find(|m| m.key == def.key);
        let (state, enabled) = match m {
            Some(m) => (
                match m.state.as_str() {
                    "granted" => ok("granted"),
                    "denied" => err("denied"),
                    _ => warn("unknown"),
                },
                m.enabled,
            ),
            None => (dim("—"), false),
        };
        t.row(&[def.key.to_string(), state, trunc(def.desc, 45)]);
        if enabled {
            if let Some(m) = m {
                if !m.evidence.is_empty() {
        t.row(&["".to_string(), "".to_string(), dim(&format!("↳ {}", m.evidence))]);
                }
            }
        }
    }
    let orgs = db.orgs();
    if !orgs.is_empty() {
        println!();
        println!("{}", bold("organization access:"));
        for (login, role, read, write, admin) in orgs {
            println!(
                "  {} — role: {} — read:{} write:{} admin:{}",
                bold(&login),
                role,
                mark(read),
                mark(write),
                mark(admin)
            );
        }
    }
    Ok(())
}

pub async fn can(key: String) -> Result<()> {
    if module_def(&key).is_none() {
        anyhow::bail!("unknown module '{key}' — see `modules list`");
    }
    let (db, _octo) = open_client()?;
    let granted = db.module_enabled(&key);
    println!(
        "{}: {}",
        key,
        if granted { ok("granted") } else { err("locked") }
    );
    std::process::exit(if granted { 0 } else { 2 });
}
