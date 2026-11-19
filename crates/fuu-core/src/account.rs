//! Account model and provider presets.

use serde::{Deserialize, Serialize};

/// Transport security. Plain-text connections are not offered: Fuu always uses
/// implicit TLS or enforced STARTTLS, with certificate verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    #[default]
    Tls,
    /// Plain connection upgraded with STARTTLS; failure to upgrade aborts.
    StartTls,
    /// Unencrypted. Only accepted with the `insecure-test-transport` feature, for
    /// the local integration-test server.
    #[cfg(feature = "insecure-test-transport")]
    InsecurePlainForTests,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub security: Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AuthKind {
    /// Password or app password, kept in the OS keychain.
    #[default]
    Password,
    /// OAuth 2.0 (XOAUTH2); the refresh token is kept in the OS keychain.
    OAuth2,
}

/// One configured account. Secrets are never part of this structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Stable identifier, also the keychain entry name.
    pub id: String,
    pub display_name: String,
    pub email: String,
    /// Login name; usually the email address.
    pub username: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    #[serde(default)]
    pub auth: AuthKind,
    /// Identity provider for `AuthKind::OAuth2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<crate::oauth::Provider>,
    /// Plain-text signature put below new messages (after a blank line).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// Whether new messages get the signature.
    #[serde(default = "enabled_by_default", skip_serializing_if = "is_true")]
    pub signature_enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Account {
    /// A fresh identifier derived from the address and the current time.
    pub fn new_id(email: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let local: String = email
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect();
        format!("{local}-{:x}", nanos & 0xffff_ffff)
    }

    /// Initials for the account avatar.
    pub fn initial(&self) -> String {
        self.display_name
            .chars()
            .chain(self.email.chars())
            .find(|c| !c.is_whitespace())
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default()
    }
}

/// Known provider settings, chosen from the address domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub provider: &'static str,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    /// Shown to the user when password login needs extra steps at the provider.
    pub note: Option<&'static str>,
    /// Sign-in with the provider's OAuth, when supported.
    pub oauth: Option<crate::oauth::Provider>,
    /// Whether password (app password) login is accepted.
    pub password_login: bool,
}

fn server(host: &str, port: u16, security: Security) -> ServerConfig {
    ServerConfig { host: host.to_owned(), port, security }
}

/// Look up provider settings for an address. Unknown domains get `None`; the UI
/// then falls back to `imap.<domain>` / `smtp.<domain>` guesses.
pub fn preset_for(email: &str) -> Option<Preset> {
    let domain = email.rsplit_once('@')?.1.trim().to_ascii_lowercase();
    match domain.as_str() {
        "gmail.com" | "googlemail.com" => Some(Preset {
            provider: "Gmail",
            imap: server("imap.gmail.com", 993, Security::Tls),
            smtp: server("smtp.gmail.com", 465, Security::Tls),
            note: Some("Google でログインするか、2 段階認証を有効にしてアプリ パスワードを使ってください。"),
            oauth: Some(crate::oauth::Provider::Google),
            password_login: true,
        }),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "outlook.jp" | "hotmail.co.jp" | "live.jp" => {
            Some(Preset {
                provider: "Outlook.com",
                imap: server("outlook.office365.com", 993, Security::Tls),
                smtp: server("smtp-mail.outlook.com", 587, Security::StartTls),
                note: Some("Outlook.com は Microsoft アカウントでのログインのみ受け付けます。"),
                oauth: Some(crate::oauth::Provider::Microsoft),
                password_login: false,
            })
        }
        "icloud.com" | "me.com" | "mac.com" => Some(Preset {
            provider: "iCloud",
            imap: server("imap.mail.me.com", 993, Security::Tls),
            smtp: server("smtp.mail.me.com", 587, Security::StartTls),
            note: Some("Apple アカウントでアプリ用パスワードを作成して使ってください。"),
            oauth: None,
            password_login: true,
        }),
        "yahoo.co.jp" => Some(Preset {
            provider: "Yahoo!メール",
            imap: server("imap.mail.yahoo.co.jp", 993, Security::Tls),
            smtp: server("smtp.mail.yahoo.co.jp", 465, Security::Tls),
            note: Some("Yahoo!メールの設定で IMAP / SMTP のアクセスを有効にしてください。"),
            oauth: None,
            password_login: true,
        }),
        _ => None,
    }
}

/// Best-effort guess for unknown domains.
pub fn guess_for(email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let domain = email.rsplit_once('@')?.1.trim().to_ascii_lowercase();
    if domain.is_empty() || !domain.contains('.') {
        return None;
    }
    Some((server(&format!("imap.{domain}"), 993, Security::Tls), server(&format!("smtp.{domain}"), 465, Security::Tls)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_by_domain() {
        let gmail = preset_for("someone@Gmail.com").unwrap();
        assert_eq!(gmail.imap, server("imap.gmail.com", 993, Security::Tls));
        assert_eq!(preset_for("a@hotmail.co.jp").unwrap().smtp.security, Security::StartTls);
        assert_eq!(preset_for("a@icloud.com").unwrap().provider, "iCloud");
        assert!(preset_for("a@example.org").is_none());
        assert!(preset_for("not-an-address").is_none());
    }

    #[test]
    fn guesses_for_unknown_domains() {
        let (imap, smtp) = guess_for("me@mail.example.jp").unwrap();
        assert_eq!(imap.host, "imap.mail.example.jp");
        assert_eq!(smtp.host, "smtp.mail.example.jp");
        assert!(guess_for("me@localhost").is_none());
    }

    #[test]
    fn ids_are_keychain_safe() {
        let id = Account::new_id("Taro.Yamada+news@example.com");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{id}");
    }
}
