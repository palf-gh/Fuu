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
