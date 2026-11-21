//! Updates from GitHub Releases.
//!
//! The release workflow publishes `latest.json` next to the packages; each
//! package is signed with the project's updater key (minisign), whose public
//! half is built in here. A download whose signature does not match is never
//! installed. Checking sends one HTTPS request to GitHub (no identifiers
//! beyond what any download sends).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cargo_packager_updater::{Config, Update, UpdaterBuilder, WindowsConfig, WindowsUpdateInstallMode};
use slint::ComponentHandle;

use crate::AppWindow;

const ENDPOINT: &str = "https://github.com/palf-gh/Fuu/releases/latest/download/latest.json";
const RELEASES: &str = "https://github.com/palf-gh/Fuu/releases/latest";
/// minisign public key of the release workflow's signing key.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDlEM0NDRDkzOTQxMEEzQUIKUldTcm94Q1VrODA4bmZtL3pzSllWd0s3bE41aUY4bFpoa0Fub3JxUmZzcDlkTlJXNFZTMUZkTG8K";

/// `update-state` in the UI.
pub mod state {
    pub const IDLE: i32 = 0;
    pub const AVAILABLE: i32 = 1;
    pub const DOWNLOADING: i32 = 2;
    /// Installed (macOS, AppImage): restart to use it.
    pub const READY: i32 = 3;
    pub const CHECKING: i32 = 4;
}

static PENDING: Mutex<Option<Update>> = Mutex::new(None);

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether this copy can replace itself: not a distribution package (.deb),
/// whose files belong to the package manager.
pub fn can_self_update() -> bool {
    !cfg!(all(unix, not(target_os = "macos"))) || std::env::var_os("APPIMAGE").is_some()
}

fn config() -> Config {
    Config {
        endpoints: vec![ENDPOINT.parse().expect("valid endpoint")],
        pubkey: PUBKEY.into(),
        windows: Some(WindowsConfig { install_mode: Some(WindowsUpdateInstallMode::Passive), ..Default::default() }),
    }
}

/// Look for a newer release in the background. `manual` reports "up to date"
/// and errors; the start-up check stays quiet unless there is an update.
pub fn check(ui: &AppWindow, manual: bool) {
    if ui.get_update_state() == state::DOWNLOADING || ui.get_update_state() == state::READY {
        return;
    }
    if manual {
        ui.set_update_state(state::CHECKING);
        ui.set_update_status("確認しています…".into());
    }
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let result = current_version()
            .parse()
            .map_err(|e: semver::Error| e.to_string())
            .and_then(|v| UpdaterBuilder::new(v, config()).timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string()))
            .and_then(|u| u.check().map_err(|e| e.to_string()));
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            match result {
                Ok(Some(update)) => {
                    ui.set_update_version(update.version.clone().into());
                    ui.set_update_state(state::AVAILABLE);
                    ui.set_update_status(format!("新しいバージョン {} があります。", update.version).into());
                    *PENDING.lock().unwrap() = Some(update);
                }
                Ok(None) => {
                    ui.set_update_state(state::IDLE);
                    ui.set_update_status(format!("最新のバージョン（{}）です。", current_version()).into());
                }
                Err(e) => {
                    ui.set_update_state(state::IDLE);
                    if manual {
                        ui.set_update_status(format!("確認できませんでした: {e}").into());
                    }
                }
            }
        });
    });
}

/// Download, verify and install the pending update. Windows hands over to
/// the installer (which restarts Fuu); macOS and AppImage swap the app in
/// place and ask for a restart. A .deb install only opens the release page.
pub fn install(ui: &AppWindow) {
    if !can_self_update() {
        let _ = open::that_detached(RELEASES);
        return;
    }
    let Some(update) = PENDING.lock().unwrap().take() else { return };
    // Queued signature edits reach the engine while the download runs.
    crate::mail::flush_signatures();
    ui.set_update_state(state::DOWNLOADING);
    ui.set_update_progress(0.0);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let received = Arc::new(AtomicU64::new(0));
        let progress = {
            let (weak, received) = (weak.clone(), received.clone());
            move |chunk: usize, total: Option<u64>| {
                let done = received.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let fraction = (done as f32 / total as f32).min(1.0);
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_update_progress(fraction);
                        }
                    });
                }
            }
        };
        // download_extended verifies the signature against PUBKEY.
        let downloaded = update.download_extended(progress, || {});
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let bytes = match downloaded {
                Ok(b) => b,
                Err(e) => return failed(&ui, update, format!("ダウンロードできませんでした: {e}")),
            };
            // Windows: the installer takes over and this process exits here.
            crate::mail::flush_signatures_on_quit();
            match update.install(bytes) {
                Ok(()) => {
                    ui.set_update_state(state::READY);
                    ui.set_update_status(format!("バージョン {} を入れました。再起動すると使えます。", update.version).into());
                }
                Err(e) => failed(&ui, update, format!("更新できませんでした: {e}")),
            }
        });
    });
}

fn failed(ui: &AppWindow, update: Update, message: String) {
    crate::notify(ui, message.clone(), true);
    ui.set_update_status(message.into());
    ui.set_update_state(state::AVAILABLE);
    *PENDING.lock().unwrap() = Some(update);
}

/// Start the updated Fuu and quit this one (macOS, AppImage).
pub fn restart() {
    #[cfg(not(windows))]
    {
        let exe = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok());
        if let Some(exe) = exe {
            // The new process waits for this one to release the instance lock.
            let _ = std::process::Command::new(exe).env("FUU_RELAUNCHED", "1").spawn();
        }
    }
    crate::mail::flush_signatures_on_quit();
    let _ = slint::quit_event_loop();
}
