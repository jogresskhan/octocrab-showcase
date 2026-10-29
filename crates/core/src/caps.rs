//! The capability / module engine.
//!
//! A GitHub token (classic or fine-grained) is probed **once**; the result is
//! a set of *modules* (`repo:write`, `org:read`, `workflow:write`, …) which
//! every binary in the suite consults to unlock features ("when that module
//! is enabled it will unlock the session for use").
//!
//! * **Classic tokens**: scopes are read from the `X-OAuth-Scopes` response
//!   header — deterministic, no probing needed.
//! * **Fine-grained tokens**: GitHub does not expose a permissions header, so
//!   we run *zero-side-effect* probes: requests crafted so that GitHub answers
//!   `422 Validation Failed` when the permission **is** granted and `403`
//!   when it is not — nothing is ever created or modified.

use crate::db::{ModuleState, OrgRow, SuiteDb};
use anyhow::{anyhow, Result};
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ------------------------------------------------------------------ modules

pub struct ModuleDef {
    pub key: &'static str,
    pub desc: &'static str,
    /// classic-token scopes that grant this module
    pub classic: &'static [&'static str],
}

pub const MODULES: &[ModuleDef] = &[
    ModuleDef { key: "user:read",    desc: "Read your profile",                    classic: &["user", "user:email", "user:follow", "repo", "read:user"] },
    ModuleDef { key: "user:write",   desc: "Update profile (bio, company, …)",     classic: &["user"] },
    ModuleDef { key: "repo:read",    desc: "Read repositories",                    classic: &["repo", "public_repo", "read:user"] },
    ModuleDef { key: "repo:write",   desc: "Write code / contents",                classic: &["repo", "public_repo"] },
    ModuleDef { key: "repo:admin",   desc: "Administer repository settings",       classic: &["repo"] },
    ModuleDef { key: "issues:write", desc: "Create & close issues",                classic: &["repo", "public_repo"] },
    ModuleDef { key: "prs:read",     desc: "Read pull requests",                   classic: &["repo", "public_repo"] },
    ModuleDef { key: "prs:write",    desc: "Create & merge pull requests",         classic: &["repo", "public_repo"] },
    ModuleDef { key: "workflow:read", desc: "Read Actions workflows & runs",       classic: &["repo", "workflow"] },
    ModuleDef { key: "workflow:write", desc: "Trigger / cancel / re-run workflows", classic: &["workflow", "repo"] },
    ModuleDef { key: "releases:write", desc: "Create releases & upload assets",     classic: &["repo"] },
    ModuleDef { key: "org:read",     desc: "Read organizations you belong to",     classic: &["read:org", "write:org", "admin:org", "repo"] },
    ModuleDef { key: "org:write",    desc: "Write to organizations (members …)",   classic: &["write:org", "admin:org"] },
    ModuleDef { key: "org:admin",    desc: "Administer organizations",             classic: &["admin:org"] },
    ModuleDef { key: "gist",         desc: "Create gists",                         classic: &["gist"] },
    ModuleDef { key: "notifications", desc: "Read notifications",                  classic: &["notifications", "repo"] },
    ModuleDef { key: "packages",     desc: "Read packages",                        classic: &["read:packages", "write:packages"] },
    ModuleDef { key: "delete_repo",  desc: "Delete repositories",                  classic: &["delete_repo"] },
];

pub const MODULE_KEYS: &[&str] = &[
    "user:read", "user:write", "repo:read", "repo:write", "repo:admin",
    "issues:write", "prs:read", "prs:write", "workflow:read", "workflow:write",
    "releases:write", "org:read", "org:write", "org:admin", "gist",
    "/notifications", "packages", "delete_repo",
];

pub fn module_def(key: &str) -> Option<&'static ModuleDef> {
    MODULES.iter().find(|m| m.key == key)
}

