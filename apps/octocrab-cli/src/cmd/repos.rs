//! `repos` + `tags` — repository inspection and creation.

use crate::cmd::auth::open_client;
use crate::util::*;
use anyhow::Result;
use serde_json::json;

pub async fn list(limit: usize, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    let key = format!("me:repos:{limit}");
    let cached = octocrab_core::cache::get_json::<serde_json::Value>(&db, &key);
    let repos = match cached {
        Some(v) => v,
        None => {
            let page = octo
                .current()
                .list_repos_for_authenticated_user()
                .sort("pushed")
                .per_page(limit.min(100) as u8)
                .send()
                .await?;
            let v = serde_json::to_value(&page.items)?;
            octocrab_core::cache::set_json(&db, &key, &v, octocrab_core::cache::TTL_SHORT);
            v
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&repos)?);
        return Ok(());
    }
    let t = Table::new(&[34, 8, 10, 10, 40]);
    t.row(&["REPOSITORY", "VISIB", "STARS", "PUSHED", "DESCRIPTION"]);
    if let Some(arr) = repos.as_array() {
        for r in arr {
            let name = r.get("full_name").and_then(|v| v.as_str()).unwrap_or("?");
            let private = r.get("private").and_then(|v| v.as_bool()).unwrap_or(false);
            let stars = r.get("stargazers_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let pushed = r
                .get("pushed_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .get(0..10)
                .unwrap_or("");
            t.row(&[
                trunc(name, 33),
                if private { warn("private") } else { ok("public") },
                stars.to_string(),
                pushed.to_string(),
                trunc(desc, 39),
            ]);
        }
    }
    Ok(())
}

pub async fn get(full_name: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let repo = octo.repos(&owner, &repo).get().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&repo)?);
        return Ok(());
    }
    println!("{}  {}", bold(repo.full_name.as_deref().unwrap_or("")), if repo.private.unwrap_or(false) { warn("[private]") } else { ok("[public]") });
    if let Some(d) = &repo.description {
        println!("description: {d}");
    }
    if let Some(b) = &repo.default_branch {
        println!("default branch: {b}");
    }
    println!(
        "stars: {} forks: {} open issues: {}",
        repo.stargazers_count.unwrap_or(0),
        repo.forks_count.unwrap_or(0),
        repo.open_issues_count.unwrap_or(0)
    );
    println!("url: {}", repo.html_url.map(|u| u.to_string()).unwrap_or_default());
    Ok(())
}

pub async fn create(name: String, desc: Option<String>, public: bool, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let payload = json!({
        "name": name,
        "private": !public,
        "auto_init": true,
        "description": desc.unwrap_or_else(|| "Created by octocrab-cli".into()),
    });
    let repo = octo
        .post::<_, serde_json::Value>("/user/repos", Some(&payload))
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&repo)?);
        return Ok(());
    }
    println!(
        "{} {}",
        ok("repository created:"),
        bold(repo.get("full_name").and_then(|v| v.as_str()).unwrap_or("?"))
    );
    Ok(())
}

pub async fn file(full_name: String, path: String, reference: Option<String>) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let repo_handler = octo.repos(&owner, &repo);
    let mut builder = repo_handler.get_content().path(&path);
    if let Some(r) = reference {
        builder = builder.r#ref(r);
    }
    let contents = builder.send().await?;
    use base64::Engine;
    for item in contents.items {
        if let Some(bytes) = item.decoded_content() {
            print!("{bytes}");
        } else if let Some(content) = item.content {
            let cleaned: String = content.replace('\n', "");
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(cleaned.as_bytes())
                .unwrap_or_default();
            print!("{}", String::from_utf8_lossy(&decoded));
        }
    }
    Ok(())
}

pub async fn tags(full_name: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let page = octo
        .repos(&owner, &repo)
        .list_tags()
        .per_page(limit.min(100) as u8)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[40, 44]);
    t.row(&["TAG", "COMMIT"]);
    for tag in &page.items {
        t.row(&[trunc(&tag.name, 39), trunc(&tag.commit.sha, 43)]);
    }
    Ok(())
}

