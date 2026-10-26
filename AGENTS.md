# DumbManager Rust

This is a new, isolated Rust/Tauri Windows application. Never change the existing
DumbManager or DumbAPI installation, source, data, credentials, runtime records,
launch shortcuts, or GitHub repository. Reuse their frontend source and original
artwork where needed for the authorized 1:1 port; never copy runtime state or keys.
This repository stays private.

Use `io.dumbmanager.rust` as the application identifier and `DumbManager Rust` as
the visible name. Store data under the Tauri application data directory, never
under the original application's directory. No listeners or startup tasks by
default. External services use explicitly configured connections; do not discover
or inherit tokens from another installation. Tests use temporary state.

Keep Rust domain logic independent of Tauri. Keep the web frontend plain HTML,
CSS and JavaScript. Real operations only: no demo success, fabricated itches or
fake service states. Explain migration gaps plainly. No secret or personal data
in source, tests, assets, logs, or commits. Preserve mandatory licenses.

Preserve the original DumbManager interface. Its existing HTML, CSS, fonts,
navigation, capture flow, dialogs and full-page ASCII animations are the visual
source of truth. The user rejected the replacement dashboard. Do not redesign,
simplify, rename or rearrange the original interface during the Rust migration.
Native transport and truthful unavailable states must fit the existing UI.

Respect reduced motion, stop animation while hidden, cap canvas DPR and work,
and keep dense decoration behind legible controls. Use semantic controls and
keyboard navigation. State survives reload. No automatic acceptance, publishing,
webhook sends, agent dispatch, or launching the original application.

Shared work ownership for the first milestone:
- Native core: `src-tauri/` Rust/backend/config and core tests.
- Interface and artwork: `web/`, `assets/`, icon generator and visual notes.
- Build and packaging: root docs/config/CI, `crates/card-renderer` and tooling.

Follow `docs/ipc-contract.md`. Coordinate changes to shared contracts through the
parent. Do not commit another worker's unfinished edits or change global Git
configuration. Run relevant checks, report exact limits, and never claim 1:1
feature parity until the inventory has been verified.
