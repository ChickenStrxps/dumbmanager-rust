# Original UI native backend

This document records the compatibility core baseline. Later native service
implementations intercept the external routes described here; use the
[current inventory](migration-inventory.md) and [capability audit](capability-inventory.md)
for the integrated app. The baseline refusals below do not describe those services.

The bundled original HTML/CSS/JS uses `native-transport.js` to turn same-origin
`POST /api/*` requests into `invoke('legacy_api', {path, payload})`. The Rust
command returns the original result object directly. A rejection is
`{error: string, code: string, status: number}`; the transport creates a failed
JSON Response so the original `post()` shows the error and restores capture
text. Domain errors must not be marked as network failures or placed in the
offline outbox.

This implementation has no Python bridge, HTTP listener, external API client,
token discovery, arbitrary command runner, or caller-selected file access.
All persistence is in the existing Rust app data directory and stamped SQLite
database. No original DumbManager/DumbAPI runtime, records or credentials are
read or migrated. Reading the original source was solely for its contracts.

## Implemented contracts

- `manager.state`, `status`, `list`, `get`: genuine empty state, original
  snake_case record fields, newest-write ordering, search, project/state filters
  and pagination. State includes up to 500 itches and 80 contexts; list searches
  the most recent 2,000 itches and returns at most 100 per page. Status counts
  count actual records. `manager.export` returns all local itch/context records
  as JSON, without settings, credentials or external chat history.
- `manager.capture`: title/project/original text, moment, observed event time,
  explicit/selected local context, historical context snapshot and retry key.
  A moment without words has state `needs_description`; adding its title moves
  it to `open`. Blank ordinary capture fails. Capture titles allow 500 Unicode
  characters; later titles allow 2,000, including line breaks. No truncation.
- `manager.update`: title, desired_outcome, next_step, test_url, state, plan,
  feedback, recheck, run_state and evidence. Stable plan IDs and arbitrary
  bounded evidence/feedback objects persist. Missing fields retain their values.
  Unknown fields and invalid values fail the complete transaction. Stale
  expected_version returns `409 conflict`, without changing the itch.
- `manager.complete`: requires `accepted: true` and a nonempty summary. Saves
  real summary/evidence/acceptance/time/elsewhere fields, then resolves. Marking
  ready, completing a plan or saving agent-reported status does not accept.
  A described open itch may be explicitly accepted, matching the original API;
  no intermediate ready event is fabricated. Moments still need words first.
- `manager.repeat`: adds an actual occurrence, reopens and clears recheck/run
  state, resets plan checkboxes and keeps earlier outcomes and evidence.
- `manager.settings`: original boolean motion preference, persisted in native
  settings. False maps to off; native reduced/off both report false. Other
  native settings are retained. Theme and page/navigation restoration remain
  the original frontend's independent WebView storage behavior.
- `manager.context.publish`, `context.select`, `context`: local, user-supplied
  task/testing/entities/workspace/candidates and exact thread-ID notes. Upserting
  the same project/thread increments its real version. Publication selects only
  when explicitly requested. Capture preserves an earlier supplied snapshot
  after newer task notes are published. Saving these notes does not verify,
  create or connect a project/T3 thread.
- `manager.brief`: prepares copyable local text without sending or starting
  work. It explains that original DumbAPI cannot update the isolated Rust itch.
- `manager.suggest`: saved local prefix matches from scoped titles/context
  candidates (and supplied chat candidates). Returns the original items shape;
  generation explicitly says unavailable. No AI generation, Jev ranking or
  external thread-content lookup is implied.
- Local extension `manager.history`: most recent 500 real compatibility audit
  events. Local extension `manager.card_export {id, format: 'png'|'gif'}` uses
  the existing verified in-process card renderer and returns
  `{path,width,height,frames,bytes}` in the native exports directory. Existing
  `card_export` IPC also works for original-UI captures. Neither operation
  accepts an itch. These export APIs are backend capabilities; the original
  Social draft/share workflow has not been ported to them.

Write retry keys and their exact canonical inputs/results commit with records,
versions, native projection and audit events in one `BEGIN IMMEDIATE`
transaction. An identical retry returns its original result after reload;
reusing a key for different input returns a conflict. Failed writes leave no
receipt, so the same key can be retried after fixing the failure.

Inputs are allowlisted and bounded: 64 KiB per request, 24,000 bytes of changes,
22,000 bytes per context snapshot and 128 KiB per full itch. Existing plan,
feedback, evidence, candidate and occurrence limits are enforced. Exhausted
history/size limits fail explicitly rather than dropping earlier input.

## Native schema and status bridge

Schema version 2 adds `legacy_itches`, `legacy_contexts`, `legacy_events` and
`legacy_requests`. The exact original record is stored with an FK to the first
milestone core itch; writes update both representations atomically. Existing
Rust records, native histories and settings survive migration. Each migrated
record receives a `native_snapshot` version-1 baseline, not invented prior
versions, occurrences, outcomes or agent reports. Existing core description/kind
remain in their original columns and are exposed as description/native_kind in
the compatibility snapshot.

Original `open` means **stashed**. Original open, needs_description and archived
project to core stashed; working projects to core open; ready and resolved map
directly. The original record remains authoritative for its exact status.
First-milestone core open/stashed both appear as original open. Native title
edits preserve archived/working/moment details; actual native transitions
update compatibility state/version without inventing an external run state.
Core counts have coarse buckets and must not be used as original state counts.
Original captures have no kind; the core's required kind uses an internal idea
storage default, without claiming an observed classification in the original
record. New capture IDs use the core's UUID spelling; UI code must treat IDs as
opaque strings.

## Explicitly unavailable user flows

T3 connection/pairing/distributions/sends/chat, external projects, global
shortcuts/snipping/helper control, replay recording, file/image/video uploads,
attachments, external preview tickets, external jobs, Git/browser tools,
analytics, anime batches, scouting and scout suggestion review, research,
learning, inspiration services, AI/voice/typing-provider settings, social drafts,
social image/GIF sharing/clipboard, posting checks, webhook/Discord delivery and
DumbBridge have not been ported. Their writes return `501 unavailable`; no
external state is changed and no fake success, project, thread or receipt is
created. A capture with nonempty attachment IDs fails before saving any text,
so unsupported receipts are not silently discarded.

Optional original init/status reads are truthful: projects and T3 thread/
distribution lists return empty unavailable/not-connected shapes, native helper
status says running=false, replay status says armed=false with an unavailable
explanation, jobs list is empty with available=false. Unknown paths return 404;
unknown capabilities return 501; malformed/unsupported inputs return 400.
Errors stay short and never echo unbounded caller input.

The command is declared in build.rs, generate_handler and main's local-only
allow-legacy-api ACL. Remote origins gain no access. No CSP change was made by
this backend task. The parent should review the original frontend's dynamic
style attributes/style elements and blob previews/downloads against its CSP;
changing that policy remains a parent-owned integration decision.

## Verification

The original UI contract suite, now removed, exercised actual original init/capture/update/
review/settings/context shapes, complete field preservation, reload, state
bridging, idempotence, real conflict/transaction failures, migration, concurrent
retry and unsupported/unknown capability refusal. Native state tests exercise
the same command service and an actual PNG export from an original capture.
All tests create temporary state. The original UI's page restoration still
requires the parent's end-to-end WebView QA; no claim of full feature parity
or visual equivalence follows from headless contract tests.

The historical checks below were performed before the development suites were
removed. Current build validation uses Cargo check/Clippy and JavaScript syntax
checks; the previous QA runners are no longer included. See the migration
inventory for the current service scope.
