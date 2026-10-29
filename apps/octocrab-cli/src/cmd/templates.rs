//! `templates` — data-driven repo scaffolding: install GitHub Actions
//! workflows (multi-OS CI + packaging/release), a GitHub Pages site, a
//! community-health kit and a README scaffold, all rendered from
//! `octocrab_core::templates::TemplateParams` and pushed via the Contents
//! API. Everything is module-gated.

use crate::cmd::auth::open_client;
use crate::cmd::repos::split_repo;
use crate::util::*;
use anyhow::Result;
use octocrab_core::templates::{template_def, render, TemplateParams, TEMPLATES};

pub async fn list(json: bool) -> Result<()> {
    if json {
        let v: Vec<serde_json::Value> = TEMPLATES
            .iter()
            .map(|t| {
                serde_json::json!({
                    "key": t.key,
                    "name": t.name,
                    "desc": t.desc,
                    "files": t.files,
                    "gate": t.gate,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!("{}", bold("data-driven repo templates"));
    for t in TEMPLATES {
        println!(
            "  {}  {} — {}",
            cyan(&format!("{:>9}", t.key)),
            bold(t.name),
            t.desc
        );
        println!(
            "             files: {} • gate: {}",
            dim(&t.files.join(", ")),
            t.gate
        );
    }
    println!();
    println!("{}", dim("install: octocrab-cli templates install <key> OWNER/REPO --bins cli,gui"));
    Ok(())
}

/// Build [`TemplateParams`] from the live repo + CLI flags, so templates come
/// out tailored to the target repository (data-driven, not copy-paste).
async fn params_for(full_name: &str, args: &InstallArgs) -> Result<TemplateParams> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(full_name)?;
    let repo_json = octo.repos(&owner, &repo).get().await?;
    let tagline = args
        .tagline
        .clone()
        .unwrap_or_else(|| "built, packaged and shipped by the Octocrab Suite".into());
    Ok(TemplateParams {
        project: args.project.clone().unwrap_or_else(|| repo.clone()),
        owner,
        repo,
        description: args
            .desc
            .clone()
            .or_else(|| repo_json.description.clone())
            .unwrap_or_default(),
        tagline,
        bins: args
            .bins
            .as_ref()
            .map(|b| b.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default(),
        branches: vec![repo_json
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".into())],
        exclude: args
            .exclude
            .as_ref()
            .map(|b| b.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default(),
        license: args
            .license
            .clone()
            .or_else(|| {
                repo_json
                    .license
                    .as_ref()
                    .map(|l| l.spdx_id.clone())
                    .filter(|s| s != "NOASSERTION")
            })
            .unwrap_or_else(|| "MIT".into()),
        contact: args.contact.clone().unwrap_or_default(),
    })
}

pub struct InstallArgs {
    pub project: Option<String>,
    pub desc: Option<String>,
    pub tagline: Option<String>,
    pub bins: Option<String>,
    pub exclude: Option<String>,
    pub license: Option<String>,
    pub contact: Option<String>,
    pub branch: Option<String>,
    pub message: Option<String>,
}

impl Default for InstallArgs {
    fn default() -> Self {
        Self {
            project: None,
            desc: None,
            tagline: None,
            bins: None,
            exclude: None,
            license: None,
            contact: None,
            branch: None,
            message: None,
        }
    }
}

pub async fn install(key: String, full_name: String, args: InstallArgs, json: bool) -> Result<()> {
    let def = template_def(&key)
        .ok_or_else(|| anyhow::anyhow!("unknown template '{key}' — see `templates list`"))?;
    let (db, octo) = open_client()?;
    if !db.module_enabled(def.gate) {
        anyhow::bail!("installing '{key}' needs module '{}' — not granted this session", def.gate);
    }
    let (owner, repo) = split_repo(&full_name)?;
    let params = params_for(&full_name, &args).await?;
    let files = render(&key, &params)?;
    let message = args.message.unwrap_or_else(|| format!("ci: install {key} template via octocrab-cli"));
    let branch = args.branch.as_deref();

    let mut written = Vec::new();
    for (path, content) in &files {
        let url = octocrab_core::contents::put_file(
            &octo, &owner, &repo, path, content, &message, branch,
        )
        .await?;
        written.push(serde_json::json!({"path": path, "url": url}));
        println!("{} {} → {}", ok("installed"), bold(path), dim(&url));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "template": key, "repo": format!("{owner}/{repo}"), "files": written
            }))?
        );
    } else {
        println!();
        println!(
            "{} template '{}' installed into {} — {} file(s)",
            ok("✓"),
            cyan(&key),
            bold(&full_name),
            files.len()
        );
        if key == "pages" || key == "site" {
            println!(
                "{}",
                dim(&format!(
                    "next: octocrab-cli pages {full_name} enable  →  octocrab-cli pages {full_name} build"
                ))
            );
        }
        if key == "release" {
            println!(
                "{}",
                dim("tag a release (git tag v0.1.0 && git push --tags) or dispatch it from `workflows trigger`")
            );
        }
    }
    Ok(())
}
