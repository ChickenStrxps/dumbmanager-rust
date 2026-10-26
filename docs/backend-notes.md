# Native backend implementation

This milestone implements a real local Rust backend and a Windows Tauri 2 shell.
It is a separate application, not a wrapper around the Python installation. See
`ipc-contract.md`, `renderer-contract.md`, `isolation.md`, and the candid feature
inventory in `migration-inventory.md`.

## Storage and commands

`src-tauri/core` is a headless crate with no Tauri or renderer dependency. It owns
validation, SQLite storage, status rules, settings, export validation, and the
bounded export gate. SQLite is bundled through rusqlite; runtime does not require
a Python process, a SQLite installation, a server, or another project.

`src-tauri/src/state.rs` implements the native command service. The eight Tauri
commands in `src/commands.rs` call that service through `spawn_blocking`, keeping
SQLite, rendering, and filesystem work off the UI thread. Rejected IPC promises
carry readable strings. Input objects reject unknown fields. Results use the
camelCase shapes in the contract. The console QA example calls the same native
service without starting Tauri or WebView2.

The desktop resolves its data directory only through Tauri's app-data resolver
for `io.dumbmanager.rust`, and checks that identifier before opening storage.
The database is `dumbmanager-rust.sqlite3`, exports go directly into `exports/`,
and the WebView2 profile is in this app's local-data directory under `webview/`.
The desktop executable rejects command-line arguments, including data-directory
overrides. It never discovers or imports another installation's configuration.

Database schema version 1 uses SQLite application ID `0x444D5253` (`DMRS`). A
nonempty database without that ID, a database with another application ID, or a
newer schema is refused. Ownership/version reads use one SQLite snapshot to
avoid inconsistent reads during concurrent first initialization. Transient
startup locks have a bounded retry window and SQLite has a five-second busy
timeout. Linked database files and linked data/export directories are refused.
The tests use synthetic foreign files; no real legacy database was opened.

Writes use `BEGIN IMMEDIATE` transactions, WAL, foreign keys, and
`synchronous=FULL`. Itch mutations and their history entries commit together.
All settings fields commit together; tests inject a real SQLite failure after
an earlier field update and verify the entire save rolls back after reopening.
A clean exit attempts a WAL checkpoint. Do not copy a live database file alone
as a backup; close the app first so its WAL is folded into the main file.

New itches are open. Open, Stashed, and Ready can move between each other.
Only an explicit transition from Ready can resolve an itch; the frontend must
send that from its acceptance button. A resolved itch can only reopen to Open.
Editing, exporting, loading, restarting, and saving settings never accept an
itch. Identical edits and same-status transitions are no-ops. A local history is
stored, but there is no history IPC command in this milestone. Headless
`Store::delete_itch` performs explicit transactional deletion with cascading
history cleanup; no desktop IPC or background action exposes or calls it. New create
requests receive fresh UUIDs; creation is not an idempotent retry API.

Titles are required and limited to 200 Unicode scalar values; descriptions to
8,000, projects to 64, and search queries to 200. Unsupported control and bidi
override characters are rejected. Descriptions normalize newlines. New itches
use the saved default project, or `unsorted` when no project is supplied.
`all` is reserved for the project filter. Settings support full/reduced/off
motion and the midnight theme, with an optional default project.

Search lowercases Unicode text and requires every whitespace-separated term in
the combined title, description, project, and kind. SQL parameters handle exact
status/project filters; SQL wildcard characters in search remain literal.
Results sort by an increasing write revision, including same-millisecond writes.
Text search currently scans the filtered records in Rust, with no FTS index,
pagination, or demonstrated large-dataset performance guarantee. Concurrent
edits use the last committed write; there is no optimistic edit-conflict UI.
There is no delete IPC command, automatic agent dispatch, or background worker.

## Real native exports

`src/card.rs` maps title, description, project, status, and ID into the real
`crates/card-renderer` API. The itch is copied out of SQLite before rendering;
the database mutex is released during the slow operation. A two-permit atomic
semaphore admits at most two simultaneous exports. Further requests get a Busy
error and can be retried; there is no unbounded render queue.

