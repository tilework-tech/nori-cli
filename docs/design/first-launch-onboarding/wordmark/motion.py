"""Motion studies for wordmark styles 9 (rounded line) and 4 (braille).

Every frame is a grid of terminal cells: one character, one foreground and one
optional background colour per cell, so each effect is something a real
terminal can show.

    python motion.py                 # write motion/*.gif (needs Pillow)
    python motion.py --play 9.2      # play one effect in this terminal (truecolor)
    python motion.py --list
"""

import math
import os
import random
import sys
import time

import specimen as sp
from specimen import BG, GREEN, INK, NORI, RICE, Cell, mix

FPS = 25
OUT = os.path.join(sp.HERE, "motion")
WHITE = (0xFF, 0xFF, 0xFF)
WARM = (0xFF, 0xF8, 0xE8)
DIM = (0x5A, 0x5F, 0x68)


def ease_out(x):
    x = max(0.0, min(1.0, x))
    return 1 - (1 - x) ** 3


def ease_in_out(x):
    x = max(0.0, min(1.0, x))
    return 3 * x * x - 2 * x * x * x


def blank(cols, rows):
    return [[Cell() for _ in range(cols)] for _ in range(rows)]


def put(grid, r, c, ch, fg, bg=None):
    if 0 <= r < len(grid) and 0 <= c < len(grid[0]):
        cell = grid[r][c]
        cell.ch, cell.fg, cell.bits = ch, fg, ("text", ch)
        if bg is not None:
            cell.bg = bg


# ================================================================ style 9
WORD9 = [
    "                @",
    "╭───╮ ╭───╮ ╭── ╷",
    "│   │ │   │ │   │",
    "╵   ╵ ╰───╯ ╵   ╵",
]
DOT9 = (0, 16)
GLYPHS9 = {(r, c): ch for r, line in enumerate(WORD9) for c, ch in enumerate(line) if ch not in " @"}
LETTER9 = {(r, c): (0 if c <= 4 else 1 if c <= 10 else 2 if c <= 14 else 3) for r, c in GLYPHS9}
LETTER9[DOT9] = 3

# Pen order: how a hand would write each stroke.
STROKES9 = [
    [(3, 0), (2, 0), (1, 0), (1, 1), (1, 2), (1, 3), (1, 4), (2, 4), (3, 4)],
    [(1, 9), (1, 8), (1, 7), (1, 6), (2, 6), (3, 6), (3, 7), (3, 8), (3, 9), (3, 10), (2, 10), (1, 10)],
    [(3, 12), (2, 12), (1, 12), (1, 13), (1, 14)],
    [(1, 16), (2, 16), (3, 16)],
]
# Half glyph shown while the pen is still entering a cell from a side.
ENTER_STUB = {(0, 1): "╴", (0, -1): "╶", (1, 0): "╵", (-1, 0): "╷"}

C9, R9, OFF9 = 31, 9, (3, 7)


def at9(grid, r, c, ch, fg, bg=None):
    put(grid, r + OFF9[0], c + OFF9[1], ch, fg, bg)


def dist_field(points, cols, rows, weight):
    """Glow field: sum of exp falloff from weighted cells (a cell is 1 wide, 2 tall)."""
    field = [[0.0] * cols for _ in range(rows)]
    for (pr, pc), w in zip(points, weight):
        if w <= 0:
            continue
        for r in range(max(0, pr - 3), min(rows, pr + 4)):
            for c in range(max(0, pc - 6), min(cols, pc + 7)):
                d = math.hypot(c - pc, 2 * (r - pr))
                field[r][c] += w * math.exp(-d / 1.7)
    return field


