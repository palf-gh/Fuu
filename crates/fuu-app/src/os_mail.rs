//! Fuu as the system's mail app: `mailto:` URLs from the operating system,
//! one running instance, and registering as (or checking for) the default
//! handler of `mailto:`.
//!
//! - Windows starts `fuu.exe "mailto:…"` for each click. A second instance
//!   hands the URL to the running one over a per-user named pipe and exits.
//! - macOS delivers URLs to the running app as an Apple Event (kAEGetURL).
//!
//! URLs from here are untrusted input; they only ever reach
//! `fuu_core::mailto::parse`, which keeps the address, subject and body.

use std::cell::RefCell;

thread_local! {
    static SINK: RefCell<Option<Box<dyn Fn(String)>>> = RefCell::new(None);
    static QUEUE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Hand a request to the UI (UI thread): a `mailto:` URL, or "" for "show the
/// window" (a second launch without a URL). Queued until a sink is set.
pub fn deliver(request: String) {
    let pending = SINK.with(|s| match &*s.borrow() {
        Some(sink) => {
            sink(request.clone());
            None
        }
        None => Some(request),
    });
    if let Some(request) = pending {
        QUEUE.with(|q| q.borrow_mut().push(request));
    }
}

/// Install the receiver of requests and replay the ones that came early.
pub fn set_sink(sink: impl Fn(String) + 'static) {
    SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
    for request in QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut())) {
        deliver(request);
    }
}

/// The `mailto:` URL among the command-line arguments (Windows launches).
pub fn mailto_argument() -> Option<String> {
    std::env::args().skip(1).find(|a| a.len() >= 7 && a[..7].eq_ignore_ascii_case("mailto:"))
}

/// Whether Fuu is the default handler of `mailto:` (None: cannot tell).
pub fn is_default() -> Option<bool> {
    #[cfg(windows)]
    return Some(win::is_default());
    #[cfg(target_os = "macos")]
    return mac::is_default();
    #[allow(unreachable_code)]
    None
}

/// Ask the system to make Fuu the default mail app. On Windows this registers
/// Fuu as a candidate and opens the Default apps page (Windows lets only the
/// user choose); on macOS it sets the handler (the system may ask to confirm).
pub fn make_default() -> Result<(), String> {
    #[cfg(windows)]
    return win::make_default();
    #[cfg(target_os = "macos")]
    return mac::make_default();
    #[allow(unreachable_code)]
    Err("この OS には対応していません".into())
}

/// Windows: become the single running instance, or pass `request` to the one
/// already running. Returns false when this process should exit.
pub fn claim_instance(request: Option<&str>) -> bool {
    #[cfg(windows)]
    return win::claim_instance(request.unwrap_or_default());
    #[allow(unreachable_code)]
    {
        let _ = request;
        true
    }
}

#[cfg(target_os = "macos")]
pub use mac::install_url_handler;

#[cfg(windows)]
mod win {
    use std::io::{Read, Write};
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

    use fuu_core::mailto::MAX_URL;

    const PROG_ID: &str = "Fuu.mailto";
    const APP: &str = "Fuu";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Per user and per Windows session.
    fn pipe_name() -> String {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let session = std::env::var("SESSIONNAME").unwrap_or_default();
        format!(r"\\.\pipe\dev.fuu.Fuu-{user}-{session}")
    }

