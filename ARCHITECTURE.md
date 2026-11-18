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

## Threads

- Main thread: Slint event loop. The wry webview is created here (WebView2 STA and
  macOS main-thread requirements).
- Background: a tokio runtime (one IMAP session per account in M1+).
- UI -> core: command channel. Core -> UI: `slint::invoke_from_event_loop`.

## Webview placement

The webview is a native child window laid over the reader pane's placeholder
rectangle. Its geometry is exported from Slint (`reader-x/y/w/h`) and synchronised
from Rust. Anything Slint draws over the reader (dialogs, the composer) sets
`reader-covered`, which hides the webview, because a native child always sits above
Slint's own drawing.
