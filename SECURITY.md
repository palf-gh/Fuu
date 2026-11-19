# Security design (HTML mail)

## Display policy

| Item | Policy |
|---|---|
| Remote images | Blocked by default. Loaded only when the user allows it for the current message, fetched by Fuu (never by the webview) and served through the private protocol. |
| CID images | Resolved only from the message's own `Content-ID` parts, addressed by manifest index. Unknown CIDs render as blank. |
| External CSS | Never loaded: `<link rel=stylesheet>`, `@import` and CSS `url()` are removed. |
| Links | Opened in the OS default browser after the user confirms the real URL. Nothing navigates inside the webview. `mailto:` opens Fuu's composer. |
| Tracking pixels | Suppressed by the remote-image block. Even when remote images are allowed, 1×1 / hidden / 0–2px images are not fetched. |
| HTML CSS | Applied only within an allow-list of properties. |

## Layers

1. **Parse limits** (`fuu-parser`): size, MIME depth, part count, decoded size;
   panics contained; everything normalised to UTF-8.
2. **Sanitiser** (`fuu-sanitiser`, M2): ammonia allow-list for tags/attributes;
   cssparser property allow-list; URL rewriting (`cid:` -> `res/<n>`, raster `data:`
   only, remote images -> placeholder); `http`/`https`/`mailto` links only;
   suspicious-link detection; bidi control characters stripped from headers.
3. **Renderer hardening (Blitz)** (`fuu-render::blitz`):
   - No JavaScript engine exists in the process path.
   - Every load goes through `ManifestNet`: only manifest raster entries
     (`fuu://doc/res/<n>`) and raster `data:` images are served. Everything else
     (remote URLs, stylesheets, fonts, SVG) is refused and completed with an empty
     body so render-blocking resources cannot stall layout.
   - Navigation and form submission are refused by `RefuseNavigation`; links are
     hit-tested from host clicks and classified (`http(s)`/`mailto` only).
4. **Remote content policy** (`fuu-sanitiser`, `fuu-core::remote`): blocked by
   default; each remote `<img>` points at an unloadable `remote/<n>` placeholder.
   Tracking-pixel candidates (0–2 px, hidden) are never listed. On "表示する" for one
   message, Fuu (never the renderer) fetches the listed images with no cookies, no
   Referer, a generic User-Agent, at most 3 http(s) redirects, timeouts, an 8 MB cap,
   raster formats confirmed by magic bytes, and refuses loopback, private, link-local
   and CGNAT addresses; fetched images become manifest entries. Permission lasts for
   that message in the current session. No automatic read receipts.
5. **Attachments**: never auto-opened; file names sanitised; executable types
   warned; Mark-of-the-Web / quarantine attribute on save.
6. **Attachment previews** (`fuu-preview`, see below): converted to neutral HTML
   and always passed through the sanitiser (layer 2) and Blitz (layer 3) like a
   mail body.

Blitz never navigates on its own: links are acted on only from a host click, and
every external link still requires explicit user confirmation.

Transport: TLS required for IMAP/SMTP (implicit TLS or enforced STARTTLS);
certificate verification cannot be disabled. Secrets live only in the OS keychain.

OAuth 2.0: authorisation code flow with PKCE (S256) and a random `state`, in the
system browser. The redirect listener binds loopback only (127.0.0.1, and ::1 for
Microsoft's `localhost`), accepts only `GET /` carrying the expected `state`,
never echoes the code, and closes after the first valid redirect or five minutes.
Refresh tokens live only in the OS keychain (chunked across entries to fit the
Windows size limit); access tokens only in memory. Refreshes are serialised per
account and rotated refresh tokens are persisted. The app opens only Google or
Microsoft sign-in URLs from the engine's browser event. No client IDs ship with Fuu.

## Attachment previews

Images, PDF, Word (docx), Excel (xlsx/xlsm) and PowerPoint (pptx) attachments
open in a preview tab when clicked; every other type is only saved (executables
after the warning). SVG is never previewed (scriptable). `.doc`, `.ppt`, `.xls`,
`.xlsb` and `.ods` fall back to saving: their sheet sizes cannot be checked before
a reader allocates them.

Threats: parser bugs and hangs on hostile files (PDF, Office XML, image codecs),
decompression bombs (images, zip), memory exhaustion from huge page or sheet
counts or from sizes declared in the file (an allocation failure aborts the whole
process, which `catch_unwind` cannot stop), external references used for tracking (linked pictures, remote
templates), script or navigation smuggled through the generated HTML.

Defences:

- **One path to the screen.** `fuu-preview` turns the file into HTML with `cid:`
  images plus inline raster resources (a `ParsedMessage`); the app runs the
  sanitiser on it and Blitz shows the result. The converters escape all text, emit
  only allow-listed tags and CSS, and only `http`/`https`/`mailto` link targets;
  the sanitiser would strip anything else anyway.
- **Memory-safe parsers.** PDF is rasterised by hayro (pure Rust,
  `forbid(unsafe_code)`); Office XML is read with quick-xml and zip (deflate only,
  pure Rust); spreadsheets with calamine; images with the `image` crate.
- **Limits.** Input ≤ 64 MB. Images: header checked before decoding, ≤ 50 MP,
  allocation-capped, scaled to ≤ 2400 px and re-encoded (the renderer only decodes
  Fuu's own PNG/JPEG). PDF: first 15 pages, ≤ 6 MP per page. Zip packages: ≤ 4000
  entries, ≤ 48 MB per entry and 256 MB in total, enforced on the bytes actually
  decompressed (not the declared sizes); spreadsheets are decompressed once under
  these limits before calamine reads them. calamine stores a sheet as a dense grid
  of its bounding box and reserves the declared shared-string count, so before it
  runs Fuu measures each sheet's bounding box from the cell references (sheets
  over 2 million cells are skipped with a note) and refuses any `sst` part
  declaring more than 2 million strings. Shown: 12 sheets × 500 rows × 40 columns.
  Word: 6000 paragraphs, 60 pictures. Slides: 60, with 80 pictures.
- **No external references.** Relationships with `TargetMode="External"` and
  `r:link` pictures are never fetched; the preview says how many were dropped.
- **Isolation from the UI.** Conversion runs on a worker thread under
  `catch_unwind`; a panic or error becomes a notice, and a slow file only delays
  its own tab. Nothing is written to disk; previews are kept in memory (6 at most).
- **No actions on the message from a preview tab.** Reply, archive and delete
  buttons and their shortcuts are disabled there; the only action is 保存.
- **Fonts.** PDFs that do not embed CJK fonts fall back to the OS's own Japanese /
  Chinese / Korean font files (read-only, fixed paths).

Tests: per-format unit tests (including a zip bomb, an image header claiming
60000 × 60000 px, a two-cell sheet spanning A1:XFD1048576, an inflated
shared-string count, a docx with a `javascript:` link and an external picture, a pptx
with an external picture), the `preview` fuzz target, and its stable replay
(`crates/fuu-preview/tests/fuzz_replay.rs`), which also checks that no `<script>`
tag or `javascript:` URL survives sanitising.