PNG is 2400 by 1350 with one frame. GIF is 1200 by 675, 72 frames at 50 ms each,
looping infinitely over 3.6 seconds. Rendering runs in-process with embedded
bitmap fonts and fixed bounds. Font coverage is limited to the renderer's
ASCII/Latin support; full Unicode typography is not implemented.

The renderer uses unique create-new filenames and cleans up reported failures.
The backend checks that the reported output is a plain direct child of its
fixed export folder, has the requested extension/signature, plausible bounds,
and nonzero actual bytes below 64 MiB. Size comes from the filesystem. No caller
can choose an export destination. The core helper for publishing already-drawn
bytes uses an atomic, non-overwriting same-directory hard link; the production
renderer currently streams to its unique final filename. A process killed
mid-export can leave an unfinished file. Image export is not a transaction with
the database. Export never posts or uploads anything.

`open_export_folder` and the tray menu open only the validated fixed export
folder through Windows ShellExecute. The QA harness does not invoke that shell
action; the parent must verify Explorer and the tray in the real desktop app.

## Desktop boundary

The product/window name is DumbManager Rust and the executable is
`dumbmanager-rust.exe`. Tauri is configured for an independent current-user NSIS
installer, product identity, icons, and shortcut. The tray has its own identifier
and icon. Single-instance names derive from `io.dumbmanager.rust`; a second
launch only reveals the existing window. No global hotkeys, startup entries,
services, network listeners, or unrelated app changes are installed by startup.

Only the local `main` window receives the eight generated app-command
permissions. No generic filesystem, shell, opener, core, or single-instance
plugin command permissions are granted to the page. Remote navigation and new
windows are denied. CSP allows bundled resources and Tauri IPC, with no remote
script sources, frames, or asset-protocol filesystem access. The config has no
development server URL. Navigation tests cover remote schemes, credentials,
lookalike domains, ports, and development-origin isolation.

## Validation and handoff

Executed with Rust/Cargo 1.95.0 on Windows x64:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -p dumbmanager-core -p dumbmanager-rust --all-targets --locked
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --locked
cargo clippy --manifest-path src-tauri/Cargo.toml -p dumbmanager-core -p dumbmanager-rust --all-targets --locked -- -D warnings
cargo build --manifest-path src-tauri/Cargo.toml --bin dumbmanager-rust --locked
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo fmt --manifest-path src-tauri/core/Cargo.toml -- --check
```

There are 76 headless core tests and three desktop tests (79 total), covering
restart, CRUD inputs/edits, statuses, history, filtering/search, serialization,
atomic settings rollback, separate stores, foreign/invalid/newer databases,
export validation, real concurrent connections, concurrent first startup, and
concurrent file/folder creation. The repaired first-start race was additionally
checked in six consecutive targeted runs. Core-only development can use
`cargo test --manifest-path src-tauri/Cargo.toml -p dumbmanager-core --locked`.
The core and desktop are separate Cargo packages without a workspace, so run
their formatting checks separately; do not format the parent-owned renderer as
a side effect of backend work.

For deterministic native-service QA with actual image output:

```powershell
$qaRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('dumbmanager-rust-check-' + [guid]::NewGuid().ToString('N'))
cargo run --manifest-path src-tauri/Cargo.toml --example native-check --locked -- "$qaRoot"
```

The root must not exist and must be a direct child of the OS temporary folder.
The harness refuses other roots before opening storage. It leaves only its new
root, synthetic test records, two real exports, and `native-check-report.json`
for review. It checks fresh empty state, honest integrations, settings
validation/atomicity, create/edit/review/search/filters, restart, separate-store
isolation, format rejection, the two-slot export limit, and real renderer output.
It never resolves even its own synthetic itch. A successful run produced an
18,011-byte PNG and a 1,090,280-byte GIF; bytes vary with the fresh itch ID.

The Windows debug executable compiled successfully at
`src-tauri/target/debug/dumbmanager-rust.exe`. Packaging, installer execution,
WebView IPC delivery, actual desktop interactions, Explorer/tray behavior,
installed shortcuts, and whole-process performance remain parent verification.
No claim of 1:1 feature parity or full desktop acceptance is made here.

## Development-only native UI QA handoff

The opt-in `native-ui-qa` Cargo example opens the actual desktop frontend with
the same generated context, eight-command IPC handler, window/navigation policy,
CSP, capabilities, tray actions, renderer, and exit checkpoint as production.
It starts empty and does not automate or fabricate any command responses. Launch
it manually from the repository root, without arguments:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --example native-ui-qa --features desktop-qa --locked
```

