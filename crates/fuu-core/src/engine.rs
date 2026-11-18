//! The mail engine: a background tokio runtime that owns the configuration, the
//! secret store and one IMAP session per account. The UI sends [`Command`]s and
//! receives [`Event`]s through a callback (which the app forwards to its event loop).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::account::Account;
use crate::cache::BodyCache;
use crate::config::{self, AccountsFile};
use crate::imap::{self, Credential, Folder, FolderPage, FolderRole, ImapError, ImapSession};
use crate::oauth::{self, OAuthClients, OAuthError, Provider};
use crate::secrets::{self, SecretStore};

/// How long the browser sign-in may take.
const OAUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Number of newest messages loaded per folder (paging arrives with the cache in M2).
pub const PAGE_SIZE: u32 = 200;

#[derive(Debug, Clone)]
pub enum Command {
    LoadAccounts,
    /// Verify the settings by logging in, then store the password and the account.
    AddAccount { account: Account, password: String },
    /// Sign in with the account's OAuth provider in the browser, verify, then store
    /// the refresh token and the account. Also used to sign in again.
    AddOAuthAccount { account: Account },
    /// Abandon a browser sign-in in progress.
    CancelOAuth,
    RemoveAccount { account_id: String },
    /// Load the folder list and the given folder (INBOX when `None`).
    OpenAccount { account_id: String, folder: Option<String> },
    OpenFolder { account_id: String, path: String },
    /// Load a message body (cache first), marking it as read on the server.
    FetchBody { account_id: String, folder: String, uid: u32 },
    /// Fetch remote images the user allowed for one message.
    FetchRemoteImages { account_id: String, folder: String, uid: u32, urls: Vec<String> },
    /// Send a message; `answered` marks the replied-to message.
    Send { account_id: String, outgoing: crate::compose::Outgoing, answered: Option<(String, u32)>, draft_id: u64 },
    /// Move a message to the account's Trash or Archive folder.
    Move { account_id: String, folder: String, uid: u32, target: MoveTarget },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    Trash,
    Archive,
}

#[derive(Debug, Clone)]
pub enum Event {
    Accounts(Vec<Account>),
    AccountAdded(Account),
    AccountAddFailed(String),
    Folders { account_id: String, folders: Vec<Folder> },
    Messages { account_id: String, path: String, page: FolderPage },
    Body { account_id: String, folder: String, uid: u32, raw: Arc<Vec<u8>> },
    BodyFailed { account_id: String, folder: String, uid: u32, message: String },
    Moved { account_id: String, folder: String, uid: u32, target: MoveTarget },
    Sent { account_id: String, draft_id: u64, warning: Option<String> },
    SendFailed { draft_id: u64, message: String },
    RemoteImages { account_id: String, folder: String, uid: u32, images: Vec<Option<crate::remote::Image>> },
    /// Open this URL in the system browser to continue an OAuth sign-in.
    OAuthBrowser { url: String },
    /// Short progress text for the status line.
    Status(String),
    Error(String),
}

pub struct Engine {
    tx: mpsc::UnboundedSender<Command>,
}

struct Inner {
    config_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    clients: OAuthClients,
    /// Access tokens in memory only: account id → (token, expiry).
    tokens: Mutex<HashMap<String, (String, i64)>>,
    /// One refresh at a time per account (providers may rotate refresh tokens).
    refreshing: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    oauth_cancel: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    accounts: Mutex<AccountsFile>,
    sessions: Mutex<HashMap<String, Arc<Mutex<ImapSession>>>>,
    /// Last seen UIDVALIDITY per (account, folder), for cache lookups.
    validity: Mutex<HashMap<(String, String), u32>>,
    folders: Mutex<HashMap<String, Vec<Folder>>>,
    cache: Option<BodyCache>,
    emit: Box<dyn Fn(Event) + Send + Sync>,
}

