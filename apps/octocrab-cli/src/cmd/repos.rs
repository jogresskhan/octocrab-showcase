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
