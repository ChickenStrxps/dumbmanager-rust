#!/usr/bin/env python3
"""Deterministic, original pixel-type icon art for DumbManager Rust.

Dev-only helper (needs Pillow: ``py -3 -m pip install pillow``). It is not part of
the app build. No randomness, clocks, network or system fonts are used: glyph
texture comes from the bundled IBM Plex Mono (OFL) in ``web/fonts``. The same
Pillow/FreeType build always writes byte-identical files.

Concept: the original app's rail mark - black pixel "DM" on a hot magenta block
with a hard deep-magenta shadow - promoted to the Windows icon. One letterform on
a unit grid (written below as ASCII art), three levels of detail:
  * 16-40 px  pixel letters on whole pixels (hand-hinted at 16/20/32/40), flat
              ink, no texture; 40 adds a 1 px extrusion.
  * 48-64 px  the same letters on the unit grid plus their hard extrusion.
  * 128-512   the letters become windows onto the app's ASCII aquarium: smoke
              rises through them on the frontend's own 10 px glyph cell, drawn
              with web/aquarium.js's ramp, Bayer threshold, ripple texture,
              blue-to-pink palette and faint pink cell tiles. The glyph size
              stays fixed, so 128/256/512 hold 1/2/4 glyph columns per stroke.

Outputs (paths relative to the repository root):
  assets/icons/icon.ico                  16 20 24 32 40 48 64 128 (BMP) + 256 (PNG)
  assets/icons/32x32.png 128x128.png 128x128@2x.png icon.png (512)
  assets/previews/icon-contact-sheet.png actual-size previews and shell mockups

Usage:  py -3 tools/generate-art.py [--verify] [--out-root DIR]
"""
from __future__ import annotations

import argparse
import hashlib
import math
import struct
import sys
from io import BytesIO
from pathlib import Path

try:
    from PIL import Image, ImageChops, ImageDraw, ImageFont
    from PIL import __version__ as PIL_VERSION
except ImportError:  # pragma: no cover - dev tool guard
    sys.exit("Pillow is required (dev-only): py -3 -m pip install pillow")

REPO = Path(__file__).resolve().parents[1]
FONT_MONO = REPO / "web" / "fonts" / "IBMPlexMono.ttf"

ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
ALL_SIZES = ICO_SIZES + (512,)


def rgb(hex_color: str) -> tuple[int, int, int]:
    h = hex_color.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


# Palette: the original interface's own tokens (web/style.css :root).
INK = rgb("060608")    # --bg
MAG = rgb("ff5cc8")    # --amber, the hot magenta accent
DEEP = rgb("8a1f6c")   # --accent-deep, the hard shadow colour
PERI = rgb("7c86ff")   # --accent2

# ------------------------------------------------------------ letterform
# Master "DM", one character per grid unit ('#' = ink). Silkscreen-like pixel
# caps: 2-unit strokes, a stepped corner on the D, corner-touching steps on the M.
DM_UNITS = (
    "####....##....##",
    "#####...##....##",
    "##..##..###..###",
    "##..##..###..###",
    "##..##..##.##.##",
    "##..##..##.##.##",
    "##..##..##....##",
    "##..##..##....##",
    "#####...##....##",
    "####....##....##",
)

# Hand-hinted bitmaps, one character per pixel, for sizes where the unit grid
# would land between pixels. Same skeleton, weights chosen per size.
DM_HINTED = {
    16: (
        "###..#...#",
        "##.#.##.##",
        "##.#.#.#.#",
        "##.#.#...#",
        "##.#.#...#",
        "##.#.#...#",
        "###..#...#",
    ),
    20: (
        "####..##...##",
        "##..#.###.###",
        "##..#.##.#.##",
        "##..#.##...##",
        "##..#.##...##",
        "##..#.##...##",
        "####..##...##",
    ),
    32: (
        "#######....###.....###",
        "########...###.....###",
        "#########..###.....###",
        "###...###..####...####",
        "###...###..####...####",
        "###...###..####...####",
        "###...###..###.###.###",
        "###...###..###.###.###",
        "###...###..###.###.###",
        "###...###..###.....###",
        "#########..###.....###",
        "########...###.....###",
        "#######....###.....###",
    ),
    40: (
        "########.....###.......###",
        "#########....###.......###",
        "##########...###.......###",
        "###....###...#####...#####",
        "###....###...#####...#####",
        "###....###...#####...#####",
        "###....###...###..###..###",
        "###....###...###..###..###",
        "###....###...###..###..###",
        "###....###...###.......###",
        "###....###...###.......###",
        "###....###...###.......###",
        "##########...###.......###",
        "#########....###.......###",
        "########.....###.......###",
    ),
}

