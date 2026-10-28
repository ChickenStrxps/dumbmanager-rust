# Native backend

The Tauri shell uses the original bundled frontend through `legacy_api`. JSON
results and domain errors preserve the original HTTP-shaped contract. Binary
uploads and card renders have separate bounded envelopes. The phone listener
uses an authenticated subset of this contract rather than exposing native IPC.

`src-tauri/core` owns SQLite transactions, version checks, retry keys and review
rules. `src-tauri/src/services` owns explicitly configured external integrations.
Secrets use current-user Windows DPAPI in this installation's service directory.

The desktop and MCP console use the same application identifier and owned data
folder. The console accepts only `--profile mcp` or `--profile dots`. Neither
entry point accepts a caller-supplied data-directory override.

Build with Rust 1.88 or newer. The image dependency requires this minimum.
Tauri embeds the Windows resource manifest and the accepted DM icon in binaries.

The original isolated verification covered real SQLite transactions, retry and
migration behavior, image/GIF decoding, provider fixtures and native WebView
round trips. Development runners, fixture sources and test-only entry points
were removed from this repository after verification at the owner's request.
Use `cargo check`, `cargo clippy --lib --bins -- -D warnings` and the Windows
build workflow for compile-time checks. See the migration inventory for remaining
product and live-service gaps; historical checks do not establish full parity.
