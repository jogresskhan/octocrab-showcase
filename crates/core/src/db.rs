//! SQLite database — the single source of truth shared by all three binaries.
//!
//! Lives in the Tauri-style app-data directory. Tables:
//! * `tokens`       — AES-GCM encrypted GitHub tokens (never plaintext!)
//! * `modules`      — capability modules granted to the active token
//! * `accounts`     — cached `/user` payload
//! * `orgs`         — organization access map (read/write/admin per org)
//! * `cache`        — TTL cache for data-driven UIs (trees, files, api json)
//! * `meta`         — key/value state (auth summary, last check, …)
//! * `events`       — audit trail
//! * `achievements` — log of achievement-oriented account actions

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenMeta {
    pub id: i64,
    pub label: String,
    pub kind: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleState {
    pub key: String,
    pub enabled: bool,
    /// granted | denied | unknown
    pub state: String,
    /// scope:repo | probe:contents-write | inherited …
    pub source: String,
    pub evidence: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrgRow {
    pub login: String,
    pub role: String,
    pub read: bool,
    pub write: bool,
    pub admin: bool,
}

pub struct SuiteDb {
    conn: Connection,
    path: PathBuf,
}

impl SuiteDb {
    // ---------------------------------------------------------------- open

    pub fn open() -> Result<Self> {
        let path = crate::db_path();
        Self::open_at(&path)
    }

    pub fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("opening db at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        let db = Self {
            conn,
            path: path.to_path_buf(),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key        TEXT PRIMARY KEY,
                value      TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tokens (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                label        TEXT NOT NULL UNIQUE,
                kind         TEXT NOT NULL DEFAULT 'unknown',
                token_enc    BLOB NOT NULL,
                created_at   INTEGER NOT NULL,
                last_used_at INTEGER,
                active       INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS modules (
                key        TEXT PRIMARY KEY,
                enabled    INTEGER NOT NULL DEFAULT 0,
                state      TEXT NOT NULL DEFAULT 'unknown',
                source     TEXT NOT NULL DEFAULT '',
                evidence   TEXT NOT NULL DEFAULT '',
                checked_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS accounts (
                login      TEXT PRIMARY KEY,
                json       TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS orgs (
                login      TEXT PRIMARY KEY,
                role       TEXT NOT NULL DEFAULT '',
                read       INTEGER NOT NULL DEFAULT 0,
                write      INTEGER NOT NULL DEFAULT 0,
                admin      INTEGER NOT NULL DEFAULT 0,
                json       TEXT NOT NULL DEFAULT '{}',
                checked_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS cache (
                key        TEXT PRIMARY KEY,
                value      TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id     INTEGER PRIMARY KEY AUTOINCREMENT,
                ts     INTEGER NOT NULL,
                kind   TEXT NOT NULL,
                detail TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS achievements (
                id     INTEGER PRIMARY KEY AUTOINCREMENT,
                ts     INTEGER NOT NULL,
                action TEXT NOT NULL,
                detail TEXT NOT NULL DEFAULT '',
                ok     INTEGER NOT NULL DEFAULT 1
            );
            "#,
        )?;
        Ok(())
    }

    // --------------------------------------------------------------- meta

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value, updated_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=?2, updated_at=?3",
            params![key, value, crate::now()],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .ok()
            .flatten()
    }

    // -------------------------------------------------------------- tokens

    /// Store a token (encrypted). The new token becomes the active one.
    pub fn set_token(&self, label: &str, token: &str) -> Result<()> {
        let enc = crate::crypto::encrypt(token)?;
        self.conn.execute(
            "INSERT INTO tokens(label, kind, token_enc, created_at, active)
             VALUES(?1, 'unknown', ?2, ?3, 1)
             ON CONFLICT(label) DO UPDATE SET token_enc=?2, active=1, created_at=?3",
            params![label, enc, crate::now()],
        )?;
        self.conn.execute(
            "UPDATE tokens SET active = CASE WHEN label = ?1 THEN 1 ELSE 0 END",
            params![label],
        )?;
        self.log_event("auth", &format!("token '{label}' stored (encrypted)"));
        Ok(())
    }

    /// Decrypt and return the active token.
    pub fn active_token(&self) -> Result<Option<String>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT token_enc FROM tokens WHERE active = 1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match blob {
            Some(b) => Ok(Some(crate::crypto::decrypt(&b)?)),
            None => Ok(None),
        }
    }

    /// Active token label.
    pub fn active_label(&self) -> Option<String> {
        self.conn
            .query_row(
                "SELECT label FROM tokens WHERE active = 1 LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn touch_token(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE tokens SET last_used_at = ?1 WHERE active = 1",
            params![crate::now()],
        )?;
        Ok(())
    }

    pub fn set_token_kind(&self, kind: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE tokens SET kind = ?1 WHERE active = 1",
            params![kind],
        )?;
        Ok(())
    }

    pub fn list_tokens(&self) -> Vec<TokenMeta> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, label, kind, created_at, last_used_at, active FROM tokens ORDER BY id",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok(TokenMeta {
                id: r.get(0)?,
                label: r.get(1)?,
                kind: r.get(2)?,
                created_at: r.get(3)?,
                last_used_at: r.get(4)?,
                active: r.get::<_, i64>(5)? == 1,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn clear_token(&self) -> Result<()> {
        self.conn.execute("DELETE FROM tokens", [])?;
        self.conn.execute("DELETE FROM modules", [])?;
        self.set_meta("auth_state", "cleared")?;
        self.log_event("auth", "all tokens cleared");
        Ok(())
    }

    // ------------------------------------------------------------- modules

    pub fn clear_modules(&self) -> Result<()> {
        self.conn.execute("DELETE FROM modules", [])?;
        Ok(())
    }

    pub fn save_modules(&self, mods: &[ModuleState]) -> Result<()> {
        let ts = crate::now();
        for m in mods {
            self.conn.execute(
                "INSERT INTO modules(key, enabled, state, source, evidence, checked_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(key) DO UPDATE SET enabled=?2, state=?3, source=?4, evidence=?5, checked_at=?6",
                params![
                    m.key,
                    m.enabled as i64,
                    m.state,
                    m.source,
                    m.evidence,
                    ts
                ],
            )?;
        }
        Ok(())
    }

    pub fn modules(&self) -> Vec<ModuleState> {
        let mut stmt = match self
            .conn
            .prepare("SELECT key, enabled, state, source, evidence FROM modules ORDER BY key")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok(ModuleState {
                key: r.get(0)?,
                enabled: r.get::<_, i64>(1)? == 1,
                state: r.get(2)?,
                source: r.get(3)?,
                evidence: r.get(4)?,
            })
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// Is a module unlocked? (`granted` state)
    pub fn module_enabled(&self, key: &str) -> bool {
        self.conn
            .query_row(
                "SELECT state FROM modules WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .map(|s| s == "granted")
            .unwrap_or(false)
    }

    // ------------------------------------------------------------- account

    pub fn save_account(&self, login: &str, json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accounts(login, json, fetched_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(login) DO UPDATE SET json=?2, fetched_at=?3",
            params![login, json, crate::now()],
        )?;
        Ok(())
    }

    pub fn account_json(&self, login: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT json FROM accounts WHERE login = ?1",
                params![login],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    // ---------------------------------------------------------------- orgs

    pub fn replace_orgs(&self, orgs: &[OrgRow], json_by_login: &[(String, String)]) -> Result<()> {
        self.conn.execute("DELETE FROM orgs", [])?;
        let ts = crate::now();
        for o in orgs {
            let json = json_by_login
                .iter()
                .find(|(l, _)| *l == o.login)
                .map(|(_, j)| j.clone())
                .unwrap_or_else(|| "{}".into());
            self.conn.execute(
                "INSERT INTO orgs(login, role, read, write, admin, json, checked_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    o.login,
                    o.role,
                    o.read as i64,
                    o.write as i64,
                    o.admin as i64,
                    json,
                    ts
                ],
            )?;
        }
        Ok(())
    }

    pub fn orgs(&self) -> Vec<(String, String, bool, bool, bool)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT login, role, read, write, admin FROM orgs ORDER BY login")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? == 1,
                r.get::<_, i64>(3)? == 1,
                r.get::<_, i64>(4)? == 1,
            ))
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    // --------------------------------------------------------------- cache

    pub fn cache_get(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM cache WHERE key = ?1 AND expires_at > ?2",
                params![key, crate::now()],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn cache_set(&self, key: &str, value: &str, ttl_secs: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cache(key, value, expires_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=?2, expires_at=?3",
            params![key, value, crate::now() + ttl_secs],
        )?;
        Ok(())
    }

    pub fn cache_clear_prefix(&self, prefix: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM cache WHERE key LIKE ?1 || '%'",
            params![prefix],
        )?;
        Ok(())
    }

    // ------------------------------------------------------- events & boosts

    pub fn log_event(&self, kind: &str, detail: &str) {
        let _ = self.conn.execute(
            "INSERT INTO events(ts, kind, detail) VALUES(?1, ?2, ?3)",
            params![crate::now(), kind, detail],
        );
    }

    pub fn events(&self, limit: usize) -> Vec<(i64, String, String)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT ts, kind, detail FROM events ORDER BY id DESC LIMIT ?1")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn add_achievement(&self, action: &str, detail: &str, ok: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO achievements(ts, action, detail, ok) VALUES(?1, ?2, ?3, ?4)",
            params![crate::now(), action, detail, ok as i64],
        )?;
        Ok(())
    }

    pub fn achievements(&self) -> Vec<(i64, String, String, bool)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT ts, action, detail, ok FROM achievements ORDER BY id")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? == 1,
            ))
        });
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// Human summary of everything stored (never includes the token itself).
    pub fn info_summary(&self) -> Result<String> {
        let tokens = self.list_tokens();
        let modules = self.modules();
        let granted = modules.iter().filter(|m| m.enabled).count();
        let active = tokens.iter().find(|t| t.active);
        Ok(format!(
            "db: {}\ntokens: {} (active: {})\nmodules granted: {}/{}",
            self.path.display(),
            tokens.len(),
            active
                .map(|t| format!("'{}' [{}]", t.label, t.kind))
                .unwrap_or_else(|| "none".into()),
            granted,
            modules.len()
        ))
    }

    pub fn require_active_token(&self) -> Result<String> {
        self.active_token()?
            .ok_or_else(|| anyhow!("no active token stored — run: octocrab-cli auth set"))
    }
}