# Per size: transparent margin, hard-shadow (slab) offset, corner radius,
# letter source (unit size, or "hint"), letter extrusion depth, glyph font px.
LAYOUT = {
    16: dict(margin=0, slab=1, radius=3.0, unit="hint", depth=0),
    20: dict(margin=0, slab=1, radius=3.5, unit="hint", depth=0),
    24: dict(margin=0, slab=2, radius=4.0, unit=1, depth=0),
    32: dict(margin=0, slab=2, radius=5.0, unit="hint", depth=0),
    40: dict(margin=0, slab=3, radius=6.0, unit="hint", depth=1),
    48: dict(margin=0, slab=3, radius=7.0, unit=2, depth=1),
    64: dict(margin=1, slab=4, radius=9.0, unit=2.5, depth=2),
    128: dict(margin=3, slab=6, radius=18.0, unit=5, depth=3, glyph_px=11),
    256: dict(margin=6, slab=12, radius=36.0, unit=10, depth=5, glyph_px=12),
    512: dict(margin=12, slab=24, radius=72.0, unit=20, depth=10, glyph_px=12),
}

# Glyph fields as web/aquarium.js draws them: its density ramp and cell size,
# the 4x4 Bayer thresholds ((v + .5) / 16), a 12-step blue-to-pink palette at
# 83 % (dark theme) and pink cell tiles one pixel short of the cell.
RAMP = ".,:;+*xs%$#@"
BAYER4 = tuple((v + 0.5) / 16 for v in (0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5))
PALETTE = tuple(mix(PERI, MAG, i / 11) for i in range(12))
CELL = 10            # px; the frontend's glyph cell
GLYPH_ALPHA = 0.83
TILE_ALPHA = 0.10    # stronger than the app's 0.04 so the cell grid survives downscaling

# Vent smoke inside the letters: (x at the base, sway, sway cycles, phase,
# width at the base, strength). x and width are fractions of the letters' width.
PLUMES = (
    (0.10, 0.05, 0.9, 0.10, 0.10, 1.00),
    (0.33, 0.06, 0.7, 0.55, 0.09, 0.85),
    (0.62, 0.05, 1.1, 0.30, 0.10, 0.95),
    (0.90, 0.06, 0.8, 0.80, 0.09, 0.90),
)
SMOKE_FLOOR = 0.08   # thin haze everywhere, so the top of each stroke keeps a few dots


# ------------------------------------------------------------- raster utils
def over(canvas: Image.Image, color, mask: Image.Image) -> Image.Image:
    """Composite a flat colour through an L mask."""
    layer = Image.new("RGBA", canvas.size, color + (255,))
    layer.putalpha(mask)
    return Image.alpha_composite(canvas, layer)


def rounded(size: int, box, radius: float, ss: int = 8) -> Image.Image:
    """Anti-aliased rounded rectangle mask; box edges are exclusive pixel edges."""
    x0, y0, x1, y1 = box
    m = Image.new("L", (size * ss, size * ss), 0)
    ImageDraw.Draw(m).rounded_rectangle([x0 * ss, y0 * ss, x1 * ss - 1, y1 * ss - 1], radius=radius * ss, fill=255)
    return m.resize((size, size), Image.BOX)


def shifted(mask: Image.Image, dx: int, dy: int) -> Image.Image:
    out = Image.new("L", mask.size, 0)
    out.paste(mask, (dx, dy))
    return out


