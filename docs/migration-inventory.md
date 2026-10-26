# Migration inventory: first native milestone

This is an implementation inventory, not a claim of 1:1 parity. The native
backend does not open, copy, change, or contact the existing Python projects,
accounts, settings, tokens, hotkeys, ports, startup state, or runtime records.
Backend validation uses newly created temporary state.
The original frontend is now bundled with its existing layout, fonts, navigation,
dialogs and full-page ASCII creatures. Its local capture and review handlers
were followed into the original API contracts and implemented in Rust. This is
not a full audit or port of every service. See the precise
[compatibility inventory](original-ui-backend.md).

## Implemented in Rust

- Independent Tauri 2 Windows shell with DumbManager Rust identity,
  `io.dumbmanager.rust`, `dumbmanager-rust.exe`, an owned WebView2 profile,
  independent tray/single-instance identity, and NSIS packaging configuration.
- Fresh, empty native SQLite storage. No seeded runtime records or imported
  credentials. Own application ID/schema, WAL durability, atomic writes, foreign
  database refusal, and temporary-state tests.
- Itch create/edit/read/list; bug/improvement/idea kinds; project/default-project
  handling; literal case-insensitive text search; exact project/status filters;
  newest-update ordering; and genuine counts for Open/Stashed/Ready/Resolved.
- Explicit review/status workflow. The smaller native API requires Ready before
  Resolved. The original compatibility API also permits explicit acceptance of
  a described open itch, matching the original application. Editing,
  rendering, reload, restart, and settings saves do not accept work. Resolved
  items can reopen to Open. Local audit events persist with each mutation.
- Explicit headless transactional deletion with cascading local history cleanup.
  Deletion is not exposed through the first milestone's desktop IPC contract.
- Settings persistence for full/reduced/off motion, midnight theme, and default
  project. Unknown fields and invalid values are rejected; multi-field saves
  are atomic even when SQLite fails partway through the transaction.
- Real in-process GIF/PNG card exports through the native renderer, two-slot
  concurrency bound, fixed export folder, file validation, fresh filenames,
  and no network/posting action.
- Fixed-folder Explorer command and tray action, configured for desktop QA.
- Local-only IPC capabilities, CSP, blocked remote navigation/popups, and no
  browser page access to arbitrary filesystem/shell operations.
- Original-contract capture and moments, full detail/edit fields, four-step
  plans with stable IDs, feedback, evidence notes, version conflicts, atomic
  retries and real mutation history. No earlier history is invented during the
  schema-v2 upgrade.
- User-authored local context notes and preserved historical capture snapshots;
  copyable local briefs and prefix suggestions from saved titles. These do not
  connect or verify a T3 destination, discover projects, or invoke a provider.
- Original boolean motion setting and original WebView page/filter/dialog
  restoration. Native WebView checks verified capture, edit, plan persistence,
  review cancellation and return to the same open itch after reload.
- Headless core tests and a console native-service QA executable using a new
  OS-temp child. It exercises persistence, filtering, review and real export
  without opening normal app data or a WebView.

## Explicit gaps

- **T3 threads and agents:** no live thread discovery, context publication,
  selection, dispatch, implementation rounds, review orchestration, or agent
  sends. No inferred connection to an existing T3 session.
- **Git and pull requests:** no checkout/worktree discovery, GitHub accounts,
  branch management, commit/push, PR creation/review/merge, or automatic status
  synchronization.
- **Browser automation:** the local WebView displays the UI. It is not an
  automation connection to another browser or the original app's browser
  profile. No remote page controls or browser-task migration is implemented.
- **Jev / TypeSafe:** no configured model endpoint, credentials, structured
  judgments, verification, or routing. No model success is simulated.
- **Mercury / Inception:** no connection, streaming, model selection, inference,
  or migrated provider configuration.
- **Discord / DumbBridge / webhooks:** no account, token, webhook destination,
  board synchronization, message send, image post, or Discord bot. Export only
  creates a local file.
- **Scout:** no automated discovery, background scouting, ingestion, deduping,
  polling, or generated records.
- **Phone companion:** no listener, pairing URL/QR, invitation, remote actions,
  mobile share state, inherited port, or network token.
- **Voice:** no microphone permission, capture, transcription, voice hotkey,
  model connection, or automatic submission.
- **AnalyticsOne and metrics integrations:** no live account/service stats or
  historical analytics migration. Local counts describe this app's own stored
  itches only.
- **Data migration:** no import/merge/export of the Python database, account
  settings, secrets, history, attachments, runtime jobs, or legacy card cache.
  Existing data remains in its existing installation.
- **Additional domain surfaces:** no delete IPC, attachment store, reminders,
  scheduling, multi-user sync, cloud backup, exposed history browser or full-text
  index. Original state reads include the latest 500 itches and 80 contexts;
  list pagination searches the most recent 2,000 itches, with 100 per page.
- **Renderer coverage:** no full Unicode typography or runtime system-font
  loading. Native artwork is an independent visual implementation; visual or
  pixel parity with legacy cards is not claimed.
- **Original Social flow:** native GIF/PNG export exists through IPC and the
  local manager.card_export extension. The original Social draft, preview,
  sharing and clipboard service has not been connected to those exports.
- **Desktop acceptance:** original-interface WebView capture, edit, reload and
  review cancellation have been checked. The independent installer, shortcuts,
  title/taskbar icon and Explorer opening were checked in the earlier shell.
  Installer rebuild/upgrade with the restored frontend, tray interaction,
  uninstall, broad accessibility and whole-app performance remain separate
  checks. Compilation alone does not establish them.

The backend reports migrated native card export as available and every listed
external integration as unavailable. There are no pretend connections,
simulated agents, fabricated live jobs, or success responses for these gaps.
Future integrations need explicit configuration, narrow permissions, real
implementation and tests before their runtime state may become available.
