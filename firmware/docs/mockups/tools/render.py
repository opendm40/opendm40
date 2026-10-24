# Mockup renderer for docs/combined-view-design.md.
# Uses the real stock font data (stockfont_data.rs) through sim.py (a host
# mimic of src/stockfont.rs label/glyphs/rbox) plus the proposed F39 font,
# derived from F78 by the exact 2x2 box filter the design specifies.
import math, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sim
from PIL import Image

for n in ['TEXT22', 'ASCII26', 'HIST', 'K_FONT', 'BATTERY', 'LOCK', 'SAVE_BT', 'LIGHTNING']:
    sim.FONTS[n] = sim.Font(n)

# ---------------------------------------------------------------- F39
class F39:
    """F78 halved: w' = ceil(w/2), h' = ceil(h/2), alpha = round(mean of the
    2x2 block, missing pixels = 0), adv' = adv/2, glyph top' = floor(top/2)
    where top = 78 - 1 - h - yoff (F78 line_h 78, descent 1) -> line_h 39,
    descent 0, yoff' = 39 - h' - top'."""
    def __init__(s, src):
        s.src = src; s.lh = 39; s.desc = 0; s.four = True; s.g = {}
        for c, g in src.g.items():
            w2, h2 = (g['w'] + 1) // 2, (g['h'] + 1) // 2
            top = 78 - 1 - g['h'] - g['yoff']
            t2 = top // 2
            s.g[c] = dict(code=c, w=w2, h=h2, adv=g['adv'] // 2, yoff=39 - h2 - t2, src=g)
    def alpha(s, g, x, y):
        sg = g['src']; tot = 0
        for dx in (0, 1):
            for dy in (0, 1):
                xx, yy = 2 * x + dx, 2 * y + dy
                if xx < sg['w'] and yy < sg['h']:
                    tot += s.src.alpha(sg, xx, yy)
        return (tot + 2) // 4
sim.FONTS['F39'] = F39(sim.FONTS['F78'])

# ---------------------------------------------------------------- colours
BG, BAR, FG, DIM, BTN, SEL = 0x0883, 0x1904, 0xF79E, 0x94D3, 0x2966, 0x0419
RED, TRACE, STATUS_FG = 0xF145, 0xFFE0, 0xF7BE
S_BAR, S_FRAME, S_FILL, S_GRID = 0x4208, 0xAD55, 0x2124, 0x9492
S_PURPLE, S_ORANGE, GREEN, ACCENT = 0xA81F, 0xFAA0, 0x04A9, 0x04FC
CHIP = 0x3186          # proposed: chip fill on S_BAR (48,48,48)
TILE = 0x0883          # meter tile = DMM BG
HOLD_BG = 0xFC40

W, H = 480, 320

def mix(a, b, t):
    ca = sim.rgb(a); cb = sim.rgb(b)
    r, g, bl = [int(ca[i] * (1 - t) + cb[i] * t) for i in range(3)]
    return (r >> 3) << 11 | (g >> 2) << 5 | bl >> 3

# ---------------------------------------------------------------- scope graph
class Graph:
    def __init__(s, x0, y0, h):
        s.x0, s.y0, s.h = x0, y0, h
        s.pitch = (h - 1) / 8.0
    def row(s, r):      # 280-row model -> pixel y
        return s.y0 + int(round(r * (s.h - 1) / 280.0))
    def draw(s, l, trace=None, trig_row=None, dashed=False, cur_x=None):
        h = s.h
        hl = set(int(round(k * s.pitch)) for k in range(1, 8))
        segs = [None] * 401
        if trace:
            prev = None
            for i in range(401):
                v = trace(i)
                if v is None:
                    prev = None; continue
                r = s.row(v) - s.y0
                lo, hi = r, r
                if prev is not None:
                    lo, hi = min(lo, prev), max(hi, prev)
                segs[i] = (lo, hi) if i > 0 else None
                prev = r
        tr = s.row(trig_row) - s.y0 if trig_row is not None else None
        for i in range(401):
            x = s.x0 + i
            for y in range(h):
                edge = y == 0 or y == h - 1 or i == 0 or i == 400
                if edge:
                    c = S_FRAME
                else:
                    c = S_FILL
                    if i % 40 == 0 and (y % 5 == 0 or y in hl):
                        c = S_GRID
                    elif i % 8 == 0 and y in hl:
                        c = S_GRID
                if dashed and tr is not None and y == tr and not edge and (i // 4) % 2 == 0:
                    c = S_ORANGE
                if cur_x and i in cur_x and not edge and y % 3 == 0:
                    c = S_FRAME
                sg = segs[i]
                if sg and sg[0] <= y <= sg[1] and not edge:
                    c = TRACE
                l.px[s.y0 + y][x] = c

def square(period_cols, hi_row, lo_row, phase=0):
    def f(i):
        p = ((i + phase) % period_cols) / period_cols
        return hi_row if p < 0.5 else lo_row
    return f

def sine(period_cols, mid, amp, phase=0):
    return lambda i: mid - amp * math.sin(2 * math.pi * (i + phase) / period_cols)

# ---------------------------------------------------------------- primitives
def fill(l, x, y, w, h, c): l.fill(x, y, w, h, c)

def text(l, fn, r, t, fg, bg, align=1, sp=None):
    if sp is None:
        sp = 1 if fn in ('F14', 'HIST') else 0
    l.label(fn, r, t, fg, bg, align, sp)

def glyph_text(l, fn, r, t, fg, bg, align=8, sp=None):
    if sp is None:
        sp = 1 if fn in ('F14', 'HIST') else 0
    l.glyphs(fn, r, t, fg, bg, align, sp)

def pill(l, r, t, fg, bg, outside, fn='F14', rad=4):
    l.rbox(r, rad, bg, outside)
    glyph_text(l, fn, r, t, fg, bg, 8)

def circle(l, cx, cy, rad, c, th=2):
    for y in range(cy - rad - 1, cy + rad + 2):
        for x in range(cx - rad - 1, cx + rad + 2):
            d = math.hypot(x - cx, y - cy)
            if rad - th + 0.5 <= d <= rad + 0.5:
                l.px[y][x] = c

def magnifier_btn(l, y, axis, plus, h=41, fg=None):
    """Right-panel button: axis letter + circled +/- (scope-ui-plan S2)."""
    fg = fg if fg is not None else (TRACE if axis == 'V' else S_PURPLE)
    l.rbox((417, y, 60, h), 6, BTN, S_BAR)
    glyph_text(l, 'F21', (419, y, 24, h), axis if axis != 't' else 'T', fg, BTN, 8)
    cx, cy = 457, y + h // 2
    circle(l, cx, cy, 10, fg)
    fill(l, cx - 5, cy - 1, 11, 2, fg)
    if plus:
        fill(l, cx - 1, cy - 5, 2, 11, fg)

def text_btn(l, y, t, fg, h=41, fn='F14'):
    l.rbox((417, y, 60, h), 6, BTN, S_BAR)
    glyph_text(l, fn, (417, y, 60, h), t, fg, BTN, 8)

def battery(l, x, y, bg):
    text(l, 'BATTERY', (x, y, 40, 16), '\x27', STATUS_FG, bg, 8, 0)

def status_cluster(l, bg, y=0, h=20, uptime='00:21:13', dots=1):
    # page dots, uptime, lock, BT, battery (unified bar, 20 px)
    for k in range(4):
        cx = 300 + 9 * k
        c = FG if k == dots else 0x6B4D
        for yy in range(-2, 3):
            for xx in range(-2, 3):
                if xx * xx + yy * yy <= 5:
                    l.px[y + h // 2 + yy][cx + xx] = c
    text(l, 'F14', (334, y, 52, h), uptime, FG, bg, 1)
    text(l, 'LOCK', (388, y, 24, h), '\x21', STATUS_FG, bg, 8, 0)
    text(l, 'SAVE_BT', (414, y, 18, h), '\x21', STATUS_FG, bg, 8, 0)
    battery(l, 436, y + 2, bg)

def scope_chips(l, y=0, h=20, run='AUTO', tb='10ms', vdiv='5V', ac=False, trig='/ 2.50V', run_col=S_PURPLE):
    # scope-ui-plan 3.1 chips, F14, chip = rounded pill on the bar
    xs = [(13, 50, run, run_col), (65, 58, tb, FG), (125, 66, vdiv + (' AC' if ac else ' DC'), TRACE), (193, 84, trig, FG)]
    for x, w, t, c in xs:
        pill(l, (x, y + 2, w, h - 4), t, c, CHIP, S_BAR, rad=4)

def meas_bar(l, cells, y=301):
    fill(l, 0, y, 414, 19, S_BAR)
    for i, t in enumerate(cells):
        x = 13 + 100 * i
        fill(l, x, y + 1, 98, 1, S_FRAME)
        text(l, 'F14', (x, y + 2, 98, 16), t, TRACE, S_BAR, 1)

def save(l, path, scale=1):
    im = Image.new('RGB', (W, H)); im.putdata([sim.rgb(c) for row in l.px for c in row])
    if scale != 1:
        im = im.resize((W * scale, H * scale), Image.NEAREST)
    im.save(path)

def load_png(path):
    im = Image.open(path).convert('RGB')
    l = sim.LCD()
    px = list(im.getdata())
    for y in range(H):
        for x in range(W):
            r, g, b = px[y * W + x]
            l.px[y][x] = (r >> 3) << 11 | (g >> 2) << 5 | b >> 3
    return l
