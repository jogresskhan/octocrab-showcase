//! `auth` — token vault management. The token is read from stdin or a file
//! (never from argv/env, so it can not leak into shell history or `ps`).

use crate::util::*;
use anyhow::{anyhow, bail, Result};
use octocrab::Octocrab;
use octocrab_core::caps;
use octocrab_core::db::SuiteDb;
use std::io::Read;

pub async fn set(label: String, stdin: bool, file: Option<String>) -> Result<()> {
    let token = if stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf.trim().to_string()
    } else if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("cannot read token file {path}: {e}"))?;
        // shred the file after reading — one-time transport only
        let _ = std::fs::remove_file(&path);
        content.trim().to_string()
    } else {
        bail!("nothing to store — use --stdin or --file <path>");
    };

    if token.len() < 20 {
        bail!("token looks too short — refusing to store");
    }
    if !token.starts_with("ghp_")
        && !token.starts_with("github_pat_")
        && !token.starts_with("gho_")
        && !token.starts_with("ghu_")
        && !token.starts_with("ghs_")
    {
        bail!("token does not look like a GitHub PAT (expected ghp_/github_pat_/gho_/ghu_/ghs_ prefix)");
    }

    let db = SuiteDb::open()?;
    db.set_token(&label, &token)?;
    println!(
        "{}",
        ok(&format!("token '{label}' stored (AES-256-GCM encrypted)"))
    );
    println!(
        "{}",
        dim(&format!("vault: {}", octocrab_core::db_path().display()))
    );
    println!(
        "{}",
        dim("run `octocrab-cli auth check` to map capabilities")
    );
    Ok(())
}

pub async fn status(json: bool) -> Result<()> {
    let db = SuiteDb::open()?;
    if json {
        let v = serde_json::json!({
            "db": db.path().display().to_string(),
            "tokens": db.list_tokens(),
            "login": db.get_meta("auth_login"),
            "kind": db.get_meta("auth_kind"),
            "scopes": db.get_meta("auth_scopes").map(|s| s.split(',').map(|x| x.trim().to_string()).collect::<Vec<_>>()).unwrap_or_default(),
            "auth_state": db.get_meta("auth_state"),
            "modules_granted": db.modules().iter().filter(|m| m.enabled).count(),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let tokens = db.list_tokens();
    if tokens.is_empty() {
        println!("{}", warn("no token stored"));
        println!(
            "{}",
            dim("  →  printf '%s' '<token>' | octocrab-cli auth set --stdin")
        );
        return Ok(());
    }
    let t = Table::new(&[3, 18, 14, 22, 8]);
    t.row(&["", "LABEL", "KIND", "CREATED", "ACTIVE"]);
    for tok in &tokens {
        t.row(&[
            if tok.active { "●" } else { " " }.to_string(),
            trunc(&tok.label, 17),
            trunc(&tok.kind, 13),
            fmt_ts(tok.created_at),
            if tok.active { "yes" } else { "no" }.to_string(),
        ]);
    }
    if let (Some(login), Some(kind)) = (db.get_meta("auth_login"), db.get_meta("auth_kind")) {
        println!();
        println!("identity: {} ({})", bold(&login), kind);
        if let Some(scopes) = db.get_meta("auth_scopes") {
            if !scopes.is_empty() {
                println!("scopes:   {}", cyan(&scopes));
            }
        }
        let granted = db.modules().iter().filter(|m| m.enabled).count();
        println!("modules:  {} granted", bold(&granted.to_string()));
    }
    Ok(())
}

pub async fn check(json: bool) -> Result<()> {
    let db = SuiteDb::open()?;
    let (octo, _token) = octocrab_core::client::from_db(&db)?;
    println!("{}", dim("probing token capabilities (zero side effects)…"));
    let report = caps::check(&octo, &db).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report.to_summary_json())?
        );
        return Ok(());
    }
    println!();
    println!(
        "identity: {} — token kind: {} — {} module(s) granted",
        bold(&report.login),
        cyan(&report.kind),
        bold(
            &report
                .modules
                .iter()
                .filter(|m| m.enabled)
                .count()
                .to_string()
        )
    );
    if !report.scopes.is_empty() {
        println!("scopes:   {}", cyan(&report.scopes.join(", ")));
    }
    println!();
    let t = Table::new(&[18, 10, 46]);
    t.row(&["MODULE", "STATE", "EVIDENCE"]);
    for m in &report.modules {
        let state = match m.state.as_str() {
            "granted" => ok("granted"),
            "denied" => err("denied"),
            _ => warn("unknown"),
        };
        t.row(&[m.key.clone(), state, trunc(&m.evidence, 45)]);
    }
    if !report.orgs.is_empty() {
        println!();
        println!("{}", bold("organization access:"));
        let t = Table::new(&[18, 10, 8, 8, 8]);
        t.row(&["ORG", "ROLE", "READ", "WRITE", "ADMIN"]);
        for o in &report.orgs {
            t.row(&[
                o.login.clone(),
                o.role.clone(),
                if o.read { ok("✓") } else { err("✗") },
                if o.write { ok("✓") } else { err("✗") },
                if o.admin { ok("✓") } else { err("✗") },
            ]);
        }
    }
    Ok(())
}

