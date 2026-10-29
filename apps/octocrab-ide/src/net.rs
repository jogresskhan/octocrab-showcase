//! All GitHub I/O for the IDE, written as plain `async fn`s over
//! `Arc<Octocrab>`, executed on background threads via one shared tokio
//! runtime (`block_on` inside `Promise::spawn_blocking` closures) so the UI
//! thread never touches the network.

use anyhow::{anyhow, bail, Result};
use octocrab::Octocrab;
use octocrab_ui_egui::{
    CommunityHealth, JobRow, PagesStatus, ReleaseRow, RepoOverview, RunRow, SearchHit, StepRow,
    TagRow, TreeItem, TreeKind,
};
use std::sync::{Arc, OnceLock};

/// The process-wide tokio runtime (multi-thread, all features enabled).
pub fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// Drive an async block to completion on the current background thread.
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    global_runtime().block_on(fut)
}

/// Wrap a fallible future with a hard timeout so the UI never hangs.
pub async fn with_timeout<T, E, F>(secs: u64, fut: F) -> Result<T>
where
    E: Into<anyhow::Error>,
    F: std::future::Future<Output = Result<T, E>>,
{
    tokio::time::timeout(std::time::Duration::from_secs(secs), fut)
        .await
        .map_err(|_| anyhow!("timed out after {secs}s"))?
        .map_err(Into::into)
}

// ------------------------------------------------------------------ repo

#[derive(Clone, Debug)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub default_branch: String,
}

pub const SHOWCASE: (&str, &str) = ("jogresskhan", "octocrab-showcase");

fn parse_repo_value(v: &serde_json::Value) -> Option<RepoInfo> {
    let full = v.get("full_name")?.as_str()?;
    let (owner, name) = full.split_once('/')?;
    let branch = v
        .get("default_branch")
        .and_then(|b| b.as_str())
        .unwrap_or("main")
        .to_string();
    Some(RepoInfo {
        owner: owner.to_string(),
        name: name.to_string(),
        default_branch: branch,
    })
}

/// Preferred showcase repo, falling back to the first repo of `/user/repos`.
pub async fn default_repo(octo: &Arc<Octocrab>) -> Result<RepoInfo> {
    let (o, r) = SHOWCASE;
    let fut =
        octo.get::<serde_json::Value, _, _>(format!("/repos/{o}/{r}"), None::<&[(&str, &str)]>);
    if let Ok(v) = with_timeout(10, fut).await {
        if let Some(info) = parse_repo_value(&v) {
            return Ok(info);
        }
    }
    let params = [
        ("per_page", "30"),
        ("sort", "pushed"),
        ("affiliation", "owner"),
    ];
    let fut = octo.get::<serde_json::Value, _, _>("/user/repos", Some(&params));
    let v = with_timeout(10, fut).await?;
    if let Some(arr) = v.as_array() {
        for item in arr {
            if let Some(info) = parse_repo_value(item) {
                return Ok(info);
            }
        }
    }
    Err(anyhow!("no accessible repositories found"))
}

pub async fn identity(octo: &Arc<Octocrab>) -> Result<String> {
    let handler = octo.current();
    let fut = handler.user();
    let user = with_timeout(10, fut).await?;
    Ok(user.login)
}

// ------------------------------------------------------------------ tree

pub async fn fetch_tree(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<(Vec<TreeItem>, bool)> {
    let rh = octo.repos(owner, repo);
    let fut = rh.get_tree(branch).recursive(true).send();
    let tree = with_timeout(20, fut).await?;
    let mut paths: Vec<(String, TreeKind)> = tree
        .tree
        .iter()
        .map(|e| {
            (
                e.path.clone(),
                if e.r#type == "tree" {
                    TreeKind::Dir
                } else {
                    TreeKind::File
                },
            )
        })
        .collect();
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((TreeItem::build_from_paths(&paths), tree.truncated))
}

