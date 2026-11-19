//! `accounts.toml` in the user configuration directory. Never contains secrets.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::account::Account;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountsFile {
    #[serde(default, rename = "account")]
    pub accounts: Vec<Account>,
}

#[derive(Debug)]
pub enum ConfigError {
    NoConfigDir,
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoConfigDir => f.write_str("設定フォルダーが見つかりません"),
            Self::Io(e) => write!(f, "設定ファイルの読み書きに失敗しました: {e}"),
            Self::Parse(e) => write!(f, "設定ファイルを解釈できません: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// `<config dir>/Fuu/accounts.toml`
pub fn default_path() -> Result<PathBuf, ConfigError> {
    Ok(dirs::config_dir().ok_or(ConfigError::NoConfigDir)?.join("Fuu").join("accounts.toml"))
}

pub fn load(path: &Path) -> Result<AccountsFile, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AccountsFile::default()),
        Err(e) => Err(ConfigError::Io(e)),
    }
}

/// Write atomically (temporary file, then rename) so a crash never leaves a
/// half-written configuration.
pub fn save(path: &Path, file: &AccountsFile) -> Result<(), ConfigError> {
    let text = toml::to_string_pretty(file).map_err(|e| ConfigError::Parse(e.to_string()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(ConfigError::Io)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(ConfigError::Io)?;
    std::fs::rename(&tmp, path).map_err(ConfigError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{AuthKind, Security, ServerConfig};

    fn sample() -> Account {
        Account {
            id: "taro-1".into(),
            display_name: "山田 太郎".into(),
            email: "taro@example.jp".into(),
            username: "taro@example.jp".into(),
            imap: ServerConfig { host: "imap.example.jp".into(), port: 993, security: Security::Tls },
            smtp: ServerConfig { host: "smtp.example.jp".into(), port: 587, security: Security::StartTls },
            auth: AuthKind::Password,
            oauth: None,
            signature: String::new(),
            signature_enabled: true,
        }
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("none.toml")).unwrap(), AccountsFile::default());
    }

    #[test]
    fn round_trip_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fuu").join("accounts.toml");
        let file = AccountsFile { accounts: vec![sample()] };
        save(&path, &file).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[[account]]"));
        assert!(!text.to_lowercase().contains("password ="), "no secrets in the file");
        assert_eq!(load(&path).unwrap(), file);
        assert!(!path.with_extension("toml.tmp").exists());
    }

    #[test]
    fn signatures_round_trip_and_old_files_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.toml");
        let mut account = sample();
        account.signature = "山田 太郎\n株式会社サンプル".into();
        account.signature_enabled = false;
        let file = AccountsFile { accounts: vec![account] };
        save(&path, &file).unwrap();
        assert_eq!(load(&path).unwrap(), file);
        // Files written before signatures existed: empty and enabled.
        let mut plain = AccountsFile { accounts: vec![sample()] };
        save(&path, &plain).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("signature"), "defaults are not written: {text}");
        plain.accounts[0].signature_enabled = true;
        assert_eq!(load(&path).unwrap(), plain);
    }

    #[test]
    fn rejects_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.toml");
        std::fs::write(&path, "[[account]]\nid = 3").unwrap();
        assert!(matches!(load(&path), Err(ConfigError::Parse(_))));
    }
}
