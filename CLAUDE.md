# Fuu — agent guide

- Use British English spelling in identifiers, comments and docs (`sanitiser`,
  `colour`), except for externally defined names.
- Branches: `main` (release), `develop` (integration). Cut feature/fix branches
  from `develop`; do not commit directly to `main` without approval.
- Respect the engine boundaries in `ARCHITECTURE.md`. Never add a dependency that
  lets `fuu-render` see MIME types or `fuu-parser` see UI types.
- Any change touching HTML display, the sanitiser, the webview or remote content
  must update `SECURITY.md` and its tests.
- Never commit OAuth client IDs, tokens or account data.
- Verify on both Windows and macOS before calling UI work done.
- Plan and milestones: `docs/plan.md`.
- README: `README.md` is English (the main page); each other language has its
  own `README.<lang>.md`. The Japanese text (`README.ja.md`) is the source: edit
  it first, polish it with the yomiyasu skill, then translate it into English
  and every other language. Translations are AI-made and say so, inviting
  corrections.
- UI strings are English in the source (`@tr()` in Slint, `tr!` in Rust);
  translations live in `crates/fuu-app/lang/<lang>/LC_MESSAGES/fuu.po`.
