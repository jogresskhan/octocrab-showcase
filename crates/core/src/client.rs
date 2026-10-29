//! Octocrab client builder — the *only* way any binary in the suite talks to
//! GitHub. The token always comes from the encrypted database vault; there is
//! no CLI flag, no environment variable and no `curl` involved.

use crate::db::SuiteDb;
use anyhow::{Context, Result};
use octocrab::Octocrab;
use std::sync::{Arc, OnceLock};

/// Process-wide fallback runtime. octocrab's client construction spawns
/// `tower::buffer` workers, which require a Tokio context; binaries that call
/// us from a non-async thread (the egui UI thread) get one from here.
fn fallback_runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build octocrab-core fallback runtime")
    })
}

/// Build an authenticated Octocrab client from a raw token string.
pub fn build(token: &str) -> Result<Arc<Octocrab>> {
    // Ensure a Tokio context exists for octocrab's background workers.
    let _guard = if tokio::runtime::Handle::try_current().is_ok() {
        None
    } else {
        Some(fallback_runtime().enter())
    };
    let octo = Octocrab::builder()
        .user_access_token(token.to_string())
        .build()
        .context("building octocrab client")?;
    Ok(Arc::new(octo))
}

/// Build a client from the active token in the database.
/// Returns `(client, token)`.
pub fn from_db(db: &SuiteDb) -> Result<(Arc<Octocrab>, String)> {
    let token = db.require_active_token()?;
    db.touch_token()?;
    let client = build(&token)?;
    Ok((client, token))
}

/// Build an unauthenticated client (rate-limited, public data only).
pub fn anonymous() -> Result<Arc<Octocrab>> {
    let _guard = if tokio::runtime::Handle::try_current().is_ok() {
        None
    } else {
        Some(fallback_runtime().enter())
    };
    let octo = Octocrab::builder().build().context("building octocrab")?;
    Ok(Arc::new(octo))
}

/// Convenience: authenticated client + the parsed `/user` payload.
pub async fn me(octo: &Arc<Octocrab>) -> Result<octocrab::models::Author> {
    let user = octo.current().user().await.context("GET /user failed")?;
    Ok(user)
}
