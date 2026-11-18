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

Blitz never navigates on its own: links are acted on only from a host click, and
every external link still requires explicit user confirmation.

Transport: TLS required for IMAP/SMTP (implicit TLS or enforced STARTTLS);
certificate verification cannot be disabled. Secrets live only in the OS keychain.
