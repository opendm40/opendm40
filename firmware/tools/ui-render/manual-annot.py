#!/usr/bin/env python3
"""Composed and annotated pictures of the user manual (docs/manual/img/).

    tools/ui-render/manual-annot.py RENDERS [STOCK]  (manual-img.py runs it)

RENDERS is the `run.sh OUT --manual` folder (a few crops come from renders
that are not manual images themselves); STOCK, when given, the
tools/stock-ui/run.sh folder (the stock scope page for the stock vs
opendm40 pairs; without it those pictures stay as they are). The stock
multimeter screens in stock-shots/ are captures of the stock firmware in
the emulator (emu/run.py stock). Everything else is built from the
screenshots already in docs/manual/img. One visual language: white
background, #0a7bd6 arrows and callouts, DejaVu Sans labels, a thin grey
frame around each screenshot. Stored as palette PNGs (256 colours).
"""
import math
import os
import sys

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
DST = os.path.normpath(os.path.join(HERE, '../../../docs/manual/img'))
FONT = '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'
BOLD = '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf'
MONO = '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf'
BLUE = (10, 123, 214)
GREY = (176, 176, 176)
DARK = (40, 40, 40)
MID = (110, 110, 110)
WHITE = (255, 255, 255)
RED = (200, 40, 40)
RENDERS = None
STOCK = None


def font(size, bold=False, mono=False):
    return ImageFont.truetype(MONO if mono else BOLD if bold else FONT, size)


def shot(name, scale=1.0, crop=None, src=None):
    """A screenshot from docs/manual/img (or the render folder), cropped and
    scaled (NEAREST for whole multiples: crisp pixels)."""
    path = os.path.join(src or DST, name + '.png')
    im = Image.open(path).convert('RGB')
    if crop:
        im = im.crop(crop)
    if scale != 1.0:
        w, h = im.size
        rs = Image.NEAREST if float(scale).is_integer() else Image.LANCZOS
        im = im.resize((round(w * scale), round(h * scale)), rs)
    return im


def render(name, **kw):
    return shot(name, src=RENDERS, **kw)


def canvas(w, h):
    im = Image.new('RGB', (w, h), WHITE)
    return im, ImageDraw.Draw(im)


def paste(im, d, s, x, y):
    im.paste(s, (x, y))
    d.rectangle((x - 1, y - 1, x + s.size[0], y + s.size[1]), outline=GREY)


def text_c(d, x, y, t, f, fill=DARK):
    """Text centred on x (multi-line), top at y."""
    lines = t.split('\n')
    for i, l in enumerate(lines):
        w = d.textlength(l, font=f)
        d.text((x - w / 2, y + i * (f.size + 4)), l, font=f, fill=fill)


def arrow(d, a, b, w=4, head=12, fill=BLUE):
    d.line((a, b), fill=fill, width=w)
    ang = math.atan2(b[1] - a[1], b[0] - a[0])
    p1 = (b[0] - head * math.cos(ang - 0.45), b[1] - head * math.sin(ang - 0.45))
    p2 = (b[0] - head * math.cos(ang + 0.45), b[1] - head * math.sin(ang + 0.45))
    d.polygon([b, p1, p2], fill=fill)


def arrow2(d, a, b, **kw):
    arrow(d, a, b, **kw)
    arrow(d, b, a, **kw)


def badge(d, x, y, n, r=13):
    d.ellipse((x - r, y - r, x + r, y + r), fill=BLUE)
    f = font(15, bold=True)
    t = str(n)
    w = d.textlength(t, font=f)
    d.text((x - w / 2, y - 9), t, font=f, fill=WHITE)


def callout(d, label, at, tx, ty, f, align='left'):
    """A label at (tx, ty) with a thin blue leader line to `at` and a dot."""
    w = d.textlength(label, font=f)
    if align == 'left':
        lx, ax = tx, tx - 4
    else:
        lx, ax = tx - w, tx + 4
    d.text((lx, ty - f.size * 0.6), label, font=f, fill=DARK)
    d.line((ax, ty, at[0], at[1]), fill=BLUE, width=2)
    d.ellipse((at[0] - 4, at[1] - 4, at[0] + 4, at[1] + 4), fill=BLUE)


def finger(im, x, y, r=17, n=1, gap=30):
    """`n` fingertips side by side, centred on x."""
    o = Image.new('RGBA', im.size, (0, 0, 0, 0))
    od = ImageDraw.Draw(o)
    for k in range(n):
        cx = x + (k - (n - 1) / 2) * gap
        od.ellipse((cx - r, y - r, cx + r, y + r), fill=(10, 123, 214, 110), outline=(255, 255, 255, 230), width=3)
    im.paste(Image.alpha_composite(im.convert('RGBA'), o).convert('RGB'))


def save(im, name):
    q = im.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    out = os.path.join(DST, name + '.png')
    q.save(out, optimize=True)
    print(f'{name:15s} {os.path.getsize(out):6d}')


# --------------------------------------------------------------- pictures

def status_bar():
    sc = 2
    bar = render('status-all', scale=sc, crop=(0, 0, 480, 34))
    im, d = canvas(bar.size[0] + 40, bar.size[1] + 110)
    paste(im, d, bar, 20, 20)
    spans = [(27, 97), (129, 179), (205, 305), (335, 358), (364, 383), (390, 405), (435, 474)]
    for k, (a, b) in enumerate(spans):
        x = 20 + (a + b) / 2 * sc
        d.line((x, 20 + bar.size[1] + 2, x, 20 + bar.size[1] + 38), fill=BLUE, width=2)
        badge(d, x, 20 + bar.size[1] + 52, k + 1)
    save(im, 'status-bar')