def e9_pen(t):
    """9.1 Pen draw-on: one continuous stroke per letter with a spark at the nib."""
    speed = 26.0  # half-cell steps per second
    steps, full_at = [], {}
    for k, stroke in enumerate(STROKES9):
        for i, cell in enumerate(stroke):
            if i:
                pr, pc = stroke[i - 1]
                steps.append(("stub", cell, ENTER_STUB[(cell[0] - pr, cell[1] - pc)]))
            steps.append(("full", cell, GLYPHS9[cell]))
            full_at[cell] = len(steps) - 1
        steps.extend([("lift", None, None)] * (4 if k < 3 else 2))
    done_t = len(steps) / speed
    s = int(t * speed)
    g = blank(C9, R9)
    fade_out = 1 - ease_in_out((t - 4.1) / 0.4)
    flash = math.exp(-max(0.0, t - done_t - 0.3) / 0.25) if t > done_t + 0.3 else 0.0
    for i in range(min(s, len(steps))):
        kind, cell, ch = steps[i]
        if kind != "full":
            continue
        age = (s - i) / speed
        fg = mix(WARM, INK, age / 0.45)
        fg = mix(fg, WHITE, flash)
        at9(g, *cell, ch, mix(BG, fg, fade_out))
    head = None
    if s < len(steps):
        kind, cell, ch = steps[s]
        if kind != "lift":
            head = cell
            bg = mix(BG, NORI[3], 0.8)
            at9(g, *cell, ch, WHITE, bg)
    if head:
        rnd = random.Random(s * 7919)
        for _ in range(3):
            dr, dc = rnd.randint(-1, 1), rnd.randint(-2, 2)
            if (dr, dc) == (0, 0) or (head[0] + dr, head[1] + dc) in GLYPHS9:
                continue
            spark = rnd.choice("·˙∙⁺*")
            at9(g, head[0] + dr, head[1] + dc, spark, rnd.choice([GREEN, WARM, RICE[2]]))
    pop = t - (done_t + 0.15)
    if pop > 0:
        ch = "▪" if pop < 0.08 else "■"
        at9(g, *DOT9, ch, mix(BG, mix(WHITE, GREEN, pop / 0.3), fade_out))
    return g


def neon_levels(t):
    """Per-letter tube intensity: stuttering ignition, an idle buzz, then off."""
    lv = []
    for k in range(4):
        start = 0.35 + 0.32 * k
        x = t - start
        if x < 0:
            v = 0.0
        elif x < 0.45:
            pattern = [1, 0, 0, 1, 1, 0, 1, 0.2, 1, 1, 1]
            v = pattern[min(len(pattern) - 1, int(x * 24))]
        else:
            v = 1.0
        if k == 2 and 3.0 < t < 3.12:
            v = 0.25
        if k == 1 and (3.6 < t < 3.66 or 3.74 < t < 3.78):
            v = 0.4
        lv.append(v * (1 - ease_in_out((t - 4.6) / 0.25)))
    return lv


def e9_neon(t):
    """9.2 Neon tube: bright core, green glow in the background of nearby cells."""
    lv = neon_levels(t)
    cells = list(GLYPHS9) + [DOT9]
    pts = [(r + OFF9[0], c + OFF9[1]) for r, c in cells]
    field = dist_field(pts, C9, R9, [lv[LETTER9[cell]] for cell in cells])
    g = blank(C9, R9)
    for r in range(R9):
        for c in range(C9):
            f = 0.7 * (1 - math.exp(-field[r][c] * 0.22)) ** 1.6
            if f > 0.02:
                g[r][c].bg = mix(BG, (0x1E, 0x6E, 0x40), f)
    core, off = (0xDC, 0xFF, 0xE6), (0x2A, 0x33, 0x2E)
    for cell in GLYPHS9:
        v = lv[LETTER9[cell]]
        at9(g, *cell, GLYPHS9[cell], mix(off, core, v))
    at9(g, *DOT9, "■", mix(off, (0x7C, 0xFF, 0x9E), lv[3]))
    return g


PATH9 = [cell for stroke in STROKES9 for cell in stroke] + [DOT9]


def e9_chase(t):
    """9.3 Light chase: two bright spots run along the strokes like marquee bulbs."""
    n = len(PATH9)
    period = 3.2
    g = blank(C9, R9)
    for i, cell in enumerate(PATH9):
        b = 0.0
        for k in range(2):
            p = (t / period + k / 2) * n
            behind = (p - i) % n
            b = max(b, math.exp(-behind / 2.2))
        fg = mix(DIM, GREEN, min(1.0, b * 1.4))
        fg = mix(fg, WHITE, max(0.0, b - 0.6) / 0.4)
        bg = mix(BG, NORI[2], max(0.0, b - 0.5)) if b > 0.5 else None
        ch = "■" if cell == DOT9 else GLYPHS9[cell]
        at9(g, *cell, ch, fg, bg)
    return g


