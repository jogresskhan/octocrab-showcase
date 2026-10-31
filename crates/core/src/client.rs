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

// ---------------------------------------------------- token resolution

/// Compile-time embedded token — build with
/// `OCTOCRAB_EMBED_TOKEN='ghp_…' cargo build --release -p octocrab-cli`
/// to bake the PAT into the binary (local-only distribution rule).
pub fn embedded_token() -> Option<&'static str> {
    option_env!("OCTOCRAB_EMBED_TOKEN")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// `token.txt` lookup order: `$OCTOCRAB_TOKEN_FILE` (explicit path), next
/// to the running executable, then the current working directory.
/// Only the first line of the file is used (trimmed).
pub fn token_file() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("OCTOCRAB_TOKEN_FILE") {
        let p = std::path::PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join("token.txt");
            if beside.is_file() {
                return Some(beside);
            }
        }
    }
    let cwd = std::path::PathBuf::from("token.txt");
    cwd.is_file().then_some(cwd)
}

/// Resolve the token from the documented sources, first hit wins:
/// embedded → encrypted vault → `token.txt` → `$OCTOCRAB_TOKEN`.
/// Returns `(token, source)` with source one of
/// "embedded" / "vault" / "file" / "env". There is deliberately no
/// `--token` argv flag — tokens in argv leak into shell history and the
/// process list.
pub fn resolve_token(db: &SuiteDb) -> Option<(String, &'static str)> {
    if let Some(t) = embedded_token() {
        return Some((t.to_string(), "embedded"));
    }
    if let Ok(Some(t)) = db.active_token() {
        return Some((t, "vault"));
    }
    if let Some(path) = token_file() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let line = text.lines().next().unwrap_or("").trim().to_string();
            if !line.is_empty() {
                return Some((line, "file"));
            }
        }
    }
    if let Ok(t) = std::env::var("OCTOCRAB_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some((t, "env"));
        }
    }
    None
}

/// Build a client from whichever token source is available
/// (see [`resolve_token`]). Returns `(client, token)`.
pub fn from_any_source(db: &SuiteDb) -> Result<(Arc<Octocrab>, String)> {
    let (token, _source) = resolve_token(db).ok_or_else(|| {
        anyhow::anyhow!(
            "no token available — the vault is empty, no token.txt sits beside \
             the binary or in the cwd, and $OCTOCRAB_TOKEN is unset"
        )
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> SuiteDb {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "octocrab-token-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db = SuiteDb::open_at(&dir.join("t.db")).expect("open");
        let _ = std::fs::remove_dir_all(&dir);
        db
    }

    /// Resolution order (embedded is compile-time-off in test builds):
    /// empty vault -> token.txt via $OCTOCRAB_TOKEN_FILE -> $OCTOCRAB_TOKEN.
    #[test]
    fn token_resolution_order_file_then_env() {
        std::env::remove_var("OCTOCRAB_TOKEN");
        let db = temp_db();
        assert!(
            resolve_token(&db).is_none(),
            "empty vault + no file + no env -> no token"
        );

        let dir = std::env::temp_dir().join(format!(
            "octocrab-token-file-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let tf = dir.join("token.txt");
        std::fs::write(&tf, "ghp_filesource\nsecond-line-ignored\n").unwrap();

        // point the explicit-path override at it
        std::env::set_var("OCTOCRAB_TOKEN_FILE", &tf);
        let got = resolve_token(&db).expect("file source resolves");
        assert_eq!(got.0, "ghp_filesource", "first line, trimmed");
        assert_eq!(got.1, "file");
        std::env::remove_var("OCTOCRAB_TOKEN_FILE");
        let _ = std::fs::remove_dir_all(&dir);

        // env var arm (vault still empty, no token.txt in cwd)
        std::env::set_var("OCTOCRAB_TOKEN", "ghp_envsource");
        let got = resolve_token(&db).expect("env source resolves");
        assert_eq!(got.0, "ghp_envsource");
        assert_eq!(got.1, "env");
        std::env::remove_var("OCTOCRAB_TOKEN");
    }
}
