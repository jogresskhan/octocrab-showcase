//! Tauri IPC commands for the Octocrab Control Center.
//!
//! Security model:
//! * Every command re-opens the shared SQLite vault and builds the octocrab
//!   client from the AES-GCM encrypted token vault — the raw token never
//!   leaves the database and is never echoed to the UI.
//! * Tokens are never accepted as command arguments; any argument that looks
//!   like a GitHub token (`ghp_`, `github_pat_`, `gho_`, …) is rejected.
//! * Every command returns `serde_json::Value`; errors are wrapped as
//!   `{"error": "…"}` payloads so the frontend can render them instead of
//!   the IPC bridge failing.

use octocrab::Octocrab;
use octocrab_core::{cache, client, db::SuiteDb};
use serde_json::{json, Value};
use std::sync::Arc;

/// Every async command returns `Ok(value)` — errors are carried inside the
/// JSON payload (`{"error": …}`) so the UI can display them.
type Cmd = Result<Value, Value>;

// ------------------------------------------------------------------ helpers

fn fail(e: impl std::fmt::Display) -> Value {
    json!({ "error": e.to_string() })
}

fn open_db() -> Result<SuiteDb, Value> {
    SuiteDb::open().map_err(fail)
}

fn client_from_db(db: &SuiteDb) -> Result<(Arc<Octocrab>, String), Value> {
    client::from_db(db).map_err(fail)
}

/// Any argument fragment that looks like a GitHub token is rejected outright.
const SECRET_MARKERS: [&str; 6] = ["ghp_", "github_pat_", "gho_", "ghu_", "ghs_", "ghr_"];

fn reject_secrets(args: &[&str]) -> Option<Value> {
    for arg in args {
        let lower = arg.to_ascii_lowercase();
        for marker in SECRET_MARKERS {
            if lower.contains(marker) {
                return Some(json!({
                    "error": "rejected: command arguments must never contain a token — credentials are read from the encrypted vault",
                }));
            }
        }
    }
    None
}

/// Read a string field off a serialized octocrab model (missing → "").
fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Read any field off a serialized octocrab model (missing → null).
fn u(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(Value::Null)
}

fn orgs_from_db(db: &SuiteDb) -> Vec<Value> {
    db.orgs()
        .into_iter()
        .map(|(login, role, read, write, admin)| {
            json!({ "login": login, "role": role, "read": read, "write": write, "admin": admin })
        })
        .collect()
}

fn masked_label(label: &str) -> String {
    if label.is_empty() {
        return "none".into();
    }
    let head = label.get(..2).unwrap_or("");
    let tail = label.get(label.len() - 2..).unwrap_or("");
    format!("{head}***{tail}")
}

// ---------------------------------------------------------------- auth/db

/// Identity, token meta (never the token itself), granted module count, orgs.
/// Minimal IPC probe: if this never resolves, the IPC bridge itself is broken.
#[tauri::command]
pub fn ping() -> Cmd {
    Ok(json!({ "pong": true }))
}

#[tauri::command]
pub async fn auth_status() -> Cmd {
    let db = open_db()?;
    let modules = db.modules();
    let granted = modules.iter().filter(|m| m.enabled).count();
    let scopes: Vec<String> = db
        .get_meta("auth_scopes")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "login": db.get_meta("auth_login"),
        "kind": db.get_meta("auth_kind"),
        "auth_state": db.get_meta("auth_state"),
        "scopes": scopes,
        "token_label": masked_label(&db.active_label().unwrap_or_default()),
        "granted": granted,
        "total_modules": modules.len(),
        "orgs": orgs_from_db(&db),
        "caps_checked_at": db.get_meta("caps_checked_at"),
    }))
}

/// Capability modules + org access map (pure DB read, no network).
#[tauri::command]
pub async fn modules() -> Cmd {
    let db = open_db()?;
    let mods: Vec<Value> = db
        .modules()
        .iter()
        .map(|m| {
            json!({
                "key": m.key,
                "enabled": m.enabled,
                "state": m.state,
                "source": m.source,
                "evidence": m.evidence,
            })
        })
        .collect();
    let granted = mods.iter().filter(|m| m["enabled"] == json!(true)).count();
    Ok(json!({
        "modules": mods,
        "granted": granted,
        "total": mods.len(),
        "orgs": orgs_from_db(&db),
    }))
}

