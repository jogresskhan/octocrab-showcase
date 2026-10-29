//! Contents write helper — create or update a single file via the Contents
//! API (sha-probed so re-installs update in place). Used by the CLI's
//! `repos upload`, the template installer and the site generator.

use anyhow::Result;
use octocrab::Octocrab;
use std::sync::Arc;

/// Create or update `path` in `owner/repo` with `content`.
/// Returns the file's html_url.
pub async fn put_file(
    octo: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    path: &str,
    content: &str,
    message: &str,
    branch: Option<&str>,
) -> Result<String> {
    let rh = octo.repos(owner, repo);

    let probe = rh.get_content().path(path);
    let probe = match branch {
        Some(b) => probe.r#ref(b),
        None => probe,
    };
    let existing_sha = probe
        .send()
        .await
        .ok()
        .and_then(|c| c.items.first().map(|i| i.sha.clone()));

    let url = if let Some(sha) = existing_sha {
        let mut builder = rh.update_file(path, message, content, &sha);
        if let Some(b) = branch {
            builder = builder.branch(b);
        }
        builder.send().await?.content.html_url
    } else {
        let mut builder = rh.create_file(path, message, content);
        if let Some(b) = branch {
            builder = builder.branch(b);
        }
        builder.send().await?.content.html_url
    };

    Ok(url.map(|u| u.to_string()).unwrap_or_default())
}