The built artifact is `src-tauri/target/debug/examples/native-ui-qa.exe`.
Its visible window title and tray labels say
`DumbManager Rust [TEMP UI QA - DEVELOPMENT ONLY]`; its Windows AppUserModelID
and single-instance identifier are `io.dumbmanager.rust.ui-qa`. The example
requires the opt-in `desktop-qa` feature, which defaults off. Both its Cargo
build script and a non-debug compile guard reject release use. The normal
`dumbmanager-rust.exe` always follows the original AppId-checked startup path
and continues to reject arguments; no data-directory argument or environment
override was added.

Only after single-instance detection does the QA example generate a fresh
`dumbmanager-rust-ui-qa-<pid>-<timestamp>` direct child of the OS temporary
directory. It reuses the existing atomic create-and-refuse-existing guard.
SQLite and exports live in that root's `data/` and `data/exports/`; the WebView
profile is in its separate `webview/`. It prints the root to its console and
writes `TEMP-UI-QA.txt` there. All records must be entered manually as synthetic
input. Closing the window leaves this root for inspection. A later fresh launch
creates a different root; this tool deliberately has no reuse-path option.
No normal application store or other installation is opened by this example.

Validation on Rust 1.95.0 Windows x64 passed all 80 core/desktop tests with
`--all-targets --features desktop-qa --locked`, default-feature all-target
`cargo check`, Clippy with the QA feature and `-D warnings`, formatting for both
backend packages, and builds of the normal debug binary and QA example. The new
test checks distinct, fresh direct-temp roots, empty separated data/WebView
folders, and refusal to adopt an existing root; the shared guard's existing
outside-temp and relative-path refusal tests also pass. Compiling with the QA
feature and debug assertions disabled was rejected with the intended error.
The compiled Cargo build script was separately exercised with a release profile
and rejected it before invoking tauri-build.

The app package now declares Rust 1.87 to match its renderer dependency; this
round did not install or test that minimum toolchain. The bundle resource map
includes root `LICENSE` and `THIRD_PARTY_NOTICES.txt` under those exact names.
Their debug resource copies matched the source files by SHA-256. Installer
generation/execution remains unverified.

No desktop was launched in this implementation round. Positive save/edit/status
and PNG/GIF round trips through real WebView IPC, form/review cancellation, and
Explorer/tray behavior still require the parent's manual Computer Use pass.
Headless checks and a compiled QA example do not establish those outcomes.
Rebuild the QA example after frontend/asset changes so its embedded page is
current. The previously repaired first-start ownership/WAL behavior, honest
integration states, and explicitly incomplete migration parity remain unchanged.

The first manual QA launch found `STATUS_ENTRYPOINT_NOT_FOUND` before Rust
startup. The production executable had its Common Controls v6 manifest, while
Cargo examples had no resources. The parent linked Tauri's generated Windows
resource library into the opt-in MSVC examples as well. This preserves the same
manifest and icons without adding a different runtime manifest or weakening any
app guard. A compiled example alone had not revealed that Windows loader issue.

## Parent integration verification

The original frontend is now connected through `legacy_api`; the current
contract and migration limits are in `original-ui-backend.md`. The expanded
backend suite passed 100 tests, including the original UI contract, concurrent
retries, rollback, migration and real PNG export. Check, Clippy and formatting
passed. The frontend syntax check covered 24 JavaScript files.

The restored production frontend and IPC were exercised in the opt-in native
QA window against fresh temporary state. Capture, editing, stable plan IDs,
literal text rendering, cancellation of acceptance, reload navigation and the
persisted motion preference passed. The normal release was then packaged and
installed in the Rust edition's own directory. Its original interface and
empty independent profile were inspected. See `visual-review.md` for the
package comparison and remaining external-service limitations.
