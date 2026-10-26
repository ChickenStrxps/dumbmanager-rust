#!/usr/bin/env python3
"""Deterministic, original ASCII-derived icon art for DumbManager Rust.

Dev-only helper (needs Pillow: ``py -3 -m pip install pillow``). It is not part of
the app build. No randomness, clocks, network or system fonts are used: glyph
texture comes from the bundled IBM Plex Mono (OFL) in ``web/fonts``. The same
Pillow/FreeType build always writes byte-identical files.

Design rule: one fish, one palette, three levels of detail.
  * 16-48 px  bold silhouette only (tile, two-tone fish, big eye). No texture.
  * 64 px     the same silhouette plus a soft shaded body.
  * 128-512   dense ASCII texture: every glyph is picked from a density ramp by
              lighting, fins use ray glyphs ( / \\ | ), edges use outline glyphs.

Outputs (paths relative to the repository root):
  assets/icons/icon.ico                  16 20 24 32 40 48 64 128 (BMP) + 256 (PNG)
  assets/icons/32x32.png 128x128.png 128x128@2x.png icon.png (512)
  assets/previews/icon-contact-sheet.png actual-size previews + contexts

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
    from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont, ImageOps
    from PIL import __version__ as PIL_VERSION
except ImportError:  # pragma: no cover - dev tool guard
    sys.exit("Pillow is required (dev-only): py -3 -m pip install pillow")

REPO = Path(__file__).resolve().parents[1]
FONT_MEDIUM = REPO / "web" / "fonts" / "IBMPlexMono.ttf"
FONT_REGULAR = FONT_MEDIUM

ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
BASE = 512  # master render size; every large output is derived from it


def rgb(hex_color: str) -> tuple[int, int, int]:
    h = hex_color.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


# Palette: the app's dark magenta / cyan identity.
INK = rgb("07070d")
NAVY_TOP, NAVY_MID, NAVY_BOT = rgb("1c2040"), rgb("101326"), rgb("07080f")
RIM = rgb("3a4170")
MAG_HI, MAG, MAG_LO, MAG_DEEP = rgb("ffc4ee"), rgb("ff5cc8"), rgb("b52a90"), rgb("5a1049")
CYN_HI, CYN, CYN_LO, CYN_DEEP = rgb("c4fff6"), rgb("5eead4"), rgb("1f9d93"), rgb("0b4a4d")
ICE = rgb("f4f6ff")
VIO = rgb("9a8cff")


# ---------------------------------------------------------------- geometry
class ShapePath:
    """Tiny path builder that flattens beziers to polylines (256-unit space)."""

    def __init__(self):
        self.pts: list[tuple[float, float]] = []
        self.cur = (0.0, 0.0)

    def M(self, x, y):
        self.cur = (x, y)
        self.pts.append(self.cur)
        return self

    L = M

    def C(self, x1, y1, x2, y2, x, y, n=44):
        x0, y0 = self.cur
        for i in range(1, n + 1):
            t = i / n
            u = 1 - t
            self.pts.append((
                u ** 3 * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t ** 3 * x,
                u ** 3 * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t ** 3 * y,
            ))
        self.cur = (x, y)
        return self


FISH_TILT = -7.0     # degrees, nose slightly up
FISH_SHIFT = (6, -3)  # optical centring inside the tile
FISH_PIVOT = (130, 130)


def xform(points, tilt=FISH_TILT, shift=FISH_SHIFT):
    a = math.radians(tilt)
    ca, sa = math.cos(a), math.sin(a)
    px, py = FISH_PIVOT
    return [((x - px) * ca - (y - py) * sa + px + shift[0],
             (x - px) * sa + (y - py) * ca + py + shift[1]) for x, y in points]


def fish_shapes():
    """All fish parts in 256-unit space, already tilted."""
    body = (ShapePath().M(74, 118).C(108, 64, 214, 56, 226, 130)
            .C(218, 200, 120, 212, 74, 146).pts)
    tail = (ShapePath().M(92, 116).C(72, 104, 52, 90, 30, 72).C(38, 94, 48, 116, 54, 130)
            .C(48, 144, 38, 166, 30, 188).C(52, 170, 72, 156, 92, 144).pts)
    dorsal = (ShapePath().M(186, 88).C(168, 64, 146, 52, 118, 46).C(124, 62, 128, 76, 134, 92).pts)
    belly = (ShapePath().M(172, 184).C(160, 200, 142, 212, 120, 220)
             .C(126, 204, 130, 192, 136, 178).pts)
    pectoral = (ShapePath().M(172, 142).C(158, 150, 144, 160, 130, 176)
                .C(152, 176, 168, 166, 180, 152).pts)
    gill = (ShapePath().M(170, 98).C(152, 114, 152, 148, 170, 166).pts)
    return {
        "body": xform(body), "tail": xform(tail), "dorsal": xform(dorsal),
        "belly": xform(belly), "pectoral": xform(pectoral), "gill": xform(gill),
        "eye": xform([(196, 116)])[0],
        "nose": xform([(226, 130)])[0],
    }


SHAPES = fish_shapes()


# ------------------------------------------------------------- raster utils
def over(canvas: Image.Image, fill, mask: Image.Image) -> Image.Image:
    """Porter-Duff 'over' of a colour (or RGBA image) through an L mask."""
    if isinstance(fill, tuple):
        layer = Image.new("RGBA", canvas.size, fill[:3] + (255,))
        if len(fill) == 4 and fill[3] < 255:
            mask = mask.point(lambda v, a=fill[3]: v * a // 255)
    else:
        layer = fill.convert("RGBA")
        mask = ImageChops.multiply(mask, layer.getchannel("A"))
    layer.putalpha(mask)
    return Image.alpha_composite(canvas, layer)


def poly_mask(size, pts, scale, offset=(0, 0)) -> Image.Image:
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).polygon([((x * scale) + offset[0], (y * scale) + offset[1]) for x, y in pts], fill=255)
    return m


def vgradient(size, stops):
    """Vertical gradient RGBA image; stops = [(t, (r,g,b)), ...] ascending."""
    w, h = size
    col = Image.new("RGB", (1, h))
    px = col.load()
    for y in range(h):
        t = y / max(1, h - 1)
        for i in range(len(stops) - 1):
            t0, c0 = stops[i]
            t1, c1 = stops[i + 1]
            if t <= t1 or i == len(stops) - 2:
                px[0, y] = mix(c0, c1, (t - t0) / max(1e-6, t1 - t0))
                break
    return col.resize((w, h), Image.NEAREST).convert("RGBA")


def rounded_mask(size, inset, radius):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle([inset, inset, size[0] - 1 - inset, size[1] - 1 - inset], radius=radius, fill=255)
    return m


def font(path, px):
    if path.exists():
        return ImageFont.truetype(str(path), px)
    return ImageFont.load_default()  # contact sheet only; icon texture needs the bundled font


# ------------------------------------------------------------ small sizes
def render_vector(size: int, detail: int) -> Image.Image:
    """Bold silhouette render. detail 0: <=24 px, 1: 32-48 px, 2: 64 px (soft shading)."""
    ss = 8 if size <= 48 else 4
    S = size * ss
    k = S / 256.0
    canvas = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    inset_px = 0.0 if size <= 24 else (1.0 if size <= 40 else 0.03125 * size)
    inset = inset_px * ss
    radius = (0.215 * (size - 2 * inset_px)) * ss
    tile = rounded_mask((S, S), inset, radius)
    canvas = over(canvas, vgradient((S, S), [(0, NAVY_TOP), (0.55, NAVY_MID), (1, NAVY_BOT)]), tile)
    rim = ImageChops.subtract(tile, tile.filter(ImageFilter.MinFilter(max(3, int(ss * (1.0 if size <= 32 else 1.4)) | 1))))
    canvas = over(canvas, RIM + (255 if size <= 32 else 190,), rim)

    sh = SHAPES
    zoom = 1.0 + (0.045 if detail == 0 else 0.0)  # small sizes: let the fish use the tile

    def pts(name):
        return [(128 + (x - 128) * zoom, 130 + (y - 130) * zoom) for x, y in sh[name]]

    def draw_part(name, fill_stops, hi=None):
        nonlocal canvas
        mask = poly_mask((S, S), pts(name), k)
        canvas = over(canvas, vgradient((S, S), fill_stops), mask)
        return mask

    cyan = [(0, CYN_HI), (0.4, CYN), (1, CYN_LO)] if detail else [(0, CYN), (1, CYN_LO)]
    magenta = [(0.0, MAG_HI), (0.28, MAG), (1, MAG_LO)] if detail else [(0, MAG), (1, MAG_LO)]
    draw_part("tail", cyan)
    draw_part("dorsal", cyan)
    draw_part("belly", cyan)
    body_mask = draw_part("body", magenta)

    if detail >= 2:
        pec = ImageChops.multiply(poly_mask((S, S), pts("pectoral"), k), body_mask)
        canvas = over(canvas, MAG_LO + (170,), pec)
        gm = Image.new("L", (S, S), 0)
        ImageDraw.Draw(gm).line([(x * k, y * k) for x, y in pts("gill")], fill=255, width=max(1, int(5 * k)))
        canvas = over(canvas, MAG_DEEP + (200,), ImageChops.multiply(gm, body_mask))

    eye_dx = -6 if detail == 0 else (-2 if detail == 1 else 0)  # keep the eye off the nose pixel column
    ex, ey = (128 + (sh["eye"][0] + eye_dx - 128) * zoom) * k, (130 + (sh["eye"][1] - 130) * zoom) * k
    er = (15 if detail == 0 else 13) * k
    em = Image.new("L", (S, S), 0)
    ImageDraw.Draw(em).ellipse([ex - er, ey - er, ex + er, ey + er], fill=255)
    canvas = over(canvas, ICE, em)
    if size >= 24:
        pr = er * (0.5 if detail == 0 else 0.46)
        pm = Image.new("L", (S, S), 0)
        ImageDraw.Draw(pm).ellipse([ex - pr + k, ey - pr + k, ex + pr + k, ey + pr + k], fill=255)
        canvas = over(canvas, INK, pm)

    if detail >= 1:  # two bubbles, ring + dot, to the nose side
        bm = Image.new("L", (S, S), 0)
        bd = ImageDraw.Draw(bm)
        for (bx, by, br, bw) in ((226, 60, 11, 4.0), (210, 38, 6, 3.2)):
            bd.ellipse([(bx - br) * k, (by - br) * k, (bx + br) * k, (by + br) * k], outline=255, width=max(1, int(bw * k)))
        canvas = over(canvas, CYN + (230,), bm)

    return canvas.resize((size, size), Image.LANCZOS)


# --------------------------------------------------------------- big sizes
RAMP = " .:-=+*#%@"
SCALE_CYCLE = (0, 1, 2, 1, 0, -1)  # ramp offsets along a row: reads as scale rows
CELL_W, CELL_H = 12, 16            # glyph cell at the 512 master (6x8 at 256)
GLYPH_PX = 20                      # Plex Mono: 0.6 em advance = 12 px


def edge_glyph(gx: float, gy: float) -> str:
    """Outline glyph from the mask gradient (points inward)."""
    tx, ty = -gy, gx
    n = math.hypot(tx, ty) or 1.0
    tx, ty = tx / n, ty / n
    if abs(ty) < 0.38:
        return "-" if gy > 0 else "_"
    if abs(tx) < 0.38:
        return "|"
    return "\\" if tx * ty > 0 else "/"


def ray_glyph(dx: float, dy: float) -> str:
    n = math.hypot(dx, dy) or 1.0
    dx, dy = dx / n, dy / n
    if abs(dy) < 0.36:
        return "-"
    if abs(dx) < 0.36:
        return "|"
    return "\\" if dx * dy > 0 else "/"


def render_ascii_master(texture: float = 1.0) -> Image.Image:
    """512 px master with the dense ASCII texture."""
    S = BASE
    ss = 2
    k = S / 256.0
    big = S * ss
    canvas = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    # tile, soft magenta / cyan light, rim, faint terminal dot grid
    tile = rounded_mask((S, S), 16, 112).filter(ImageFilter.GaussianBlur(0.6))
    canvas = over(canvas, vgradient((S, S), [(0, NAVY_TOP), (0.5, NAVY_MID), (1, NAVY_BOT)]), tile)
    for (cx, cy, rad, col, a) in ((430, 60, 150, MAG, 40), (70, 450, 170, CYN, 30)):
        glow = Image.new("L", (S, S), 0)
        ImageDraw.Draw(glow).ellipse([cx - rad, cy - rad, cx + rad, cy + rad], fill=255)
        glow = glow.filter(ImageFilter.GaussianBlur(rad * 0.45))
        canvas = over(canvas, col + (a,), ImageChops.multiply(glow, tile))
    rim = ImageChops.subtract(tile, tile.filter(ImageFilter.MinFilter(5)))
    canvas = over(canvas, RIM + (210,), rim)
    dots = Image.new("L", (S, S), 0)
    dd = ImageDraw.Draw(dots)
    for y in range(40, S - 28, 24):
        for x in range(40, S - 28, 24):
            dd.rectangle([x, y, x + 1, y + 1], fill=255)
    canvas = over(canvas, (120, 140, 220, 30), ImageChops.multiply(dots, tile))

    # fish part masks (2x supersampled, reduced to the master size)
    def mask_of(name):
        return poly_mask((big, big), SHAPES[name], k * ss).resize((S, S), Image.LANCZOS)

    parts = {n: mask_of(n) for n in ("tail", "dorsal", "belly", "body")}
    fins = ImageChops.lighter(ImageChops.lighter(parts["tail"], parts["dorsal"]), parts["belly"])
    body = parts["body"]
    union = ImageChops.lighter(fins, body)

    # mid-tone base so glyphs read as light on a filled shape (keeps the mass of the small icons)
    canvas = over(canvas, vgradient((S, S), [(0.12, mix(MAG_DEEP, MAG_LO, .55)), (0.88, MAG_DEEP)]), body)
    canvas = over(canvas, mix(CYN_DEEP, CYN_LO, .4), fins)

    cols, rows = 42, 32
    ox, oy = (S - cols * CELL_W) // 2, 0
    region = (ox, oy, ox + cols * CELL_W, oy + rows * CELL_H)

    def cov(m):
        return list(m.crop(region).resize((cols, rows), Image.BOX).tobytes())

    cov_body, cov_fins = cov(body), cov(fins)
    cov_parts = {n: cov(parts[n]) for n in ("tail", "dorsal", "belly")}
    depth = body.filter(ImageFilter.GaussianBlur(26))
    grad_src = union.filter(ImageFilter.GaussianBlur(7))
    bx0, by0, bx1, by1 = body.getbbox()
    nose_x = SHAPES["nose"][0] * k
    roots = {"tail": (96 * k, 130 * k), "dorsal": (152 * k, 96 * k), "belly": (150 * k, 178 * k)}
    glyph_font = font(FONT_MEDIUM, GLYPH_PX)

    mask_layer = Image.new("L", (S, S), 0)
    color_layer = Image.new("RGB", (S, S), (0, 0, 0))
    md, cd = ImageDraw.Draw(mask_layer), ImageDraw.Draw(color_layer)

    def put(i, j, ch, color):
        x0, y0 = ox + i * CELL_W, oy + j * CELL_H
        cd.rectangle([x0, y0, x0 + CELL_W - 1, y0 + CELL_H - 1], fill=color)
        md.text((x0 + CELL_W / 2, y0 + CELL_H * 0.80), ch, font=glyph_font, fill=255, anchor="ms")

    def lum_body(cxp, cyp):
        d = depth.getpixel((int(cxp), int(cyp))) / 255.0
        top = 1.0 - (cyp - by0) / max(1, by1 - by0)
        head = max(0.0, 1.0 - abs(nose_x - 30 * k - cxp) / (80 * k))  # soft highlight near the gill
        return 0.14 + 0.52 * d ** 0.85 + 0.26 * top + 0.10 * head

    for j in range(rows):
        for i in range(cols):
            idx = j * cols + i
            cb, cf = cov_body[idx] / 255.0, cov_fins[idx] / 255.0
            if max(cb, cf) < 0.42:
                continue
            cxp, cyp = ox + (i + 0.5) * CELL_W, oy + (j + 0.5) * CELL_H
            gxp, gyp = int(min(S - 7, max(6, cxp))), int(min(S - 7, max(6, cyp)))
            gx = grad_src.getpixel((gxp + 5, gyp)) - grad_src.getpixel((gxp - 5, gyp))
            gy = grad_src.getpixel((gxp, gyp + 6)) - grad_src.getpixel((gxp, gyp - 6))
            edge = max(cb, cf) < 0.9 and (abs(gx) + abs(gy)) > 3
            if cb >= cf:
                if edge:
                    put(i, j, edge_glyph(gx, gy), mix(MAG, MAG_HI, 0.45))
                    continue
                lum = max(0.0, min(0.999, lum_body(cxp, cyp)))
                step = int(lum * (len(RAMP) - 1))
                pat = SCALE_CYCLE[(i + 2 * (j % 2)) % 6]
                gi = max(1, min(len(RAMP) - 1, step + pat - 1))
                t = gi / (len(RAMP) - 1)
                put(i, j, RAMP[gi], mix(MAG_LO, MAG_HI, t ** 0.8))
            else:
                best = max(cov_parts, key=lambda n: cov_parts[n][idx])
                rx, ry = roots[best]
                dist = math.hypot(cxp - rx, cyp - ry) / (120 * k)
                col = mix(CYN_HI, CYN, min(1.0, dist * 0.7))
                if edge:
                    put(i, j, edge_glyph(gx, gy), mix(CYN, CYN_HI, 0.7))
                else:
                    put(i, j, ray_glyph(cxp - rx, cyp - ry), col)

    # ambient glyphs: water surface, bubbles, seaweed, sand and a tiny [dm] signature
    for i in range(8, 35):
        if (i * 7) % 5 < 3:
            put(i, 3, "~", mix(CYN_DEEP, CYN_LO, 0.55 + 0.45 * math.sin(i * 1.7)))
    for (i, j, ch) in ((36, 5, "O"), (38, 8, "o"), (35, 10, "."), (39, 11, ".")):
        put(i, j, ch, mix(CYN_DEEP, CYN, 0.8))
    for n, j in enumerate(range(22, 29)):
        put(5 + (n % 2), j, ")(" [n % 2], mix(CYN_DEEP, CYN, 0.6))
        if j > 24:
            put(8 + (n % 2), j, "()"[n % 2], mix(CYN_DEEP, CYN_LO, 0.7))
    for i in range(4, cols - 4):
        if i < 31 or i > 36:
            put(i, rows - 3, ".,:."[(i * 3) % 4], mix(NAVY_BOT, VIO, 0.30))
    for n, ch in enumerate("[dm]"):
        put(31 + n, rows - 3, ch, mix(CYN_DEEP, CYN, 0.85))

    layer = color_layer.convert("RGBA")
    layer.putalpha(ImageChops.multiply(mask_layer, tile).point(lambda v: int(v * 0.95 * texture)))
    canvas = Image.alpha_composite(canvas, layer)

    # crisp vector eye on top of the texture
    ex, ey = SHAPES["eye"][0] * k * ss, SHAPES["eye"][1] * k * ss
    er = 11.0 * k * ss

    def disc(cx, cy, r):
        m = Image.new("L", (big, big), 0)
        ImageDraw.Draw(m).ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)
        return m.resize((S, S), Image.LANCZOS)

    canvas = over(canvas, INK, disc(ex, ey, er + 3.2 * ss))
    canvas = over(canvas, ICE, disc(ex, ey, er))
    canvas = over(canvas, INK, disc(ex + 2.6 * ss, ey + 0.8 * ss, er * 0.52))
    canvas = over(canvas, ICE, disc(ex + 0.2 * ss, ey - 3.4 * ss, er * 0.17))
    return canvas


# --------------------------------------------------------------- assembling
def build_images():
    master = render_ascii_master(1.0)
    out = {512: master}
    out[256] = master.resize((256, 256), Image.LANCZOS)
    # 128: clean vector mass with the texture folded in, so it stays luminous at icon-grid size
    ascii128 = render_ascii_master(0.9).resize((128, 128), Image.LANCZOS)
    out[128] = Image.blend(render_vector(128, 2), ascii128, 0.55).filter(ImageFilter.UnsharpMask(radius=0.7, percent=50, threshold=2))
    ascii64 = master.resize((64, 64), Image.LANCZOS)
    out[64] = Image.blend(render_vector(64, 2), ascii64, 0.25)
    for s in (48, 40, 32):
        out[s] = render_vector(s, 1)
    for s in (24, 20, 16):
        out[s] = render_vector(s, 0)
    return out


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
    W, H = 1520, 1250
    sheet = Image.new("RGB", (W, H), rgb("0a0b13"))
    d = ImageDraw.Draw(sheet)
    f_title, f_head, f_small = font(FONT_MEDIUM, 22), font(FONT_MEDIUM, 14), font(FONT_REGULAR, 12)
    muted, text, cyn = rgb("9a97b0"), rgb("ecebf6"), CYN

    d.text((32, 24), "DumbManager Rust - icon contact sheet", font=f_title, fill=text)
    d.text((32, 56), f"generated by tools/generate-art.py  |  Pillow {PIL_VERSION}  |  ICO frames: " + " ".join(map(str, ICO_SIZES)), font=f_small, fill=muted)

    def label(x, y, t, c=muted):
        d.text((x, y), t, font=f_small, fill=c)

    backgrounds = [("dark taskbar", (32, 32, 36)), ("light window", (243, 243, 247)), ("accent colour", (36, 79, 140))]
    small = (16, 20, 24, 32, 40, 48, 64)

    # 1) actual size on three backgrounds
    y = 96
    d.text((32, y), "1. Actual size (1:1 pixels, as Windows draws them)", font=f_head, fill=cyn)
    y += 30
    sw_h, gap = 84, 8
    x = 32
    for s in small:
        sw = max(s + 20, 84)
        for r, (_, bg) in enumerate(backgrounds):
            yy = y + r * (sw_h + gap)
            d.rectangle([x, yy, x + sw - 1, yy + sw_h - 1], fill=bg)
            paste_icon(sheet, images[s], (x + (sw - s) // 2, yy + (sw_h - s) // 2))
        label(x, y + 3 * (sw_h + gap) + 2, f"{s}px")
        x += sw + 16
    for r, (name, _) in enumerate(backgrounds):
        label(x + 8, y + r * (sw_h + gap) + 34, name)
    y += 3 * (sw_h + gap) + 44

    # 2) magnified, nearest neighbour
    d.text((32, y), "2. Same pixels magnified (nearest neighbour) - silhouette check", font=f_head, fill=cyn)
    y += 30
    x = 32
    plate_h = 0
    for s, z in ((16, 9), (20, 7), (24, 6), (32, 5), (40, 4), (48, 3), (64, 2)):
        big = images[s].resize((s * z, s * z), Image.NEAREST)
        plate = checker((s * z + 12, s * z + 12))
        sheet.paste(plate, (x, y))
        paste_icon(sheet, big, (x + 6, y + 6))
        label(x, y + 178, f"{s}px x{z}")
        x += s * z + 28
        plate_h = max(plate_h, s * z + 12)
    y += 204

    # 3) in context
    d.text((32, y), "3. In context", font=f_head, fill=cyn)
    y += 30
    d.rectangle([32, y, 452, y + 34], fill=(32, 32, 36))
    paste_icon(sheet, images[16], (44, y + 9))
    d.text((70, y + 9), "DumbManager Rust", font=f_small, fill=(240, 240, 244))
    d.rectangle([32, y + 44, 452, y + 78], fill=(243, 243, 247))
    paste_icon(sheet, images[16], (44, y + 53))
    d.text((70, y + 53), "DumbManager Rust", font=f_small, fill=(26, 26, 30))
    d.rectangle([32, y + 98, 452, y + 146], fill=(28, 28, 32))
    for n, (s, name) in enumerate(((24, "pinned"), (24, "running"), (32, "150% dpi"))):
        bx = 56 + n * 64
        d.rounded_rectangle([bx - 12, y + 102, bx + 36, y + 142], radius=6, fill=(52, 52, 58) if name == "running" else (28, 28, 32))
        paste_icon(sheet, images[s], (bx + 12 - s // 2, y + 122 - s // 2))
        if name == "running":
            d.rounded_rectangle([bx + 2, y + 138, bx + 22, y + 140], radius=1, fill=cyn)
    label(262, y + 114, "24 / 24 running / 32")
    for n, (s, plate) in enumerate(((48, (243, 243, 247)), (64, (32, 32, 36)), (48, (32, 32, 36)))):
        bx = 480 + n * 170
        d.rounded_rectangle([bx, y, bx + 150, y + 146], radius=8, fill=plate)
        paste_icon(sheet, images[s], (bx + 75 - s // 2, y + 18 + (64 - s) // 2))
        d.text((bx + 75, y + 112), "DumbManager Rust", font=font(FONT_REGULAR, 11), fill=(30, 30, 34) if plate[0] > 128 else (230, 230, 236), anchor="mm")
        label(bx, y + 155, ("File / light", "File / dark", "Shortcut / dark")[n])
    # Light taskbar at native 20/24/32 px, separate from the file-grid plates.
    d.rectangle([1016, y + 98, 1488, y + 146], fill=(235, 236, 241))
    for n, s in enumerate((20, 24, 32)):
        bx = 1040 + n * 64
        if n == 1:
            d.rounded_rectangle([bx - 12, y + 102, bx + 36, y + 142], radius=6, fill=(214, 216, 224))
        paste_icon(sheet, images[s], (bx + 12 - s // 2, y + 122 - s // 2))
        if n == 1:
            d.rounded_rectangle([bx + 2, y + 138, bx + 22, y + 140], radius=1, fill=(30, 116, 145))
    label(1016, y + 155, "Light taskbar / 20, 24 running, 32 px")
    y += 190

    # 4) large sizes, 1:1
    d.text((32, y), "4. Large sizes 1:1 - 128, 256 (128x128@2x.png), icon.png (512): 1:1 centre crop and scaled", font=f_head, fill=cyn)
    y += 30
    sheet.paste(Image.new("RGB", (148, 148), (32, 32, 36)), (32, y))
    paste_icon(sheet, images[128], (42, y + 10))
    label(32, y + 154, "128")
    sheet.paste(Image.new("RGB", (276, 276), (243, 243, 247)), (200, y))
    paste_icon(sheet, images[256], (210, y + 10))
    label(200, y + 282, "256")
    sheet.paste(Image.new("RGB", (276, 276), (32, 32, 36)), (496, y))
    paste_icon(sheet, images[512].crop((96, 128, 96 + 256, 128 + 256)), (506, y + 10))
    label(496, y + 282, "icon.png 512 - 1:1 centre crop")
    sheet.paste(Image.new("RGB", (276, 276), (10, 11, 19)), (792, y))
    paste_icon(sheet, images[512].resize((256, 256), Image.LANCZOS), (802, y + 10))
    label(792, y + 282, "icon.png 512 - scaled to 256")
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
    sheet = contact_sheet(images)
    written.append((previews / "icon-contact-sheet.png", png_bytes(sheet)))

    report = []
    for path, data in written:
        path.write_bytes(data)
        report.append((str(path.relative_to(root)).replace("\\", "/"), hashlib.sha256(data).hexdigest()[:16] + f"  {len(data):>7} B"))

    if verify:
        check_outputs(icons, images)
    return report


def check_outputs(icons: Path, images: dict[int, Image.Image]) -> None:
    ico = Image.open(icons / "icon.ico")
    found = sorted(ico.info.get("sizes", []))
    want = sorted((s, s) for s in ICO_SIZES)
    assert found == want, f"ICO sizes {found} != {want}"
    for s in ICO_SIZES:
        frame = ico.ico.getimage((s, s)).convert("RGBA")
        diff = ImageChops.difference(frame, images[s].convert("RGBA"))
        worst = max(diff.tobytes())
        assert worst <= 1, f"ICO frame {s}px differs from source by {worst}"
    for name, s in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        im = Image.open(icons / name)
        assert im.size == (s, s) and im.mode == "RGBA", f"{name}: {im.size} {im.mode}"
    print("verify: ICO frames", [f"{s}" for s in ICO_SIZES], "match source; PNG sizes OK")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--verify", action="store_true", help="re-open the outputs and check sizes/pixels")
    ap.add_argument("--out-root", type=Path, default=REPO, help="write under this root instead of the repo")
    args = ap.parse_args()
    if not FONT_MEDIUM.exists():
        print(f"warning: {FONT_MEDIUM} missing; texture falls back to PIL's default font", file=sys.stderr)
    for rel, digest in write_all(args.out_root, args.verify):
        print(f"{rel:<44} {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
