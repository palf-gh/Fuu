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
use crate::imap::{self, Folder, FolderPage, FolderRole, ImapError, ImapSession};
use crate::secrets::SecretStore;

/// Number of newest messages loaded per folder (paging arrives with the cache in M2).
pub const PAGE_SIZE: u32 = 200;

#[derive(Debug, Clone)]
pub enum Command {
    LoadAccounts,
    /// Verify the settings by logging in, then store the password and the account.
    AddAccount { account: Account, password: String },
    RemoveAccount { account_id: String },
    /// Load the folder list and the given folder (INBOX when `None`).
    OpenAccount { account_id: String, folder: Option<String> },
    OpenFolder { account_id: String, path: String },
    /// Load a message body (cache first), marking it as read on the server.
    FetchBody { account_id: String, folder: String, uid: u32 },
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
        let inner = Arc::new(Inner {
            config_path,
            secrets,
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
            Command::RemoveAccount { account_id } => self.remove_account(&account_id).await,
            Command::OpenAccount { account_id, folder } => self.open_account(&account_id, folder).await,
            Command::OpenFolder { account_id, path } => self.open_folder(&account_id, &path).await,
            Command::FetchBody { account_id, folder, uid } => self.fetch_body(&account_id, &folder, uid).await,
            Command::Move { account_id, folder, uid, target } => self.move_message(&account_id, &folder, uid, target).await,
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
        let session = match imap::login(&account.imap, &account.username, &password).await {
            Ok(s) => s,
            Err(e) => return self.emit(Event::AccountAddFailed(e.to_string())),
        };
        if let Err(e) = self.secrets.set(&account.id, &password) {
            return self.emit(Event::AccountAddFailed(e.to_string()));
        }
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
                let _ = self.secrets.delete(&account.id);
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
        let _ = self.secrets.delete(account_id);
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
        let password = self.secrets.get(&account.id).map_err(|e| e.to_string())?;
        self.emit(Event::Status(format!("{} に接続しています…", account.imap.host)));
        let session = imap::login(&account.imap, &account.username, &password).await.map_err(|e| e.to_string())?;
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
}
