# Native card renderer

Parent-owned crate: `../crates/card-renderer` from `src-tauri`.

```toml
dumbmanager-card-renderer = { path = "../crates/card-renderer" }
```

```rust
use dumbmanager_card_renderer::{CardInput, export_card};
let card = CardInput {
    title: &itch.title,
    description: &itch.description,
    project: &itch.project,
    status: &itch.status,
    seed: &itch.id,
};
let exported = export_card(&export_directory, &card, &format)?;
```

`Export` fields: `path:PathBuf, width:u32, height:u32, frames:usize, bytes:u64`.
Dispatch blocking work off the UI thread with `tauri::async_runtime::spawn_blocking`
and allow only a bounded number of simultaneous exports. 1200x675 GIF, 72 frames
at 50ms (3.6s infinite loop); 2400x1350 PNG. Indexed palette, fixed resolution/frame
count, no external processes or runtime fonts. Content is kept still. Files use
create-new unique names under the supplied export directory. No network or posting.
Only ASCII/Latin font coverage currently; do not claim full Unicode typography.

`cargo test --manifest-path crates/card-renderer/Cargo.toml` checks decoder output,
periodic closure/static copy, word wrapping, invalid input and path confinement.
`cargo run --release --manifest-path crates/card-renderer/Cargo.toml --example render`
writes two actual synthetic artwork examples into ignored `artifacts/cards/`.
