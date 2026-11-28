"""Wordmark specimen: ten lowercase `nori` treatments rendered as terminal cells.

Writes `specimen.png` (exact geometric rendering of the cells) and
`specimen.ans` (truecolor ANSI, `cat` it in a real terminal to compare).

Run: python specimen.py  (needs Pillow)
"""

import math
import os
import random


HERE = os.path.dirname(os.path.abspath(__file__))
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"

BG = (0x16, 0x18, 0x1c)
RICE = [(0x52, 0x56, 0x5E), (0x8A, 0x8F, 0x98), (0xC1, 0xC7, 0xCD), (0xE4, 0xE8, 0xED), (0xFF, 0xFF, 0xFF)]
NORI = [(0x10, 0x16, 0x12), (0x17, 0x25, 0x1B), (0x1B, 0x3A, 0x27), (0x2C, 0x6B, 0x41)]
GREEN = (0x42, 0xBE, 0x65)
INK = RICE[3]
LABEL = RICE[1]


def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


# ---------------------------------------------------------------- letterforms
# Centerline strokes in x-height units (x right, y down, x-height 0..1).
HW = 0.13  # half stroke width


def d_seg(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def d_arc(px, py, cx, cy, r, a0, a1):
    """Distance to an arc; angles in degrees, y up (screen y is flipped)."""
    if a1 - a0 >= 360:
        return abs(math.hypot(px - cx, py - cy) - r)
    ang = math.degrees(math.atan2(cy - py, px - cx)) % 360
    lo, hi = a0 % 360, a1 % 360
    inside = lo <= ang <= hi if lo <= hi else (ang >= lo or ang <= hi)
    if inside:
        return abs(math.hypot(px - cx, py - cy) - r)
    best = 1e9
    for a in (a0, a1):
        ex, ey = cx + r * math.cos(math.radians(a)), cy - r * math.sin(math.radians(a))
        best = min(best, math.hypot(px - ex, py - ey))
    return best


B = 1 - HW  # baseline for stroke ends
LETTERS = {
    # name: (width, distance fn)
    "n": (0.92, lambda x, y: min(
        d_seg(x, y, HW, HW, HW, B),
        d_arc(x, y, 0.46, 0.46, 0.33, 0, 180),
        d_seg(x, y, 0.79, 0.46, 0.79, B))),
    "o": (1.0, lambda x, y: d_arc(x, y, 0.5, 0.5, 0.5 - HW, 0, 360)),
    "r": (0.78, lambda x, y: min(
        d_seg(x, y, HW, HW, HW, B),
        d_arc(x, y, 0.5, 0.5, 0.5 - HW, 50, 180))),
    "i": (2 * HW, lambda x, y: d_seg(x, y, HW, HW, HW, B)),
}
GAP = 0.26
DOT_Y = (-0.48, -0.2)


def layout():
    xs, x = [], 0.0
    for ch in "nori":
        xs.append(x)
        x += LETTERS[ch][0] + GAP
    return xs, x - GAP


XS, WORD_W = layout()


def sdf(x, y):
    """Signed distance in x-height units; also returns 'dot' flag."""
    best = 1e9
    for ch, ox in zip("nori", XS):
        w, f = LETTERS[ch]
        if ox - 0.3 <= x <= ox + w + 0.3:
            best = min(best, f(x - ox, y) - HW)
    ox = XS[3]
    # square i dot, slightly rounded
    qx, qy = abs(x - (ox + HW)) - HW, abs(y - (DOT_Y[0] + DOT_Y[1]) / 2) - (DOT_Y[1] - DOT_Y[0]) / 2
    dd = math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - 0.02
    if dd < best:
        return dd, True
    return best, False


TOP = DOT_Y[0] - 0.04


def coverage(u0, v0, u1, v1, scale, n=4):
    """Fraction of ink in a box given in cell units (1 wide, 2 tall per cell
    in square units). scale = cells... x-height in square units."""
    ink = dot = 0
    for j in range(n):
        for i in range(n):
            x = (u0 + (u1 - u0) * (i + 0.5) / n) / scale
            y = (v0 + (v1 - v0) * (j + 0.5) / n) / scale + TOP
            d, isdot = sdf(x, y)
            if d < 0:
                ink += 1
                dot += isdot
    tot = n * n
    return ink / tot, (dot / ink if ink else 0)


def grid_size(scale):
    cols = math.ceil(WORD_W * scale) + 1
    rows = math.ceil((1 - TOP) * scale / 2)
    return cols, rows


# ---------------------------------------------------------------- cells
class Cell:
    __slots__ = ("ch", "fg", "bg", "bits")

    def __init__(self, ch=" ", fg=INK, bg=None, bits=None):
        self.ch, self.fg, self.bg, self.bits = ch, fg, bg, bits


def subgrid(scale, sx, sy):
    """Return rows of cells; each cell has an sx*sy grid of (cov, dot)."""
    cols, rows = grid_size(scale)
    out = []
    for r in range(rows):
        row = []
        for c in range(cols):
            sub = []
            for j in range(sy):
                for i in range(sx):
                    u0, u1 = c + i / sx, c + (i + 1) / sx
                    v0, v1 = 2 * r + 2 * j / sy, 2 * r + 2 * (j + 1) / sy
                    sub.append(coverage(u0, v0, u1, v1, scale, 3))
            row.append(sub)
        out.append(row)
    return out


def ink_color(dotfrac, cov=1.0):
    base = mix(INK, GREEN, 1 if dotfrac > 0.5 else 0)
    return base


# Style 1: half blocks with anti-aliased colours ("mochi")
def s_halfblock(scale):
    g = subgrid(scale, 1, 2)
    rows = []
    for row in g:
        out = []
        for (tc, td), (bc, bd) in row:
            top = mix(BG, ink_color(td), tc)
            bot = mix(BG, ink_color(bd), bc)
            if tc < 0.04 and bc < 0.04:
                out.append(Cell())
            else:
                out.append(Cell("▀", top, bot, ("half", top, bot)))
        rows.append(out)
    return rows


QUADS = {0: " ", 1: "▘", 2: "▝", 3: "▀", 4: "▖", 5: "▌", 6: "▞", 7: "▛",
         8: "▗", 9: "▚", 10: "▐", 11: "▜", 12: "▄", 13: "▙", 14: "▟", 15: "█"}


def s_quadrant(scale):
    g = subgrid(scale, 2, 2)
    rows = []
    for row in g:
        out = []
        for sub in row:
            m = sum(1 << k for k, (cv, _) in enumerate(sub) if cv >= 0.5)
            dot = any(d > 0.5 for cv, d in sub if cv >= 0.5)
            out.append(Cell(QUADS[m], GREEN if dot else INK, None, ("mask", 2, 2, m)))
        rows.append(out)
    return rows


def sextant_char(m):
    if m == 0:
        return " "
    if m == 63:
        return "█"
    if m == 21:
        return "▌"
    if m == 42:
        return "▐"
    idx = m - 1
    if m > 21:
        idx -= 1
    if m > 42:
        idx -= 1
    return chr(0x1FB00 + idx)


def s_sextant(scale):
    g = subgrid(scale, 2, 3)
    rows = []
    for row in g:
        out = []
        for sub in row:
            m = sum(1 << k for k, (cv, _) in enumerate(sub) if cv >= 0.5)
            dot = any(d > 0.5 for cv, d in sub if cv >= 0.5)
            out.append(Cell(sextant_char(m), GREEN if dot else INK, None, ("mask", 2, 3, m)))
        rows.append(out)
    return rows


BRAILLE = [(0, 0, 0x01), (0, 1, 0x02), (0, 2, 0x04), (1, 0, 0x08), (1, 1, 0x10), (1, 2, 0x20), (0, 3, 0x40), (1, 3, 0x80)]


def s_braille(scale):
    g = subgrid(scale, 2, 4)
    rows = []
    for row in g:
        out = []
        for sub in row:
            bits, dot = 0, False
            for x, y, b in BRAILLE:
                cv, d = sub[y * 2 + x]
                if cv >= 0.5:
                    bits |= b
                    dot |= d > 0.5
            out.append(Cell(chr(0x2800 + bits), GREEN if dot else INK, None, ("braille", bits)))
        rows.append(out)
    return rows


def s_shaded(scale, seed=7):
    """Rice paper: strokes lit like tubes from the upper left, ░▒▓█ + grain."""
    rnd = random.Random(seed)
    cols, rows_n = grid_size(scale)
    rows = []
    for r in range(rows_n):
        out = []
        for c in range(cols):
            cov, dot = coverage(c, 2 * r, c + 1, 2 * r + 2, scale, 4)
            if cov < 0.45:
                out.append(Cell())
                continue
            if dot > 0.5:
                out.append(Cell("█", GREEN, None, ("shade", 4)))
                continue
            x, y = (c + 0.5) / scale, (2 * r + 1) / scale + TOP
            e = 0.02
            gx = sdf(x + e, y)[0] - sdf(x - e, y)[0]
            gy = sdf(x, y + e)[0] - sdf(x, y - e)[0]
            n = math.hypot(gx, gy) or 1
            depth = -sdf(x, y)[0] / HW  # 0 at edge, 1 at centerline
            lit = (-gx / n * 0.7 - gy / n * 0.7) * (1 - depth) + 0.55 + rnd.uniform(-0.18, 0.18)
            lvl = 4 if lit > 0.75 else 3 if lit > 0.45 else 2 if lit > 0.2 else 1
            out.append(Cell({4: "█", 3: "▓", 2: "▒", 1: "░"}[lvl], INK, None, ("shade", lvl)))
        rows.append(out)
    return rows


def s_sheet(scale):
    """Letters knocked out of a seaweed sheet; the sheet is the half-block bg."""
    g = subgrid(scale, 1, 2)
    rows = []
    pad = 2
    for row in [None] + g + [None]:
        out = [Cell(" ", INK, NORI[2], ("half", NORI[2], NORI[2])) for _ in range(pad)]
        if row is None:
            out += [Cell(" ", INK, NORI[2], ("half", NORI[2], NORI[2])) for _ in range(len(g[0]))]
        else:
            for (tc, td), (bc, bd) in row:
                ti = GREEN if td > 0.5 else RICE[4]
                bi = GREEN if bd > 0.5 else RICE[4]
                top = mix(NORI[2], ti if td <= 0.5 else NORI[0], tc)
                bot = mix(NORI[2], bi if bd <= 0.5 else NORI[0], bc)
                out.append(Cell("▀", top, bot, ("half", top, bot)))
        out += [Cell(" ", INK, NORI[2], ("half", NORI[2], NORI[2])) for _ in range(pad)]
        rows.append(out)
    return rows


def s_extruded(scale):
    """Rice letters with a solid seaweed extrusion down and to the right."""
    cols, rows_n = grid_size(scale)
    cols += 2
    off = 1.6  # extrusion depth in half-cell pixels
    rows = []
    for r in range(rows_n + 1):
        out = []
        for c in range(cols):
            px = []
            for k in range(2):
                v0 = 2 * r + k
                cov, dot = coverage(c, v0, c + 1, v0 + 1, scale, 4)
                sh = 0.0
                for t in (0.5, 1.0):
                    sc, _ = coverage(c - off * t, v0 - off * t, c + 1 - off * t, v0 + 1 - off * t, scale, 3)
                    sh = max(sh, sc)
                base = mix(BG, NORI[3], sh)
                px.append(mix(base, GREEN if dot > 0.5 else INK, cov))
            if px[0] == BG and px[1] == BG:
                out.append(Cell())
            else:
                out.append(Cell("▀", px[0], px[1], ("half", px[0], px[1])))
        rows.append(out)
    return rows


# ----- hand-built box-drawing styles
def text_rows(lines, dot_cells=()):
    rows = []
    for r, line in enumerate(lines):
        out = []
        for c, ch in enumerate(line):
            out.append(Cell(ch, GREEN if (r, c) in dot_cells else INK, None, ("text", ch)))
        rows.append(out)
    return rows


def find_dots(lines, marker):
    cells, clean = set(), []
    for r, line in enumerate(lines):
        for c, ch in enumerate(line):
            if ch == marker:
                cells.add((r, c))
        clean.append(line.replace(marker, "■"))
    return clean, cells


def s_rounded():
    lines = [
        "                @",
        "╭───╮ ╭───╮ ╭── ╷",
        "│   │ │   │ │   │",
        "╵   ╵ ╰───╯ ╵   ╵",
    ]
    lines, dots = find_dots(lines, "@")
    return text_rows(lines, dots)


def s_rounded_small():
    lines = [
        "╭─╮╭─╮╭─ ▪",
        "╵ ╵╰─╯╵  │",
    ]
    rows = text_rows(lines)
    rows[0][9].fg = GREEN
    return rows


def s_heavy():
    lines = [
        "              @",
        "┏━━━┓ ┏━━━┓ ┏━━╸ ╻",
        "┃   ┃ ┃   ┃ ┃    ┃",
        "╹   ╹ ┗━━━┛ ╹    ╹",
    ]
    lines, dots = find_dots(lines, "@")
    return text_rows(lines, dots)


SHADOW = {
    "n": ["        ", "        ", "██████╗ ", "██╔══██╗", "██║  ██║", "██║  ██║", "╚═╝  ╚═╝"],
    "o": ["         ", "         ", " ██████╗ ", "██╔═══██╗", "██║   ██║", "╚██████╔╝", " ╚═════╝ "],
    "r": ["        ", "        ", "██╗ ███╗", "███████║", "██╔════╝", "██║     ", "╚═╝     "],
    "i": ["██╗", "╚═╝", "██╗", "██║", "██║", "██║", "╚═╝"],
}


def s_shadow():
    """ANSI Shadow figlet look, redrawn lowercase."""
    lines = [" ".join(SHADOW[ch][k] for ch in "nori") for k in range(7)]
    rows = []
    for r, line in enumerate(lines):
        out = []
        for c, ch in enumerate(line):
            dot = r < 2 and c >= len(line) - 3
            if ch == "█":
                out.append(Cell(ch, GREEN if dot else INK, None, ("text", ch)))
            else:
                out.append(Cell(ch, NORI[3] if dot else RICE[1], None, ("text", ch)))
        rows.append(out)
    return rows


def s_tiny_text():
    """One-row fallbacks using Unicode letter styles."""
    variants = ["nori", "𝗻𝗼𝗿𝗶", "ｎｏｒｉ", "ɴᴏʀɪ"]
    return variants


# ---------------------------------------------------------------- render
CW, CH = 14, 28  # cell size in px for the PNG


def draw_cell(d, img, x0, y0, cell, font):
    if cell.bg:
        d.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=cell.bg)
    b = cell.bits
    if b is None:
        return
    kind = b[0]
    if kind == "half":
        d.rectangle([x0, y0, x0 + CW - 1, y0 + CH // 2 - 1], fill=b[1])
        d.rectangle([x0, y0 + CH // 2, x0 + CW - 1, y0 + CH - 1], fill=b[2])
    elif kind == "mask":
        sx, sy, m = b[1], b[2], b[3]
        for k in range(sx * sy):
            if m >> k & 1:
                i, j = k % sx, k // sx
                d.rectangle([x0 + i * CW // sx, y0 + j * CH // sy,
                             x0 + (i + 1) * CW // sx - 1, y0 + (j + 1) * CH // sy - 1], fill=cell.fg)
    elif kind == "braille":
        for x, y, bit in BRAILLE:
            if b[1] & bit:
                cx, cy = x0 + CW * (0.3 + 0.4 * x), y0 + CH * (0.125 + 0.25 * y)
                rr = CW * 0.13
                d.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], fill=cell.fg)
    elif kind == "shade":
        lvl = b[1]
        if lvl == 4:
            d.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=cell.fg)
        else:
            dens = {1: 0.25, 2: 0.5, 3: 0.75}[lvl]
            step = 2
            for yy in range(0, CH, step):
                for xx in range(0, CW, step):
                    k = ((xx // step) + (yy // step) * 2) % 4
                    if (k + 0.5) / 4 < dens or (lvl == 2 and ((xx // step) + (yy // step)) % 2 == 0):
                        d.rectangle([x0 + xx, y0 + yy, x0 + xx + step - 1, y0 + yy + step - 1], fill=cell.fg)
    elif kind == "text" and b[1] in BOX:
        draw_box(d, x0, y0, BOX[b[1]], cell.fg)
    elif kind == "text":
        ch = b[1]
        if ch != " ":
            d.text((x0, y0 + CH * 0.5), ch, font=font, fill=cell.fg, anchor="lm")


# char -> (up, down, left, right) weights: 1 light, 2 heavy, 3 double; "r" rounded
BOX = {
    "─": (0, 0, 1, 1), "│": (1, 1, 0, 0), "╵": (1, 0, 0, 0), "╷": (0, 1, 0, 0),
    "╴": (0, 0, 1, 0), "╶": (0, 0, 0, 1),
    "╭": "r╭", "╮": "r╮", "╰": "r╰", "╯": "r╯",
    "━": (0, 0, 2, 2), "┃": (2, 2, 0, 0), "┏": (0, 2, 0, 2), "┓": (0, 2, 2, 0),
    "┗": (2, 0, 0, 2), "┛": (2, 0, 2, 0), "╹": (2, 0, 0, 0), "╻": (0, 2, 0, 0), "╸": (0, 0, 2, 0),
    "═": (0, 0, 3, 3), "║": (3, 3, 0, 0), "╔": (0, 3, 0, 3), "╗": (0, 3, 3, 0),
    "╚": (3, 0, 0, 3), "╝": (3, 0, 3, 0), "█": "full", "■": "square",
}


def draw_box(d, x0, y0, spec, fg):
    cx, cy = x0 + CW / 2, y0 + CH / 2
    if spec == "full":
        d.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=fg)
        return
    if spec == "square":
        d.rectangle([x0 + CW * 0.15, cy - CW * 0.35, x0 + CW * 0.85, cy + CW * 0.35], fill=fg)
        return
    t = 2
    if isinstance(spec, str):
        corner = spec[1]
        rr = CW / 2
        box = {"╭": (cx, cy, cx + 2 * rr, cy + 2 * rr, 180, 270),
               "╮": (cx - 2 * rr, cy, cx, cy + 2 * rr, 270, 360),
               "╰": (cx, cy - 2 * rr, cx + 2 * rr, cy, 90, 180),
               "╯": (cx - 2 * rr, cy - 2 * rr, cx, cy, 0, 90)}[corner]
        d.arc(box[:4], box[4], box[5], fill=fg, width=t)
        if corner in "╭╮":
            d.rectangle([cx - t / 2, cy + rr, cx + t / 2 - 1, y0 + CH - 1], fill=fg)
        else:
            d.rectangle([cx - t / 2, y0, cx + t / 2 - 1, cy - rr], fill=fg)
        return
    up, down, left, right = spec
    def vline(ya, yb, w):
        if w == 3:
            for off in (-3, 3):
                d.rectangle([cx + off - 1, ya, cx + off, yb], fill=fg)
        elif w:
            hw = 1 if w == 1 else 2
            d.rectangle([cx - hw, ya, cx + hw - 1, yb], fill=fg)
    def hline(xa, xb, w):
        if w == 3:
            for off in (-3, 3):
                d.rectangle([xa, cy + off - 1, xb, cy + off], fill=fg)
        elif w:
            hw = 1 if w == 1 else 2
            d.rectangle([xa, cy - hw, xb, cy + hw - 1], fill=fg)
    ext = 3 if 3 in spec else 2
    vline(y0, cy + (ext if (left or right) else 0), up)
    vline(cy - (ext if (left or right) else 0), y0 + CH - 1, down)
    hline(x0, cx + (ext if (up or down) else 0), left)
    hline(cx - (ext if (up or down) else 0), x0 + CW - 1, right)


def render_rows(rows):
    return rows


def ansi(rows):
    out = []
    for row in rows:
        s = []
        for c in row:
            if c.bg:
                s.append("\x1b[48;2;%d;%d;%dm" % c.bg)
            else:
                s.append("\x1b[49m")
            s.append("\x1b[38;2;%d;%d;%dm%s" % (c.fg + (c.ch,)))
        out.append("".join(s) + "\x1b[0m")
    return "\n".join(out)


def main():
    from PIL import Image, ImageDraw, ImageFont

    options = [
        ("1  mochi: half blocks + anti-aliased colour", "any font", [s_halfblock(4.0), s_halfblock(8.0)]),
        ("2  smooth: sextants (2x3 mosaic)", "Ghostty/kitty/WezTerm; not Menlo", [s_sextant(4.0), s_sextant(8.0)]),
        ("3  pixel: quadrants (2x2 mosaic)", "any font", [s_quadrant(4.0), s_quadrant(8.0)]),
        ("4  dot matrix: braille (2x4)", "most fonts; faint", [s_braille(4.0), s_braille(8.0)]),
        ("5  rice paper: ░▒▓ lit + grain noise", "any font", [s_shaded(6.0), s_shaded(9.0)]),
        ("6  extruded: rice letters on a seaweed block shadow", "any font", [s_extruded(4.0), s_extruded(8.0)]),
        ("7  nori sheet: letters knocked out of seaweed", "any font", [s_sheet(4.0), s_sheet(8.0)]),
        ("8  shadow: ANSI Shadow figlet, lowercase", "any font", [s_shadow()]),
        ("9  rounded line: ╭─╮ box drawing", "any font", [s_rounded_small(), s_rounded()]),
        ("10 heavy line: toilet 'future' look", "any font", [s_heavy()]),
    ]
    font = ImageFont.truetype(FONT, int(CH * 0.8))
    lfont = ImageFont.truetype(FONT, 20)
    pad, gapx, gapy = 30, 4, 2
    blocks = []
    for title, support, variants in options:
        h = max(len(v) for v in variants)
        w = sum(len(v[0]) for v in variants) + gapx * (len(variants) - 1)
        blocks.append((title, support, variants, w, h))
    width = pad * 2 + max(b[3] for b in blocks) * CW
    height = pad + sum(36 + b[4] * CH + gapy * CH for b in blocks)
    img = Image.new("RGB", (width, height), BG)
    d = ImageDraw.Draw(img)
    y = pad
    ans = []
    for title, support, variants, w, h in blocks:
        d.text((pad, y), title, font=lfont, fill=RICE[3])
        tw = d.textlength(title, font=lfont)
        d.text((pad + tw + 16, y), "· " + support, font=lfont, fill=LABEL)
        y += 36
        x = pad
        ans.append("\x1b[1m%s\x1b[0m  \x1b[2m%s\x1b[0m" % (title, support))
        for v in variants:
            off = h - len(v)
            for r, row in enumerate(v):
                for c, cell in enumerate(row):
                    draw_cell(d, img, x + c * CW, y + (r + off) * CH, cell, font)
            x += (len(v[0]) + gapx) * CW
            ans.append(ansi(v))
        ans.append("")
        y += h * CH + gapy * CH
    img.save(os.path.join(HERE, "specimen.png"))
    with open(os.path.join(HERE, "specimen.ans"), "w") as f:
        f.write("\n".join(ans) + "\n")


if __name__ == "__main__":
    main()