/// Default target repository for the dashboard: prefer
/// `jogresskhan/octocrab-showcase`, fall back to the first `/user/repos` hit.
#[tauri::command]
pub async fn default_target() -> Cmd {
    const PREFERRED: &str = "jogresskhan/octocrab-showcase";
    let db = open_db()?;
    let cache_key = "target:default";
    if let Some(v) = cache::get_json::<Value>(&db, cache_key) {
        return Ok(v);
    }
    let (octo, _token) = client_from_db(&db)?;
    let mut out = json!({
        "owner": Value::Null,
        "repo": Value::Null,
        "full_name": Value::Null,
        "source": "none",
    });

    if let Ok(v) = octo
        .get::<Value, _, _>(&format!("/repos/{PREFERRED}"), None::<&[(&str, &str)]>)
        .await
    {
        let full_name = s(&v, "full_name");
        if !full_name.is_empty() {
            let owner = v.get("owner").map(|o| s(o, "login")).unwrap_or_default();
            out = json!({
                "owner": owner,
                "repo": s(&v, "name"),
                "full_name": full_name,
                "source": "preferred",
            });
        }
    }

    if out.get("owner").and_then(Value::as_str).is_none() {
        let repos: Vec<Value> = octo
            .get(
                "/user/repos",
                Some(&[("per_page", "5"), ("sort", "pushed"), ("affiliation", "owner")]),
            )
            .await
            .unwrap_or_default();
        if let Some(r) = repos.first() {
            let owner = r.get("owner").map(|o| s(o, "login")).unwrap_or_default();
            out = json!({
                "owner": owner,
                "repo": s(r, "name"),
                "full_name": s(r, "full_name"),
                "source": "fallback",
            });
        }
    }

    cache::set_json(&db, cache_key, &out, cache::TTL_MEDIUM);
    Ok(out)
}

// ------------------------------------------------------------- workflows