HEAVY = {"─": "━", "│": "┃", "╭": "┏", "╮": "┓", "╰": "┗", "╯": "┛", "╵": "╹", "╷": "╻"}
DOUBLE = {"─": "═", "│": "║", "╭": "╔", "╮": "╗", "╰": "╚", "╯": "╝", "╵": "╹", "╷": "╻"}


def e9_weight(t):
    """9.4 Weight morph: thin, heavy, double and back, staggered across the word."""
    g = blank(C9, R9)
    for cell, ch in list(GLYPHS9.items()) + [(DOT9, "▪")]:
        x = (t - cell[1] * 0.045) % 3.0
        level = 0 if x < 0.8 else 1 if x < 1.05 else 2 if x < 1.65 else 1 if x < 1.9 else 0
        if cell == DOT9:
            at9(g, *cell, "▪" if level == 0 else "■", mix(GREEN, WHITE, 0.25 * level))
            continue
        glyph = [ch, HEAVY[ch], DOUBLE[ch]][level]
        fg = [INK, WHITE, mix(WHITE, GREEN, 0.45)][level]
        at9(g, *cell, glyph, fg)
    return g


def e9_drop(t):
    """9.5 Seaweed dot drop: the i dot falls in half-cell steps, squashes, bounces."""
    g = blank(C9, R9)
    ripple = t - 0.8
    for cell, ch in GLYPHS9.items():
        fg = INK
        if LETTER9[cell] == 3 and 0 < ripple < 0.5:
            fg = mix(GREEN, INK, abs(ripple - 0.08 * cell[0]) / 0.15)
        at9(g, *cell, ch, fg)
    fade = 1 - ease_in_out((t - 2.7) / 0.3)
    land_r = DOT9[0] + OFF9[0]
    land_h = 2 * land_r + 1
    col = DOT9[1] + OFF9[1]
    color = mix(BG, GREEN, fade)
    if 0.3 <= t < 0.8:
        h = int((land_h + 1) * ((t - 0.3) / 0.5) ** 2)
        put(g, h // 2, col, "▀" if h % 2 == 0 else "▄", GREEN)
    elif 0.8 <= t < 0.88:
        put(g, land_r, col, "▬", GREEN)
        put(g, land_r, col - 1, "╶", RICE[1])
        put(g, land_r, col + 1, "╴", RICE[1])
    elif 0.88 <= t < 1.12:
        x = (t - 0.88) / 0.24
        lift = round(2 * 4 * x * (1 - x))
        h = land_h - lift
        put(g, h // 2, col, "■" if lift == 0 else ("▀" if h % 2 == 0 else "▄"), GREEN)
    elif t >= 1.12:
        glow = math.exp(-(t - 1.12) / 0.5)
        put(g, land_r, col, "■", color, mix(BG, NORI[3], glow * fade))
    if 0.8 <= t < 1.2:
        x = (t - 0.8) / 0.4
        for side in (-1, 1):
            put(g, land_r - (1 if x < 0.5 else 0), col + side * (1 + round(2 * x)), "·", mix(GREEN, BG, x))
    return g


# ================================================================ style 4
SCALE4 = 10.0
COLS4, ROWS4 = sp.grid_size(SCALE4)
C4, R4, OFF4 = COLS4 + 8, ROWS4 + 4, (2, 4)
DOTS_W, DOTS_H = 2 * C4, 4 * R4


def letter_dots():
    """Target dots of the wordmark in canvas dot coordinates: {(x, y): is_i_dot}."""
    out = {}
    for dy in range(4 * ROWS4):
        for dx in range(2 * COLS4):
            x = (dx + 0.5) * 0.5 / SCALE4
            y = (dy + 0.5) * 0.5 / SCALE4 + sp.TOP
            d, isdot = sp.sdf(x, y)
            if d < 0:
                out[(dx + 2 * OFF4[1], dy + 4 * OFF4[0])] = isdot
    return out


TARGET4 = letter_dots()
BIT = {(x, y): b for x, y, b in sp.BRAILLE}


def dots_to_grid(dots, base=None):
    """dots: {(x, y): (intensity, rgb)}. A cell takes the colour of its brightest dot."""
    g = base or blank(C4, R4)
    acc = {}
    for (x, y), (v, rgb) in dots.items():
        if v < 0.08 or not (0 <= x < DOTS_W and 0 <= y < DOTS_H):
            continue
        key = (y // 4, x // 2)
        bits, best, col = acc.get(key, (0, -1.0, None))
        bits |= BIT[(x % 2, y % 4)]
        if v > best:
            best, col = v, mix(BG, rgb, min(1.0, v))
        acc[key] = (bits, best, col)
    for (r, c), (bits, _, col) in acc.items():
        cell = g[r][c]
        cell.ch, cell.fg, cell.bits = chr(0x2800 + bits), col, ("braille", bits)
    return g


def ink4(isdot):
    return GREEN if isdot else INK


def e4_rice(t):
    """4.1 Rice-grain assembly: grains arc in from the ball (off to the left) and settle."""
    rnd = random.Random(11)
    dots = {}
    fade = 1 - ease_in_out((t - 3.9) / 0.3)
    for (tx, ty), isdot in sorted(TARGET4.items()):
        start = 0.1 + 1.3 * tx / DOTS_W + rnd.random() * 0.35
        dur = 0.55 + rnd.random() * 0.35
        sx, sy = -6 - rnd.random() * 10, DOTS_H * (0.35 + rnd.random() * 0.3)
        cx, cy = (sx + tx) / 2, -rnd.random() * 18
        x = (t - start) / dur
        if x <= 0:
            continue
        if x < 1:
            e = ease_out(x)
            px = (1 - e) ** 2 * sx + 2 * (1 - e) * e * cx + e * e * tx
            py = (1 - e) ** 2 * sy + 2 * (1 - e) * e * cy + e * e * ty
            key = (round(px), round(py))
            dots[key] = (1.0, WARM)
        else:
            settle = (t - start - dur) / 0.35
            rgb = mix(mix(WHITE, GREEN, 0.5), ink4(isdot), settle)
            dots[(tx, ty)] = (fade, rgb)
    return dots_to_grid(dots)


LED_UNLIT = (0x26, 0x2B, 0x31)


def e4_led(t):
    """4.2 LED sign: scrolls in at dot resolution, a bright column sweeps, scrolls out."""
    g = blank(C4, R4)
    r0, r1, c0, c1 = OFF4[0] - 1, OFF4[0] + ROWS4 + 1, OFF4[1] - 2, OFF4[1] + COLS4 + 2
    for r in range(r0, r1):
        for c in range(c0, c1):
            g[r][c].ch, g[r][c].fg, g[r][c].bits = "⣿", LED_UNLIT, ("braille", 0xFF)
    panel_w = 2 * (c1 - c0)
    if t < 1.4:
        shift = round(panel_w * (1 - ease_out(t / 1.4)))
    elif t < 3.4:
        shift = 0
    else:
        shift = -round(panel_w * ease_in_out((t - 3.4) / 0.8))
    sweep = (t - 1.6) / 1.2 * (panel_w + 10) + 2 * c0 - 5
    dots = {}
    for (x, y), isdot in TARGET4.items():
        nx = x + shift
        if not (2 * c0 <= nx < 2 * c1):
            continue
        b = math.exp(-(((nx - sweep) / 3.0) ** 2)) if 1.6 < t < 2.8 else 0.0
        dots[(nx, y)] = (1.0, mix(ink4(isdot), WHITE, b) if isdot else mix((0xF2, 0xF4, 0xEA), WHITE, b))
    for r in range(r0, r1):
        for c in range(c0, c1):
            if any((2 * c + i, 4 * r + j) in dots for i in range(2) for j in range(4)):
                g[r][c].ch, g[r][c].bits = " ", None
    return dots_to_grid(dots, g)


def e4_dissolve(t):
    """4.3 Dissolve / reform: noise settles into the word, then blows away as dust."""
    rnd = random.Random(5)
    dots = {}
    for (tx, ty), isdot in sorted(TARGET4.items()):
        sx, sy = rnd.uniform(0, DOTS_W), rnd.uniform(0, DOTS_H)
        delay = rnd.random() * 0.5
        blow = 2.9 + 0.9 * (tx / DOTS_W) + rnd.random() * 0.2
        vx, vy = 25 + rnd.random() * 40, rnd.uniform(-8, 4)
        if t < 0.5:
            if (int(t * FPS) + tx * 7 + ty * 13) % 3 == 0:
                dots[(round(sx), round(sy))] = (0.5, RICE[1])
            continue
        x = ease_in_out((t - 0.5 - delay) / 1.0)
        px, py = sx + (tx - sx) * x, sy + (ty - sy) * x
        rgb = mix(RICE[1], ink4(isdot), x)
        v = 0.5 + 0.5 * x
        if t > blow:
            dt = t - blow
            px, py = tx + vx * dt * dt * 1.6, ty + vy * dt * dt + math.sin(dt * 9 + tx) * 1.5
            v = 1 - dt / 0.9
            rgb = mix(ink4(isdot), RICE[0], dt / 0.9)
        dots[(round(px), round(py))] = (v, rgb)
    noise_left = 1 - ease_in_out((t - 0.5) / 0.8)
    if noise_left > 0:
        nr = random.Random(int(t * FPS))
        for _ in range(int(260 * noise_left)):
            dots.setdefault((nr.randrange(DOTS_W), nr.randrange(DOTS_H)), (0.45 * noise_left + 0.1, RICE[0]))
    return dots_to_grid(dots)


def e4_shimmer(t):
    """4.4 Shimmer: a diagonal band of light sweeps through the dots, scattering glints."""
    period = 2.4
    x = (t % period) / 1.7
    band = -12 + x * (DOTS_W + 24 + DOTS_H * 0.6)
    dots = {}
    for (px, py), isdot in TARGET4.items():
        s = px + (DOTS_H - py) * 0.6
        b = math.exp(-(((s - band) / 5.0) ** 2)) if x <= 1 else 0.0
        base = GREEN if isdot else RICE[2]
        rgb = mix(base, WHITE, b)
        if s > band:
            rgb = mix(rgb, (0xB8, 0xFF, 0xCC), b * 0.6)
        dots[(px, py)] = (1.0, rgb)
    rnd = random.Random(int(t * FPS) * 31)
    if x <= 1:
        for _ in range(14):
            py = rnd.randrange(4 * OFF4[0], 4 * (OFF4[0] + ROWS4))
            px = round(band - (DOTS_H - py) * 0.6 + rnd.uniform(-4, 4))
            if (px, py) not in dots:
                dots[(px, py)] = (0.9, WHITE)
    return dots_to_grid(dots)


BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def tube_normals():
    """Surface normal of each letter dot, treating strokes as round tubes."""
    out = {}
    e = 0.01
    for (px, py), isdot in TARGET4.items():
        x = (px - 2 * OFF4[1] + 0.5) * 0.5 / SCALE4
        y = (py - 4 * OFF4[0] + 0.5) * 0.5 / SCALE4 + sp.TOP
        d = sp.sdf(x, y)[0]
        gx = sp.sdf(x + e, y)[0] - sp.sdf(x - e, y)[0]
        gy = sp.sdf(x, y + e)[0] - sp.sdf(x, y - e)[0]
        gl = math.hypot(gx, gy) or 1
        rr = min(1.0, max(0.0, 1 + d / sp.HW))
        out[(px, py)] = (gx / gl * rr, gy / gl * rr, math.sqrt(1 - rr * rr), isdot)
    return out


NORMALS4 = tube_normals()


def e4_halftone(t):
    """4.5 Halftone depth: dots dither out on the dark side of each stroke as the light circles."""
    th = 2 * math.pi * t / 4.0
    lx, ly, lz = 0.8 * math.cos(th), 0.8 * math.sin(th), 0.6
    dots = {}
    for (px, py), (nx, ny, nz, isdot) in NORMALS4.items():
        b = max(0.0, nx * lx + ny * ly + nz * lz) * 0.85 + 0.2
        if isdot:
            dots[(px, py)] = (1.0, mix(NORI[3], (0x7C, 0xFF, 0x9E), b))
            continue
        if b * 16 > BAYER[py % 4][px % 4] + 0.5:
            dots[(px, py)] = (1.0, mix(RICE[0], WHITE, b))
    return dots_to_grid(dots)


# ================================================================ output
EFFECTS = [
    ("9.1", "pen", "rounded line · pen draw-on", 4.5, e9_pen),
    ("9.2", "neon", "rounded line · neon tube", 4.9, e9_neon),
    ("9.3", "chase", "rounded line · light chase", 3.2, e9_chase),
    ("9.4", "weight", "rounded line · weight morph", 3.0 + 16 * 0.045, e9_weight),
    ("9.5", "drop", "rounded line · seaweed dot drop", 3.0, e9_drop),
    ("4.1", "rice", "braille · rice-grain assembly", 4.2, e4_rice),
    ("4.2", "led", "braille · LED sign", 4.3, e4_led),
    ("4.3", "dissolve", "braille · dissolve / reform", 4.2, e4_dissolve),
    ("4.4", "shimmer", "braille · shimmer", 2.4, e4_shimmer),
    ("4.5", "halftone", "braille · halftone depth", 4.0, e4_halftone),
]


def frames(fn, dur):
    return [fn(i / FPS) for i in range(int(round(dur * FPS)))]


def to_image(grid, title, font, lfont, scale=2):
    from PIL import Image, ImageDraw

    pad, head = 16, 30
    w, h = len(grid[0]) * sp.CW, len(grid) * sp.CH
    img = Image.new("RGB", (w + 2 * pad, h + 2 * pad + head), BG)
    d = ImageDraw.Draw(img)
    d.text((pad, pad), title, font=lfont, fill=RICE[1])
    for r, row in enumerate(grid):
        for c, cell in enumerate(row):
            x0, y0 = pad + c * sp.CW, pad + head + r * sp.CH
            draw_cell(d, img, x0, y0, cell, font)
    return img.resize((img.width * scale, img.height * scale), Image.NEAREST)


def draw_cell(d, img, x0, y0, cell, font):
    ch = cell.ch
    if cell.bg:
        d.rectangle([x0, y0, x0 + sp.CW - 1, y0 + sp.CH - 1], fill=cell.bg)
    if ch == "▀":
        d.rectangle([x0, y0, x0 + sp.CW - 1, y0 + sp.CH // 2 - 1], fill=cell.fg)
    elif ch == "▄":
        d.rectangle([x0, y0 + sp.CH // 2, x0 + sp.CW - 1, y0 + sp.CH - 1], fill=cell.fg)
    elif ch == "▬":
        d.rectangle([x0, y0 + sp.CH * 0.42, x0 + sp.CW - 1, y0 + sp.CH * 0.62], fill=cell.fg)
    elif ch == "▪":
        cx, cy, s = x0 + sp.CW / 2, y0 + sp.CH / 2, sp.CW * 0.22
        d.rectangle([cx - s, cy - s, cx + s, cy + s], fill=cell.fg)
    else:
        bg, cell.bg = cell.bg, None
        sp.draw_cell(d, img, x0, y0, cell, font)
        cell.bg = bg


def ansi_frame(grid):
    out = []
    for row in grid:
        s = []
        for c in row:
            s.append("\x1b[48;2;%d;%d;%dm" % c.bg if c.bg else "\x1b[49m")
            s.append("\x1b[38;2;%d;%d;%dm%s" % (c.fg + (c.ch,)))
        out.append("".join(s) + "\x1b[0m")
    return "\n".join(out)


def play(key):
    _, _, title, dur, fn = next(e for e in EFFECTS if e[0] == key)
    fs = frames(fn, dur)
    sys.stdout.write("\x1b[?25l\x1b[2J")
    try:
        while True:
            for g in fs:
                t0 = time.monotonic()
                sys.stdout.write("\x1b[H\x1b[2m%s  (ctrl-c to quit)\x1b[0m\n%s" % (title, ansi_frame(g)))
                sys.stdout.flush()
                time.sleep(max(0.0, 1 / FPS - (time.monotonic() - t0)))
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write("\x1b[0m\x1b[?25h\n")


def main():
    if "--list" in sys.argv:
        for key, _, title, dur, _ in EFFECTS:
            print(f"{key}  {title}  ({dur:.1f}s loop)")
        return
    if "--play" in sys.argv:
        play(sys.argv[sys.argv.index("--play") + 1])
        return
    from PIL import ImageFont

    font = ImageFont.truetype(sp.FONT, int(sp.CH * 0.8))
    lfont = ImageFont.truetype(sp.FONT, 16)
    os.makedirs(OUT, exist_ok=True)
    only = sys.argv[1:]
    for key, slug, title, dur, fn in EFFECTS:
        if only and key not in only:
            continue
        imgs = [to_image(g, f"{key}  {title}", font, lfont) for g in frames(fn, dur)]
        path = os.path.join(OUT, f"{key}-{slug}.gif")
        imgs[0].save(path, save_all=True, append_images=imgs[1:], duration=1000 // FPS, loop=0, optimize=True)
        print(path, len(imgs), "frames")


if __name__ == "__main__":
    main()
