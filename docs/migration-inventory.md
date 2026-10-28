# Native migration inventory

This is an implementation inventory, not a claim of complete parity. The Rust
app has its own identity, SQLite database, WebView profile and credentials.
Existing DumbManager, DumbAPI, T3 and Knife installations remain independent.
No listener, global shortcut or capture starts automatically.

## Implemented

- Original frontend layout, fonts, dialogs, navigation and ASCII background.
- SQLite capture, moments, edits, stashing, explicit acceptance, history,
  transactionally merged duplicates, Unicode search, pagination and retry keys.
- App-owned image/video uploads, original-byte retrieval and configurable
  FFmpeg/FFprobe frameboards. Windows OCR, crop/mask flattening and sanitized
  work images feed the original card flow.
- Social drafts, original frozen WebView GIF artwork, deterministic still cards,
  exact-preview GIF selection, explicit clipboard handoff and webhook receipts.
- Explicit T3 pairing, registered projects, bounded thread reads, previews,
  attachments, chat sends and itch/research handoffs with retry-safe receipts.
- Git operations on explicitly registered roots; configured isolated Chromium
  Browse sessions; Mercury/Jev settings and structured judgments; research,
  learning, source-bounded scouting and selected inspiration sources.
- OpenAI/Gemini session provisioning, scoped tool approvals and a stdio MCP
  console. Unconfigured integrations remain unconfigured.
- Opt-in phone HTTP listener with one-time invitations, HttpOnly sessions,
  CSRF/Host/Origin checks and a restricted itch/chat capability surface.
- Explicitly enabled global shortcuts, screen-region snipping into local evidence,
  and user-facing shortcut diagnostics. The new selector passes compiler checks;
  its interrupted UI check still needs to be completed.

## Still required before full replacement

- Selected-window replay capture and final snipping UI verification.
- Signed Discord interaction routing and its end-to-end message workflows.
- Complete learning-topic parity. The original public RSS and TypeSafe blog
  adapters are now implemented; captured X still needs a configured source.
- GitHub pull-request flow and any original capability not in the native registry.
- Final native UI, installer, microphone, clipboard-paste and real FFmpeg checks.
- Independent review and user visual review of the integrated release candidate.

Configured adapters have primarily been checked with isolated local fixtures;
that does not establish live provider, T3, Discord, phone or proxy compatibility.
No production message, webhook, account adoption or deployment was used for QA.

## Verification and repository contents

The integrated service suite passed 67 checks before removal. The renderer
passed nine checks plus a separately invoked real Windows OCR check. Eight
loopback HTTP checks exercised pairing, access boundaries, capture/retry and
upload against a temporary native app. Compile-time checks still pass after
removal. Earlier core checks covered persistence, concurrency and original API
contracts. These results apply to their recorded scope, not every later change.

At the owner's request, development test suites, smoke scripts, benchmarks,
fixture runners and QA-only executable entries are no longer in the repository.
App artwork, build files and mandatory dependency licenses remain.
