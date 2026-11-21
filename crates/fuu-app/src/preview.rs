//! Attachment previews: converted by `fuu-preview` on a worker thread, always
//! sanitised, then shown by the reader like a mail body. Nothing touches disk.

use std::cell::RefCell;
use std::rc::Rc;

use fuu_model::SafeDocument;
use slint::ComponentHandle;

use crate::mail;
use crate::tabs::AttachmentRef;
use crate::AppWindow;
use fuu_i18n::tr;

/// Converted previews kept in memory (most recent last).
const CACHE_SIZE: usize = 6;

type Key = (String, String, u32, usize);

enum Entry {
    Loading,
    Ready(Rc<SafeDocument>),
    Failed(String),
}

thread_local! {
    static CACHE: RefCell<Vec<(Key, Entry)>> = const { RefCell::new(Vec::new()) };
}

fn key(a: &AttachmentRef) -> Key {
    (a.message.account_id.clone(), a.message.folder.clone(), a.message.uid, a.index)
}

/// Whether an attachment opens as a preview (otherwise it is saved).
pub fn previewable(name: &str, mime: &str) -> bool {
    fuu_preview::kind_of(name, mime).is_some()
}

/// Short description for the reader header: "PDF · 1.2 MB".
pub fn describe(a: &AttachmentRef) -> String {
    match fuu_preview::kind_of(&a.name, &a.mime) {
        Some(kind) if !a.size.is_empty() => format!("{} · {}", kind.label(), a.size),
        Some(kind) => kind.label().to_owned(),
        None => a.size.clone(),
    }
}

/// The document to show for an attachment tab: the preview when ready,
/// otherwise a notice while it loads (starting the conversion if needed).
pub fn document(ui: &AppWindow, a: &AttachmentRef) -> SafeDocument {
    let k = key(a);
    let cached = CACHE.with(|c| {
        c.borrow().iter().find(|(key, _)| *key == k).map(|(_, e)| match e {
            Entry::Loading => Err(None),
            Entry::Ready(doc) => Ok(doc.clone()),
            Entry::Failed(why) => Err(Some(why.clone())),
        })
    });
    match cached {
        Some(Ok(doc)) => return (*doc).clone(),
        Some(Err(None)) => return mail::notice_document(tr!("Preparing the preview…")),
        Some(Err(Some(why))) => return mail::notice_document(&tr!("{0}. Use Save to save the file and open it.", why)),
        None => {}
    }
    // The message body holds the bytes; it may still be loading.
    let body = match mail::body_or_request(&a.message) {
        Ok(body) => body,
        Err(notice) => return notice,
    };
    let Some((name, mime, data)) = mail::attachment_data(&body, a.index) else {
        return mail::notice_document(tr!("Could not read the attachment."));
    };
    store(k.clone(), Entry::Loading);
    let weak = ui.as_weak();
    let spawned = std::thread::Builder::new().name("fuu-preview".into()).spawn(move || {
        let result = std::panic::catch_unwind(|| fuu_preview::preview(&name, &mime, &data).map(|m| fuu_sanitiser::sanitise(&m)));
        let entry = match result {
            Ok(Ok(doc)) => Ok(doc),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(tr!("Something went wrong while preparing the preview").to_owned()),
        };
        let _ = slint::invoke_from_event_loop(move || {
            store(k, match entry {
                Ok(doc) => Entry::Ready(Rc::new(doc)),
                Err(why) => Entry::Failed(why),
            });
            if let Some(ui) = weak.upgrade() {
                // Re-applies the active tab only if it is an attachment preview.
                ui.invoke_preview_ready();
            }
        });
    });
    if spawned.is_err() {
        return mail::notice_document(tr!("Could not prepare a preview."));
    }
    mail::notice_document(tr!("Preparing the preview…"))
}

fn store(k: Key, entry: Entry) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|(key, _)| *key != k);
        c.push((k, entry));
        while c.len() > CACHE_SIZE {
            c.remove(0);
        }
    });
}