pub async fn fetch_file(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
) -> Result<String> {
    let rh = octo.repos(owner, repo);
    let fut = rh.get_content().path(path).r#ref(branch).send();
    let items = with_timeout(20, fut).await?;
    let first = items
        .items
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("empty contents response"))?;
    // base64 payload may contain newlines — strip all whitespace first.
    let raw = first.content.unwrap_or_default();
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    use base64::Engine as _;
    let bytes = base64::prelude::BASE64_STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| anyhow!("base64 decode failed: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// -------------------------------------------------------------- workflows

pub async fn fetch_runs(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: usize,
) -> Result<Vec<RunRow>> {
    let limit_s = limit.to_string();
    let params = [("per_page", limit_s.as_str())];
    let fut = octo.get::<serde_json::Value, _, _>(
        format!("/repos/{owner}/{repo}/actions/runs"),
        Some(&params),
    );
    let v = with_timeout(15, fut).await?;
    let mut rows = Vec::new();
    if let Some(arr) = v.get("workflow_runs").and_then(|x| x.as_array()) {
        for it in arr {
            let id = it.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let name = it
                .get("name")
                .and_then(|x| x.as_str())
                .or_else(|| it.get("display_title").and_then(|x| x.as_str()))
                .unwrap_or("(unnamed)")
                .to_string();
            let branch = it
                .get("head_branch")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let status = it
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            let conclusion = it
                .get("conclusion")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let created = it
                .get("created_at")
                .and_then(|x| x.as_str())
                .map(|s| s.get(..16).unwrap_or(s).replace('T', " "))
                .unwrap_or_default();
            rows.push(RunRow {
                id,
                name,
                branch,
                status,
                conclusion,
                created,
            });
        }
    }
    Ok(rows)
}

fn status_str(s: octocrab::models::workflows::Status) -> &'static str {
    use octocrab::models::workflows::Status::*;
    match s {
        Pending => "pending",
        Queued => "queued",
        InProgress => "in_progress",
        Completed => "completed",
        Failed => "failure",
        Waiting => "waiting",
        _ => "unknown",
    }
}

fn conclusion_str(c: octocrab::models::workflows::Conclusion) -> &'static str {
    use octocrab::models::workflows::Conclusion::*;
    match c {
        ActionRequired => "action_required",
        Cancelled => "cancelled",
        Failure => "failure",
        Neutral => "neutral",
        Skipped => "skipped",
        Success => "success",
        TimedOut => "timed_out",
        _ => "unknown",
    }
}