pub async fn clear() -> Result<()> {
    let db = SuiteDb::open()?;
    db.clear_token()?;
    println!("{}", ok("all tokens cleared from vault"));
    Ok(())
}

pub async fn tokens() -> Result<()> {
    let db = SuiteDb::open()?;
    for t in db.list_tokens() {
        println!(
            "{} {} [{}] created {}{}",
            if t.active { "●" } else { " " },
            bold(&t.label),
            t.kind,
            fmt_ts(t.created_at),
            if t.active {
                "".to_string()
            } else {
                dim(" (inactive)")
            }
        );
    }
    Ok(())
}

/// Shared helper used by every other command.
pub fn open_client() -> Result<(SuiteDb, std::sync::Arc<Octocrab>)> {
    let db = SuiteDb::open()?;
    let (octo, _t) = octocrab_core::client::from_db(&db)?;
    Ok((db, octo))
}

/// git credential-helper bridge (`auth git-credential`).
///
/// Speaks the git credential protocol on stdin/stdout so `git push/pull`
/// can authenticate **straight from the encrypted vault**:
///
/// ```text
/// git -c credential.helper= \
///     -c credential.helper='!octocrab-cli auth git-credential' \
///     push origin main
/// ```
///
/// The token never appears in argv, env vars, the remote URL, `.git/config`
/// or shell history — it flows vault → helper stdout pipe → git only.
/// Only `https://github.com` is served; everything else is refused.
/// `store`/`erase` are accepted but intentionally no-ops: the vault is the
/// single source of truth and git must never persist (or wipe) the token.
pub fn git_credential(op: &str) -> Result<()> {
    if op != "get" {
        return Ok(()); // store/erase: no-op, vault stays authoritative
    }

    let mut request = String::new();
    std::io::stdin().read_to_string(&mut request)?;

    let mut protocol = String::new();
    let mut host = String::new();
    for line in request.lines() {
        if let Some(v) = line.strip_prefix("protocol=") {
            protocol = v.to_string();
        } else if let Some(v) = line.strip_prefix("host=") {
            host = v.to_string();
        }
    }
    if !protocol.is_empty() && protocol != "https" {
        bail!("vault credential helper only serves https (got '{protocol}')");
    }
    if host != "github.com" {
        bail!("vault credential helper only serves github.com (got '{host}')");
    }

    let db = SuiteDb::open()?;
    let (_octo, token) = octocrab_core::client::from_db(&db)?;
    let username = db
        .get_meta("auth_login")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "octocrab-cli".to_string());

    // git credential protocol response (terminated by a blank line)
    println!("protocol=https");
    println!("host=github.com");
    println!("username={username}");
    println!("password={token}");
    Ok(())
}