#[tauri::command]
pub async fn list_workflows(owner: String, repo: String) -> Cmd {
    if let Some(e) = reject_secrets(&[owner.as_str(), repo.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let page = octo
        .workflows(owner.as_str(), repo.as_str())
        .list()
        .per_page(100u8)
        .send()
        .await
        .map_err(fail)?;
    let workflows: Vec<Value> = page
        .items
        .iter()
        .map(|w| {
            let v = serde_json::to_value(w).unwrap_or(Value::Null);
            let name = s(&v, "name");
            json!({
                "id": u(&v, "id"),
                "name": if name.is_empty() { "(unnamed)".to_string() } else { name },
                "path": s(&v, "path"),
                "state": s(&v, "state"),
                "html_url": s(&v, "html_url"),
            })
        })
        .collect();
    Ok(json!({
        "owner": owner,
        "repo": repo,
        "workflows": workflows,
        "total": workflows.len(),
    }))
}

#[tauri::command]
pub async fn list_runs(owner: String, repo: String, limit: u8, branch: Option<String>) -> Cmd {
    if let Some(e) = reject_secrets(&[
        owner.as_str(),
        repo.as_str(),
        branch.as_deref().unwrap_or(""),
    ]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let wh = octo.workflows(owner.as_str(), repo.as_str());
    let mut builder = wh.list_all_runs().per_page(limit.clamp(1, 100));
    if let Some(b) = branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        builder = builder.branch(b.to_string());
    }
    let page = builder.send().await.map_err(fail)?;
    let runs: Vec<Value> = page
        .items
        .iter()
        .map(|r| {
            let v = serde_json::to_value(r).unwrap_or(Value::Null);
            json!({
                "id": u(&v, "id"),
                "run_number": u(&v, "run_number"),
                "name": s(&v, "name"),
                "head_branch": s(&v, "head_branch"),
                "head_sha": s(&v, "head_sha"),
                "event": s(&v, "event"),
                "status": s(&v, "status"),
                "conclusion": v.get("conclusion").cloned().unwrap_or(Value::Null),
                "workflow_id": u(&v, "workflow_id"),
                "created_at": s(&v, "created_at"),
                "updated_at": s(&v, "updated_at"),
                "html_url": s(&v, "html_url"),
            })
        })
        .collect();
    Ok(json!({
        "owner": owner,
        "repo": repo,
        "branch": branch,
        "runs": runs,
        "total": runs.len(),
    }))
}

/// Run + jobs + step trees. Completed runs are cached in the vault DB
/// (TTL_LONG = 1h) — in-flight runs are always fetched fresh.
#[tauri::command]
pub async fn run_details(owner: String, repo: String, run_id: u64) -> Cmd {
    if let Some(e) = reject_secrets(&[owner.as_str(), repo.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let cache_key = format!("run:{owner}/{repo}:{run_id}");
    if let Some(mut v) = cache::get_json::<Value>(&db, &cache_key) {
        v["cached"] = json!(true);
        return Ok(v);
    }
    let (octo, _token) = client_from_db(&db)?;
    let run = octo
        .workflows(owner.as_str(), repo.as_str())
        .get(octocrab::models::RunId(run_id))
        .await
        .map_err(fail)?;
    let run_v = serde_json::to_value(&run).unwrap_or(Value::Null);
    let jobs_page = octo
        .workflows(owner.as_str(), repo.as_str())
        .list_jobs(octocrab::models::RunId(run_id))
        .per_page(100u8)
        .send()
        .await
        .map_err(fail)?;
    let jobs: Vec<Value> = jobs_page
        .items
        .iter()
        .map(|j| {
            let v = serde_json::to_value(j).unwrap_or(Value::Null);
            let steps: Vec<Value> = v
                .get("steps")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|st| {
                            let name = s(st, "name");
                            json!({
                                "name": if name.is_empty() { "(unnamed)".to_string() } else { name },
                                "number": u(st, "number"),
                                "status": s(st, "status"),
                                "conclusion": st.get("conclusion").cloned().unwrap_or(Value::Null),
                                "started_at": s(st, "started_at"),
                                "completed_at": s(st, "completed_at"),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": u(&v, "id"),
                "name": s(&v, "name"),
                "status": s(&v, "status"),
                "conclusion": v.get("conclusion").cloned().unwrap_or(Value::Null),
                "started_at": s(&v, "started_at"),
                "completed_at": s(&v, "completed_at"),
                "runner_name": v.get("runner_name").cloned().unwrap_or(Value::Null),
                "steps": steps,
            })
        })
        .collect();
    let out = json!({
        "run": {
            "id": u(&run_v, "id"),
            "name": s(&run_v, "name"),
            "head_branch": s(&run_v, "head_branch"),
            "head_sha": s(&run_v, "head_sha"),
            "event": s(&run_v, "event"),
            "status": s(&run_v, "status"),
            "conclusion": run_v.get("conclusion").cloned().unwrap_or(Value::Null),
            "run_number": u(&run_v, "run_number"),
            "created_at": s(&run_v, "created_at"),
            "updated_at": s(&run_v, "updated_at"),
            "html_url": s(&run_v, "html_url"),
        },
        "jobs": jobs,
        "cached": false,
    });
    if s(&run_v, "status") == "completed" {
        cache::set_json(&db, &cache_key, &out, cache::TTL_LONG);
    }
    Ok(out)
}

/// Trigger a workflow via `workflow_dispatch`. Gated on the `workflow:write`
/// capability module — a locked module produces an error payload, not a 403
/// from GitHub.
#[tauri::command]
pub async fn dispatch_workflow(
    owner: String,
    repo: String,
    workflow_id: String,
    git_ref: String,
) -> Cmd {
    if let Some(e) = reject_secrets(&[
        owner.as_str(),
        repo.as_str(),
        workflow_id.as_str(),
        git_ref.as_str(),
    ]) {
        return Ok(e);
    }
    let db = open_db()?;
    if !db.module_enabled("workflow:write") {
        return Ok(json!({
            "ok": false,
            "error": "workflow:write module is locked — the active token has not been granted workflow dispatch",
        }));
    }
    let (octo, _token) = client_from_db(&db)?;
    octo.actions()
        .create_workflow_dispatch(
            owner.as_str(),
            repo.as_str(),
            workflow_id.as_str(),
            git_ref.as_str(),
        )
        .send()
        .await
        .map_err(fail)?;
    db.log_event(
        "dispatch",
        &format!("dispatched workflow '{workflow_id}' on {owner}/{repo} @ {git_ref}"),
    );
    Ok(json!({
        "ok": true,
        "dispatched": true,
        "workflow_id": workflow_id,
        "ref": git_ref,
    }))
}

// ------------------------------------------------------ releases & tags

/// Releases are fetched via the raw REST route so the JSON field names stay
/// stable regardless of octocrab model drift.
#[tauri::command]
pub async fn list_releases(owner: String, repo: String, limit: u8) -> Cmd {
    if let Some(e) = reject_secrets(&[owner.as_str(), repo.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let per_page = limit.clamp(1, 100).to_string();
    let raw = octo
        .get::<Value, _, _>(
            format!("/repos/{owner}/{repo}/releases").as_str(),
            Some(&[("per_page", per_page.as_str())]),
        )
        .await
        .map_err(fail)?;
    let releases: Vec<Value> = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    json!({
                        "id": u(v, "id"),
                        "tag_name": s(v, "tag_name"),
                        "name": v.get("name").cloned().unwrap_or(Value::Null),
                        "draft": v.get("draft").cloned().unwrap_or(json!(false)),
                        "prerelease": v.get("prerelease").cloned().unwrap_or(json!(false)),
                        "published_at": v.get("published_at").cloned().unwrap_or(Value::Null),
                        "target_commitish": s(v, "target_commitish"),
                        "html_url": s(v, "html_url"),
                        "body": v.get("body").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "owner": owner,
        "repo": repo,
        "releases": releases,
        "total": releases.len(),
    }))
}

#[tauri::command]
pub async fn latest_release(owner: String, repo: String) -> Cmd {
    if let Some(e) = reject_secrets(&[owner.as_str(), repo.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let v = octo
        .get::<Value, _, _>(
            format!("/repos/{owner}/{repo}/releases/latest").as_str(),
            None::<&[(&str, &str)]>,
        )
        .await
        .map_err(fail)?;
    Ok(json!({
        "owner": owner,
        "repo": repo,
        "found": true,
        "id": u(&v, "id"),
        "tag_name": s(&v, "tag_name"),
        "name": v.get("name").cloned().unwrap_or(Value::Null),
        "draft": v.get("draft").cloned().unwrap_or(json!(false)),
        "prerelease": v.get("prerelease").cloned().unwrap_or(json!(false)),
        "published_at": v.get("published_at").cloned().unwrap_or(Value::Null),
        "html_url": s(&v, "html_url"),
        "body": v.get("body").cloned().unwrap_or(Value::Null),
    }))
}

#[tauri::command]
pub async fn list_tags(owner: String, repo: String, limit: u8) -> Cmd {
    if let Some(e) = reject_secrets(&[owner.as_str(), repo.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let per_page = limit.clamp(1, 100).to_string();
    let raw = octo
        .get::<Value, _, _>(
            format!("/repos/{owner}/{repo}/tags").as_str(),
            Some(&[("per_page", per_page.as_str())]),
        )
        .await
        .map_err(fail)?;
    let tags: Vec<Value> = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    json!({
                        "name": s(v, "name"),
                        "sha": v.get("commit").map(|c| s(c, "sha")).unwrap_or_default(),
                        "zipball_url": s(v, "zipball_url"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "owner": owner,
        "repo": repo,
        "tags": tags,
        "total": tags.len(),
    }))
}

// ------------------------------------------------------- repos & orgs

/// Repositories the authenticated user has access to (`GET /user/repos`).
#[tauri::command]
pub async fn list_repos(limit: u8) -> Cmd {
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let per_page = limit.clamp(1, 100).to_string();
    let raw: Vec<Value> = octo
        .get(
            "/user/repos",
            Some(&[
                ("per_page", per_page.as_str()),
                ("sort", "pushed"),
                ("affiliation", "owner,collaborator,organization_member"),
            ]),
        )
        .await
        .map_err(fail)?;
    let repos: Vec<Value> = raw
        .iter()
        .map(|v| {
            json!({
                "full_name": s(v, "full_name"),
                "name": s(v, "name"),
                "owner": v.get("owner").map(|o| s(o, "login")).unwrap_or_default(),
                "private": v.get("private").cloned().unwrap_or(json!(false)),
                "fork": v.get("fork").cloned().unwrap_or(json!(false)),
                "description": v.get("description").cloned().unwrap_or(Value::Null),
                "language": v.get("language").cloned().unwrap_or(Value::Null),
                "stars": u(v, "stargazers_count"),
                "pushed_at": v.get("pushed_at").cloned().unwrap_or(Value::Null),
                "default_branch": s(v, "default_branch"),
                "html_url": s(v, "html_url"),
            })
        })
        .collect();
    Ok(json!({ "repos": repos, "total": repos.len() }))
}

/// Organizations the authenticated user belongs to (`GET /user/orgs`),
/// enriched with the role map persisted by the capability check.
#[tauri::command]
pub async fn list_orgs() -> Cmd {
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let raw: Vec<Value> = octo
        .get("/user/orgs", Some(&[("per_page", "50")]))
        .await
        .map_err(fail)?;
    let known = db.orgs();
    let orgs: Vec<Value> = raw
        .iter()
        .map(|v| {
            let login = s(v, "login");
            let (role, read, write, admin) = known
                .iter()
                .find(|(l, ..)| *l == login)
                .map(|(_, role, r, w, a)| (role.clone(), *r, *w, *a))
                .unwrap_or_else(|| ("unknown".to_string(), false, false, false));
            json!({
                "login": login,
                "description": v.get("description").cloned().unwrap_or(Value::Null),
                "avatar_url": s(v, "avatar_url"),
                "role": role,
                "read": read,
                "write": write,
                "admin": admin,
            })
        })
        .collect();
    Ok(json!({
        "orgs": orgs,
        "total": orgs.len(),
        "known_in_db": known.len(),
    }))
}

/// Organization profile + first page of repos + rough member count.
#[tauri::command]
pub async fn org_details(org: String) -> Cmd {
    if let Some(e) = reject_secrets(&[org.as_str()]) {
        return Ok(e);
    }
    let db = open_db()?;
    let (octo, _token) = client_from_db(&db)?;
    let org_v = serde_json::to_value(octo.orgs(org.as_str()).get().await.map_err(fail)?)
        .unwrap_or(Value::Null);
    let repos_page = octo
        .orgs(org.as_str())
        .list_repos()
        .per_page(30u8)
        .send()
        .await
        .map_err(fail)?;
    let repos: Vec<Value> = repos_page
        .items
        .iter()
        .map(|r| {
            let v = serde_json::to_value(r).unwrap_or(Value::Null);
            json!({
                "full_name": s(&v, "full_name"),
                "name": s(&v, "name"),
                "description": v.get("description").cloned().unwrap_or(Value::Null),
                "language": v.get("language").cloned().unwrap_or(Value::Null),
                "stars": u(&v, "stargazers_count"),
                "html_url": s(&v, "html_url"),
            })
        })
        .collect();
    let members_fetched = match octo.orgs(org.as_str()).list_members().per_page(100u8).send().await
    {
        Ok(p) => p.items.len(),
        Err(_) => 0,
    };
    Ok(json!({
        "login": s(&org_v, "login"),
        "name": org_v.get("name").cloned().unwrap_or(Value::Null),
        "description": org_v.get("description").cloned().unwrap_or(Value::Null),
        "blog": org_v.get("blog").cloned().unwrap_or(Value::Null),
        "location": org_v.get("location").cloned().unwrap_or(Value::Null),
        "is_verified": org_v.get("is_verified").cloned().unwrap_or(Value::Null),
        "public_repos": u(&org_v, "public_repos"),
        "html_url": s(&org_v, "html_url"),
        "repos": repos,
        "repos_fetched": repos.len(),
        "members_fetched": members_fetched,
    }))
}