fn fmt_secs(secs: i64) -> String {
    if secs <= 0 {
        String::new()
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn between(a: chrono::DateTime<chrono::Utc>, b: Option<chrono::DateTime<chrono::Utc>>) -> String {
    b.map(|b| fmt_secs((b - a).num_seconds()))
        .unwrap_or_default()
}

pub async fn fetch_jobs(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<Vec<JobRow>> {
    let wh = octo.workflows(owner, repo);
    let fut = wh
        .list_jobs(octocrab::models::RunId(run_id))
        .per_page(50)
        .send();
    let page = with_timeout(15, fut).await?;
    Ok(page
        .items
        .into_iter()
        .map(|j| JobRow {
            id: j.id.0,
            name: j.name.clone(),
            status: status_str(j.status).to_string(),
            conclusion: j.conclusion.map(conclusion_str).map(|s| s.to_string()),
            started: j.started_at.format("%H:%M:%S").to_string(),
            duration: between(j.started_at, j.completed_at),
            steps: j
                .steps
                .into_iter()
                .map(|s| StepRow {
                    name: s.name.clone(),
                    status: status_str(s.status).to_string(),
                    conclusion: s.conclusion.map(conclusion_str).map(|s| s.to_string()),
                    duration: match (s.started_at, s.completed_at) {
                        (Some(a), Some(b)) => fmt_secs((b - a).num_seconds()),
                        _ => String::new(),
                    },
                })
                .collect(),
        })
        .collect())
}

// ------------------------------------------------------- releases & tags

pub async fn fetch_releases(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: usize,
) -> Result<Vec<ReleaseRow>> {
    let rh = octo.repos(owner, repo);
    let rel = rh.releases();
    let fut = rel.list().per_page(limit as u8).send();
    let page = with_timeout(15, fut).await?;
    Ok(page
        .items
        .into_iter()
        .map(|r| ReleaseRow {
            tag: r.tag_name.clone(),
            name: r.name.clone().unwrap_or_else(|| r.tag_name.clone()),
            date: r
                .published_at
                .or(r.created_at)
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
            prerelease: r.prerelease,
            draft: r.draft,
        })
        .collect())
}

pub async fn fetch_tags(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: usize,
) -> Result<Vec<TagRow>> {
    let rh = octo.repos(owner, repo);
    let fut = rh.list_tags().per_page(limit as u8).send();
    let page = with_timeout(15, fut).await?;
    Ok(page
        .items
        .into_iter()
        .map(|t| TagRow {
            name: t.name.clone(),
            sha: t.commit.sha.chars().take(7).collect(),
        })
        .collect())
}

// ----------------------------------------------------------------- pulls

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PullRow {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub draft: bool,
    pub branch: String,
}

pub async fn fetch_pulls(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<Vec<PullRow>> {
    let params = [("per_page", "30"), ("state", "open")];
    let fut =
        octo.get::<serde_json::Value, _, _>(format!("/repos/{owner}/{repo}/pulls"), Some(&params));
    let v = with_timeout(15, fut).await?;
    let mut rows = Vec::new();
    if let Some(arr) = v.as_array() {
        for it in arr {
            rows.push(PullRow {
                number: it.get("number").and_then(|x| x.as_u64()).unwrap_or(0),
                title: it
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("(untitled)")
                    .to_string(),
                author: it
                    .pointer("/user/login")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                draft: it.get("draft").and_then(|x| x.as_bool()).unwrap_or(false),
                branch: it
                    .pointer("/head/ref")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------- code search

pub async fn code_search(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    q: &str,
) -> Result<Vec<SearchHit>> {
    let query = format!("{q} repo:{owner}/{repo}");
    let params = [("q", query.as_str()), ("per_page", "30")];
    let fut = octo.get::<serde_json::Value, _, _>("/search/code", Some(&params));
    let v = with_timeout(15, fut).await?;
    let mut hits = Vec::new();
    if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
        for it in arr {
            let path = it
                .get("path")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let detail = it
                .pointer("/repository/full_name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if !path.is_empty() {
                hits.push(SearchHit {
                    path,
                    detail,
                    gh: true,
                });
            }
        }
    }
    Ok(hits)
}

// ------------------------------------------------------------- dispatch

/// `(workflow path, workflow name)` pairs usable for dispatch.
pub async fn list_workflows(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
) -> Result<Vec<(String, String)>> {
    let params = [("per_page", "10")];
    let fut = octo.get::<serde_json::Value, _, _>(
        format!("/repos/{owner}/{repo}/actions/workflows"),
        Some(&params),
    );
    let v = with_timeout(15, fut).await?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("workflows").and_then(|x| x.as_array()) {
        for w in arr {
            let path = w
                .get("path")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let name = w
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if !path.is_empty() {
                out.push((path, name));
            }
        }
    }
    Ok(out)
}

/// Dispatch a workflow by path/ref. Uses `_post` (no body parse — GitHub
/// answers 204 No Content).
pub async fn dispatch_workflow(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    workflow_ref: &str,
    git_ref: &str,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/actions/workflows/{workflow_ref}/dispatches");
    let body = serde_json::json!({ "ref": git_ref });
    let fut = octo._post(route.as_str(), Some(&body));
    let resp = with_timeout(15, fut).await?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(anyhow!("dispatch returned HTTP {status}"))
    }
}

// ------------------------------------------------- templates / pages / repo

/// Push one file through the Contents API (sha-probed update-or-create).
pub async fn put_file(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    path: &str,
    content: &str,
    message: &str,
    branch: Option<&str>,
) -> Result<String> {
    with_timeout(
        40,
        octocrab_core::contents::put_file(octo, owner, repo, path, content, message, branch),
    )
    .await
}

/// Render the `site` template from *live* repo data: `repos().get()` for
/// project/description/license plus the latest release tag as the tagline.
pub async fn site_files(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
) -> Result<Vec<(String, String)>> {
    let rh = octo.repos(owner, repo);
    let repo_model = with_timeout(15, rh.get()).await?;
    let tag = with_timeout(15, rh.releases().list().per_page(1).send())
        .await
        .ok()
        .and_then(|page| page.items.first().map(|r| r.tag_name.clone()))
        .unwrap_or_default();
    let params = octocrab_core::templates::TemplateParams {
        project: repo_model.name.clone(),
        owner: owner.to_string(),
        repo: repo.to_string(),
        description: repo_model.description.clone().unwrap_or_default(),
        tagline: if tag.is_empty() {
            "built with the Octocrab Suite".to_string()
        } else {
            format!("latest release {tag}")
        },
        license: repo_model
            .license
            .map(|l| l.name)
            .unwrap_or_else(|| "MIT".to_string()),
        ..Default::default()
    };
    octocrab_core::templates::render("site", &params)
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Digest pages info + latest build into a `PagesStatus`.
fn pages_status_from(info: serde_json::Value, build: Option<serde_json::Value>) -> PagesStatus {
    PagesStatus {
        enabled: true,
        html_url: json_str(&info, "html_url"),
        build_type: json_str(&info, "build_type"),
        status: json_str(&info, "status"),
        build_status: build
            .as_ref()
            .map(|b| json_str(b, "status"))
            .unwrap_or_default(),
        error: None,
    }
}

/// Pages info (404 → `enabled: false` with the reason set).
pub async fn pages_status(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<PagesStatus> {
    match octocrab_core::pages::info(octo, owner, repo).await {
        Err(e) => Ok(PagesStatus {
            enabled: false,
            error: Some(e.to_string()),
            ..Default::default()
        }),
        Ok(info) => {
            let build = octocrab_core::pages::latest_build(octo, owner, repo)
                .await
                .ok();
            Ok(pages_status_from(info, build))
        }
    }
}

/// Enable Pages (workflow build type), then re-read the status.
pub async fn pages_enable(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<PagesStatus> {
    octocrab_core::pages::enable(octo, owner, repo).await?;
    pages_status(octo, owner, repo).await
}

/// Request a Pages build, then report the latest build status.
pub async fn pages_build(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<PagesStatus> {
    octocrab_core::pages::request_build(octo, owner, repo).await?;
    let build = octocrab_core::pages::latest_build(octo, owner, repo)
        .await
        .ok();
    let build_status = || {
        build
            .as_ref()
            .map(|b| json_str(b, "status"))
            .unwrap_or_default()
    };
    let mut status = match octocrab_core::pages::info(octo, owner, repo).await {
        Ok(info) => pages_status_from(info, build.clone()),
        Err(_) => PagesStatus {
            enabled: true, // request_build succeeded → Pages exists
            ..Default::default()
        },
    };
    if status.build_status.is_empty() {
        status.build_status = build_status();
    }
    Ok(status)
}

/// Digest of GET /repos/{o}/{r} for the Repo panel.
pub async fn repo_overview(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<RepoOverview> {
    let (status, v) = octocrab_core::raw::raw_get(octo, &format!("/repos/{owner}/{repo}")).await?;
    if status >= 400 {
        bail!("repo lookup failed: HTTP {status}");
    }
    Ok(RepoOverview {
        description: json_str(&v, "description"),
        homepage: json_str(&v, "homepage"),
        topics: v
            .get("topics")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        stars: v
            .get("stargazers_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        license: v
            .get("license")
            .and_then(|l| l.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(none)")
            .to_string(),
        default_branch: v
            .get("default_branch")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("main")
            .to_string(),
        html_url: json_str(&v, "html_url"),
    })
}

/// Community-health checklist keys in fixed display order.
const COMMUNITY_FILES: [(&str, &str); 6] = [
    ("readme", "README"),
    ("license", "LICENSE"),
    ("contributing", "CONTRIBUTING"),
    ("code_of_conduct", "Code of Conduct"),
    ("issue_template", "Issue templates"),
    ("pull_request_template", "PR template"),
];

/// Digest of GET /repos/{o}/{r}/community/profile.
pub async fn community_profile(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
) -> Result<CommunityHealth> {
    let (status, v) =
        octocrab_core::raw::raw_get(octo, &format!("/repos/{owner}/{repo}/community/profile"))
            .await?;
    if status >= 400 {
        bail!("community profile: HTTP {status}");
    }
    let files_obj = v.get("files").cloned().unwrap_or(serde_json::Value::Null);
    Ok(CommunityHealth {
        health_percent: v
            .get("health_percentage")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        files: COMMUNITY_FILES
            .iter()
            .map(|(key, label)| {
                let present = files_obj.get(*key).map(|f| !f.is_null()).unwrap_or(false);
                (label.to_string(), present)
            })
            .collect(),
    })
}

/// PATCH /repos/{o}/{r} (description, homepage) + PUT /repos/{o}/{r}/topics.
pub async fn save_repo_settings(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    description: &str,
    homepage: &str,
    topics: &[String],
) -> Result<String> {
    let (status, body) = octocrab_core::raw::raw_send(
        octo,
        "PATCH",
        &format!("/repos/{owner}/{repo}"),
        Some(serde_json::json!({
            "description": description,
            "homepage": homepage,
        })),
    )
    .await?;
    if status >= 400 {
        bail!(
            "PATCH /repos failed: HTTP {status} — {}",
            json_str(&body, "message")
        );
    }
    let (status, body) = octocrab_core::raw::raw_send(
        octo,
        "PUT",
        &format!("/repos/{owner}/{repo}/topics"),
        Some(serde_json::json!({ "names": topics })),
    )
    .await?;
    if status >= 400 {
        bail!(
            "PUT /topics failed: HTTP {status} — {}",
            json_str(&body, "message")
        );
    }
    Ok(format!(
        "settings saved · {} topic(s) updated",
        topics.len()
    ))
}

// --------------------------------------------------------- local walkdir

/// Walk the local workspace into tree paths (skips .git / target /
/// node_modules, caps at 100k entries) — runs on a background thread.
pub fn walk_local_workspace(root: &str) -> Vec<TreeItem> {
    let root_path = std::path::Path::new(root);
    let root_str = root_path.to_string_lossy().to_string();
    let mut paths: Vec<(String, TreeKind)> = Vec::new();
    for entry in walkdir::WalkDir::new(root_path)
        .follow_links(false)
        .max_depth(14)
        .into_iter()
        .filter_entry(|e| !skipped_dir(e))
        .filter_map(|e| e.ok())
    {
        if entry.depth() == 0 {
            continue;
        }
        let is_dir = entry.file_type().is_dir();
        let p = entry.path().to_string_lossy().to_string();
        let rel = p
            .strip_prefix(&root_str)
            .unwrap_or(&p)
            .trim_start_matches('/')
            .to_string();
        if rel.is_empty() {
            continue;
        }
        paths.push((
            rel,
            if is_dir {
                TreeKind::Dir
            } else {
                TreeKind::File
            },
        ));
        if paths.len() >= 100_000 {
            break;
        }
    }
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    TreeItem::build_from_paths(&paths)
}

fn skipped_dir(e: &walkdir::DirEntry) -> bool {
    if !e.file_type().is_dir() {
        return false;
    }
    matches!(
        e.file_name().to_str(),
        Some(".git") | Some("target") | Some("node_modules") | Some(".turbo") | Some("dist")
    )
}