def edges(n: int, unit: float) -> list[int]:
    """Whole-pixel edges for n units (grid fitting for fractional units)."""
    return [int(i * unit + 0.5) for i in range(n + 1)]


def font(px: int):
    if FONT_MONO.exists():
        return ImageFont.truetype(str(FONT_MONO), px)
    return ImageFont.load_default()  # fallback only; texture expects the bundled font


# ------------------------------------------------------------------- icon
def geometry(size: int):
    spec = LAYOUT[size]
    m, o = spec["margin"], spec["slab"]
    face = (m, m, size - m - o, size - m - o)
    if spec["unit"] == "hint":
        rows = DM_HINTED[size]
        xs, ys = edges(len(rows[0]), 1), edges(len(rows), 1)
    else:
        rows = DM_UNITS
        xs, ys = edges(len(rows[0]), spec["unit"]), edges(len(rows), spec["unit"])
    fw = face[2] - face[0]
    ox = face[0] + (fw - xs[-1]) // 2
    oy = face[1] + (fw - ys[-1]) // 2
    return spec, face, rows, xs, ys, ox, oy


def letter_mask(size, rows, xs, ys, ox, oy) -> Image.Image:
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    for j, row in enumerate(rows):
        for i, ch in enumerate(row):
            if ch == "#":
                d.rectangle([ox + xs[i], oy + ys[j], ox + xs[i + 1] - 1, oy + ys[j + 1] - 1], fill=255)
    return m


def smoke(u: float, t: float) -> float:
    """Vent smoke density at (u, t), both 0..1 across the letters (t = 0 at the top).

    Plumes rise from the base of the letters, swaying and widening as they climb
    and thinning towards the top.
    """
    rise = 1.0 - t
    v = SMOKE_FLOOR
    for x0, sway, cycles, phase, width, strength in PLUMES:
        cx = x0 + sway * math.sin(2 * math.pi * (rise * cycles + phase))
        w = width * (0.55 + 0.9 * rise)
        v += strength * math.exp(-((u - cx) / w) ** 2) * (1.0 - 0.75 * rise)
    return v


def glyph_cells(ink: Image.Image, xs, ys, ox, oy):
    """Aquarium glyph field on CELL px cells lying wholly inside the letters.

    Returns (tiles, glyphs): tile rectangles and (x0, y0, x1, y1, char, colour).
    """
    cols, rows = xs[-1] // CELL, ys[-1] // CELL
    tiles, glyphs = [], []
    for y in range(rows):
        for x in range(cols):
            x0, y0 = ox + x * CELL, oy + y * CELL
            if ink.crop((x0, y0, x0 + CELL, y0 + CELL)).getextrema()[0] < 255:
                continue  # cell crosses a letter edge: leave it solid ink
            tiles.append((x0, y0, x0 + CELL - 2, y0 + CELL - 2))
            # web/aquarium.js glyph field: ripple texture, Bayer threshold,
            # palette tone and ramp character from the same density.
            texture = 0.84 + 0.1 * math.sin(x * 0.71 + y * 0.23) + 0.06 * math.cos(y * 0.53 - x * 0.31)
            v = min(1.0, smoke((x + 0.5) / cols, (y + 0.5) / rows) * texture)
            th = BAYER4[(y & 3) * 4 + (x & 3)]
            if v * 1.5 < th:
                continue
            tone = min(11, int(v * 9 + th * 2))
            ch = RAMP[min(len(RAMP) - 1, int(v * 10 + th * 2))]
            glyphs.append((x0, y0, x0 + CELL - 1, y0 + CELL - 1, ch, PALETTE[tone]))
    return tiles, glyphs


