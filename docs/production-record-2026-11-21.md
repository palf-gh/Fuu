# Production record: Claude handover

## 21 November 2026

- Resumed the GitHub builds, signed updater, localisation and README work from
  the interrupted Claude session. The existing uncommitted changes include the
  translated READMEs, development documentation and an alpha.3 release draft.
- Confirmed that GitHub Actions run 44855584472 succeeded for commit
  `ce5767c5705ed9afeb73def505fdcdbb401d5c12`. This does not validate the
  subsequent uncommitted changes.
- Checked that all nine newly requested non-English UI catalogues have no
  missing strings using `scripts/i18n.py missing`; checked local links in the
  READMEs and development guide; and ran `git diff --check` successfully.
- The user previously said not to release yet. Do not create a release or tag
  until the user changes that instruction. The alpha.3 draft and version changes
  remain uncommitted for review. Verify the final changes and updater end to end
  before a release; the CI result alone does not establish updater operation.

## Conversation expansion

- Added on-demand conversation expansion to the message list. The IMAP engine
  follows Message-ID, In-Reply-To and References across the current folder and
  the recognised Sent folder; each result retains its folder and UID for opening.
- The search does not infer conversations from subjects. Messages in other
  folders cannot bridge a conversation, and Sent cannot be searched if the server
  does not identify that folder. Existing list entries are not deduplicated.
- `cargo test -p fuu-core --lib`: 48 passed, two ignored.
  `cargo test -p fuu-app --bin fuu`: 23 passed, one ignored. A Windows snapshot
  confirmed the expanded layout with example received and sent messages.
- Follow-up: started an isolated GreenMail container and ran the full IMAP/SMTP
  integration suite serially: six passed, including cross-folder conversation
  matching and exclusion of an unrelated message with the same subject. Removed
  the test container afterwards.
- Built the feature on macOS with Rust 1.99 in a temporary clone and inspected
  the conversation snapshot with sample received and sent messages. Windows and
  macOS snapshots verify layout; they do not exercise an interactive live account.

## Alpha.3 release preparation

- The user authorised the release after the conversation checks. Added the
  feature and its folder-search limits to the alpha.3 release notes. The existing
  alpha.2 build has no updater, so installing alpha.3 requires a manual download.
- Merged the release changes through pull requests 1 and 2 after CI passed on
  Windows, macOS and Linux. Tagged `v0.1.0-alpha.3` on `main`; release workflow
  44862140508 built and published all four update targets successfully.
- Downloaded the published assets: all eight SHA-256 entries matched, and each
  of the four update payload signatures verified against the public key embedded
  in the app. The GitHub latest release resolves to `v0.1.0-alpha.3`. An actual
  in-app upgrade was not performed; alpha.2 does not contain the updater.

## CJK font fallback

- Reproduced missing Simplified Chinese glyphs on Windows and macOS. Slint's
  default system font fallback did not include fonts containing those glyphs.
- Registered installed system fonts as fallbacks on Windows and macOS, while
  respecting an existing `SLINT_FONT_PATH`, and selected a region-appropriate
  primary font when the UI language changes.
- Inspected Windows snapshots after switching from English to Simplified Chinese,
  and in Japanese, Traditional Chinese and Korean; inspected macOS before and
  after switching to Simplified Chinese. These checks use sample content and
  system fonts present on the test machines. Machines without a suitable CJK
  font may still need one installed.

## Alpha.4 release preparation

- The user authorised a new release for the CJK font fix. Prepared version
  `0.1.0-alpha.4` and release notes; alpha.3 includes the updater.
- CI passed on Windows, macOS and Linux. Release workflow 44872670935 published
  alpha.4; its Windows installer matched the published checksum and its
  signature verified against the embedded key. The alpha.3 updater detected
  alpha.4, but its download timed out twice on this slow connection.

## Update download timeout

- The updater library carries the metadata check's timeout into the payload
  request; the 30-second limit applied to the entire installer download. Kept
  the short metadata timeout and allowed 30 minutes for the signed payload.
- Alpha.4 still contains the original timeout. Users affected by the slow
  connection must install a fixed version manually before it can update itself
  with the longer download allowance.
- Retested against the published alpha.4 Windows payload with a 30-second
  metadata timeout and 30-minute payload timeout. The update was detected,
  downloaded over the slow connection and verified successfully. Preparing
  alpha.5 to deliver the fix.
