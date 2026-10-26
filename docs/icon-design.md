# DumbManager Rust - application icon

## Concept

The icon is the original app's own rail logo: a black pixel "DM" on a hot magenta
block (`#ff5cc8`) with a hard deep-magenta offset shadow (`#8a1f6c`). It replaces
the old multicolour fish on a dark tile, which was hard to make out on the taskbar.

- **16-64 px:** flat black pixel letters on whole pixels. The letters are
  hand-hinted at 16, 20, 32 and 40, and drawn on the unit grid at 24, 48 and 64.
  There is no texture at these sizes. From 40 px up the letters get a hard
  extrusion.
- **128-512 px:** the letters contain a fixed ASCII smoke field using the app's
  aquarium style on the frontend's 10 px glyph cell, drawn the same way
  `web/aquarium.js` draws its glyph fields:
  - the ramp `.,:;+*xs%$#@`
  - Bayer thresholds `(v+.5)/16`
  - the ripple texture
  - a 12-step periwinkle-to-pink palette at 83 %
  - pink cell tiles
- **Fixed glyph size:** the glyphs stay the same size at every large size, so 128,
  256 and 512 hold 1, 2 and 4 glyph columns per stroke. The detail sits inside the
  mark, not around it.
- **Colours:** the cool accent is the app's periwinkle `--accent2` (`#7c86ff`). The
  app has no cyan token.

## Why it reads small

- The tile is bright and saturated, so it stands apart from dark-tile icons on
  both dark and light taskbars.
- Black on `#ff5cc8` has about a 7:1 luminance contrast. The deep slab gives the
  tile a dark edge on light taskbars.
- The letters sit on whole pixels with no anti-aliasing and no texture below
  128 px. The pixel letterform is the terminal/ASCII cue at small sizes.
- The silhouette is the same at every size: a magenta tile, a bottom-right slab
  and a wide two-letter mark at about 70 % of the face width.

## Generated assets

`py -3 tools/generate-art.py [--verify] [--out-root DIR]` writes these files. It
uses Pillow only, the bundled `web/fonts/IBMPlexMono.ttf`, and has no randomness.

| File | Size | sha256 (first 16) |
|---|---|---|
| `assets/icons/icon.ico` | 16, 20, 24, 32, 40, 48, 64, 128 (32-bit BGRA DIB + AND mask), 256 (PNG); 116994 B | `b431bff8f38ba8d8` |
| `assets/icons/32x32.png` | 32x32 RGBA, 413 B | `e51ec5155d904b66` |
| `assets/icons/128x128.png` | 128x128 RGBA, 2379 B | `a24ecd8daff47ce6` |
| `assets/icons/128x128@2x.png` | 256x256 RGBA, 6332 B | `7baddaaff58d8376` |
| `assets/icons/icon.png` | 512x512 RGBA, 18397 B | `0f406d887a90e606` |

The review images in `assets/previews/` are git-ignored and are not shipped:

- `icon-contact-sheet.png` is produced by the generator. It shows actual-size
  icons on four backgrounds, magnified views, shell mockups and the large sizes.
- `icon-directions.png` is a one-off scratch image that the tool does not
  regenerate. It shows the old fish against the three directions studied:
  - A, the magenta block (chosen)
  - B, an ink tile, rejected because it reads like other dark tiles and sinks
    on dark taskbars
  - C, an o_O keycap, rejected because it carries no DumbManager identity

## Verification done

- `--verify` passed:
  - The ICO header is 0/1/9.
  - All nine directory entries are 32 bpp, 1 plane and in order.
  - The 256 frame is PNG 8-bit RGBA. The other frames are 40-byte DIB headers
    with double height.
  - Every frame decoded by Pillow matches its source image within 1 level.
  - All four PNGs are 8-bit RGBA (IHDR colour type 6).
  - An in-process re-render is byte-identical.
- Two separate process runs produced identical sha256 for every output.
- I parsed the ICO directory separately with `struct`. Pillow lists the nine
  sizes. The PNG corners are fully transparent.
- I inspected the images visually at 1:1 and magnified, on dark (#202020), light
  (#eeeeee), white and accent-blue backgrounds, plus the 512 at 1:1 and a 96 px
  LANCZOS downscale of the 256 frame.

## Mockups vs native

Every taskbar, title-bar, Explorer and Start view in the contact sheet is drawn
by Pillow. They approximate Windows 11 and are **not** native captures. The
separate installed-package checks below used the real Windows shell.

## Installed-package verification

- The release executable and NSIS installer built successfully, and the Rust
  installation was updated in its own directory.
- All nine new ICO frame payloads are embedded in the executable. None of the
  previous fish frame payloads remain.
- The installed executable matches the build, allowing Tauri's NSIS bundle
  marker. The title bar and both small and large Explorer views display the
  new DM artwork.
- The real taskbar displays the new mark after refreshing the shell cache,
  setting the Rust shortcut's explicit executable icon, and reopening it.
  Only the Rust app and its own shortcut were restarted or changed.
- The tray still uses the generated 32 px PNG through its existing compile-time
  image include. Its popup was not inspected separately. Start menu launch
  was exercised; its menu tile was not inspected separately.
- Checks used the current monitor scaling. Different Windows scale settings,
  Alt-Tab and extra-large Explorer views were not separately inspected.
- The original frontend and native application source remained byte-identical
  across 92 tracked files. No app data, application ID or original installation
  was changed.

Small icons are full-bleed (no transparent margin at 16-48 px). The inspected
shell views did not clip them. Sizes 64-512 keep a 1-12 px margin. Review hashes
and native-check details are refreshed in the ignored preview report.