/// Edit repository "repo page" settings — description, homepage, topics and
/// feature flags (issues, projects, wiki, discussions). PATCH + topics PUT,
/// gated on repo:admin.
#[allow(clippy::too_many_arguments)]
pub async fn edit(
    full_name: String,
    desc: Option<String>,
    homepage: Option<String>,
    topics: Option<String>,
    has_issues: Option<bool>,
    has_wiki: Option<bool>,
    has_discussions: Option<bool>,
    json: bool,
) -> Result<()> {
    let (db, octo) = open_client()?;
    if !db.module_enabled("repo:admin") {
        anyhow::bail!("editing repo settings needs module 'repo:admin' — not granted this session");
    }
    let (owner, repo) = split_repo(&full_name)?;

    let mut payload = serde_json::Map::new();
    if let Some(d) = &desc {
        payload.insert("description".into(), serde_json::Value::String(d.clone()));
    }
    if let Some(h) = &homepage {
        payload.insert("homepage".into(), serde_json::Value::String(h.clone()));
    }
    if let Some(v) = has_issues {
        payload.insert("has_issues".into(), serde_json::Value::Bool(v));
    }
    if let Some(v) = has_wiki {
        payload.insert("has_wiki".into(), serde_json::Value::Bool(v));
    }
    if let Some(v) = has_discussions {
        payload.insert("has_discussions".into(), serde_json::Value::Bool(v));
    }
    let mut last_body = serde_json::Value::Null;
    if !payload.is_empty() {
        let (status, body) = octocrab_core::raw::raw_send(
            &octo,
            "PATCH",
            &format!("/repos/{owner}/{repo}"),
            Some(serde_json::Value::Object(payload)),
        )
        .await?;
        if status >= 400 {
            anyhow::bail!("repo edit failed: HTTP {status}");
        }
        last_body = body.clone();
        println!(
            "{} settings saved — desc: {} • homepage: {}",
            ok("✓"),
            bold(body.get("description").and_then(|v| v.as_str()).unwrap_or("—")),
            bold(body.get("homepage").and_then(|v| v.as_str()).unwrap_or("—"))
        );
    }

    if let Some(t) = topics {
        let names: Vec<String> = t
            .split(',')
            .map(|s| s.trim().to_lowercase().replace(' ', "-"))
            .filter(|s| !s.is_empty())
            .collect();
        let (status, body) = octocrab_core::raw::raw_send(
            &octo,
            "PUT",
            &format!("/repos/{owner}/{repo}/topics"),
            Some(serde_json::json!({ "names": names })),
        )
        .await?;
        if status >= 400 {
            anyhow::bail!("topics update failed: HTTP {status} — {}",
                body.get("message").and_then(|v| v.as_str()).unwrap_or(""));
        }
        last_body = serde_json::json!({ "names": names });
        println!("{} topics set: {}", ok("✓"), cyan(&last_body["names"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()));
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&last_body)?);
    }
    Ok(())
}

/// Community health / contribution settings status — which community files
/// exist (README, license, contributing, CoC, issue & PR templates).
pub async fn community(full_name: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let (status, body) = octocrab_core::raw::raw_get(
        &octo,
        &format!("/repos/{owner}/{repo}/community/profile"),
    )
    .await?;
    if status == 404 {
        anyhow::bail!("community profile unavailable for {owner}/{repo} (private repo without repo scope?)");
    }
    if status >= 400 {
        anyhow::bail!("community profile failed: HTTP {status}");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }
    let files = body.get("files").cloned().unwrap_or(serde_json::Value::Null);
    println!("{} — community health", bold(&full_name));
    let health = body
        .get("health_percentage")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    println!("health: {}%", bold(&health.to_string()));
    let row = |key: &str| -> (bool, String) {
        let f = files.get(key);
        match f {
            Some(v) if !v.is_null() => (
                true,
                v.get("html_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("present")
                    .to_string(),
            ),
            _ => (false, String::new()),
        }
    };
    let t = Table::new(&[26, 10, 44]);
    t.row(&["FILE", "STATE", "URL"]);
    for key in [
        "readme",
        "license",
        "contributing",
        "code_of_conduct",
        "issue_template",
        "pull_request_template",
    ] {
        let (present, url) = row(key);
        let state = if present { ok("present") } else { err("missing") };
        let hint = if present {
            url
        } else {
            format!("templates install community → {full_name}")
        };
        t.row(&[key.to_string(), state, trunc(&hint, 43)]);
    }
    Ok(())
}

/// Scaffold the community-health kit (thin wrapper over `templates install community`).
pub async fn scaffold(full_name: String, contact: Option<String>, json: bool) -> Result<()> {
    super::templates::install(
        "community".into(),
        full_name,
        super::templates::InstallArgs {
            contact,
            ..Default::default()
        },
        json,
    )
    .await
}

pub fn split_repo(s: &str) -> Result<(String, String)> {
    let (o, r) = s
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected OWNER/REPO, got '{s}'"))?;
    Ok((o.to_string(), r.to_string()))
}

/// Create or update a file in a repo (contents write).
pub async fn upload(
    full_name: String,
    path: String,
    file: String,
    message: String,
    branch: Option<String>,
) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let content = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("cannot read {file}: {e}"))?;
    let rh = octo.repos(&owner, &repo);

    // existing file → update_file with its blob sha; new file → create_file
    let probe = rh.get_content().path(&path);
    let probe = match branch.as_ref() {
        Some(b) => probe.r#ref(b),
        None => probe,
    };
    let existing_sha = probe
        .send()
        .await
        .ok()
        .and_then(|c| c.items.first().map(|i| i.sha.clone()));

    let url = if let Some(sha) = existing_sha {
        let mut builder = rh.update_file(&path, &message, content, &sha);
        if let Some(b) = &branch {
            builder = builder.branch(b);
        }
        let resp = builder.send().await?;
        resp.content
            .html_url
            .map(|u| u.to_string())
            .unwrap_or_default()
    } else {
        let mut builder = rh.create_file(&path, &message, content);
        if let Some(b) = &branch {
            builder = builder.branch(b);
        }
        let resp = builder.send().await?;
        resp.content
            .html_url
            .map(|u| u.to_string())
            .unwrap_or_default()
    };
    println!("{} file written — {}", ok("✓"), dim(&url));
    Ok(())
}
