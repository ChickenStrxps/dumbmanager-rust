# Original interface verification

The original DumbManager frontend is the reference. This port preserves its
rail, pixel headings, capture panel, navigation, dialogs, fonts and full-page
ASCII creatures. It does not introduce a replacement dashboard.

`original-ui-frontend.md` records source provenance and the small native-host
changes. `original-ui-backend.md` records the native compatibility contract.
Source equality and a browser layout comparison establish visual provenance;
they do not establish that every original service has been ported.

## Reference

A read-only static copy of the original frontend was inspected at 1280 by 800
CSS pixels. It loaded the original bundled fonts and all three full-page canvas
layers without a JavaScript page error. API requests were deliberately
unavailable in this reference; no original application or account was contacted.

The restored frontend matched the reference's measured layout, navigation,
fonts, colors and canvas layers at both 1280 by 800 and 390 by 844. Neither
surface reported a JavaScript page error or horizontal document overflow.
Animation phases are intentionally not compared pixel by pixel.

The reference rail is 236 pixels wide. The capture panel starts at x=274.390625,
y=232.90625 and spans 967.21875 by 222 pixels. The heading uses Silkscreen;
body text uses Space Grotesk and command input uses IBM Plex Mono. The preserved
background, accent and muted colors are `#060608`, `#ff5cc8` and `#8d8a9e`.

## Windows identity

The independent Windows icon is generated in 16, 20, 24, 32, 40, 48, 64, 128
and 256 pixel ICO entries, with a separate 512 pixel PNG master. Small sizes
prioritize a clear structural mark; larger sizes integrate fine ASCII detail.
The icon design and asset checks are recorded in `icon-design.md`. Windows
shell checks are separate from verification of the restored frontend.
The updated DM icon was checked in the installed title bar, real taskbar and
small and large Explorer views. Windows initially retained the previous taskbar
image; refreshing its cache and the Rust shortcut's explicit icon, then
reopening the app, displayed the new mark. The application identity was retained.

## Native review

The opt-in debug QA executable loaded the restored frontend with the production
CSP and real IPC against fresh OS-temp state. Manual checks saved a synthetic
itch, edited its title and desired outcome, and persisted four distinct stable
plan IDs. Literal `<safe> & review` text remained text in the original dialog.
The original acceptance dialog was opened and cancelled; SQLite remained open
with no outcomes. Reload returned to the same open itch and retained the edits
and plan. The original motion button persisted `off` in SQLite. Navigating to
Itch pile and reloading retained both that page and the paused preference. No
original runtime or data was used.

T3 controls correctly showed an unavailable explanation and no selectable
destination. This is not an end-to-end T3 verification. Layout previews have
no native backend and never save simulated data. Remaining external integration
and Social export gaps are recorded in the migration inventory.

## Installed package

The corrected release build was packaged with NSIS and installed over the
earlier Rust-only preview in its separate per-user directory. The installed
window was inspected with the original rail, capture panel, fonts and animated
ASCII background. Its title is DumbManager Rust and its status is Rust local.
The normal profile contained zero itches; synthetic QA records remain confined
to the disposable development profile.

The installed executable matches the release executable except for Tauri's
documented bundle marker (`UNK` becomes `NSS` for NSIS). The installed license
and dependency notices match the source files byte for byte. The separate
Start menu shortcut remains present. No original installation was upgraded.