def touch_lock():
    sc = 4
    a = shot('dmm-bar', sc, (322, 2, 372, 32))
    b = render('status-locked', scale=sc, crop=(322, 2, 372, 32))
    im, d = canvas(620, 230)
    paste(im, d, a, 40, 50)
    paste(im, d, b, 380, 50)
    f = font(16, bold=True)
    text_c(d, 40 + a.size[0] / 2, 18, 'Unlocked', f)
    text_c(d, 380 + b.size[0] / 2, 18, 'Locked', f)
    arrow(d, (260, 100), (360, 100))
    arrow(d, (360, 140), (260, 140))
    text_c(d, 310, 172, 'press the power key\ntwice quickly', font(14), MID)
    save(im, 'touch-lock')


def gestures():
    s = 0.5
    base = shot('dmm-bar', s)
    w, h = base.size
    im, d = canvas(2 * w + 60, 2 * h + 150)
    cells = [
        ('Swipe left / right:\nnext or previous view', 1, 'h'),
        ('One finger down:\nstart / stop recording', 1, 'v'),
        ('Three fingers down:\nscreenshot', 3, 'v'),
        ('Two fingers down:\npower off?', 2, 'v'),
    ]
    f = font(14)
    for k, (t, n, how) in enumerate(cells):
        x = 20 + (k % 2) * (w + 20)
        y = 56 + (k // 2) * (h + 70)
        im.paste(base, (x, y))
        cx, cy = x + w // 2, y + h // 2
        if how == 'h':
            arrow2(d, (cx - 80, cy + 10), (cx + 80, cy + 10), w=5, head=14)
            finger(im, cx, cy + 10)
        else:
            gap = 34
            for j in range(n):
                fx = cx + (j - (n - 1) / 2) * gap
                arrow(d, (fx, cy - 50), (fx, cy + 40), w=4, head=12)
            finger(im, cx, cy - 40, n=n, gap=gap)
        d = ImageDraw.Draw(im)
        d.rectangle((x - 1, y - 1, x + w, y + h), outline=GREY)
        text_c(d, x + w / 2, y - 44, t, f)
    save(im, 'gestures')


def keys():
    s = 0.6
    scr = shot('dmm-bar', s)
    w, h = scr.size
    im, d = canvas(w + 260, h + 150)
    x0, y0 = 30, 30
    d.rounded_rectangle((x0 - 18, y0 - 18, x0 + w + 18, y0 + h + 92), radius=22, outline=MID, width=3)
    paste(im, d, scr, x0, y0)
    f = font(14, bold=True)
    kw = (w - 30) / 4
    for k, t in enumerate(['MENU', 'RUN', 'AUTO', 'MF']):
        kx = x0 + k * (kw + 10)
        d.rounded_rectangle((kx, y0 + h + 22, kx + kw, y0 + h + 62), radius=8, fill=(225, 236, 247), outline=BLUE, width=2)
        text_c(d, kx + kw / 2, y0 + h + 32, t, f, BLUE)
    px = x0 + w + 18
    d.rounded_rectangle((px - 4, y0 + 60, px + 8, y0 + 130), radius=4, fill=BLUE)
    callout(d, 'Power key', (px + 8, y0 + 95), px + 50, y0 + 95, font(15))
    text_c(d, px + 110, y0 + 112, '(on the side)', font(13), MID)
    text_c(d, x0 + w / 2, y0 + h + 100, 'touch keys below the screen', font(13), MID)
    save(im, 'keys')


def box(d, x, y, w, h, title, lines=(), fill=(240, 246, 252)):
    d.rounded_rectangle((x, y, x + w, y + h), radius=10, fill=fill, outline=BLUE, width=2)
    f = font(15, bold=True)
    text_c(d, x + w / 2, y + 10 if lines else y + h / 2 - 10, title, f)
    fm = font(13, mono=True)
    for i, l in enumerate(lines):
        text_c(d, x + w / 2, y + 38 + i * 19, l, fm, DARK)


def install():
    im, d = canvas(880, 260)
    box(d, 20, 60, 190, 120, 'Computer', ['dm40_opendm40_', 'v0.1.bin'])
    arrow(d, (215, 120), (285, 120))
    text_c(d, 250, 90, 'USB', font(13), MID)
    box(d, 290, 60, 210, 120, 'Meter: USB disk', ['(stock: USB Connect)', 'the only dm40*.bin'])
    arrow(d, (505, 120), (585, 120))
    text_c(d, 545, 132, 'eject,\nswitch off\nand on', font(13), MID)
    s = shot('dmm-bar', 0.5)
    paste(im, d, s, 600, 50)
    text_c(d, 600 + s.size[0] / 2, 20, 'opendm40 starts', font(15, bold=True))
    save(im, 'install')


def strip_of(names, caps, title=None, s=0.5):
    w, h = 240, 160
    n = len(names)
    top = 10
    im, d = canvas(n * (w + 20) + 20, h + 70 + top)
    for k, (nm, c) in enumerate(zip(names, caps)):
        x = 20 + k * (w + 20)
        paste(im, d, src_shot(nm, s), x, top + 10)
        text_c(d, x + w / 2, top + h + 20, c, font(14))
    return im


def whats_new():
    for name, items in [
        ('new-dmm', [('range-v', 'AUTO+ by default'), ('dmm-rate', 'FAST / MED / SLOW,\nan extra digit'), ('dmm-limit-hi', 'Limits: OK, HI, LO')]),
        ('new-instr', [('power', 'Power meter'), ('inrush', 'Inrush current'), ('wave', 'Wave view')]),
        ('new-safety', [('ajack', 'Wrong-connection alerts'), ('alert-ovld', 'Overload: the 10 A shunt'), ('fuse-check', 'Fuse check')]),
        ('new-scope', [('scope-tb-corner', 'Timebase corner taps'), ('scope-trig-pos', 'Trigger position drag'), ('scope-cur-x', 'CUR: cursors in one tap')]),
        ('new-gen', [('gen-clamped', 'The 50 kHz limit shown'), ('gen-triangle', 'An adjustable offset')]),
        ('new-data', [('record', 'The Record page'), ('gallery', 'The gallery')]),
        ('new-other', [('auto-off', 'Power-off warning'), ('help', 'On-screen help'), ('device', 'The Device page')]),
    ]:
        save(strip_of([i[0] for i in items], [i[1] for i in items]), name)


def together():
    sc = 1.5
    s = shot('combined', sc)
    w, h = s.size
    im, d = canvas(w + 40, h + 40)
    x0, y0 = 20, 20
    paste(im, d, s, x0, y0)
    f = font(15)
    g = lambda x, y: (x0 + x * sc, y0 + y * sc)
    tag(d, 'multimeter (V / A jacks)', g(150, 56), *g(40, 72), f)
    tag(d, 'generator running', g(404, 48), *g(250, 72), f)
    tag(d, 'oscilloscope (CHA)', None, *g(150, 236), f)
    save(im, 'new-together')


def functions_row():
    sc = 2
    row = shot('dmm-bar', sc, (0, 277, 480, 320))
    w, h = row.size
    im, d = canvas(w + 40, h + 190)
    x0, y0 = 20, 95
    paste(im, d, row, x0, y0)
    labels = ['DC, AC,\nAC+DC, AUTO', 'DC, AC,\nAC+DC, AUTO', 'normal,\nlow current', 'capacitance', 'diode,\ncontinuity', 'frequency,\ntemperature', 'power meter:\nDC, AC, AC+DC']
    f = font(14)
    for i, t in enumerate(labels):
        cx = x0 + (4 + 68 * i + 32) * sc
        if i % 2 == 0:
            d.line((cx, y0 - 4, cx, y0 - 26), fill=BLUE, width=2)
            text_c(d, cx, y0 - 74, t, f)
        else:
            d.line((cx, y0 + h + 4, cx, y0 + h + 26), fill=BLUE, width=2)
            text_c(d, cx, y0 + h + 32, t, f)
    save(im, 'functions')


def jack(d, x, y, label, r=24, fill=(60, 60, 60)):
    d.ellipse((x - r, y - r, x + r, y + r), fill=(60, 60, 60), outline=MID, width=3)
    d.ellipse((x - 9, y - 9, x + 9, y + 9), fill=(150, 150, 150))
    text_c(d, x, y + r + 8, label, font(16, bold=True))


def jacks():
    im, d = canvas(860, 290)
    text_c(d, 245, 14, 'Multimeter (isolated)', font(15, bold=True), MID)
    text_c(d, 680, 14, 'Scope and generator', font(15, bold=True), MID)
    d.rounded_rectangle((20, 40, 470, 270), radius=14, outline=GREY, width=2)
    d.rounded_rectangle((500, 40, 840, 270), radius=14, outline=GREY, width=2)
    for t, x in {'V/Ω': 100, 'COM': 245, 'A': 390}.items():
        jack(d, x, 90, t)
    jack(d, 600, 90, 'CHA')
    jack(d, 740, 90, 'GEN')
    f = font(14)
    text_c(d, 100, 160, 'V, Ω, continuity,\ndiode, capacitance,\nHz, °C', f)
    text_c(d, 245, 160, 'common', f)
    text_c(d, 390, 160, 'A, mA, µA', f)
    text_c(d, 600, 160, 'oscilloscope\ninput', f)
    text_c(d, 740, 160, 'generator\noutput', f)
    text_c(d, 245, 238, 'Power meter: V/Ω, COM and A at once', font(13), MID)
    save(im, 'jacks')


def trend_chart():
    sc = 2
    c = shot('dmm-bar', sc, (0, 172, 480, 278))
    w, h = c.size
    im, d = canvas(w + 470, h + 40)
    x0, y0 = 250, 20
    paste(im, d, c, x0, y0)
    f = font(15)
    g = lambda x, y: (x0 + x * sc, y0 + (y - 172) * sc)
    side_label(d, 'scale (follows the data)', g(0, 182), x0 - 20, f, right=False)
    side_label(d, 'tap: 160 / 320 / 640 ms/div', g(0, 200), x0 - 20, f, right=False)
    side_label(d, 'rate chip: tap to step', g(0, 244), x0 - 20, f, right=False)
    side_label(d, 'MAX, AVG', g(480, 188), x0 + w + 20, f)
    side_label(d, 'REL box: tap', g(480, 224), x0 + w + 20, f)
    side_label(d, 'MIN and the timer', g(480, 262), x0 + w + 20, f)
    save(im, 'trend-chart')


def scope_page():
    sc = 1.25
    s = shot('scope-run', sc)
    w, h = s.size
    im, d = canvas(w + 420, h + 120)
    x0, y0 = 200, 90
    paste(im, d, s, x0, y0)
    f = font(14)
    g = lambda x, y: (x0 + x * sc, y0 + y * sc)
    for x, t, row in [(44, 'run state', 0), (110, 'timebase', 1), (176, 'MCX | DMM', 0), (246, 'record bar', 1), (316, 'V/div', 0), (383, 'trigger', 1)]:
        up_label(d, t, g(x, 1), y0 - 34 - row * 30, f)
    L, R = x0 - 20, x0 + w + 20
    side_label(d, 'A: the trace zero', g(0, 160), L, f, right=False)
    side_label(d, 'slower timebase', g(25, 288), L, f, right=False)
    side_label(d, 'measurements', g(0, 311), L, f, right=False)
    side_label(d, 'zoom in, CHA, zoom out', g(480, 80), R, f)
    side_label(d, 'math scale, math (FFT)', g(480, 210), R, f)
    side_label(d, 'TRIG: trigger menu', g(480, 300), R, f)
    tag(d, 'T: trigger position', g(216, 26), *g(236, 32), f)
    tag(d, 'T: trigger level', g(405, 125), *g(290, 70), f)
    tag(d, 'CUR: cursors', g(135, 281), *g(96, 246), f)
    tag(d, 'faster timebase', g(365, 281), *g(270, 246), f)
    save(im, 'scope-page')


def scope_drag():
    sc = 1.25
    s = shot('scope-run', sc)
    w, h = s.size
    im, d = canvas(w + 40, h + 40)
    x0, y0 = 20, 20
    im.paste(s, (x0, y0))
    g = lambda x, y: (x0 + x * sc, y0 + y * sc)
    tl = g(398, 125)
    tp = g(213, 40)
    tr = g(100, trace_y('scope-run', 100))
    for (x, y), vert in [(tl, True), (tp, False), (tr, True)]:
        if vert:
            arrow2(d, (x, y - 60), (x, y + 60), w=5, head=14)
        else:
            arrow2(d, (x - 70, y), (x + 70, y), w=5, head=14)
        finger(im, x, y)
        d = ImageDraw.Draw(im)
    d.rectangle((x0 - 1, y0 - 1, x0 + w, y0 + h), outline=GREY)
    f = font(15)
    tag(d, 'trigger level:\nT on the right edge', None, *g(262, 150), f)
    tag(d, 'trigger position:\nT on the top edge', None, *g(262, 34), f)
    tag(d, 'the trace', None, *g(124, trace_y('scope-run', 100) + 26), f)
    save(im, 'scope-drag')


def meter_strip():
    sc = 2
    s = shot('combined', sc, (0, 18, 480, 64))
    w, h = s.size
    im, d = canvas(w + 40, h + 130)
    x0, y0 = 20, 60
    paste(im, d, s, x0, y0)
    f = font(14)
    g = lambda x, y: (x0 + x * sc, y0 + (y - 18) * sc)
    up_label(d, 'function', g(50, 22), y0 - 46, f)
    up_label(d, 'reading', g(150, 23), y0 - 46, f)
    up_label(d, 'small values', g(255, 23), y0 - 46, f)
    up_label(d, 'generator', g(375, 22), y0 - 46, f)
    down_label(d, 'state badge', g(50, 63), y0 + h + 30, f)
    save(im, 'meter-strip')


def analog_power():
    im, d = canvas(620, 300)
    cols = ['Multimeter\nside', 'Scope and\ngenerator side']
    rows = ['Multimeter page', 'Combined view', 'Oscilloscope page', 'Generator page']
    on = [(1, 0), (1, 1), (0, 1), (0, 1)]
    fb, f = font(15, bold=True), font(15)
    for j, c in enumerate(cols):
        text_c(d, 340 + j * 180, 20, c, fb)
    for i, r in enumerate(rows):
        y = 100 + i * 45
        d.text((20, y - 10), r, font=f, fill=DARK)
        for j in range(2):
            x = 340 + j * 180
            if on[i][j]:
                d.ellipse((x - 12, y - 12, x + 12, y + 12), fill=BLUE)
            else:
                d.ellipse((x - 12, y - 12, x + 12, y + 12), outline=GREY, width=3)
        d.line((20, y + 22, 600, y + 22), fill=(230, 230, 230))
    text_c(d, 310, 271, 'Auto (per view). Each side also stays on while something needs it.', font(12), MID)
    save(im, 'analog-power')


def gen_offset():
    im, d = canvas(900, 280)
    f, fb = font(13), font(15, bold=True)
    panels = [('Offset auto: 0 V to Vpp', 2.0, 1.0), ('Offset 1.5 V: around 1.5 V', 2.0, 1.5), ('0 Vpp, offset 1.2 V: DC', 0.0, 1.2)]
    for k, (t, vpp, mid) in enumerate(panels):
        x0, y0, w, h = 40 + k * 290, 60, 240, 170
        text_c(d, x0 + w / 2, 20, t, fb)
        d.rectangle((x0, y0, x0 + w, y0 + h), outline=GREY)
        yv = lambda v: y0 + h - v / 3.0 * h
        for v in (0, 3):
            d.line((x0, yv(v), x0 + w, yv(v)), fill=MID, width=1)
            d.text((x0 - 30, yv(v) - 8), f'{v} V', font=f, fill=MID)
        pts = [(x0 + i, yv(mid + vpp / 2 * math.sin(i / w * 4 * math.pi))) for i in range(w + 1)]
        d.line(pts, fill=BLUE, width=3)
    save(im, 'gen-offset')


def csv_flow():
    im, d = canvas(900, 230)
    s = shot('dmm-rec', 0.4)
    paste(im, d, s, 20, 50)
    text_c(d, 20 + s.size[0] / 2, 20, 'Recording', font(15, bold=True))
    arrow(d, (225, 115), (290, 115))
    box(d, 300, 60, 210, 110, 'USB disk', ['LOGS/LOG007.CSV'])
    arrow(d, (515, 115), (580, 115))
    rows = ['time_s,value,unit,flags', '0.250,1.23456,V,', '0.750,,V,OL']
    d.rectangle((590, 60, 880, 170), outline=GREY, fill=(248, 248, 248))
    fm = font(14, mono=True)
    for i, r in enumerate(rows):
        d.text((602, 75 + i * 28), r, font=fm, fill=DARK)
    text_c(d, 735, 20, 'Spreadsheet', font(15, bold=True))
    save(im, 'csv-flow')


def folder(d, x, y, name, files, f, fm):
    d.rectangle((x, y + 4, x + 26, y + 22), fill=(240, 190, 60))
    d.rectangle((x, y, x + 12, y + 6), fill=(240, 190, 60))
    d.text((x + 36, y), name, font=f, fill=DARK)
    d.text((x + 150, y + 2), files, font=fm, fill=MID)


def usb_disk():
    im, d = canvas(820, 290)
    f, fm = font(16, bold=True), font(14, mono=True)
    d.text((20, 15), 'DM40', font=font(18, bold=True), fill=BLUE)
    items = [('README.TXT', '', False), ('LOGS', 'LOG001.CSV  LOG002.CSV  FAST001.CSV', True), ('SCOPE', 'SCOP001.CSV', True),
             ('SHOTS', 'S001.BMP  S002.BMP  S003.BMP', True), ('INFO', 'CALIB.BIN  UID.BIN  ...  FIRMWARE.BIN', True), ('DEBUG', 'SPIFLASH.BIN', True)]
    for i, (n, files, isdir) in enumerate(items):
        y = 55 + i * 37
        d.line((30, y + 12, 50, y + 12), fill=GREY, width=2)
        if isdir:
            folder(d, 56, y, n + '/', files, f, fm)
        else:
            d.rectangle((60, y, 78, y + 22), outline=MID, width=2)
            d.text((92, y), n, font=f, fill=DARK)
    d.line((30, 40, 30, 55 + 5 * 37 + 12), fill=GREY, width=2)
    save(im, 'usb-disk')


def usb_row():
    sc = 2
    s = render('settings3-edit', scale=sc, crop=(0, 132, 480, 165))
    im, d = canvas(s.size[0] + 40, s.size[1] + 70)
    paste(im, d, s, 20, 20)
    text_c(d, im.size[0] / 2, s.size[1] + 34, 'PC can edit, waiting for the re-plug', font(14), MID)
    save(im, 'usb-mode')


def usb_link(name, ejected):
    im, d = canvas(820, 200)
    box(d, 20, 60, 160, 70, 'Computer')
    s = shot('dmm-bar', 0.4)
    paste(im, d, s, 620, 40)
    fb = font(15, bold=True)
    ya, yb = 80, 112
    if ejected:
        for xx in range(190, 600, 16):
            d.line((xx, ya, xx + 8, ya), fill=RED, width=4)
        text_c(d, 400, ya - 32, 'USB disk DM40: ejected until re-plug', fb, RED)
    else:
        arrow2(d, (190, ya), (605, ya), w=4, head=12)
        text_c(d, 400, ya - 32, 'USB disk DM40', fb, BLUE)
    arrow2(d, (190, yb), (605, yb), w=4, head=12)
    text_c(d, 400, yb + 12, 'measuring instrument (USBTMC, SCPI)', fb, BLUE)
    text_c(d, 400, 172, 'one USB cable', font(13), MID)
    save(im, name)


def delete_bar():
    sc = 2
    s = shot('gallery-view', sc, (0, 0, 480, 40))
    im, d = canvas(s.size[0] + 40, s.size[1] + 80)
    paste(im, d, s, 20, 20)
    f = font(14)
    callout(d, 'Delete, then confirm', (20 + 445 * sc, 20 + 36 * sc), 20 + 600, s.size[1] + 60, f, 'right')
    callout(d, '< Back', (20 + 40 * sc, 20 + 36 * sc), 20 + 120, s.size[1] + 60, f)
    save(im, 'gallery-delete')


def bt_names():
    im, d = canvas(860, 220)
    s = shot('dmm-bar', 0.4)
    paste(im, d, s, 20, 40)
    box(d, 580, 20, 260, 50, 'Phone app')
    box(d, 580, 140, 260, 50, 'Computer (SCPI)')
    arrow2(d, (220, 80), (570, 45))
    arrow2(d, (220, 120), (570, 165))
    f = font(14, mono=True)
    text_c(d, 395, 30, 'DM40A (Bluetooth LE)', f, BLUE)
    text_c(d, 395, 168, 'C-1-ATK-DM40-CALI (SPP)', f, BLUE)
    text_c(d, 395, 92, 'one at a time', font(13), MID)
    save(im, 'bt-names')


def bt_symbol():
    sc = 3
    s = render('status-all', scale=sc, crop=(320, 0, 480, 34))
    im, d = canvas(s.size[0] + 40, s.size[1] + 80)
    paste(im, d, s, 20, 20)
    down_label(d, 'connected over Bluetooth', (20 + (397 - 320) * sc, 20 + s.size[1] - 6), 20 + s.size[1] + 36, font(15))
    save(im, 'bt-symbol')


def backup():
    im, d = canvas(860, 250)
    box(d, 20, 30, 300, 190, 'Meter: USB disk, INFO/', ['CALIB.BIN  BOOTDATA.BIN', 'UID.BIN  ROMBOOT.BIN', 'LOADER.BIN  OTP.BIN', 'OPTBYTES.BIN  BACKUP.TXT', 'FIRMWARE.BIN'])
    arrow(d, (330, 125), (440, 125))
    text_c(d, 385, 95, 'copy once', font(13), MID)
    box(d, 450, 60, 380, 120, 'Computer', ['a private folder,', 'off the meter'])
    save(im, 'backup')


def loader_key():
    im, d = canvas(880, 250)
    x0, x1, y = 40, 840, 130
    d.line((x0, y, x1, y), fill=MID, width=3)
    tt = lambda s: x0 + s / 5.5 * (x1 - x0)
    d.rectangle((tt(0), y - 34, tt(4.0), y - 14), fill=BLUE)
    d.rectangle((tt(4.0), y - 34, tt(4.5), y - 14), fill=(120, 190, 240))
    text_c(d, (tt(0) + tt(4.0)) / 2, y - 62, 'hold the power key', font(14, bold=True), BLUE)
    text_c(d, tt(4.25), y - 56, '+0.5 s', font(13), BLUE)
    f = font(13)
    for s in (0, 4.0, 4.5):
        d.line((tt(s), y - 8, tt(s), y + 8), fill=DARK, width=2)
    text_c(d, tt(0) + 40, y + 16, 'press:\nthe screen lights up', f)
    text_c(d, tt(3.6), y + 16, 'backlight\nblinks, beep', f)
    text_c(d, tt(5.0), y + 16, 'let go:\nupdate loader', f)
    for s in range(0, 6):
        text_c(d, tt(s), y + 70, f'{s} s', font(12), MID)
    text_c(d, 440, 225, 'Letting go less than 0.5 s after the beep cancels: the meter starts normally.', font(12), MID)
    save(im, 'loader-key')


def loader_install():
    im, d = canvas(860, 230)
    box(d, 20, 50, 250, 120, 'Computer', ['dm40_opendm40_v0.1.bin', 'or dm40_v_2_6.bin'])
    arrow(d, (275, 110), (345, 110))
    box(d, 350, 50, 230, 120, "Loader's USB disk", ['one dm40*.bin', 'only'])
    arrow(d, (585, 110), (655, 110))
    box(d, 660, 50, 180, 120, 'Meter screen', ['press', '"update"'])
    save(im, 'loader-install')


def cannot_detect():
    im, d = canvas(860, 300)
    fb = font(15, bold=True)
    for k, (t, src) in enumerate([('A lead in A, no current: not seen', False), ('Voltage across A and COM: reads as a current', True)]):
        x0 = 20 + k * 430
        text_c(d, x0 + 200, 14, t, fb)
        d.rounded_rectangle((x0, 45, x0 + 400, 285), radius=14, outline=GREY, width=2)
        xs = {'V/Ω': x0 + 80, 'COM': x0 + 200, 'A': x0 + 320}
        for n, x in xs.items():
            jack(d, x, 100, n, r=20)
        d.line((xs['COM'], 160, xs['COM'], 230), fill=DARK, width=4)
        d.line((xs['A'], 160, xs['A'], 230), fill=(200, 30, 30), width=4)
        if src:
            d.rectangle((xs['COM'] - 10, 230, xs['A'] + 10, 270), outline=DARK, width=2)
            text_c(d, (xs['COM'] + xs['A']) / 2, 240, 'voltage source', font(13))
        else:
            text_c(d, (xs['COM'] + xs['A']) / 2, 240, 'nothing flows', font(13), MID)
    save(im, 'cannot-detect')


def tag(d, label, at, tx, ty, f):
    """A white label box (top-left at tx, ty) with a blue frame and a leader
    line from its nearest edge to `at`."""
    lines = label.split('\n')
    w = max(d.textlength(l, font=f) for l in lines) + 12
    h = len(lines) * (f.size + 4) + 6
    d.rounded_rectangle((tx, ty, tx + w, ty + h), radius=5, fill=WHITE, outline=BLUE, width=2)
    for i, l in enumerate(lines):
        d.text((tx + 6, ty + 3 + i * (f.size + 4)), l, font=f, fill=DARK)
    if at is not None:
        cx = min(max(at[0], tx), tx + w)
        cy = min(max(at[1], ty), ty + h)
        if tx <= at[0] <= tx + w:
            cy = ty if at[1] < ty else ty + h
        elif ty <= at[1] <= ty + h:
            cx = tx if at[0] < tx else tx + w
        d.line((cx, cy, at[0], at[1]), fill=BLUE, width=2)
        d.ellipse((at[0] - 4, at[1] - 4, at[0] + 4, at[1] + 4), fill=BLUE)


def up_label(d, label, at, ty, f):
    """A label centred above `at`, top at ty, with a vertical leader down."""
    text_c(d, at[0], ty, label, f)
    d.line((at[0], ty + f.size + 6, at[0], at[1]), fill=BLUE, width=2)
    d.ellipse((at[0] - 4, at[1] - 4, at[0] + 4, at[1] + 4), fill=BLUE)


def down_label(d, label, at, ty, f):
    """A label centred below `at`, top at ty, with a vertical leader up."""
    d.line((at[0], at[1], at[0], ty - 4), fill=BLUE, width=2)
    d.ellipse((at[0] - 4, at[1] - 4, at[0] + 4, at[1] + 4), fill=BLUE)
    text_c(d, at[0], ty, label, f)


def side_label(d, label, at, tx, f, right=True):
    """A label level with `at`, to the right (or left) of tx, with a
    horizontal leader."""
    w = d.textlength(label, font=f)
    if right:
        d.text((tx + 6, at[1] - f.size * 0.6), label, font=f, fill=DARK)
        d.line((at[0], at[1], tx, at[1]), fill=BLUE, width=2)
    else:
        d.text((tx - 6 - w, at[1] - f.size * 0.6), label, font=f, fill=DARK)
        d.line((tx, at[1], at[0], at[1]), fill=BLUE, width=2)
    d.ellipse((at[0] - 4, at[1] - 4, at[0] + 4, at[1] + 4), fill=BLUE)


def trace_y(name, x, top=20, bottom=280):
    """Row of the yellow trace in column x of a scope screenshot."""
    im = Image.open(os.path.join(DST, name + '.png')).convert('RGB')
    for y in range(top, bottom):
        r, g, b = im.getpixel((x, y))
        if r > 180 and g > 180 and b < 90:
            return y
    return (top + bottom) // 2


def swipe():
    s = shot('dmm-bar', 0.75)
    w, h = s.size
    im, d = canvas(w + 420, h + 120)
    x0, y0 = 20, 40
    im.paste(s, (x0, y0))
    cx, cy = x0 + w // 2, y0 + h // 2 + 10
    arrow2(d, (cx - 110, cy), (cx + 110, cy), w=5, head=16)
    finger(im, cx, cy)
    d = ImageDraw.Draw(im)
    d.rectangle((x0 - 1, y0 - 1, x0 + w, y0 + h), outline=GREY)
    text_c(d, x0 + w / 2, 10, 'swipe left: next view    swipe right: previous', font(14), MID)
    nx, nw, nh = x0 + w + 50, 190, 34
    names = ['Multimeter', 'DMM + Scope', 'Oscilloscope', 'Generator']
    fb = font(15, bold=True)
    ys = [y0 + k * 66 for k in range(4)]
    for k, (t, y) in enumerate(zip(names, ys)):
        if t == 'Oscilloscope':
            for xx in range(nx, nx + nw, 12):
                d.line((xx, y, min(xx + 6, nx + nw), y), fill=BLUE, width=2)
                d.line((xx, y + nh, min(xx + 6, nx + nw), y + nh), fill=BLUE, width=2)
            for yy in range(y, y + nh, 12):
                d.line((nx, yy, nx, min(yy + 6, y + nh)), fill=BLUE, width=2)
                d.line((nx + nw, yy, nx + nw, min(yy + 6, y + nh)), fill=BLUE, width=2)
        else:
            d.rounded_rectangle((nx, y, nx + nw, y + nh), radius=8, fill=(240, 246, 252), outline=BLUE, width=2)
        text_c(d, nx + nw / 2, y + 7, t, fb)
        if k < 3:
            arrow2(d, (nx + nw / 2, y + nh + 4), (nx + nw / 2, ys[k + 1] - 4), w=3, head=9)
    rx = nx + nw + 22
    d.line((nx + nw + 4, ys[3] + nh / 2, rx, ys[3] + nh / 2), fill=BLUE, width=3)
    d.line((rx, ys[3] + nh / 2, rx, ys[0] + nh / 2), fill=BLUE, width=3)
    arrow(d, (rx, ys[0] + nh / 2), (nx + nw + 4, ys[0] + nh / 2), w=3, head=9)
    text_c(d, nx + nw / 2 + 10, ys[3] + nh + 16, 'From the oscilloscope (dashed): MENU,\nor Settings > More > Scope swipe', font(12), MID)
    save(im, 'swipe')


def src_shot(item, scale):
    """item: a manual image name, or (dir, name) for another folder."""
    if isinstance(item, tuple):
        return shot(item[1], scale, src=item[0])
    return shot(item, scale)


def ajack_wiring():
    im, d = canvas(560, 340)
    text_c(d, 280, 14, 'DC V selected, the leads still in A and COM', font(15, bold=True))
    d.rounded_rectangle((20, 45, 540, 325), radius=14, outline=GREY, width=2)
    xs = {'V/Ω': 120, 'COM': 280, 'A': 440}
    for n, x in xs.items():
        jack(d, x, 100, n, r=20)
    d.line((xs['COM'], 160, xs['COM'], 240), fill=DARK, width=4)
    d.line((xs['A'], 160, xs['A'], 240), fill=(200, 30, 30), width=4)
    d.rectangle((xs['COM'] - 10, 240, xs['A'] + 10, 285), outline=DARK, width=2)
    text_c(d, 360, 252, 'voltage source', font(14))
    arrow(d, (xs['A'] + 24, 230), (xs['A'] + 24, 165), w=3, head=10)
    text_c(d, 280, 292, 'a current flows through A: A-JACK', font(14, bold=True), RED)
    save(im, 'ajack-wiring')


def scope_topbar():
    sc = 2
    s = shot('scope-run', sc, (0, 0, 480, 20))
    w, h = s.size
    im, d = canvas(w + 40, h + 110)
    x0, y0 = 20, 20
    paste(im, d, s, x0, y0)
    f = font(14)
    for a, b, t in [(13, 75, 'run / stop'), (80, 140, 'timebase list'), (145, 208, 'Wave view'), (286, 347, 'channel popup'), (352, 414, 'trigger menu')]:
        d.rectangle((x0 + a * sc + 1, y0 - 3, x0 + b * sc - 1, y0 + h + 2), outline=BLUE, width=3)
        down_label(d, t, (x0 + (a + b) * sc / 2, y0 + h + 4), y0 + h + 40, f)
    save(im, 'new-scope-topbar')


def pair(name, a, ca, b, cb, crop=None, scale=0.5):
    if crop:
        sa = src_shot(a, 1).crop(crop)
        sb = src_shot(b, 1).crop(crop)
        sa = sa.resize((round(sa.size[0] * scale), round(sa.size[1] * scale)), Image.NEAREST if float(scale).is_integer() else Image.LANCZOS)
        sb = sb.resize(sa.size, Image.NEAREST if float(scale).is_integer() else Image.LANCZOS)
    else:
        sa, sb = src_shot(a, scale), src_shot(b, scale)
    w, h = sa.size
    im, d = canvas(2 * w + 100, h + 80)
    paste(im, d, sa, 20, 40)
    paste(im, d, sb, w + 80, 40)
    fb = font(15, bold=True)
    text_c(d, 20 + w / 2, 12, ca, fb, MID)
    text_c(d, w + 80 + w / 2, 12, cb, fb, BLUE)
    arrow(d, (w + 32, 40 + h / 2), (w + 70, 40 + h / 2), w=3, head=10)
    save(im, name)


def stock_pairs():
    st = STOCK
    pair('new-scope-vdiv', (st, 'st01-default-td'), 'Stock', 'scope-run', 'opendm40', crop=(414, 20, 480, 142), scale=2)
    pair('new-scope-tabs', (st, 'st01-default-td'), 'Stock', 'scope-run', 'opendm40', crop=(0, 230, 414, 320), scale=1)
    pair('new-scope-trig', (st, 'st22-settings-trigger'), 'Stock', 'scope-trig-pulse', 'opendm40')
    pair('new-scope-fft', (st, 'st11-fft'), 'Stock', 'scope', 'opendm40')


def stock_dmm_pairs():
    sd = os.path.join(HERE, 'stock-shots')
    pair('new-420', (sd, 'stock-420-2ma'), 'Stock: 2 mA', 'dmm-ma', 'opendm40: 12 mA')
    pair('new-temp-f', (sd, 'stock-tc-100c'), 'Stock: 214.5 °F', (RENDERS, 'dmm-temp-f'), 'opendm40: 212.5 °F')


def power_screen():
    sc = 1.5
    s = shot('power-ac', sc)
    w, h = s.size
    im, d = canvas(w + 140, h + 40)
    x0, y0 = 70, 20
    paste(im, d, s, x0, y0)
    g = lambda x, y: (x0 + x * sc, y0 + y * sc)
    # On the screen, in empty areas next to the item: 1 V, 2 I, 3 the main
    # value, 6 the chart; outside with a leader: 4 the row of boxes (left),
    # 5 the right column (right).
    for n, (x, y) in [(1, (60, 50)), (2, (262, 50)), (3, (110, 105)), (6, (240, 205))]:
        cx, cy = g(x, y)
        d.ellipse((cx - 16, cy - 16, cx + 16, cy + 16), fill=WHITE)
        badge(d, cx, cy, n)
    for n, at, bx in [(4, (4, 164), x0 - 36), (5, (477, 226), x0 + w + 36)]:
        ax, ay = g(*at)
        d.line((bx, ay, ax, ay), fill=BLUE, width=2)
        d.ellipse((ax - 4, ay - 4, ax + 4, ay + 4), fill=BLUE)
        badge(d, bx, ay, n)
    save(im, 'power-screen')


def main():
    global RENDERS, STOCK
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    RENDERS = sys.argv[1]
    STOCK = sys.argv[2] if len(sys.argv) == 3 else None
    swipe()
    status_bar()
    touch_lock()
    gestures()
    keys()
    install()
    whats_new()
    together()
    functions_row()
    jacks()
    trend_chart()
    scope_page()
    scope_drag()
    meter_strip()
    analog_power()
    gen_offset()
    csv_flow()
    usb_disk()
    usb_row()
    usb_link('usb-link', False)
    usb_link('usb-eject', True)
    delete_bar()
    bt_names()
    bt_symbol()
    backup()
    loader_key()
    loader_install()
    cannot_detect()
    ajack_wiring()
    scope_topbar()
    stock_dmm_pairs()
    power_screen()
    if STOCK:
        stock_pairs()


if __name__ == '__main__':
    main()
