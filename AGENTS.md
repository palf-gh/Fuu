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
