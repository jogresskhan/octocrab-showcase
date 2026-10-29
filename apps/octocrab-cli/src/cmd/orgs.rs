//! `orgs` — organizations you belong to and their details.

use crate::cmd::auth::open_client;
use crate::util::*;
use anyhow::Result;
use serde_json::json;

pub async fn list(json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    let page = octo
        .get::<serde_json::Value, _, _>("/user/orgs", Some(&[("per_page", "50")]))
        .await;
    match page {
        Ok(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            if let Some(arr) = v.as_array() {
                if arr.is_empty() {
                    println!("{}", warn("you are not a member of any organization"));
                    println!("{}", dim("→ create one: octocrab-cli achievements org <login>"));
                }
                let t = Table::new(&[24, 56]);
                t.row(&["ORG", "ACCESS (from capability map)"]);
                for o in arr {
                    let login = o.get("login").and_then(|v| v.as_str()).unwrap_or("?");
                    let known = db
                        .orgs()
                        .into_iter()
                        .find(|(l, _, _, _, _)| l == login);
                    let access = match known {
                        Some((_, role, read, write, admin)) => format!(
                            "role={} read:{} write:{} admin:{}",
                            role,
                            mark(read),
                            mark(write),
                            mark(admin)
                        ),
                        None => dim("run `auth check` to map access").to_string(),
                    };
                    t.row(&[bold(login), access]);
                }
            }
        }
        Err(e) => {
            anyhow::bail!("cannot list orgs (org:read module locked?): {e}");
        }
    }
    Ok(())
}

pub async fn get(org: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let data = octo.orgs(&org).get().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }
    println!("{} — {}", bold(&data.login), data.description.clone().unwrap_or_default());
    println!("created: {}", data.created_at.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "—".into()));
    if let Some(blog) = &data.blog {
        println!("blog: {blog}");
    }
    Ok(())
}

pub async fn members(org: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let per_page = limit.min(100).to_string();
    let page = octo
        .get::<serde_json::Value, _, _>(
            &format!("/orgs/{org}/members"),
            Some(&[("per_page", per_page.as_str())]),
        )
        .await;
    let v = page?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if let Some(arr) = v.as_array() {
        let t = Table::new(&[30, 60]);
        t.row(&["MEMBER", "AVATAR"]);
        for m in arr {
            let login = m.get("login").and_then(|v| v.as_str()).unwrap_or("?");
            t.row(&[bold(login), dim(m.get("html_url").and_then(|v| v.as_str()).unwrap_or(""))]);
        }
    }
    Ok(())
}

pub async fn repos(org: String, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let page = octo
        .orgs(&org)
        .list_repos()
        .per_page(limit.min(100) as u8)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[40, 10, 50]);
    t.row(&["REPOSITORY", "VISIB", "DESCRIPTION"]);
    for r in &page.items {
        t.row(&[
            trunc(r.full_name.as_deref().unwrap_or(""), 39),
            if r.private.unwrap_or(false) { warn("private") } else { ok("public") },
            trunc(&r.description.clone().unwrap_or_default(), 49),
        ]);
    }
    Ok(())
}

/// Create a new organization (needs org:write / admin rights on the account).
pub async fn create_org(login: String, display_name: Option<String>, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let payload = json!({
        "login": login,
        "profile_name": display_name.unwrap_or_else(|| login.clone()),
    });
    let resp = octo
        .post::<_, serde_json::Value>("/organizations", Some(&payload))
        .await;
    match resp {
        Ok(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!(
                    "{} organization '{}' created — {}",
                    ok("✓"),
                    bold(&login),
                    v.get("url").and_then(|u| u.as_str()).unwrap_or("")
                );
            }
        }
        Err(e) => {
            let detail = match &e {
                octocrab::Error::GitHub { source, .. } => {
                    format!("HTTP {} — {}", source.status_code, source.message)
                }
                other => other.to_string(),
            };
            anyhow::bail!("cannot create organization: {detail}");
        }
    }
    Ok(())
}
