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
- Signed Discord intake, bounded attachments, scoped edits and durable stash,
  explicit destinations, Mercury/Jev assistance, retained research, confirmed
  T3 handoffs and original ASCII cards. See `services-discord.md` for limits.
- Explicitly enabled global shortcuts, screen-region snipping into local evidence,
  identity-bound selected-window replay, and user-facing shortcut diagnostics.
  Disposable real Windows fixtures verified occluded-window frames, close/disarm,
  stale selection rejection and a completed region snip.

## Still required before full replacement

- Integrated Discord HTTP routing and live registration/delivery verification.
- Captured X still needs an explicitly configured source; the original public
  RSS/TypeSafe feeds and evidence-linked Learning topic guides are implemented.
- Literal frontend capability routing is reconciled in `capability-inventory.md`;
  that source audit does not establish every original runtime behavior.
- Native capture/edit/evidence/review/reload checks passed in a hidden WebView.
  Final installer inspection, live microphone and clipboard-paste checks remain.
  Real FFmpeg conversion passed with synthetic video.
- Independent review and user visual review of the integrated release candidate.

Configured adapters have primarily been checked with isolated local fixtures;
that does not establish live provider, T3, Discord, phone or proxy compatibility.
No production message, webhook, account adoption or deployment was used for QA.

The original Git panel has changes, history, branches, remotes and stash controls;
it has no pull-request controls. A new GitHub pull-request interface is not a
requirement of this 1:1 migration. See [release-checklist.md](release-checklist.md)
for the remaining completion gates and recorded verification limits.

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
