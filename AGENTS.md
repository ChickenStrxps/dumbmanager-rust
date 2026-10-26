# DumbManager Rust

This is an isolated Rust/Tauri Windows application. Do not change the existing
DumbManager or DumbAPI installation, source, data, credentials, runtime records,
launch shortcuts, or GitHub repository. Reuse frontend source and original artwork
for the authorized 1:1 port; never copy runtime state or keys. This repo stays private.

Use `io.dumbmanager.rust` as the application identifier and `DumbManager Rust` as
the visible name. Store data under the Tauri application data directory, never
the original app's directory. No listeners or startup tasks by default. External
services use explicit configuration; do not discover or inherit another install's
tokens. Tests use temporary state. Keep review evidence isolated as well.

Keep Rust domain logic independent of Tauri and the frontend plain HTML, CSS,
and JavaScript. Use real operations and truthful unavailable states; no demo
success, fabricated itches, or fake service connections. Explain migration gaps.
Keep secrets and personal data out of source, tests, assets, logs, and commits.
Preserve mandatory licenses.

Preserve the original DumbManager interface, including HTML, CSS, fonts,
navigation, capture flow, dialogs and full-page ASCII animations, except where
the user's current request explicitly asks for a change. Fit native transport
and honest unavailable states into this interface.

Respect reduced motion, pause animation while hidden, cap canvas DPR and work,
and keep dense decoration behind legible semantic controls and keyboard access.
State survives reload. The app must not automatically accept itches, publish,
send webhooks, dispatch agents, or launch the original application. Development
delegation/review follows the user's request and applicable workflow policy;
this product restriction does not prohibit authorized development reviewers.

Directory map: native backend/config/tests in `src-tauri/`; interface/artwork in
`web/` and `assets/`; renderer in `crates/card-renderer`; build/art tooling and
instructions in root config, `tools/`, and `docs/`. Assign task-specific ownership
before concurrent edits; coordinate shared contracts through the parent.

Follow `docs/ipc-contract.md` and the migration/support inventories. Preserve other
workers' unfinished edits and use repository-local Git configuration. Run relevant
checks, state exact verification limits, and claim 1:1 parity only after verifying
the inventory. Keep active development and release/deployment authorization separate.
