# Signed updates

Settings → Connections → Application updates supports manual checks and an optional check when the app opens. Startup checks are off until enabled on that device. A new-version notification points back to Settings; it never installs automatically. Checks contact the public GitHub Releases endpoint; network and configuration failures are shown as errors. Installing requires an explicit confirmation and closes the app on Windows. Save unsaved work first. The official Tauri updater downloads and verifies the installer and its version-bound signature before launching it. A failed download or verification can be retried; a new check replaces the selected candidate.

## Before shipping

The public key in `src-tauri/tauri.conf.json` identifies the publisher of this edition. Preserve the corresponding private key securely and back it up: installed versions trust that key, and losing it prevents future signed updates for them. GitHub Actions uses `TAURI_SIGNING_PRIVATE_KEY`; set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as well if the key has a password. Never commit the private key or password. A fork needs its own key pair and update endpoint. Key rotation needs a separate migration plan. Tauri's updater signature is distinct from Windows Authenticode signing; this workflow does not configure an Authenticode certificate.

Set the same stable version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `package.json` (and update lockfiles as needed). Merge to main and wait for the Windows desktop workflow to succeed for that exact commit. Manually dispatch Signed Windows release on the current main commit. It validates the configuration, requires the signing key, builds the existing per-user NSIS package, enables updater artifacts only for that bundle, and produces a version-bound signature and `latest.json`. It creates a draft with the installer, signature, manifest, and matching tracked-source archive, then publishes it as latest. An existing tag stops the workflow; investigate a partial draft/tag before retrying. The release helper checks signature metadata and key ID; actual cryptographic verification is performed by the official updater at download/install time.

The endpoint is `https://github.com/ChickenStrxps/dumbmanager-rust/releases/latest/download/latest.json`; no GitHub token is embedded in the app. Only Windows x86_64 NSIS is published. Ordinary builds do not need signing secrets and do not create updater signatures.

No installed-app upgrade or rollback has been tested yet; that check is deferred for this preview. A later isolated disposable Windows test should cover a signed upgrade, bad signature, network failure, retry, and retention of that installation's data. Do not use the original DumbManager or DumbAPI installation for that test.

Reference: [Tauri v2 updater documentation](https://v2.tauri.app/plugin/updater/).
