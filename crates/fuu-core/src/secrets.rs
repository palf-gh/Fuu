//! Secret storage. Passwords live only in the OS keychain (Windows Credential
//! Manager, macOS Keychain); never in files or logs.

use std::collections::HashMap;
use std::sync::Mutex;

/// Service name of every Fuu keychain entry.
pub const SERVICE: &str = "Fuu";

#[derive(Debug)]
pub enum SecretError {
    /// The platform store could not be initialised.
    Unavailable(String),
    NotFound,
    Store(String),
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "OS のキーチェーンを利用できません: {e}"),
            Self::NotFound => f.write_str("パスワードが保存されていません"),
            Self::Store(e) => write!(f, "キーチェーンの操作に失敗しました: {e}"),
        }
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    fn set(&self, account_id: &str, secret: &str) -> Result<(), SecretError>;
    fn get(&self, account_id: &str) -> Result<String, SecretError>;
    fn delete(&self, account_id: &str) -> Result<(), SecretError>;
}

/// The OS keychain. keyring v4 selects the native store and reports an error
/// (rather than silently using an in-memory mock) when none is available.
#[derive(Debug, Default)]
pub struct KeychainStore;

fn entry(account_id: &str) -> Result<keyring::Entry, SecretError> {
    keyring::Entry::new(SERVICE, account_id).map_err(|e| match e {
        keyring::Error::NoDefaultStore => SecretError::Unavailable(e.to_string()),
        other => SecretError::Store(other.to_string()),
    })
}

impl SecretStore for KeychainStore {
    fn set(&self, account_id: &str, secret: &str) -> Result<(), SecretError> {
        entry(account_id)?.set_password(secret).map_err(|e| SecretError::Store(e.to_string()))
    }

    fn get(&self, account_id: &str) -> Result<String, SecretError> {
        entry(account_id)?.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound,
            other => SecretError::Store(other.to_string()),
        })
    }

    fn delete(&self, account_id: &str) -> Result<(), SecretError> {
        match entry(account_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }
}

/// Largest value kept in one keychain entry. Windows Credential Manager caps a
/// credential blob at 2560 bytes and may store text as UTF-16, so long values
/// (OAuth refresh tokens) are split across `id`, `id#1`, `id#2`, ….
const CHUNK: usize = 1000;
const CHUNK_MARK: &str = "fuu-chunks:";

fn chunks(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value;
    while rest.len() > CHUNK {
        let mut cut = CHUNK;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    out.push(rest);
    out
}

/// Store a value of any length (see [`CHUNK`]).
pub fn set_long(store: &dyn SecretStore, id: &str, value: &str) -> Result<(), SecretError> {
    delete_long(store, id)?;
    let parts = chunks(value);
    if parts.len() == 1 && !value.starts_with(CHUNK_MARK) {
        return store.set(id, value);
    }
    for (i, part) in parts.iter().enumerate().skip(1) {
        store.set(&format!("{id}#{i}"), part)?;
    }
    store.set(id, &format!("{CHUNK_MARK}{}:{}", parts.len(), parts[0]))
}

/// Read a value stored with [`set_long`] (or a plain single entry).
pub fn get_long(store: &dyn SecretStore, id: &str) -> Result<String, SecretError> {
    let first = store.get(id)?;
    let Some(rest) = first.strip_prefix(CHUNK_MARK) else { return Ok(first) };
    let (count, head) = rest.split_once(':').ok_or_else(|| SecretError::Store("壊れたエントリです".into()))?;
    let count: usize = count.parse().map_err(|_| SecretError::Store("壊れたエントリです".into()))?;
    let mut value = head.to_owned();
    for i in 1..count {
        value.push_str(&store.get(&format!("{id}#{i}"))?);
    }
    Ok(value)
}

/// Delete a value and any chunks it was split into.
pub fn delete_long(store: &dyn SecretStore, id: &str) -> Result<(), SecretError> {
    if let Ok(first) = store.get(id) {
        if let Some((count, _)) = first.strip_prefix(CHUNK_MARK).and_then(|r| r.split_once(':')) {
            for i in 1..count.parse::<usize>().unwrap_or(0) {
                store.delete(&format!("{id}#{i}"))?;
            }
        }
    }
    store.delete(id)
}

/// In-memory store for tests.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn set(&self, account_id: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account_id.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, account_id: &str) -> Result<String, SecretError> {
        self.0.lock().unwrap().get(account_id).cloned().ok_or(SecretError::NotFound)
    }

    fn delete(&self, account_id: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(account_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert!(matches!(s.get("a"), Err(SecretError::NotFound)));
        s.set("a", "pw").unwrap();
        assert_eq!(s.get("a").unwrap(), "pw");
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert!(matches!(s.get("a"), Err(SecretError::NotFound)));
    }

    #[test]
    fn long_values_are_chunked() {
        let s = MemoryStore::default();
        let long: String = "トークン-abc".repeat(400);
        set_long(&s, "acc", &long).unwrap();
        assert!(s.get("acc").unwrap().starts_with(CHUNK_MARK));
        assert!(s.get("acc#1").is_ok());
        assert_eq!(get_long(&s, "acc").unwrap(), long);
        set_long(&s, "acc", "short").unwrap();
        assert!(matches!(s.get("acc#1"), Err(SecretError::NotFound)), "old chunks removed");
        assert_eq!(get_long(&s, "acc").unwrap(), "short");
        delete_long(&s, "acc").unwrap();
        assert!(matches!(get_long(&s, "acc"), Err(SecretError::NotFound)));
    }

    /// A refresh-token-sized value through the real keychain.
    #[test]
    #[ignore]
    fn keychain_long_round_trip() {
        let s = KeychainStore;
        let id = format!("fuu-test-long-{}", std::process::id());
        let value: String = (0..2400).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        set_long(&s, &id, &value).expect("chunked store");
        assert_eq!(get_long(&s, &id).unwrap(), value);
        delete_long(&s, &id).unwrap();
        assert!(matches!(s.get(&format!("{id}#1")), Err(SecretError::NotFound)));
    }

    /// Real OS keychain round trip. Ignored by default because it touches the
    /// user's credential store; run with `cargo test -p fuu-core -- --ignored keychain`.
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let s = KeychainStore;
        let id = format!("fuu-test-{}", std::process::id());
        s.set(&id, "テスト用パスワード").expect("store must persist (not a mock)");
        assert_eq!(s.get(&id).unwrap(), "テスト用パスワード");
        s.delete(&id).unwrap();
        assert!(matches!(s.get(&id), Err(SecretError::NotFound)));
    }
}