def glyph_layer(size, cells, font_px, alpha, clip: Image.Image):
    """cells: iterable of (x0, y0, x1, y1, char, colour). Returns an RGBA layer.

    Glyphs are centred in their cells and clipped to ``clip``; each colour patch
    is padded by 2 px so a glyph that overhangs its cell keeps a matching colour.
    """
    f = font(font_px)
    ink = Image.new("L", (size, size), 0)
    paint = Image.new("RGB", (size, size), (0, 0, 0))
    gi, gp = ImageDraw.Draw(ink), ImageDraw.Draw(paint)
    for x0, y0, x1, y1, _, color in cells:
        gp.rectangle([x0 - 2, y0 - 2, x1 + 2, y1 + 2], fill=color)
    for x0, y0, x1, y1, ch, color in cells:
        gi.text(((x0 + x1 + 1) / 2, (y0 + y1 + 1) / 2 + 0.5), ch, font=f, fill=int(round(255 * alpha)), anchor="mm")
    layer = paint.convert("RGBA")
    layer.putalpha(ImageChops.multiply(ink, clip))
    return layer


def render(size: int) -> Image.Image:
    spec, face, rows, xs, ys, ox, oy = geometry(size)
    m, o, r = spec["margin"], spec["slab"], spec["radius"]
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas = over(canvas, DEEP, rounded(size, (m + o, m + o, size - m, size - m), r))
    canvas = over(canvas, MAG, rounded(size, face, r))

    ink = letter_mask(size, rows, xs, ys, ox, oy)
    if spec["depth"]:
        ext = Image.new("L", (size, size), 0)
        for k in range(1, spec["depth"] + 1):
            ext = ImageChops.lighter(ext, shifted(ink, k, k))
        # Where two blocks touch corner to corner (the M's steps), the 45-degree
        # sweep leaves a 1 px diagonal seam: empty pixels with solid left and
        # above neighbours but an empty upper-left one. Fill exactly those.
        solid = ImageChops.lighter(ext, ink)
        seam = ImageChops.multiply(shifted(solid, 1, 0), shifted(solid, 0, 1))
        seam = ImageChops.multiply(seam, ImageChops.invert(shifted(solid, 1, 1)))
        ext = ImageChops.lighter(ext, seam)
        canvas = over(canvas, DEEP, ImageChops.subtract(ext, ink))
    canvas = over(canvas, INK, ink)

    if spec.get("glyph_px"):
        tiles, glyphs = glyph_cells(ink, xs, ys, ox, oy)
        tile_mask = Image.new("L", (size, size), 0)
        draw = ImageDraw.Draw(tile_mask)
        for box in tiles:
            draw.rectangle(box, fill=int(round(255 * TILE_ALPHA)))
        canvas = over(canvas, MAG, tile_mask)
        canvas = Image.alpha_composite(canvas, glyph_layer(size, glyphs, spec["glyph_px"], GLYPH_ALPHA, ink))
    return canvas


def build_images() -> dict[int, Image.Image]:
    return {s: render(s) for s in ALL_SIZES}


# ------------------------------------------------------------------ files
def png_bytes(im: Image.Image) -> bytes:
    buf = BytesIO()
    im.save(buf, format="PNG", optimize=False, compress_level=9)
    return buf.getvalue()


