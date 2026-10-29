//! GitHub Pages management — enable with the "workflow" build type, inspect
//! status, request builds. All calls ride the shared raw-verb helpers so the
//! token never leaves the octocrab client.

use crate::raw;
use anyhow::{bail, Result};
use octocrab::Octocrab;
use serde_json::json;
use std::sync::Arc;

/// Pages configuration (404 → not enabled).
pub async fn info(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<serde_json::Value> {
    let (status, body) = raw::raw_get(octo, &format!("/repos/{owner}/{repo}/pages")).await?;
    if status == 404 {
        bail!("GitHub Pages is not enabled for {owner}/{repo}");
    }
    if status >= 400 {
        bail!("pages info failed: HTTP {status}");
    }
    Ok(body)
}

/// Enable Pages with the modern `workflow` build type (deployed by Actions).
/// Returns the pages JSON on success.
pub async fn enable(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<serde_json::Value> {
    let (status, body) = raw::raw_send(
        octo,
        "POST",
        &format!("/repos/{owner}/{repo}/pages"),
        Some(json!({"build_type": "workflow"})),
    )
    .await?;
    match status {
        201 | 204 => Ok(body),
        409 => bail!("Pages already enabled"),
        s => bail!("enabling Pages failed: HTTP {s} — {}", summarise(&body)),
    }
}

/// Request a fresh Pages build (legacy source builds; the workflow type
/// deploys by running the pages workflow instead).
pub async fn request_build(octo: &Arc<Octocrab>, owner: &str, repo: &str) -> Result<()> {
    let (status, body) = raw::raw_send(
        octo,
        "POST",
        &format!("/repos/{owner}/{repo}/pages/builds"),
        None,
    )
    .await?;
    if status >= 400 {
        bail!("build request failed: HTTP {status} — {}", summarise(&body));
    }
    Ok(())
}

/// Latest Pages build status.
pub async fn latest_build(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
) -> Result<serde_json::Value> {
    let (status, body) =
        raw::raw_get(octo, &format!("/repos/{owner}/{repo}/pages/builds/latest")).await?;
    if status >= 400 {
        bail!("latest build lookup failed: HTTP {status}");
    }
    Ok(body)
}

fn summarise(v: &serde_json::Value) -> String {
    v.get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("no message")
        .to_string()
}
