//! Local cache of raw message bodies (SQLite). Raw RFC 5322 bytes are stored and
//! re-parsed and re-sanitised on every display, so improvements to the sanitiser
//! apply to cached mail too. Entries are keyed by UIDVALIDITY: when a server resets
//! a folder's UIDs, old entries can never be mistaken for new messages.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use fuu_i18n::tr;

#[derive(Debug)]
pub struct CacheError(pub String);

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!("Cache operation failed: {0}", self.0))
    }
}

impl std::error::Error for CacheError {}

impl From<rusqlite::Error> for CacheError {
    fn from(e: rusqlite::Error) -> Self {
        Self(e.to_string())
    }
}

pub struct BodyCache {
    connection: Mutex<Connection>,
}

/// `<data dir>/Fuu/cache.sqlite`
pub fn default_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Fuu").join("cache.sqlite"))
}

impl BodyCache {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CacheError(e.to_string()))?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, CacheError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(connection: Connection) -> Result<Self, CacheError> {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS bodies (
                 account TEXT NOT NULL,
                 folder TEXT NOT NULL,
                 uid_validity INTEGER NOT NULL,
                 uid INTEGER NOT NULL,
                 raw BLOB NOT NULL,
                 fetched_at INTEGER NOT NULL,
                 PRIMARY KEY (account, folder, uid_validity, uid)
             );",
        )?;
        Ok(Self { connection: Mutex::new(connection) })
    }

    pub fn get(&self, account: &str, folder: &str, uid_validity: u32, uid: u32) -> Result<Option<Vec<u8>>, CacheError> {
        let c = self.connection.lock().map_err(|e| CacheError(e.to_string()))?;
        Ok(c.query_row(
            "SELECT raw FROM bodies WHERE account = ?1 AND folder = ?2 AND uid_validity = ?3 AND uid = ?4",
            params![account, folder, uid_validity, uid],
            |row| row.get(0),
        )
        .optional()?)
    }

    pub fn put(&self, account: &str, folder: &str, uid_validity: u32, uid: u32, raw: &[u8]) -> Result<(), CacheError> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default();
        let c = self.connection.lock().map_err(|e| CacheError(e.to_string()))?;
        c.execute(
            "INSERT OR REPLACE INTO bodies (account, folder, uid_validity, uid, raw, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account, folder, uid_validity, uid, raw, now],
        )?;
        Ok(())
    }

    /// Forget one message (moved or deleted).
    pub fn remove(&self, account: &str, folder: &str, uid: u32) -> Result<(), CacheError> {
        let c = self.connection.lock().map_err(|e| CacheError(e.to_string()))?;
        c.execute("DELETE FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3", params![account, folder, uid])?;
        Ok(())
    }

    /// Drop entries of a folder whose UIDVALIDITY changed.
    pub fn retain_validity(&self, account: &str, folder: &str, uid_validity: u32) -> Result<(), CacheError> {
        let c = self.connection.lock().map_err(|e| CacheError(e.to_string()))?;
        c.execute(
            "DELETE FROM bodies WHERE account = ?1 AND folder = ?2 AND uid_validity <> ?3",
            params![account, folder, uid_validity],
        )?;
        Ok(())
    }

    /// Remove everything cached for an account (account removal).
    pub fn purge_account(&self, account: &str) -> Result<(), CacheError> {
        let c = self.connection.lock().map_err(|e| CacheError(e.to_string()))?;
        c.execute("DELETE FROM bodies WHERE account = ?1", params![account])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_invalidates() {
        let cache = BodyCache::in_memory().unwrap();
        assert_eq!(cache.get("a", "INBOX", 1, 10).unwrap(), None);
        cache.put("a", "INBOX", 1, 10, b"raw").unwrap();
        cache.put("a", "INBOX", 1, 11, b"raw2").unwrap();
        assert_eq!(cache.get("a", "INBOX", 1, 10).unwrap().as_deref(), Some(&b"raw"[..]));
        assert_eq!(cache.get("a", "INBOX", 2, 10).unwrap(), None, "other UIDVALIDITY never matches");
        cache.retain_validity("a", "INBOX", 2).unwrap();
        assert_eq!(cache.get("a", "INBOX", 1, 11).unwrap(), None);
        cache.put("a", "INBOX", 2, 1, b"x").unwrap();
        cache.remove("a", "INBOX", 1).unwrap();
        assert_eq!(cache.get("a", "INBOX", 2, 1).unwrap(), None);
        cache.put("a", "Sent", 1, 1, b"y").unwrap();
        cache.purge_account("a").unwrap();
        assert_eq!(cache.get("a", "Sent", 1, 1).unwrap(), None);
    }

    #[test]
    fn persists_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fuu").join("cache.sqlite");
        BodyCache::open(&path).unwrap().put("a", "INBOX", 7, 3, "日本語".as_bytes()).unwrap();
        assert_eq!(BodyCache::open(&path).unwrap().get("a", "INBOX", 7, 3).unwrap().as_deref(), Some("日本語".as_bytes()));
    }
}