def ico_bytes(images: dict[int, Image.Image], sizes=ICO_SIZES) -> bytes:
    """Write an ICO by hand: classic 32-bit DIB frames, PNG only for 256."""
    frames = []
    for s in sizes:
        im = images[s].convert("RGBA")
        assert im.size == (s, s)
        if s >= 256:
            frames.append((s, png_bytes(im)))
            continue
        px = im.load()
        rows = []
        for y in range(s - 1, -1, -1):
            rows.append(b"".join(struct.pack("<BBBB", px[x, y][2], px[x, y][1], px[x, y][0], px[x, y][3]) for x in range(s)))
        xor = b"".join(rows)
        row_bytes = ((s + 31) // 32) * 4
        and_rows = []
        for y in range(s - 1, -1, -1):
            bits = bytearray(row_bytes)
            for x in range(s):
                if px[x, y][3] == 0:
                    bits[x // 8] |= 0x80 >> (x % 8)
            and_rows.append(bytes(bits))
        mask = b"".join(and_rows)
        header = struct.pack("<IiiHHIIiiII", 40, s, s * 2, 1, 32, 0, len(xor) + len(mask), 0, 0, 0, 0)
        frames.append((s, header + xor + mask))
    out = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
    offset = 6 + 16 * len(frames)
    for s, data in frames:
        out += struct.pack("<BBBBHHII", 0 if s >= 256 else s, 0 if s >= 256 else s, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    for _, data in frames:
        out += data
    return bytes(out)


# ------------------------------------------------------------ contact sheet
def checker(size, a=(36, 38, 48), b=(30, 32, 41), cell=8):
    im = Image.new("RGB", size, a)
    d = ImageDraw.Draw(im)
    for y in range(0, size[1], cell):
        for x in range(0, size[0], cell):
            if (x // cell + y // cell) % 2:
                d.rectangle([x, y, x + cell - 1, y + cell - 1], fill=b)
    return im


def paste_icon(dst: Image.Image, icon: Image.Image, xy):
    dst.paste(icon, xy, icon)


def contact_sheet(images: dict[int, Image.Image]) -> Image.Image:
    W, H = 1560, 1500
    sheet = Image.new("RGB", (W, H), rgb("0a0b13"))
    d = ImageDraw.Draw(sheet)
    f_title, f_head, f_small = font(22), font(14), font(12)
    muted, text, accent = rgb("9a97b0"), rgb("ecebf6"), MAG

    d.text((32, 24), "DumbManager Rust - icon contact sheet", font=f_title, fill=text)
    d.text((32, 56), f"generated by tools/generate-art.py  |  Pillow {PIL_VERSION}  |  ICO frames: "
           + " ".join(map(str, ICO_SIZES)) + "  |  shell rows are mockups, not native captures",
           font=f_small, fill=muted)

    def label(x, y, t, c=muted):
        d.text((x, y), t, font=f_small, fill=c)

    backgrounds = [("dark taskbar", (32, 32, 32)), ("light taskbar", (238, 238, 238)),
                   ("white window", (255, 255, 255)), ("accent colour", (36, 79, 140))]
    small = (16, 20, 24, 32, 40, 48, 64)

    # 1) actual size
    y = 96
    d.text((32, y), "1. Actual size (1:1 pixels)", font=f_head, fill=accent)
    y += 30
    sw_h, gap = 84, 8
    x = 32
    for s in small:
        sw = max(s + 20, 84)
        for r, (_, bg) in enumerate(backgrounds):
            yy = y + r * (sw_h + gap)
            d.rectangle([x, yy, x + sw - 1, yy + sw_h - 1], fill=bg)
            paste_icon(sheet, images[s], (x + (sw - s) // 2, yy + (sw_h - s) // 2))
        label(x, y + 4 * (sw_h + gap) + 2, f"{s}px")
        x += sw + 16
    for r, (name, _) in enumerate(backgrounds):
        label(x + 8, y + r * (sw_h + gap) + 34, name)
    y += 4 * (sw_h + gap) + 40

    # 2) magnified
    d.text((32, y), "2. Same pixels magnified (nearest neighbour)", font=f_head, fill=accent)
    y += 30
    x = 32
    for s, z in ((16, 9), (20, 7), (24, 6), (32, 5), (40, 4), (48, 3), (64, 2)):
        big = images[s].resize((s * z, s * z), Image.NEAREST)
        plate = checker((s * z + 12, s * z + 12))
        sheet.paste(plate, (x, y))
        paste_icon(sheet, big, (x + 6, y + 6))
        label(x, y + 178, f"{s}px x{z}")
        x += s * z + 28
    y += 204

    # 3) shell mockups (drawn here, not captured from Windows)
    d.text((32, y), "3. Shell mockups (approximations of Windows 11 surfaces; not native captures)", font=f_head, fill=accent)
    y += 30
    for n, (bar, fg, hover, ind) in enumerate((((32, 32, 32), (240, 240, 244), (52, 52, 56), (118, 185, 237)),
                                              ((238, 238, 238), (26, 26, 30), (250, 250, 250), (0, 95, 184)))):
        by = y + n * 60
        d.rectangle([32, by, 732, by + 47], fill=bar)
        for k, (s, state) in enumerate(((24, "pinned"), (24, "running"), (24, "active"), (32, "125%"), (40, "150%"))):
            bx = 52 + k * 64
            cx = bx + 20
            if state in ("active", "125%", "150%"):
                d.rounded_rectangle([bx, by + 2, bx + 40, by + 45], radius=5, fill=hover)
            paste_icon(sheet, images[s], (cx - s // 2, by + 23 - s // 2))
            if state != "pinned":
                w = 16 if state == "active" else 6
                d.rounded_rectangle([cx - w // 2, by + 42, cx + w // 2, by + 44], radius=1, fill=ind)
        label(380, by + 17, "taskbar: 24 pinned / running / active, 32, 40", fg)
    ty = y + 124
    for n, (bar, fg) in enumerate((((32, 32, 32), (240, 240, 244)), ((243, 243, 243), (26, 26, 30)))):
        d.rectangle([32, ty + n * 40, 732, ty + n * 40 + 31], fill=bar)
        paste_icon(sheet, images[16], (44, ty + n * 40 + 8))
        d.text((70, ty + n * 40 + 8), "DumbManager Rust", font=f_small, fill=fg)
        d.text((640, ty + n * 40 + 8), "-   []   x", font=f_small, fill=fg)
    label(32, ty + 82, "title bar 16 px, dark and light")
    for n, (s, plate, name) in enumerate(((48, (243, 243, 247), "Explorer medium / light"),
                                          (48, (32, 32, 32), "Explorer medium / dark"),
                                          (64, (32, 32, 32), "Start / 64 px"))):
        bx = 772 + n * 196
        d.rounded_rectangle([bx, y, bx + 180, y + 150], radius=8, fill=plate)
        paste_icon(sheet, images[s], (bx + 90 - s // 2, y + 18 + (64 - s) // 2))
        d.text((bx + 90, y + 112), "DumbManager Rust", font=font(11), fill=(30, 30, 34) if plate[0] > 128 else (230, 230, 236), anchor="mm")
        label(bx, y + 158, name)
    y += 250

    # 4) large sizes
    d.text((32, y), "4. Large sizes 1:1 - 128, 256 (128x128@2x.png), icon.png 512 (1:1 crop and scaled)", font=f_head, fill=accent)
    y += 30
    sheet.paste(Image.new("RGB", (148, 148), (32, 32, 32)), (32, y))
    paste_icon(sheet, images[128], (42, y + 10))
    sheet.paste(Image.new("RGB", (148, 148), (243, 243, 247)), (32, y + 160))
    paste_icon(sheet, images[128], (42, y + 170))
    label(32, y + 312, "128")
    sheet.paste(Image.new("RGB", (276, 276), (243, 243, 247)), (200, y))
    paste_icon(sheet, images[256], (210, y + 10))
    label(200, y + 282, "256")
    sheet.paste(Image.new("RGB", (276, 276), (32, 32, 32)), (496, y))
    paste_icon(sheet, images[512].crop((90, 150, 346, 406)), (506, y + 10))
    label(496, y + 282, "icon.png 512 - 1:1 crop")
    sheet.paste(Image.new("RGB", (276, 276), (10, 11, 19)), (792, y))
    paste_icon(sheet, images[512].resize((256, 256), Image.LANCZOS), (802, y + 10))
    label(792, y + 282, "icon.png 512 - scaled to 256")
    sheet.paste(Image.new("RGB", (276, 276), (238, 238, 238)), (1088, y))
    for k, s in enumerate((16, 24, 32, 48)):
        paste_icon(sheet, images[s], (1100 + k * 60, y + 40))
    paste_icon(sheet, images[256].resize((96, 96), Image.LANCZOS), (1100, y + 120))
    label(1206, y + 140, "96 px: the 256\nframe scaled by\nPillow LANCZOS,\napproximating\nExplorer Large", (60, 60, 70))
    label(1088, y + 282, "light shell, mixed sizes")
    return sheet


# ------------------------------------------------------------------ output
def write_all(root: Path, verify: bool) -> list[tuple[str, str]]:
    images = build_images()
    icons, previews = root / "assets" / "icons", root / "assets" / "previews"
    for p in (icons, previews):
        p.mkdir(parents=True, exist_ok=True)

    written: list[tuple[Path, bytes]] = []
    written.append((icons / "icon.ico", ico_bytes(images)))
    written.append((icons / "32x32.png", png_bytes(images[32])))
    written.append((icons / "128x128.png", png_bytes(images[128])))
    written.append((icons / "128x128@2x.png", png_bytes(images[256])))
    written.append((icons / "icon.png", png_bytes(images[512])))
    written.append((previews / "icon-contact-sheet.png", png_bytes(contact_sheet(images))))

    report = []
    for path, data in written:
        path.write_bytes(data)
        report.append((str(path.relative_to(root)).replace("\\", "/"), hashlib.sha256(data).hexdigest()[:16] + f"  {len(data):>7} B"))

    if verify:
        check_outputs(icons, images, dict((p.name, b) for p, b in written))
    return report


def png_header(data: bytes) -> tuple[int, int, int, int]:
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR", "not a PNG"
    w, h, depth, ctype = struct.unpack(">IIBB", data[16:26])
    return w, h, depth, ctype


def check_outputs(icons: Path, images: dict[int, Image.Image], written: dict[str, bytes]) -> None:
    raw = (icons / "icon.ico").read_bytes()
    reserved, kind, count = struct.unpack("<HHH", raw[:6])
    assert (reserved, kind, count) == (0, 1, len(ICO_SIZES)), f"ICO header {reserved} {kind} {count}"
    entries = []
    for n in range(count):
        w, h, colours, _, planes, bpp, length, offset = struct.unpack("<BBBBHHII", raw[6 + 16 * n:22 + 16 * n])
        w, h = w or 256, h or 256
        assert (planes, bpp, colours) == (1, 32, 0), f"ICO entry {w}: planes {planes} bpp {bpp}"
        assert offset + length <= len(raw), f"ICO entry {w} overruns the file"
        blob = raw[offset:offset + length]
        if w >= 256:
            assert png_header(blob) == (w, h, 8, 6), f"ICO {w}px PNG is not 8-bit RGBA"
        else:
            size, bw, bh, _, depth = struct.unpack("<IiiHH", blob[:16])
            assert (size, bw, bh, depth) == (40, w, 2 * h, 32), f"ICO {w}px DIB header"
        entries.append(w)
    assert entries == list(ICO_SIZES), f"ICO directory {entries} != {list(ICO_SIZES)}"

    ico = Image.open(icons / "icon.ico")
    for s in ICO_SIZES:
        frame = ico.ico.getimage((s, s)).convert("RGBA")
        worst = max(ImageChops.difference(frame, images[s].convert("RGBA")).tobytes())
        assert worst <= 1, f"ICO frame {s}px differs from source by {worst}"
    for name, s in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        assert png_header((icons / name).read_bytes()) == (s, s, 8, 6), f"{name}: not {s}x{s} 8-bit RGBA"
        im = Image.open(icons / name)
        assert im.size == (s, s) and im.mode == "RGBA", f"{name}: {im.size} {im.mode}"

    again = build_images()  # in-process determinism: re-render and compare bytes
    assert ico_bytes(again) == written["icon.ico"], "re-render changed icon.ico"
    for name, s in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        assert png_bytes(again[s]) == written[name], f"re-render changed {name}"
    print("verify: ICO directory", entries, "32-bit, 256 is PNG RGBA; frames match source;",
          "PNGs are 8-bit RGBA at 32/128/256/512; re-render byte-identical")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--verify", action="store_true", help="re-open the outputs and check sizes/pixels")
    ap.add_argument("--out-root", type=Path, default=REPO, help="write under this root instead of the repo")
    args = ap.parse_args()
    if not FONT_MONO.exists():
        print(f"warning: {FONT_MONO} missing; texture falls back to PIL's default font", file=sys.stderr)
    for rel, digest in write_all(args.out_root, args.verify):
        print(f"{rel:<44} {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
