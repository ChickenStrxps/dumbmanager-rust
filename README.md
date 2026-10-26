# DumbManager Rust

A separate native Windows edition of DumbManager, with the original interface,
fonts and full-page ASCII animation. Rust handles storage and native operations
underneath the existing capture, itch pile and review flows.

[![An original card exported by the native renderer](docs/media/native-card.gif)](docs/media/native-card.png)

The backend migration is in progress. The Python edition continues to run
independently. This is not a complete replacement yet; see the
[migration inventory](docs/migration-inventory.md).

## Run on Windows

The **Windows desktop** workflow is configured to build a
`DumbManager-Rust-Windows` artifact. After a successful run, download it and
choose the setup executable, or run `dumbmanager-rust.exe` directly.
Windows WebView2 must be installed.
Keep `LICENSE` and `THIRD_PARTY_NOTICES.txt` with the standalone executable;
the installer includes both automatically.

The app opens its own window and uses its own data directory and Windows
identity. No old itches, accounts or keys are imported automatically.
See [isolation](docs/isolation.md) for the exact boundary.

## Included in this milestone

- Capture, edit, search and filter local itches.
- Separate open, stashed, ready for review and accepted states.
- Explicit acceptance during review.
- Persistent settings and reload navigation.
- The original pixel headings, navigation, dialogs and fine ASCII background.
- Native looping GIF and high-resolution PNG export APIs; original Social
  draft/share controls still need their backend migration.
- Separate window, taskbar, tray and executable icons.

Existing T3, browser, Discord, voice, scouting and phone features are migration
work. Their original interface remains available; unported services report that
they are unavailable. No connection or successful operation is simulated.

## Build from source

Install Rust with the Windows MSVC toolchain, Microsoft C++ Build Tools,
WebView2 and Node.js 24. These are the normal
[Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).

```powershell
npm ci
npm run dev
```

Create the executable and Windows installer:

```powershell
py tools/generate-notices.py
npm run build
```

The outputs are in `src-tauri/target/release/` and its `bundle/nsis/` directory.
No Python runtime is required by the packaged app. Python 3 is used by the
license-notice build step and optional artwork and benchmark utilities.

## Checks

```powershell
npm run check:ui
cargo test --manifest-path src-tauri/Cargo.toml -p dumbmanager-core -p dumbmanager-rust
cargo test --manifest-path crates/card-renderer/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Performance claims and the measured renderer experiment are documented in
[performance notes](docs/performance.md). There is no measured whole-app speedup
yet. The optional layout server (`py tools/serve-ui.py`) previews the design;
native operations require the desktop executable.

MIT licensed. Fonts and dependencies retain their own license notices.
