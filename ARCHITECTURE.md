# Architecture

## Engine separation

```
raw RFC 5322 bytes
  -> MessageParser (fuu-parser)        swappable parsing engine
  -> ParsedMessage (fuu-model)         engine-neutral model
  -> Sanitiser (fuu-sanitiser)         always runs, engine independent
  -> SafeDocument (fuu-model)          sanitised HTML + text + resource manifest + flags
  -> BodyRenderer (fuu-render)         swappable rendering engine (webview / text)
```

Rules:

- The parser knows nothing about UI; a renderer knows nothing about MIME. A renderer
  receives only a `SafeDocument`.
- No third-party crate type crosses a boundary: boundary types live in `fuu-model`.
- Sanitisation is not part of any renderer, so swapping a renderer never weakens it.
- Crate dependencies enforce this: `fuu-render` depends only on `fuu-model`;
  `fuu-parser` and `fuu-sanitiser` never depend on UI crates.
- Every parser implementation runs the shared fixture contract tests; every renderer
  runs the shared renderer test-suite (zero network requests, refused schemes, no
  navigation).

## Attachment previews

```
attachment bytes (from the message's raw MIME)
  -> fuu-preview        image / PDF / docx / xlsx… / pptx -> neutral HTML + cid: images
  -> ParsedMessage      same model as a mail body
  -> Sanitiser          always runs
  -> SafeDocument -> Blitz
```

`fuu-preview` depends only on `fuu-model` and format crates (hayro, calamine,
quick-xml, zip, image); like `fuu-parser` it may know file formats and MIME types,
and it never sees UI or renderer types. The app converts on a worker thread and
keeps a few previews in memory. Limits and threats: `SECURITY.md`.

## Threads

- Main thread: Slint event loop and the Blitz renderer.
- Background: a tokio runtime (one IMAP session per account in M1+).
- UI -> core: command channel. Core -> UI: `slint::invoke_from_event_loop`.

## Reader geometry

The reader body's geometry is exported from Slint (`reader-x/y/w/h`) and synchronised
from Rust, so the renderer lays the document out at the pane width.

## Renderers

| Engine | Module | Notes |
|---|---|---|
| Blitz (only engine) | `fuu-render::blitz` | HTML/CSS only, no JavaScript engine. Painted on the CPU (vello_cpu) into a premultiplied RGBA buffer shown as a Slint `Image`, so Slint overlays draw above the document. Re-laid out when the pane width settles (debounced). Links are hit-tested by the renderer from host clicks. |
| Text | `fuu-render::text` | Plain-text fallback. |

The wry webview engine from the M0 spike was removed; the `BodyRenderer` boundary
still allows another engine to be added later.

## Right-pane tabs

Tabs belong to the right pane only (`fuu-app/src/tabs.rs`); the message list is
independent of them and with nothing open there are no tabs.

| Kind | Opened by | Notes |
|---|---|---|
| Message / sample | clicking a row (replaces the active viewer tab), middle click (unseen background tab) | |
| Attachment | clicking a previewable attachment (opens beside the message; one tab per attachment) | read-only preview; only 保存 acts, never reply / archive / delete |
| Draft | new, reply, reply all, forward, `mailto:` | from / to / CC / BCC / subject / body; send or discard closes the tab |
| Settings | title-bar button, Ctrl/Cmd+, | single instance |

Closing the active tab activates the one taking its position; up to 32 tabs and 16
reopenable closed tabs. Tabs reorder by dragging. Shortcuts: Ctrl/Cmd+T or N (new
message), W (close), Shift+T (reopen), Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+PgUp/PgDn
(Cmd+Shift+[ ] on macOS), Ctrl/Cmd+1..8, 9 (last tab).

Settings are held in memory for now; persistence arrives with the M1 config file.
