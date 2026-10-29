//! `issues` + `pulls`.

use crate::cmd::auth::open_client;
use crate::cmd::repos::split_repo;
use crate::util::*;
use anyhow::Result;
use serde_json::json;

fn state_str(v: &impl serde::Serialize) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(String::from))
        .unwrap_or_default()
}

pub async fn issues_list(full_name: String, state: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let page = octo
        .issues(&owner, &repo)
        .list()
        .state(match state.as_str() {
            "closed" => octocrab::params::State::Closed,
            "all" => octocrab::params::State::All,
            _ => octocrab::params::State::Open,
        })
        .per_page(limit.min(100) as u8)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[8, 50, 8, 22]);
    t.row(&["#", "TITLE", "STATE", "URL"]);
    for i in &page.items {
        let st = state_str(&i.state);
        t.row(&[
            i.number.to_string(),
            trunc(&i.title, 49),
            if st == "open" { ok("open") } else { dim("closed") },
            dim(&trunc(i.html_url.as_str(), 21)),
        ]);
    }
    Ok(())
}

pub async fn issues_create(full_name: String, title: String, body: Option<String>) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let handler = octo.issues(&owner, &repo);
    let mut builder = handler.create(title);
    if let Some(b) = body {
        builder = builder.body(b);
    }
    let issue = builder.send().await?;
    println!("{} issue #{} created — {}", ok("✓"), issue.number, issue.html_url);
    Ok(())
}

pub async fn issues_comment(full_name: String, number: u64, body: String) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let payload = json!({ "body": body });
    let comment = octo
        .post::<_, serde_json::Value>(
            &format!("/repos/{owner}/{repo}/issues/{number}/comments"),
            Some(&payload),
        )
        .await?;
    let url = comment.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
    println!("{} comment added — {}", ok("✓"), dim(url));
    Ok(())
}

pub async fn issues_close(full_name: String, number: u64) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let payload = json!({"state": "closed"});
    octo
        .patch::<serde_json::Value, _, _>(
            &format!("/repos/{owner}/{repo}/issues/{number}"),
            Some(&payload),
        )
        .await?;
    println!("{} issue #{number} closed", ok("✓"));
    Ok(())
}

pub async fn pulls_list(full_name: String, state: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let page = octo
        .pulls(&owner, &repo)
        .list()
        .state(match state.as_str() {
            "closed" => octocrab::params::State::Closed,
            "all" => octocrab::params::State::All,
            _ => octocrab::params::State::Open,
        })
        .per_page(limit.min(100) as u8)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[8, 50, 8, 22]);
    t.row(&["#", "TITLE", "DRAFT", "URL"]);
    for p in &page.items {
        let url = p
            .html_url
            .as_ref()
            .map(|u| trunc(u.as_str(), 21))
            .unwrap_or_default();
        t.row(&[
            p.number.to_string(),
            trunc(p.title.as_deref().unwrap_or(""), 49),
            if p.draft.unwrap_or(false) { warn("draft") } else { "ready".to_string() },
            dim(&url),
        ]);
    }
    Ok(())
}