impl Engine {
    /// Start the engine on its own thread.
    /// `cache_path`: SQLite body cache (`None` disables caching).
    pub fn start(
        config_path: PathBuf,
        cache_path: Option<PathBuf>,
        secrets: Arc<dyn SecretStore>,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Command>();
        let clients_path = config_path.with_file_name(oauth::CLIENTS_FILE);
        let inner = Arc::new(Inner {
            config_path,
            secrets,
            clients: OAuthClients::load(&clients_path),
            tokens: Mutex::new(HashMap::new()),
            refreshing: Mutex::new(HashMap::new()),
            oauth_cancel: std::sync::Mutex::new(None),
            accounts: Mutex::new(AccountsFile::default()),
            sessions: Mutex::new(HashMap::new()),
            validity: Mutex::new(HashMap::new()),
            folders: Mutex::new(HashMap::new()),
            cache: cache_path.and_then(|p| BodyCache::open(&p).map_err(|e| eprintln!("{e}")).ok()),
            emit: Box::new(on_event),
        });
        std::thread::Builder::new()
            .name("fuu-engine".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                runtime.block_on(async move {
                    while let Some(command) = rx.recv().await {
                        let inner = inner.clone();
                        tokio::spawn(async move { inner.handle(command).await });
                    }
                });
            })
            .expect("engine thread");
        Self { tx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

impl Inner {
    fn emit(&self, event: Event) {
        (self.emit)(event);
    }

    async fn handle(self: Arc<Self>, command: Command) {
        match command {
            Command::LoadAccounts => self.load_accounts().await,
            Command::AddAccount { account, password } => self.add_account(account, password).await,
            Command::AddOAuthAccount { account } => self.add_oauth_account(account).await,
            Command::CancelOAuth => {
                if let Some(cancel) = self.oauth_cancel.lock().unwrap().take() {
                    let _ = cancel.send(());
                }
            }
            Command::RemoveAccount { account_id } => self.remove_account(&account_id).await,
            Command::OpenAccount { account_id, folder } => self.open_account(&account_id, folder).await,
            Command::OpenFolder { account_id, path } => self.open_folder(&account_id, &path).await,
            Command::FetchBody { account_id, folder, uid } => self.fetch_body(&account_id, &folder, uid).await,
            Command::Move { account_id, folder, uid, target } => self.move_message(&account_id, &folder, uid, target).await,
            Command::Send { account_id, outgoing, answered, draft_id } => self.send(&account_id, outgoing, answered, draft_id).await,
            Command::FetchRemoteImages { account_id, folder, uid, urls } => {
                let images = crate::remote::fetch_images(&urls).await;
                self.emit(Event::RemoteImages { account_id, folder, uid, images });
            }
        }
    }

    async fn load_accounts(&self) {
        match config::load(&self.config_path) {
            Ok(file) => {
                let list = file.accounts.clone();
                *self.accounts.lock().await = file;
                self.emit(Event::Accounts(list));
            }
            Err(e) => self.emit(Event::Error(e.to_string())),
        }
    }

    async fn add_account(&self, mut account: Account, password: String) {
        if account.id.is_empty() {
            account.id = Account::new_id(&account.email);
        }
        self.emit(Event::Status(format!("{} に接続しています…", account.imap.host)));
        let session = match imap::login(&account.imap, &account.username, &Credential::Password(password.clone())).await {
            Ok(s) => s,
            Err(e) => return self.emit(Event::AccountAddFailed(e.to_string())),
        };
        if let Err(e) = secrets::set_long(self.secrets.as_ref(), &account.id, &password) {
            return self.emit(Event::AccountAddFailed(e.to_string()));
        }
        self.save_new_account(account, session).await;
    }

    async fn add_oauth_account(&self, mut account: Account) {
        if account.id.is_empty() {
            account.id = Account::new_id(&account.email);
        }
        account.auth = crate::account::AuthKind::OAuth2;
        let Some(provider) = account.oauth else {
            return self.emit(Event::AccountAddFailed("OAuth のプロバイダーが指定されていません".into()));
        };
        let tokens = match self.browser_sign_in(provider, &account.email).await {
            Ok(t) => t,
            Err(e) => return self.emit(Event::AccountAddFailed(e.to_string())),
        };
        let Some(refresh_token) = tokens.refresh_token.clone() else {
            return self.emit(Event::AccountAddFailed(format!("{} がリフレッシュ トークンを発行しませんでした", provider.name())));
        };
        self.emit(Event::Status(format!("{} に接続しています…", account.imap.host)));
        let credential = Credential::Bearer(tokens.access_token.clone());
        let session = match imap::login(&account.imap, &account.username, &credential).await {
            Ok(s) => s,
            Err(e) => return self.emit(Event::AccountAddFailed(e.to_string())),
        };
        if let Err(e) = secrets::set_long(self.secrets.as_ref(), &account.id, &refresh_token) {
            return self.emit(Event::AccountAddFailed(e.to_string()));
        }
        self.tokens.lock().await.insert(account.id.clone(), (tokens.access_token, oauth::now() + tokens.expires_in));
        self.save_new_account(account, session).await;
    }

    /// Run the browser part of the OAuth flow and exchange the code for tokens.
    async fn browser_sign_in(&self, provider: Provider, email: &str) -> Result<oauth::TokenResponse, OAuthError> {
        let client = self.clients.get(provider).cloned().ok_or_else(|| {
            OAuthError::Other(format!(
                "{} の OAuth クライアント ID が設定されていません。設定フォルダーの {} に client_id を書いてください",
                provider.name(),
                oauth::CLIENTS_FILE
            ))
        })?;
        let loopback = oauth::Loopback::bind(provider).await?;
        let redirect_uri = loopback.redirect_uri.clone();
        let state = oauth::random_token(24);
        let verifier = oauth::random_token(48);
        let url = oauth::authorise_url(provider, &client, &redirect_uri, &state, &oauth::pkce_challenge(&verifier), email);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        if let Some(previous) = self.oauth_cancel.lock().unwrap().replace(cancel_tx) {
            let _ = previous.send(());
        }
        self.emit(Event::OAuthBrowser { url });
        self.emit(Event::Status(format!("ブラウザで {} にログインしてください…", provider.name())));
        let code = tokio::select! {
            code = loopback.wait(&state, OAUTH_TIMEOUT) => code,
            _ = cancel_rx => Err(OAuthError::Cancelled),
        };
        self.oauth_cancel.lock().unwrap().take();
        oauth::exchange_code(provider, &client, &code?, &verifier, &redirect_uri).await
    }

    /// What to log in with: the stored password, or a (refreshed) access token.
    async fn credential(&self, account: &Account, force_refresh: bool) -> Result<Credential, String> {
        if account.auth != crate::account::AuthKind::OAuth2 {
            return secrets::get_long(self.secrets.as_ref(), &account.id).map(Credential::Password).map_err(|e| e.to_string());
        }
        let provider = account.oauth.ok_or("OAuth のプロバイダーが不明です")?;
        let lock = self.refreshing.lock().await.entry(account.id.clone()).or_default().clone();
        let _guard = lock.lock().await;
        if !force_refresh {
            if let Some((token, expires)) = self.tokens.lock().await.get(&account.id) {
                if !oauth::needs_refresh(*expires, oauth::now()) {
                    return Ok(Credential::Bearer(token.clone()));
                }
            }
        }
        let client = self.clients.get(provider).cloned().ok_or_else(|| {
            format!("{} の OAuth クライアント ID が設定されていません（{}）", provider.name(), oauth::CLIENTS_FILE)
        })?;
        let refresh_token = secrets::get_long(self.secrets.as_ref(), &account.id).map_err(|e| e.to_string())?;
        let response = oauth::refresh(provider, &client, &refresh_token).await.map_err(|e| e.to_string())?;
        if let Some(rotated) = response.refresh_token.as_deref().filter(|t| *t != refresh_token) {
            secrets::set_long(self.secrets.as_ref(), &account.id, rotated).map_err(|e| e.to_string())?;
        }
        self.tokens.lock().await.insert(account.id.clone(), (response.access_token.clone(), oauth::now() + response.expires_in));
        Ok(Credential::Bearer(response.access_token))
    }

    async fn save_new_account(&self, account: Account, session: ImapSession) {
        let saved = {
            let mut file = self.accounts.lock().await;
            file.accounts.retain(|a| a.id != account.id);
            file.accounts.push(account.clone());
            config::save(&self.config_path, &file).map(|_| file.accounts.clone())
        };
        match saved {
            Ok(list) => {
                self.sessions.lock().await.insert(account.id.clone(), Arc::new(Mutex::new(session)));
                self.emit(Event::Accounts(list));
                self.emit(Event::AccountAdded(account.clone()));
                self.open_account(&account.id, None).await;
            }
            Err(e) => {
                let _ = secrets::delete_long(self.secrets.as_ref(), &account.id);
                self.emit(Event::AccountAddFailed(e.to_string()));
            }
        }
    }

    async fn remove_account(&self, account_id: &str) {
        self.sessions.lock().await.remove(account_id);
        let saved = {
            let mut file = self.accounts.lock().await;
            file.accounts.retain(|a| a.id != account_id);
            config::save(&self.config_path, &file).map(|_| file.accounts.clone())
        };
        let _ = secrets::delete_long(self.secrets.as_ref(), account_id);
        self.tokens.lock().await.remove(account_id);
        if let Some(cache) = &self.cache {
            let _ = cache.purge_account(account_id);
        }
        match saved {
            Ok(list) => self.emit(Event::Accounts(list)),
            Err(e) => self.emit(Event::Error(e.to_string())),
        }
    }

    async fn account(&self, account_id: &str) -> Option<Account> {
        self.accounts.lock().await.accounts.iter().find(|a| a.id == account_id).cloned()
    }

    /// The cached session for an account, logging in when there is none.
    async fn session(&self, account: &Account) -> Result<Arc<Mutex<ImapSession>>, String> {
        if let Some(s) = self.sessions.lock().await.get(&account.id) {
            return Ok(s.clone());
        }
        let credential = self.credential(account, false).await?;
        self.emit(Event::Status(format!("{} に接続しています…", account.imap.host)));
        let session = match imap::login(&account.imap, &account.username, &credential).await {
            Ok(s) => s,
            // A rejected access token may just be revoked early: refresh once.
            Err(ImapError::Auth(_)) if matches!(credential, Credential::Bearer(_)) => {
                let fresh = self.credential(account, true).await?;
                imap::login(&account.imap, &account.username, &fresh).await.map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        };
        let session = Arc::new(Mutex::new(session));
        self.sessions.lock().await.insert(account.id.clone(), session.clone());
        Ok(session)
    }

    /// Run an IMAP operation, reconnecting once if the cached session has died.
    async fn with_session<T, F>(&self, account: &Account, op: F) -> Result<T, String>
    where
        F: for<'s> Fn(&'s mut ImapSession) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, ImapError>> + Send + 's>>,
    {
        for attempt in 0..2 {
            let session = self.session(account).await?;
            let mut guard = session.lock().await;
            match op(&mut guard).await {
                Ok(v) => return Ok(v),
                Err(e @ (ImapError::Auth(_) | ImapError::Tls(_))) => return Err(e.to_string()),
                Err(e) if attempt == 1 => return Err(e.to_string()),
                Err(_) => {
                    drop(guard);
                    self.sessions.lock().await.remove(&account.id);
                }
            }
        }
        unreachable!("loop returns on the second attempt")
    }

    async fn open_account(&self, account_id: &str, folder: Option<String>) {
        let Some(account) = self.account(account_id).await else { return };
        match self.with_session(&account, |s| Box::pin(imap::list_folders(s))).await {
            Ok(folders) => {
                let path = folder
                    .filter(|p| folders.iter().any(|f| &f.path == p))
                    .or_else(|| folders.iter().find(|f| f.role == imap::FolderRole::Inbox).map(|f| f.path.clone()))
                    .unwrap_or_else(|| "INBOX".into());
                self.folders.lock().await.insert(account.id.clone(), folders.clone());
                self.emit(Event::Folders { account_id: account.id.clone(), folders });
                self.open_folder(account_id, &path).await;
            }
            Err(e) => self.emit(Event::Error(e)),
        }
    }

    async fn open_folder(&self, account_id: &str, path: &str) {
        let Some(account) = self.account(account_id).await else { return };
        self.emit(Event::Status("メールを読み込んでいます…".into()));
        let path_owned = path.to_owned();
        match self
            .with_session(&account, move |s| {
                let path = path_owned.clone();
                Box::pin(async move { imap::fetch_summaries(s, &path, PAGE_SIZE).await })
            })
            .await
        {
            Ok(page) => {
                if let Some(v) = page.uid_validity {
                    self.validity.lock().await.insert((account.id.clone(), path.to_owned()), v);
                    if let Some(cache) = &self.cache {
                        let _ = cache.retain_validity(&account.id, path, v);
                    }
                }
                self.emit(Event::Status(String::new()));
                self.emit(Event::Messages { account_id: account.id.clone(), path: path.to_owned(), page });
            }
            Err(e) => self.emit(Event::Error(e)),
        }
    }

    async fn fetch_body(&self, account_id: &str, folder: &str, uid: u32) {
        let Some(account) = self.account(account_id).await else { return };
        let known = self.validity.lock().await.get(&(account.id.clone(), folder.to_owned())).copied();
        if let (Some(cache), Some(v)) = (&self.cache, known) {
            if let Ok(Some(raw)) = cache.get(&account.id, folder, v, uid) {
                return self.emit(Event::Body { account_id: account.id, folder: folder.to_owned(), uid, raw: Arc::new(raw) });
            }
        }
        let path = folder.to_owned();
        let result = self
            .with_session(&account, move |s| {
                let path = path.clone();
                Box::pin(async move { imap::fetch_body(s, &path, uid).await })
            })
            .await;
        match result {
            Ok((validity, raw)) => {
                if let Some(cache) = &self.cache {
                    let _ = cache.put(&account.id, folder, validity, uid, &raw);
                }
                self.emit(Event::Body { account_id: account.id, folder: folder.to_owned(), uid, raw: Arc::new(raw) });
            }
            Err(message) => self.emit(Event::BodyFailed { account_id: account.id, folder: folder.to_owned(), uid, message }),
        }
    }

    async fn move_message(&self, account_id: &str, folder: &str, uid: u32, target: MoveTarget) {
        let Some(account) = self.account(account_id).await else { return };
        let role = match target {
            MoveTarget::Trash => FolderRole::Trash,
            MoveTarget::Archive => FolderRole::Archive,
        };
        let destination = self.folders.lock().await.get(&account.id).and_then(|list| list.iter().find(|f| f.role == role && f.selectable).map(|f| f.path.clone()));
        let Some(destination) = destination else {
            let name = if target == MoveTarget::Trash { "ゴミ箱" } else { "アーカイブ" };
            return self.emit(Event::Error(format!("{name}フォルダーが見つかりません")));
        };
        if destination == folder {
            return self.emit(Event::Error("すでにそのフォルダーにあります".into()));
        }
        let (from, to) = (folder.to_owned(), destination.clone());
        match self
            .with_session(&account, move |s| {
                let (from, to) = (from.clone(), to.clone());
                Box::pin(async move { imap::move_message(s, &from, uid, &to).await })
            })
            .await
        {
            Ok(()) => {
                if let Some(cache) = &self.cache {
                    let _ = cache.remove(&account.id, folder, uid);
                }
                self.emit(Event::Moved { account_id: account.id, folder: folder.to_owned(), uid, target });
            }
            Err(e) => self.emit(Event::Error(e)),
        }
    }

    async fn send(&self, account_id: &str, outgoing: crate::compose::Outgoing, answered: Option<(String, u32)>, draft_id: u64) {
        let Some(account) = self.account(account_id).await else {
            return self.emit(Event::SendFailed { draft_id, message: "アカウントが見つかりません".into() });
        };
        let (_, raw) = match crate::compose::build(&outgoing) {
            Ok(v) => v,
            Err(message) => return self.emit(Event::SendFailed { draft_id, message }),
        };
        let credential = match self.credential(&account, false).await {
            Ok(c) => c,
            Err(message) => return self.emit(Event::SendFailed { draft_id, message }),
        };
        self.emit(Event::Status("送信しています…".into()));
        let recipients = outgoing.envelope_recipients();
        let mut result = crate::smtp::send(&account.smtp, &account.username, &credential, &account.email, &recipients, &raw).await;
        if matches!((&result, &credential), (Err(crate::smtp::SmtpError::Auth(_)), Credential::Bearer(_))) {
            // Refresh once and retry: nothing was accepted when authentication failed.
            match self.credential(&account, true).await {
                Ok(fresh) => {
                    result = crate::smtp::send(&account.smtp, &account.username, &fresh, &account.email, &recipients, &raw).await;
                }
                Err(message) => return self.emit(Event::SendFailed { draft_id, message }),
            }
        }
        if let Err(e) = result {
            return self.emit(Event::SendFailed { draft_id, message: e.to_string() });
        }
        // Sent: the message is out. Follow-up failures are warnings, not send failures.
        let mut warnings = Vec::new();
        if !imap::saves_sent_automatically(&account.imap.host) {
            let sent = self.folders.lock().await.get(&account.id).and_then(|list| list.iter().find(|f| f.role == FolderRole::Sent).map(|f| f.path.clone()));
            match sent {
                Some(path) => {
                    let raw = raw.clone();
                    if let Err(e) = self
                        .with_session(&account, move |s| {
                            let (path, raw) = (path.clone(), raw.clone());
                            Box::pin(async move { imap::append_message(s, &path, &raw).await })
                        })
                        .await
                    {
                        warnings.push(format!("送信済みフォルダーに保存できませんでした: {e}"));
                    }
                }
                None => warnings.push("送信済みフォルダーが見つからないため保存していません".into()),
            }
        }
        if let Some((folder, uid)) = answered {
            let _ = self
                .with_session(&account, move |s| {
                    let folder = folder.clone();
                    Box::pin(async move { imap::mark_answered(s, &folder, uid).await })
                })
                .await;
        }
        self.emit(Event::Status(String::new()));
        self.emit(Event::Sent { account_id: account.id, draft_id, warning: (!warnings.is_empty()).then(|| warnings.join(" / ")) });
    }
}