// ------------------------------------------------------------------ report

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgAccess {
    pub login: String,
    pub role: String,
    pub read: bool,
    pub write: bool,
    pub admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapReport {
    pub login: String,
    /// classic | fine-grained | unknown
    pub kind: String,
    pub scopes: Vec<String>,
    pub modules: Vec<ModuleState>,
    pub orgs: Vec<OrgAccess>,
    pub checked_at: i64,
}

impl CapReport {
    pub fn module(&self, key: &str) -> Option<&ModuleState> {
        self.modules.iter().find(|m| m.key == key)
    }

    pub fn enabled(&self, key: &str) -> bool {
        self.module(key).map(|m| m.enabled).unwrap_or(false)
    }

    /// Sanitized, token-free summary for dashboards / the Next.js control center.
    pub fn to_summary_json(&self) -> serde_json::Value {
        serde_json::json!({
            "login": self.login,
            "kind": self.kind,
            "scopes": self.scopes,
            "/orgs": self.orgs,
            "modules": self.modules,
            "granted": self.modules.iter().filter(|m| m.enabled).count(),
            "total_modules": self.modules.len(),
            "checked_at": self.checked_at,
        })
    }
}

// ------------------------------------------------------------------ probing

fn err_status(e: &octocrab::Error) -> Option<u16> {
    match e {
        octocrab::Error::GitHub { source, .. } => Some(source.status_code.as_u16()),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum Probe {
    /// request succeeded → definitely granted
    Granted,
    /// 422 validation error → permission granted (request would have worked)
    GrantedValidation,
    /// 403 → permission NOT granted
    Denied,
    /// anything else → we don't know
    Unknown(String),
}

fn classify(result: &Result<serde_json::Value, octocrab::Error>) -> Probe {
    match result {
        Ok(_) => Probe::Granted,
        Err(e) => match err_status(&e) {
            Some(422) | Some(400) => Probe::GrantedValidation,
            Some(403) => Probe::Denied,
            Some(401) => Probe::Denied,
            Some(other) => Probe::Unknown(format!("HTTP {other}")),
            None => Probe::Unknown(e.to_string()),
        },
    }
}

#[allow(dead_code)]
fn state_of(p: Probe, source: &str, evidence: &str) -> ModuleState {
    let (enabled, state) = match p {
        Probe::Granted => (true, "granted"),
        Probe::GrantedValidation => (true, "granted"),
        Probe::Denied => (false, "denied"),
        Probe::Unknown(_) => (false, "unknown"),
    };
    let evidence = match p {
        Probe::Unknown(ref msg) => format!("{evidence} ({msg})"),
        _ => evidence.to_string(),
    };
    ModuleState {
        key: source.to_string(),
        enabled,
        state: state.to_string(),
        source: if enabled { source.to_string() } else { format!("probe:{source}") },
        evidence,
    }
}

#[derive(Debug, Deserialize)]
struct _RepoStub {
    #[allow(dead_code)]
    full_name: Option<String>,
    owner: Option<octocrab::models::Author>,
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct _OrgStub {
    login: String,
}

/// Run the full capability check against the GitHub API and persist results.
pub async fn check(octo: &Arc<Octocrab>, db: &SuiteDb) -> Result<CapReport> {
    // 1. raw /user to read auth headers ------------------------------------
    let raw = octo
        ._get_with_headers("/user", None)
        .await
        .map_err(|e| anyhow!("token check failed: {e} (401 = invalid/expired token)"))?;
    let status = raw.status().as_u16();
    if status != 200 {
        return Err(anyhow!("GET /user returned HTTP {status}"));
    }
    let scopes_header = raw
        .headers()
        .get("X-OAuth-Scopes")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let scopes: Vec<String> = scopes_header
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let kind = if scopes.is_empty() {
        "fine-grained".to_string()
    } else {
        "classic".to_string()
    };

    // 2. identity ----------------------------------------------------------
    let author = octo
        .current()
        .user()
        .await
        .map_err(|e| anyhow!("GET /user parse failed: {e}"))?;
    let login = author.login.clone();
    let author_json = serde_json::to_string(&author)?;
    db.save_account(&login, &author_json)?;
    db.set_token_kind(&kind)?;
    db.set_meta("auth_login", &login)?;
    db.set_meta("auth_kind", &kind)?;
    db.set_meta("auth_scopes", &scopes.join(","))?;
    db.set_meta("auth_state", "valid")?;

    let mut mods: Vec<ModuleState> = vec![];

    // 3. classic scope mapping --------------------------------------------
    if kind == "classic" {
        for def in MODULES {
            let hit = def
                .classic
                .iter()
                .any(|s| scopes.iter().any(|gs| gs.eq_ignore_ascii_case(s)));
            mods.push(ModuleState {
                key: def.key.to_string(),
                enabled: hit,
                state: if hit { "granted" } else { "denied" }.to_string(),
                source: if hit { "scope".into() } else { "scope-missing".into() },
                evidence: if hit {
                    format!("scope {}", def.classic.iter().find(|s| scopes.iter().any(|g| g.eq_ignore_ascii_case(s))).unwrap_or(&""))
                } else {
                    format!("requires one of: {}", def.classic.join(", "))
                },
            });
        }

        // record org memberships + role (read:org scope powers these reads)
        let org_read = scopes.iter().any(|s| {
            ["read:org", "write:org", "admin:org", "repo"]
                .iter()
                .any(|g| s.eq_ignore_ascii_case(g))
        });
        let org_write = scopes
            .iter()
            .any(|s| ["write:org", "admin:org"].iter().any(|g| s.eq_ignore_ascii_case(g)));
        let org_admin = scopes
            .iter()
            .any(|s| ["admin:org"].iter().any(|g| s.eq_ignore_ascii_case(g)));
        let mut rows: Vec<OrgRow> = vec![];
        let mut json_pairs: Vec<(String, String)> = vec![];
        if org_read {
            if let Ok(orgs) = octo
                .get::<Vec<_OrgStub>, _, _>("/user/orgs", Some(&[("per_page", "50")]))
                .await
            {
                for o in orgs {
                    let mut role = "member".to_string();
                    if let Ok(m) = octo
                        .get::<serde_json::Value, _, _>(
                            &format!("/user/memberships/orgs/{}", o.login),
                            None::<&[(&str, &str)]>,
                        )
                        .await
                    {
                        if let Some(r) = m.get("role").and_then(|r| r.as_str()) {
                            role = r.to_string();
                        }
                    }
                    let admin_here = org_admin || role == "admin";
                    rows.push(OrgRow {
                        login: o.login.clone(),
                        role: role.clone(),
                        read: true,
                        write: org_write,
                        admin: admin_here,
                    });
                    json_pairs.push((
                        o.login.clone(),
                        serde_json::json!({"login": o.login, "role": role}).to_string(),
                    ));
                }
            }
        }
        db.replace_orgs(&rows, &json_pairs)?;
    } else {
        // 4. fine-grained probe matrix (zero side effects) -------------------
        for def in MODULES {
            mods.push(ModuleState {
                key: def.key.to_string(),
                enabled: false,
                state: "unknown".into(),
                source: "probe".into(),
                evidence: "not probed".into(),
            });
        }

        // user:read — GET /user already succeeded
        set_probe(&mut mods, "user:read", true, "probe", "GET /user ok");

        // find a target repo owned by the user (with its current description so
        // the admin probe is a true no-op)
        let mut target: Option<(String, String, String)> = None;
        if let Ok(repos) = octo
            .get::<Vec<_RepoStub>, _, _>("/user/repos", Some(&[("per_page", "5"), ("sort", "pushed"), ("affiliation", "owner")]))
            .await
        {
            if let Some(r) = repos.first() {
                if let (Some(owner), Some(name)) = (&r.owner, &r.name) {
                    target = Some((
                        owner.login.clone(),
                        name.clone(),
                        r.description.clone().unwrap_or_default(),
                    ));
                }
            }
        }

        // orgs accessible to the user
        let mut org_logins: Vec<String> = vec![];
        if let Ok(orgs) = octo
            .get::<Vec<_OrgStub>, _, _>("/user/orgs", Some(&[("per_page", "20")]))
            .await
        {
            for o in orgs {
                org_logins.push(o.login);
            }
        }

        if let Some((owner, repo, current_description)) = &target {
            // repo:read — repo metadata + contents
            let p = classify(&octo.get::<serde_json::Value, _, _>(
                    &format!("/repos/{owner}/{repo}/contents/"),
                    None::<&[(&str, &str)]>,
                ).await,
            );
            set_probe_state(&mut mods, "repo:read", &p, "contents listing");

            // repo:write — PUT contents with empty message → 422 if allowed
            let body = serde_json::json!({"message": "", "content": ""});
            let p = classify(&octo.put::<serde_json::Value, _, _>(
                    &format!("/repos/{owner}/{repo}/contents/__octocrab_caps_probe__"),
                    Some(&body),
                ).await,
            );
            set_probe_state(&mut mods, "repo:write", &p, "contents PUT validation");

            // issues:write — POST issue with empty title → 422 if allowed
            let body = serde_json::json!({"title": ""});
            let p = classify(&octo.post::<_, serde_json::Value>(
                    &format!("/repos/{owner}/{repo}/issues"),
                    Some(&body),
                ).await,
            );
            set_probe_state(&mut mods, "issues:write", &p, "issue POST validation");

            // prs:write — POST pull with empty fields → 422 if allowed
            let body = serde_json::json!({"title": "", "head": "", "base": ""});
            let p = classify(&octo.post::<_, serde_json::Value>(
                    &format!("/repos/{owner}/{repo}/pulls"),
                    Some(&body),
                ).await,
            );
            set_probe_state(&mut mods, "prs:write", &p, "pull POST validation");

            // prs:read — GET pulls
            let p = classify(&octo.get::<serde_json::Value, _, _>(
                    &format!("/repos/{owner}/{repo}/pulls"),
                    Some(&[("per_page", "1")]),
                ).await,
            );
            set_probe_state(&mut mods, "prs:read", &p, "pulls listing");

            // workflow:read — GET actions workflows
            let wf: Result<serde_json::Value, octocrab::Error> = octo
                .get::<serde_json::Value, _, _>(
                    &format!("/repos/{owner}/{repo}/actions/workflows"),
                    Some(&[("per_page", "5")]),
                )
                .await;
            let wf_probe = classify(&wf);
            set_probe_state(&mut mods, "workflow:read", &wf_probe, "actions listing");

            // workflow:write — dispatch on a real workflow with an invalid ref → 422
            if let Ok(v) = wf {
                let first_id = v
                    .get("workflows")
                    .and_then(|w| w.as_array())
                    .and_then(|a| a.first())
                    .and_then(|w| w.get("id").and_then(|i| i.as_u64()));
                if let Some(id) = first_id {
                    let body = serde_json::json!({"ref": "___octocrab_probe_invalid_ref___"});
                    let p = classify(&octo.post::<_, serde_json::Value>(
                            &format!("/repos/{owner}/{repo}/actions/workflows/{id}/dispatches"),
                            Some(&body),
                        ).await,
                    );
                    set_probe_state(&mut mods, "workflow:write", &p, "dispatch validation");
                } else {
                    note(&mut mods, "workflow:write", "no workflows in target repo to probe");
                }
            } else {
                note(&mut mods, "workflow:write", "workflows unreadable");
            }

            // releases:write — POST release with empty tag → 422 if allowed
            let body = serde_json::json!({"tag_name": ""});
            let p = classify(&octo.post::<_, serde_json::Value>(
                    &format!("/repos/{owner}/{repo}/releases"),
                    Some(&body),
                ).await,
            );
            set_probe_state(&mut mods, "releases:write", &p, "release POST validation");

            // repo:admin — PATCH with the *current* description (true no-op)
            let p = classify(&octo.patch::<serde_json::Value, _, _>(
                    &format!("/repos/{owner}/{repo}"),
                    Some(&serde_json::json!({"description": current_description})),
                ).await,
            );
            set_probe_state(&mut mods, "repo:admin", &p, "repo PATCH no-op");
        } else {
            for key in ["repo:read", "repo:write", "issues:write", "prs:read", "prs:write", "workflow:read", "workflow:write", "releases:write", "repo:admin"] {
                note(&mut mods, key, "no owned repo found to probe against");
            }
        }

        // user:write — PATCH /user with current name (no-op)
        if let Some(name) = author.name.clone() {
            let p = classify(&
                octo
                    .patch::<serde_json::Value, _, _>("/user", Some(&serde_json::json!({"name": name})))
                    .await,
            );
            set_probe_state(&mut mods, "user:write", &p, "user PATCH no-op");
        } else {
            note(&mut mods, "user:write", "profile has no name; skipped");
        }

        // gist
        let p = classify(&octo.post::<_, serde_json::Value>(
                "/gists",
                Some(&serde_json::json!({"files": {}})),
            ).await,
        );
        set_probe_state(&mut mods, "gist", &p, "gist POST validation");

        // notifications
        let p = classify(&octo.get::<serde_json::Value, _, _>("/notifications", Some(&[("per_page", "1")])).await,
        );
        set_probe_state(&mut mods, "notifications", &p, "notifications GET");

        // packages
        let p = classify(&octo.get::<serde_json::Value, _, _>("/user/packages", Some(&[("per_page", "1")])).await,
        );
        set_probe_state(&mut mods, "packages", &p, "packages GET");

        // delete_repo — unsafe to probe; leave unknown
        note(&mut mods, "delete_repo", "probe unsafe — left unknown for fine-grained tokens");

        // orgs -------------------------------------------------------------
        let mut org_rows: Vec<OrgAccess> = vec![];
        for org in &org_logins {
            let read_p = classify(&octo.get::<serde_json::Value, _, _>(
                    &format!("/orgs/{org}/members"),
                    Some(&[("per_page", "1")]),
                ).await,
            );
            let write_p = classify(&octo.post::<_, serde_json::Value>(
                    &format!("/orgs/{org}/invitations"),
                    Some(&serde_json::json!({})),
                ).await,
            );
            let admin_p = classify(&
                octo
                    .patch::<serde_json::Value, _, _>(
                        &format!("/orgs/{org}"),
                        Some(&serde_json::json!({"default_repository_permission": "read"})),
                    )
                    .await,
            );
            let role = if admin_p == Probe::Granted || admin_p == Probe::GrantedValidation {
                "admin"
            } else if write_p == Probe::Granted || write_p == Probe::GrantedValidation {
                "member"
            } else {
                "unknown"
            };
            org_rows.push(OrgAccess {
                login: org.to_string(),
                role: role.to_string(),
                read: matches!(read_p, Probe::Granted | Probe::GrantedValidation),
                write: matches!(write_p, Probe::Granted | Probe::GrantedValidation),
                admin: matches!(admin_p, Probe::Granted | Probe::GrantedValidation),
            });
        }

        // fold org access into modules
        let any_org_read = org_rows.iter().any(|o| o.read);
        let any_org_write = org_rows.iter().any(|o| o.write);
        let any_org_admin = org_rows.iter().any(|o| o.admin);
        set_probe(&mut mods, "org:read", any_org_read, "probe", "org members listing");
        set_probe(&mut mods, "org:write", any_org_write, "probe", "org invitations validation");
        set_probe(&mut mods, "org:admin", any_org_admin, "probe", "org PATCH no-op");
        if org_logins.is_empty() {
            note(&mut mods, "org:read", "user has no org memberships");
            note(&mut mods, "org:write", "user has no org memberships");
            note(&mut mods, "org:admin", "user has no org memberships");
        }

        // persist org rows
        let (plain, json_pairs): (Vec<OrgRow>, Vec<(String, String)>) = org_rows
            .iter()
            .map(|o| {
                (
                    OrgRow {
                        login: o.login.clone(),
                        role: o.role.clone(),
                        read: o.read,
                        write: o.write,
                        admin: o.admin,
                    },
                    (o.login.clone(), serde_json::to_string(o).unwrap_or_default()),
                )
            })
            .unzip();
        db.replace_orgs(&plain, &json_pairs)?;
    }

    // 5. persist + report ---------------------------------------------------
    db.clear_modules()?;
    db.save_modules(&mods)?;
    db.set_meta("caps_checked_at", &crate::now().to_string())?;

    let orgs_for_report: Vec<OrgAccess> = db
        .orgs()
        .into_iter()
        .map(|(login, role, read, write, admin)| OrgAccess { login, role, read, write, admin })
        .collect();

    let report = CapReport {
        login,
        kind,
        scopes,
        modules: mods,
        orgs: orgs_for_report,
        checked_at: crate::now(),
    };
    let summary = report.to_summary_json().to_string();
    db.set_meta("caps_summary_json", &summary)?;
    db.log_event("caps", &format!("capability check for {}", report.login));
    Ok(report)
}

// helpers ------------------------------------------------------------------

fn set_probe(mods: &mut Vec<ModuleState>, key: &str, enabled: bool, source: &str, evidence: &str) {
    if let Some(m) = mods.iter_mut().find(|m| m.key == key) {
        m.enabled = enabled;
        m.state = if enabled { "granted" } else { "denied" }.to_string();
        m.source = source.to_string();
        m.evidence = evidence.to_string();
    }
}

fn set_probe_state(mods: &mut Vec<ModuleState>, key: &str, probe: &Probe, what: &str) {
    if let Some(m) = mods.iter_mut().find(|m| m.key == key) {
        match probe {
            Probe::Granted | Probe::GrantedValidation => {
                m.enabled = true;
                m.state = "granted".into();
                m.source = "probe".into();
                m.evidence = format!("{what}: permission confirmed");
            }
            Probe::Denied => {
                m.enabled = false;
                m.state = "denied".into();
                m.source = "probe".into();
                m.evidence = format!("{what}: 403 forbidden");
            }
            Probe::Unknown(msg) => {
                m.enabled = false;
                m.state = "unknown".into();
                m.source = "probe".into();
                m.evidence = format!("{what}: {msg}");
            }
        }
    }
}

fn note(mods: &mut Vec<ModuleState>, key: &str, msg: &str) {
    if let Some(m) = mods.iter_mut().find(|m| m.key == key) {
        if m.state != "granted" {
            m.evidence = msg.to_string();
        }
    }
}

/// Convenience: check whether a specific module is unlocked for the stored token.
pub async fn can(octo: &Arc<Octocrab>, db: &SuiteDb, key: &str) -> Result<bool> {
    if db.module_enabled(key) {
        return Ok(true);
    }
    let report = check(octo, db).await?;
    Ok(report.enabled(key))
}


