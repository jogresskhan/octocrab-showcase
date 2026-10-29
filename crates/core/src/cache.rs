//! TTL cache used by the data-driven UIs (IDE file tree, workflow lists, …).
//!
//! Keys are namespaced like `repo:{owner}/{repo}:tree:{branch}`.

use crate::db::SuiteDb;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub const TTL_SHORT: i64 = 30;
pub const TTL_MEDIUM: i64 = 300;
pub const TTL_LONG: i64 = 3600;

pub fn get_json<T: DeserializeOwned>(db: &SuiteDb, key: &str) -> Option<T> {
    let raw = db.cache_get(key)?;
    serde_json::from_str(&raw).ok()
}

pub fn set_json<T: Serialize>(db: &SuiteDb, key: &str, value: &T, ttl: i64) {
    if let Ok(raw) = serde_json::to_string(value) {
        let _ = db.cache_set(key, &raw, ttl);
    }
}

/// Invalidate every cache entry starting with `prefix`.
pub fn invalidate(db: &SuiteDb, prefix: &str) {
    let _ = db.cache_clear_prefix(prefix);
}
