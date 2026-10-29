//! `releases` — list, latest, by-tag, create.

use crate::cmd::auth::open_client;
use crate::cmd::repos::split_repo;
use crate::util::*;
use anyhow::Result;

pub async fn list(full_name: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let page = octo
        .repos(&owner, &repo)
        .releases()
        .list()
        .per_page(limit.min(100) as u8)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[16, 30, 12, 12, 24]);
    t.row(&["TAG", "NAME", "DRAFT", "PRERELEASE", "PUBLISHED"]);
    for r in &page.items {
        t.row(&[
            trunc(&r.tag_name, 15),
            trunc(&r.name.clone().unwrap_or_default(), 29),
            if r.draft { warn("yes") } else { "no".into() },
            if r.prerelease {
                warn("yes")
            } else {
                "no".into()
            },
            r.published_at
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "—".into()),
        ]);
    }
    Ok(())
}

pub async fn latest(full_name: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let r = octo.repos(&owner, &repo).releases().get_latest().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(());
    }
    println!(
        "{} {} {}",
        bold(&r.tag_name),
        cyan(r.name.as_deref().unwrap_or("")),
        if r.prerelease {
            warn("(pre-release)")
        } else {
            String::new()
        }
    );
    if let Some(b) = &r.body {
        println!("{}", dim("──────────"));
        println!("{b}");
    }
    for a in &r.assets {
        println!("asset: {} ({} bytes)", a.name, a.size);
    }
    Ok(())
}

pub async fn get_by_tag(full_name: String, tag: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let r = octo
        .repos(&owner, &repo)
        .releases()
        .get_by_tag(&tag)
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(());
    }
    println!(
        "{} {} — {}",
        bold(&r.tag_name),
        r.name.clone().unwrap_or_default(),
        r.html_url
    );
    Ok(())
}

pub async fn create(
    full_name: String,
    tag: String,
    name: Option<String>,
    notes: Option<String>,
    prerelease: bool,
    draft: bool,
    json: bool,
) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let repo_handler = octo.repos(&owner, &repo);
    let releases = repo_handler.releases();
    let name_owned = name.clone();
    let notes_owned = notes.clone();
    let mut builder = releases.create(&tag);
    if let Some(n) = &name_owned {
        builder = builder.name(n);
    }
    if let Some(b) = &notes_owned {
        builder = builder.body(b);
    }
    if prerelease {
        builder = builder.prerelease(true);
    }
    if draft {
        builder = builder.draft(true);
    }
    let release = builder.send().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&release)?);
        return Ok(());
    }
    println!(
        "{} release {} created — {}",
        ok("✓"),
        bold(&release.tag_name),
        release.html_url
    );
    Ok(())
}
