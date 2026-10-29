//! `pages` — GitHub Pages lifecycle: enable (workflow build type), status,
//! request builds, and generate a **data-driven landing site** (`pages site`)
//! pulled from live repo data (description, license, latest release, stars)
//! and pushed to `site/` for the pages workflow to deploy.

use crate::cmd::auth::open_client;
use crate::cmd::repos::split_repo;
use crate::util::*;
use anyhow::Result;
use octocrab_core::templates::{render, TemplateParams};

pub async fn info(full_name: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let info = octocrab_core::pages::info(&octo, &owner, &repo).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    let g = |k: &str| {
        info.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("—")
            .to_string()
    };
    println!("{} — pages", bold(&full_name));
    println!("url:        {}", cyan(&g("html_url")));
    println!("status:     {}", g("status"));
    println!("build type: {}", g("build_type"));
    println!(
        "https only: {}",
        info.get("https_enforced")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    Ok(())
}

pub async fn enable(full_name: String, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    if !db.module_enabled("repo:admin") {
        anyhow::bail!("enabling Pages needs module 'repo:admin' — not granted this session");
    }
    let (owner, repo) = split_repo(&full_name)?;
    match octocrab_core::pages::enable(&octo, &owner, &repo).await {
        Ok(body) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&body)?);
            } else {
                println!(
                    "{} Pages enabled (build_type=workflow) for {}",
                    ok("✓"),
                    bold(&full_name)
                );
                println!(
                    "{}",
                    dim(&format!(
                        "next: octocrab-cli templates install pages {full_name} && octocrab-cli workflows trigger {full_name} pages.yml"
                    ))
                );
            }
            Ok(())
        }
        Err(e) => {
            // 409 = already enabled — treat as success for idempotence
            if e.to_string().contains("already enabled") {
                println!("{}", warn("Pages already enabled"));
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

pub async fn build(full_name: String, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    if !db.module_enabled("repo:write") {
        anyhow::bail!("requesting a Pages build needs module 'repo:write'");
    }
    let (owner, repo) = split_repo(&full_name)?;
    octocrab_core::pages::request_build(&octo, &owner, &repo).await?;
    let latest = octocrab_core::pages::latest_build(&octo, &owner, &repo).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&latest)?);
        return Ok(());
    }
    println!(
        "{} build queued — status: {} • commit {}",
        ok("✓"),
        cyan(latest.get("status").and_then(|v| v.as_str()).unwrap_or("?")),
        dim(latest.get("commit").and_then(|v| v.as_str()).unwrap_or("?"))
    );
    Ok(())
}

/// Generate a data-driven landing site from **live repo data** and push it to
/// `site/` — the pages workflow deploys it.
pub async fn site(full_name: String, tagline: Option<String>, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    if !db.module_enabled("repo:write") {
        anyhow::bail!("writing the site needs module 'repo:write'");
    }
    let (owner, repo) = split_repo(&full_name)?;

    // live data → template params
    let repo_json = octo.repos(&owner, &repo).get().await?;
    let release_tag = octo
        .repos(&owner, &repo)
        .releases()
        .list()
        .send()
        .await
        .ok()
        .and_then(|p| p.items.first().map(|r| r.tag_name.clone()));
    let stars = repo_json.stargazers_count.unwrap_or(0);

    let params = TemplateParams {
        project: repo.clone(),
        owner: owner.clone(),
        repo: repo.clone(),
        description: repo_json.description.clone().unwrap_or_default(),
        tagline: tagline.unwrap_or_else(|| match (&release_tag, stars) {
            (Some(t), 0) => format!("latest release {t} — built & shipped with the Octocrab Suite"),
            (Some(t), n) => {
                format!("latest release {t} • {n} stars — built & shipped with the Octocrab Suite")
            }
            (None, n) => format!("{n} stars — built & shipped with the Octocrab Suite"),
        }),
        license: repo_json
            .license
            .as_ref()
            .map(|l| l.spdx_id.clone())
            .filter(|s| s != "NOASSERTION")
            .unwrap_or_else(|| "MIT".into()),
        ..TemplateParams::default()
    };

    let files = render("site", &params)?;
    let mut written = Vec::new();
    for (path, content) in &files {
        let url = octocrab_core::contents::put_file(
            &octo,
            &owner,
            &repo,
            path,
            content,
            &format!(
                "site: regenerate landing page via octocrab-cli (release: {})",
                release_tag.as_deref().unwrap_or("none")
            ),
            None,
        )
        .await?;
        written.push(serde_json::json!({"path": path, "url": url}));
        println!("{} {} → {}", ok("written"), bold(path), dim(&url));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "repo": format!("{owner}/{repo}"),
                "release": release_tag,
                "files": written,
            }))?
        );
    } else {
        println!();
        println!(
            "{} site generated from live repo data (release: {}) — deploy it with",
            ok("✓"),
            cyan(release_tag.as_deref().unwrap_or("none"))
        );
        println!(
            "  {}",
            dim(&format!(
                "octocrab-cli workflows trigger {full_name} pages.yml --ref main"
            ))
        );
    }
    Ok(())
}