    fn create_pipe(first: bool) -> Option<RawHandle> {
        let name = wide(&pipe_name());
        let open_mode = PIPE_ACCESS_INBOUND | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        // SAFETY: valid NUL-terminated name; the handle is checked below.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                MAX_URL as u32,
                0,
                std::ptr::null(),
            )
        };
        (handle != INVALID_HANDLE_VALUE).then_some(handle as RawHandle)
    }

    fn send(request: &str) -> bool {
        for _ in 0..20 {
            if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(pipe_name()) {
                // Let the running instance bring its window to the front.
                // SAFETY: plain call; failure only means no focus change.
                unsafe { AllowSetForegroundWindow(ASFW_ANY) };
                return pipe.write_all(request.as_bytes()).is_ok();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    pub fn claim_instance(request: &str) -> bool {
        let Some(first) = create_pipe(true) else {
            // Another instance owns the pipe: hand over, or run alone if it
            // does not answer (a stale or hung instance).
            return !send(request);
        };
        // Raw handles are not Send; the address moves to the listener thread,
        // which alone uses it from then on.
        let first = first as usize;
        std::thread::spawn(move || {
            let mut handle = first as RawHandle;
            loop {
                // SAFETY: `handle` is a live pipe instance owned by this loop.
                unsafe { ConnectNamedPipe(handle as _, std::ptr::null_mut()) };
                // SAFETY: we own the handle; File closes it when dropped.
                let file = unsafe { std::fs::File::from_raw_handle(handle) };
                let mut bytes = Vec::new();
                let read = file.take(MAX_URL as u64 + 1).read_to_end(&mut bytes);
                if read.is_ok() && bytes.len() <= MAX_URL {
                    if let Ok(text) = String::from_utf8(bytes) {
                        // Only an empty request or a single mailto: URL.
                        let ok = text.is_empty() || (text.len() >= 7 && text[..7].eq_ignore_ascii_case("mailto:"));
                        if ok {
                            let _ = slint::invoke_from_event_loop(move || super::deliver(text));
                        }
                    }
                }
                match create_pipe(false) {
                    Some(next) => handle = next,
                    None => return,
                }
            }
        });
        true
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by RegCreateKeyExW below.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn set(path: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        let mut key: HKEY = std::ptr::null_mut();
        let wpath = wide(path);
        // SAFETY: valid pointers; the key is closed by `Key`.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wpath.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(format!("レジストリに書き込めませんでした（{path}: {status}）"));
        }
        let key = Key(key);
        let wname = name.map(wide);
        let data = wide(value);
        // SAFETY: REG_SZ data including its NUL terminator.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                wname.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(format!("レジストリに書き込めませんでした（{path}: {status}）"));
        }
        Ok(())
    }

    fn get(path: &str, name: &str) -> Option<String> {
        let (wpath, wname) = (wide(path), wide(name));
        let mut buf = vec![0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: buffer and length describe the same allocation.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, wpath.as_ptr(), wname.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let chars = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
    }

    /// The keys that make Fuu a candidate mail app for the current user,
    /// under `root` (normally `Software`).
    pub(super) fn registration(root: &str, exe: &str) -> Vec<(String, Option<&'static str>, String)> {
        let open = format!("\"{exe}\" \"%1\"");
        let icon = format!("\"{exe}\",0");
        let client = format!(r"{root}\Clients\Mail\{APP}");
        vec![
            (format!(r"{root}\Classes\{PROG_ID}"), None, "Fuu メールリンク".into()),
            (format!(r"{root}\Classes\{PROG_ID}\DefaultIcon"), None, icon.clone()),
            (format!(r"{root}\Classes\{PROG_ID}\shell\open\command"), None, open),
            (client.clone(), None, APP.into()),
            (format!(r"{client}\shell\open\command"), None, format!("\"{exe}\"")),
            (format!(r"{client}\Capabilities"), Some("ApplicationName"), APP.into()),
            (format!(r"{client}\Capabilities"), Some("ApplicationDescription"), "ミニマルなメールクライアント".into()),
            (format!(r"{client}\Capabilities"), Some("ApplicationIcon"), icon),
            (format!(r"{client}\Capabilities\URLAssociations"), Some("mailto"), PROG_ID.into()),
            (format!(r"{root}\RegisteredApplications"), Some(APP), format!(r"Software\Clients\Mail\{APP}\Capabilities")),
        ]
    }

    pub(super) fn write(entries: &[(String, Option<&'static str>, String)]) -> Result<(), String> {
        entries.iter().try_for_each(|(path, name, value)| set(path, *name, value))
    }

    pub fn make_default() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        write(&registration("Software", &exe.to_string_lossy()))?;
        // SAFETY: plain notification, no pointers.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
        open::that(format!("ms-settings:defaultapps?registeredAppUser={APP}")).map_err(|e| format!("設定を開けませんでした: {e}"))
    }

    pub fn is_default() -> bool {
        get(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\mailto\UserChoice", "ProgId").as_deref() == Some(PROG_ID)
    }

    #[cfg(test)]
    pub(super) fn read(path: &str, name: Option<&str>) -> Option<String> {
        get(path, name.unwrap_or(""))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSString};

    /// 'GURL': both the event class (kInternetEventClass) and id (kAEGetURL).
    const GURL: u32 = u32::from_be_bytes(*b"GURL");
    /// '----': keyDirectObject.
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FuuUrlEventHandler"]
        struct UrlHandler;

        impl UrlHandler {
            #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
            fn handle(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                let url = event.paramDescriptorForKeyword(DIRECT_OBJECT).and_then(|d| d.stringValue());
                if let Some(url) = url {
                    let url = url.to_string();
                    if url.len() <= fuu_core::mailto::MAX_URL && url.len() >= 7 && url[..7].eq_ignore_ascii_case("mailto:") {
                        super::deliver(url);
                    }
                }
            }
        }
    );

    /// Receive `mailto:` URLs. Call after the app object exists and before
    /// the event loop runs, so the URL that launched the app is not lost.
    pub fn install_url_handler(mtm: MainThreadMarker) {
        let handler: Retained<UrlHandler> = unsafe { msg_send![UrlHandler::alloc(mtm), init] };
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: the selector matches the method above; the handler lives for
        // the rest of the process (leaked below).
        unsafe {
            manager.setEventHandler_andSelector_forEventClass_andEventID(
                &handler,
                sel!(handleGetURLEvent:withReplyEvent:),
                GURL,
                GURL,
            );
        }
        std::mem::forget(handler);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSSetDefaultHandlerForURLScheme(scheme: *const c_void, bundle: *const c_void) -> i32;
        fn LSCopyDefaultHandlerForURLScheme(scheme: *const c_void) -> *mut c_void;
    }

    fn bundle_id() -> Option<String> {
        NSBundle::mainBundle().bundleIdentifier().map(|s| s.to_string())
    }

    pub fn is_default() -> Option<bool> {
        let id = bundle_id()?;
        let scheme = NSString::from_str("mailto");
        // SAFETY: NSString is toll-free bridged to CFString; the result is
        // owned (Copy rule) and handed to Retained, which releases it.
        let current = unsafe { LSCopyDefaultHandlerForURLScheme(Retained::as_ptr(&scheme).cast()) };
        if current.is_null() {
            return Some(false);
        }
        let current: Retained<NSString> = unsafe { Retained::from_raw(current.cast()) }?;
        Some(current.to_string().eq_ignore_ascii_case(&id))
    }

    pub fn make_default() -> Result<(), String> {
        let id = bundle_id().ok_or("アプリとして（Fuu.app から）起動したときだけ設定できます")?;
        let (scheme, bundle) = (NSString::from_str("mailto"), NSString::from_str(&id));
        // SAFETY: both arguments are live CFStrings (toll-free bridged).
        let status = unsafe { LSSetDefaultHandlerForURLScheme(Retained::as_ptr(&scheme).cast(), Retained::as_ptr(&bundle).cast()) };
        if status == 0 { Ok(()) } else { Err(format!("既定のメールアプリにできませんでした（{status}）")) }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use windows_sys::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};

    #[test]
    fn registration_round_trip_in_a_scratch_key() {
        let root = format!(r"Software\FuuTest-{}", std::process::id());
        let entries = super::win::registration(&root, r"C:\Apps\Fuu\fuu.exe");
        super::win::write(&entries).expect("write");
        let check = |path: &str, name: Option<&str>| super::win::read(&format!(r"{root}\{path}"), name);
        assert_eq!(check(r"Classes\Fuu.mailto\shell\open\command", None).as_deref(), Some(r#""C:\Apps\Fuu\fuu.exe" "%1""#));
        assert_eq!(check(r"Clients\Mail\Fuu\Capabilities\URLAssociations", Some("mailto")).as_deref(), Some("Fuu.mailto"));
        assert_eq!(check("RegisteredApplications", Some("Fuu")).as_deref(), Some(r"Software\Clients\Mail\Fuu\Capabilities"));
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        // SAFETY: deletes only the scratch key created above.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide.as_ptr()) };
        assert!(check("RegisteredApplications", Some("Fuu")).is_none());
    }
}
