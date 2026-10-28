ï»¿# AI service integration handoff

This remains a partial port. Owned files are `src-tauri/src/services/ai.rs`, `src-tauri/src/services/ai/`, and `web/ai-native-settings.js`. No original install, credentials, runtime, or process was used. No external provider or production T3 request was made in QA.

## Native integration

The native dispatcher and frontend settings are wired. Research freezes the
configured T3 destination; a supplied target must match. Core duplicate merging
is transactional and preserves both records, occurrence provenance and retry
identity. The public feed adapters use bounded XML/HTML parsing as described
below. No original credentials or captured X accounts are inherited.

An external T3 research/scout worker still needs an explicitly authorized MCP
connection to record reports or bounded source evidence. A prompt or a local
capability name alone does not grant that access.

## Behavior implemented in this pass

- Explicit Mercury Suggest uses typed text and bounded, selected locally saved task/testing/entities only when the saved context matches project and thread. Credential-shaped fields are omitted. No project source, history or files are sent. Both Mercury and Jev need an explicitly successful saved test before generation, splitting, ranking or duplicate judgments. Settings use separate DPAPI secrets; configured and tested remain distinct, and a prior test does not assert current availability.
- Explicit inspiration refresh has a fixed public Show HN adapter and the original six curated Field guide links, plus user-supplied links. Results use actual URLs and source timestamps; malformed/credential URLs are excluded. Network uses no proxy or redirects, fixed URL, 10-second timeout and 900 KB cap. Failures retain prior results and identify the unavailable source; a persisted lease and request key control concurrent/retried refresh, with 15-minute success/one-minute failure cooldown. Optional Jev ranking sends up to eight public preview titles and saved interests and sorts by actual validated judgments; unranked/unavailable results stay truthful. RSS sources (Hugging Face, OpenAI, DeepMind, FreeCAD, Hackaday, MIT), TypeSafe blog and captured X remain absent; coverage is explicitly partial. No live feed or model was contacted in QA.
- Duplicate detection builds local candidate pairs, and, when Jev is configured and explicitly tested, asks it to distinguish the same specific task from related topics. Unavailable Jev falls back to strong local matches; results require review and never merge themselves. At most eight candidate pairs are sent to Jev, and 500 local records are inspected.
- Learning trails include saved research excerpts with exact source IDs, links/paths and unverified provenance. Notes must cite a non-sensitive exact excerpt, completed plan step or reported command. Existing stale-revision protection still applies; no sources, commands or tests are run by learning.
- `manager.research.send` freezes run ID, exact thread, T3 installation target, request key, brief and stable transport identity before dispatch. It writes `send_unconfirmed` before attempting delivery; a retry calls T3 with the same durable key, allowing receipt repair without issuing a new command identity. It persists the T3 receipt and returns the UI-shaped research pack. Scope/version checks protect initial preparation; a saved send cannot be redirected. Receipt alone does not claim a report. As the itch and runtime document live in separate stores, the scope check cannot be made fully atomic with a concurrent itch edit without shared core support.
- Manual scout plans index only filename metadata under an explicitly registered project root, with 10,000 visited entries, 2,000 source files and depth 12 caps. Sensitive/hidden directories, sensitive names and symlinks are omitted; `manager.scout.search` searches the saved index, not contents. Index may be partial or stale. Reads enforce six calls and 300 requested lines per run, bounded file and response sizes, source-path checks, credential-line omission, and source evidence. Proposal line locations and quotes must match the saved read. Known itch titles appear in the plan. File validation and opening are separate OS operations, so this does not claim protection against a malicious local actor racing filesystem replacement; a handle-based no-follow implementation would be needed for that threat model.

## Verification and limitations

Before removal, the AI service suite passed 15 checks, including a loopback T3 WebSocket fixture that drops a response then checks stable command/message IDs and receipt recovery, isolated state, docs source, real URL parsing, ranking and duplicate judgment, indexed discovery, evidence location and 6/300 budget. `node --check web/ai-native-settings.js` passed. A transient shared-file compile error was resolved and the current rerun passed. The development test suite has since been removed from this repository at the owner's request. Those checks did not contact a live provider, send to production T3, use the original runtime or establish full UI/release parity. The newer feed and Learning checks are recorded below.

## Public inspiration sources

The native service now supports the original TypeSafe blog, Hugging Face, OpenAI,
DeepMind, FreeCAD, Hackaday CAD and MIT Health feeds as well as Show HN and the
field guide. Sources are opt-in, fixed HTTPS destinations with bounded responses,
no redirects and per-source cooldowns. Feed content is plain text, links are
validated, and source previews remain unverified. Seven external fixture checks
passed for RSS/Atom, HTML extraction, deduplication, unsafe links, malformed input
and size bounds. An explicit Hugging Face request returned 18 bounded items.
The remaining endpoints have not been exercised live. Test helpers remain
outside the repository.

## Learning trails

The native content engine retains original topic guides for unittest, Node,
Playwright, SQLite, GIFs and retry identity. A topic appears only beside a literal
mention in saved work, commands or research; a request or capture context cannot
establish that a tool was used. Desired outcomes, reported workspaces and
environments, completed steps and accepted outcomes retain source links.

Only explicit command-log entries become commands. Saved learning notes cite an
exact non-sensitive report excerpt. New work invalidates stale notes and reading
progress; acceptance alone preserves notes for the same work. Opening a trail
runs no commands, model requests or environment scan.

Twelve external fixture scenario groups passed for these content boundaries,
source links, all six guides, context/provenance, sensitive text, eligibility and
revision/progress behavior. Strict native Clippy passed. No new test suite or
fixture runner is shipped in this repository.
