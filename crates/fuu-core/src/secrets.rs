//! Secret storage. Passwords live only in the OS keychain (Windows Credential
//! Manager, macOS Keychain); never in files or logs.

use std::collections::HashMap;
use std::sync::Mutex;
use fuu_i18n::tr;

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
            Self::Unavailable(e) => f.write_str(&tr!("The system keychain is not available: {0}", e)),
            Self::NotFound => f.write_str(tr!("No password is saved")),
            Self::Store(e) => f.write_str(&tr!("Keychain operation failed: {0}", e)),
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
/// (OAuth refresh tokens) are split: the entry `id` holds a header and the first
/// part, the rest go to `id#<generation>.<n>`.
const CHUNK: usize = 1000;
/// Header of a split value: `fuu-chunks2:<generation>:<count>:<first part>`.
const CHUNK_MARK: &str = "fuu-chunks2:";
/// Older header (`fuu-chunks:<count>:<first part>`, parts in `id#<n>`); still read.
const LEGACY_MARK: &str = "fuu-chunks:";

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

/// How a stored entry is laid out: the names of the parts after the header,
/// and the first part. None for a plain (unsplit) value.
fn layout<'a>(id: &str, first: &'a str) -> Option<(Vec<String>, &'a str)> {
    if let Some(rest) = first.strip_prefix(CHUNK_MARK) {
        let mut fields = rest.splitn(3, ':');
        let (generation, count, head) = (fields.next()?, fields.next()?.parse::<usize>().ok()?, fields.next()?);
        return Some(((1..count).map(|i| format!("{id}#{generation}.{i}")).collect(), head));
    }
    let rest = first.strip_prefix(LEGACY_MARK)?;
    let (count, head) = rest.split_once(':')?;
    Some(((1..count.parse::<usize>().ok()?).map(|i| format!("{id}#{i}")).collect(), head))
}

/// Store a value of any length (see [`CHUNK`]). The new value is written in
/// full before the old one is touched: if the keychain refuses part of it, the
/// previous value is still there (a lost refresh token means signing in again).
pub fn set_long(store: &dyn SecretStore, id: &str, value: &str) -> Result<(), SecretError> {
    let old_parts = store.get(id).ok().and_then(|first| layout(id, &first).map(|(names, _)| names)).unwrap_or_default();
    let parts = chunks(value);
    if parts.len() == 1 && !value.starts_with(LEGACY_MARK) {
        store.set(id, value)?;
    } else {
        let generation = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("{:x}", d.as_nanos()))
            .unwrap_or_else(|_| "0".into());
        let mut written = Vec::new();
        let mut result = Ok(());
        for (i, part) in parts.iter().enumerate().skip(1) {
            let name = format!("{id}#{generation}.{i}");
            result = store.set(&name, part);
            if result.is_err() {
                break;
            }
            written.push(name);
        }
        if result.is_ok() {
            result = store.set(id, &format!("{CHUNK_MARK}{generation}:{}:{}", parts.len(), parts[0]));
        }
        if let Err(e) = result {
            for name in written {
                let _ = store.delete(&name);
            }
            return Err(e);
        }
    }
    // Only now drop the previous value's parts.
    for name in old_parts {
        let _ = store.delete(&name);
    }
    Ok(())
}

/// Read a value stored with [`set_long`] (or a plain single entry).
pub fn get_long(store: &dyn SecretStore, id: &str) -> Result<String, SecretError> {
    let first = store.get(id)?;
    if !first.starts_with(LEGACY_MARK) && !first.starts_with(CHUNK_MARK) {
        return Ok(first);
    }
    let (names, head) = layout(id, &first).ok_or_else(|| SecretError::Store("broken entry".into()))?;
    let mut value = head.to_owned();
    for name in names {
        value.push_str(&store.get(&name)?);
    }
    Ok(value)
}

/// Delete a value and any parts it was split into.
pub fn delete_long(store: &dyn SecretStore, id: &str) -> Result<(), SecretError> {
    if let Ok(first) = store.get(id) {
        for name in layout(id, &first).map(|(names, _)| names).unwrap_or_default() {
            store.delete(&name)?;
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

    fn keys(s: &MemoryStore) -> Vec<String> {
        let mut k: Vec<String> = s.0.lock().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn long_values_are_chunked() {
        let s = MemoryStore::default();
        let long: String = "トークン-abc".repeat(400);
        set_long(&s, "acc", &long).unwrap();
        assert!(s.get("acc").unwrap().starts_with(CHUNK_MARK));
        assert!(keys(&s).len() > 2);
        assert_eq!(get_long(&s, "acc").unwrap(), long);
        set_long(&s, "acc", "short").unwrap();
        assert_eq!(keys(&s), ["acc"], "old parts removed");
        assert_eq!(get_long(&s, "acc").unwrap(), "short");
        set_long(&s, "acc", &long).unwrap();
        set_long(&s, "acc", &format!("{long}-rotated")).unwrap();
        assert_eq!(get_long(&s, "acc").unwrap(), format!("{long}-rotated"));
        delete_long(&s, "acc").unwrap();
        assert!(keys(&s).is_empty());
        assert!(matches!(get_long(&s, "acc"), Err(SecretError::NotFound)));
    }

    /// Refuses every `set` once its allowance runs out.
    struct Flaky(MemoryStore, std::sync::atomic::AtomicUsize);

    impl SecretStore for Flaky {
        fn set(&self, id: &str, secret: &str) -> Result<(), SecretError> {
            use std::sync::atomic::Ordering::SeqCst;
            if self.1.load(SeqCst) == 0 {
                return Err(SecretError::Store("refused".into()));
            }
            self.1.fetch_sub(1, SeqCst);
            self.0.set(id, secret)
        }
        fn get(&self, id: &str) -> Result<String, SecretError> {
            self.0.get(id)
        }
        fn delete(&self, id: &str) -> Result<(), SecretError> {
            self.0.delete(id)
        }
    }

    #[test]
    fn a_refused_write_keeps_the_previous_value() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let old: String = "old-token-".repeat(300);
        let new: String = "new-token-".repeat(300);
        // The new value takes three writes (two parts, then the header).
        for allowed in 0..3 {
            let s = Flaky(MemoryStore::default(), AtomicUsize::new(usize::MAX));
            set_long(&s, "acc", &old).unwrap();
            let stored = keys(&s.0);
            s.1.store(allowed, SeqCst);
            assert!(set_long(&s, "acc", &new).is_err(), "allowed {allowed}");
            assert_eq!(get_long(&s, "acc").unwrap(), old, "allowed {allowed}");
            assert_eq!(keys(&s.0), stored, "no stray parts (allowed {allowed})");
        }
        // A short password: a refused write leaves the old one too.
        let s = Flaky(MemoryStore::default(), AtomicUsize::new(usize::MAX));
        set_long(&s, "pw", "old").unwrap();
        s.1.store(0, SeqCst);
        assert!(set_long(&s, "pw", "new").is_err());
        assert_eq!(get_long(&s, "pw").unwrap(), "old");
    }

    #[test]
    fn legacy_split_values_are_still_read_and_replaced() {
        let s = MemoryStore::default();
        s.set("acc", "fuu-chunks:3:aa").unwrap();
        s.set("acc#1", "bb").unwrap();
        s.set("acc#2", "cc").unwrap();
        assert_eq!(get_long(&s, "acc").unwrap(), "aabbcc");
        set_long(&s, "acc", "fresh").unwrap();
        assert_eq!(keys(&s), ["acc"]);
        assert_eq!(get_long(&s, "acc").unwrap(), "fresh");
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
