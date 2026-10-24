import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from render import *
import render as R

OUT = sys.argv[1] if len(sys.argv) > 1 else '.'

# ------------------------------------------------------------------ Option A
# Meter strip: y 20..75 (56 px); tile (2,21,476,54) r6 in TILE on S_BAR.
STRIP_Y, STRIP_H = 20, 56
TILE_R = (2, 21, 476, 54)
VAL_R = (56, 26, 174, 46)      # F39 cells, right aligned, bottom = 71
CELLS39 = [[18, 29, 10, 29, 29, 29, 29], [18, 29, 29, 10, 29, 29, 29],
           [18, 29, 29, 29, 10, 29, 29], [18, 29, 29, 29, 29, 10, 29],
           [18, 26, 26, 26, 29, 29, 20]]
UNIT_X = 234
SUB_X0, SUB_X1 = 290, 474     # sub-reading column
BADGE_X, BADGE_W = 6, 46

def cells_text(l, fn, r, s, cells, fg, bg):
    x, y, w, h = r
    for ch, cw in zip(s, cells):
        l.label(fn, (x, y, cw, h), ch, fg, bg, 5 if ch != '.' else 7)
        x += cw

def fmt_cells(s):
    # 7-char right-aligned like fmt7; format index from the point position
    s = s.rjust(7)
    if 'L' in s:
        return s, 4
    p = s.find('.')
    return s, {2: 0, 3: 1, 4: 2, 5: 3}.get(p, 3)

def meter_strip(l, main, unit, mode, subs=(), badges=(), hv=False, hold=False):
    fill(l, 0, STRIP_Y, 480, STRIP_H, S_BAR)
    border = 0xC9A0 if hv else S_BAR
    if hv:
        l.rbox((TILE_R[0] - 2, TILE_R[1] - 1, TILE_R[2] + 4, TILE_R[3] + 2), 8, border, S_BAR)
    l.rbox(TILE_R, 6, TILE, border)
    # badges column (F14 pills 40x15, pitch 17, from y 24)
    for k, (t, fg, bg) in enumerate(badges[:3]):
        pill(l, (BADGE_X, 24 + 17 * k, BADGE_W, 15), t, fg, bg, TILE, rad=3)
    s, f = fmt_cells(main)
    cells_text(l, 'F39', VAL_R, s, CELLS39[f], RED, TILE)
    # mode glyph (AUX_TAG) over the unit (LETTERS), both left-aligned at UNIT_X
    if mode:
        l.label('AUX_TAG', (UNIT_X, 25, 64, 21), mode, RED, TILE, 0)
    l.label('LETTERS', (UNIT_X, 44, 64, 30), unit, RED, TILE, 0)
    # sub readings: two LETTERS rows, value right-aligned to SUB_X1,
    # tag (AUX_TAG) right before the value
    for k, sub in enumerate(subs[:2]):
        if sub is None:
            continue
        tag, txt = sub
        y = 22 + 26 * k
        wv = l.width('LETTERS', txt)
        tx = SUB_X1 - wv
        l.label('LETTERS', (tx, y, wv, 26), txt, RED, TILE, 4)
        if tag:
            l.glyphs('AUX_TAG', (tx - 27, y + 2, 24, 21), tag, RED, TILE, 1)

def right_panel_a(l, y0=76):
    fill(l, 414, y0, 66, 300 - y0 + 20, S_BAR)
    ys = [y0 + 46 * k for k in range(5)]
    magnifier_btn(l, ys[0], 'V', True)
    magnifier_btn(l, ys[1], 'V', False)
    magnifier_btn(l, ys[2], 't', True)
    magnifier_btn(l, ys[3], 't', False)
    text_btn(l, ys[4], 'AUTOSET', S_PURPLE)

def frame_a(l, run='AUTO', tb='10ms', vdiv='5V', ac=False, trig='/ 2.50V'):
    fill(l, 0, 0, 480, 20, S_BAR)
    scope_chips(l, run=run, tb=tb, vdiv=vdiv, ac=ac, trig=trig)
    status_cluster(l, S_BAR, dots=1)
    fill(l, 0, 76, 13, 225, S_BAR)

G_A = Graph(13, 76, 225)

def offset_marker(l, g, row, c=TRACE):
    y = g.row(row)
    for dy in range(-5, 6):
        w = 6 - abs(dy)
        fill(l, 2, y + dy, 5 + w, 1, c)
    l.glyphs('F14', (1, y - 7, 8, 14), '1', S_BAR, c, 0, 0)

def trig_marker(l, g, row, x_right=412):
    y = g.row(row)
    for dy in range(-5, 6):
        w = 6 - abs(dy)
        fill(l, x_right - w, y + dy, w, 1, S_ORANGE)

def tpos_marker(l, g, col):
    x = g.x0 + col
    for dy in range(0, 6):
        w = 6 - dy
        fill(l, x - w + 1, g.y0 + 1 + dy, 2 * w - 1, 1, S_ORANGE)

PWM = square(100, 140 - 35 * 1.9, 140 + 35 * 0.1, phase=40)

def a_dcv(path, mode='\x04', main='4.8176', unit='V', subs=(), badges=None, meas=None, trace=PWM, ac=False, extra=None, hv=False):
    l = sim.LCD(S_BAR)
    frame_a(l, ac=ac)
    if badges is None:
        badges = [('AUTO', FG, CHIP)]
    meter_strip(l, main, unit, mode, subs, badges, hv=hv)
    G_A.draw(l, trace, trig_row=140 - 35)
    offset_marker(l, G_A, 140)
    trig_marker(l, G_A, 140 - 35)
    tpos_marker(l, G_A, 200)
    right_panel_a(l)
    meas_bar(l, meas or ['Freq:40.0Hz', 'Vpp:10.4V', 'Duty:50.0%', 'Vrms:4.82V'])
    if extra:
        extra(l)
    save(l, path)
    return l

# ------------------------------------------------------------------ Option B
def b_tile(path):
    l = sim.LCD(S_BAR)
    fill(l, 0, 0, 480, 20, S_BAR)
    scope_chips(l)
    status_cluster(l, S_BAR, dots=1)
    g = Graph(13, 20, 281)
    g.draw(l, PWM, trig_row=140 - 35)
    offset_marker(l, g, 140)
    trig_marker(l, g, 140 - 35)
    tpos_marker(l, g, 200)
    # floating tile bottom-left inside the graph: (17, 236, 276, 61)
    tr = (17, 236, 276, 61)
    l.rbox((tr[0] - 1, tr[1] - 1, tr[2] + 2, tr[3] + 2), 7, S_FRAME, S_FILL)
    l.rbox(tr, 6, TILE, S_FRAME)
    pill(l, (22, 241, 40, 15), 'AUTO', FG, CHIP, TILE, rad=3)
    s, f = fmt_cells('4.8176')
    cells_text(l, 'F39', (64, 246, 174, 46), s, CELLS39[f], RED, TILE)
    l.label('AUX_TAG', (242, 244, 46, 21), '\x04', RED, TILE, 0)
    l.label('LETTERS', (242, 262, 46, 30), 'V', RED, TILE, 0)
    # right panel 7 buttons (scope-ui-plan S2)
    fill(l, 414, 20, 66, 300, S_BAR)
    ys = [20 + 41 * k for k in range(7)]
    magnifier_btn(l, ys[0], 'V', True, h=40)
    text_btn(l, ys[1], 'DC', TRACE, h=40)
    magnifier_btn(l, ys[2], 'V', False, h=40)
    magnifier_btn(l, ys[3], 't', True, h=40)
    magnifier_btn(l, ys[4], 't', False, h=40)
    text_btn(l, ys[5], 'Auto /', S_PURPLE, h=40)
    text_btn(l, ys[6], 'CUR', FG, h=40)
    fill(l, 0, 20, 13, 281, S_BAR)
    offset_marker(l, g, 140)
    meas_bar(l, ['Freq:40.0Hz', 'Vpp:10.4V', 'Duty:50.0%', 'Vrms:4.82V'])
    save(l, path)

if __name__ == '__main__':
    a_dcv(os.path.join(OUT, 'test_a.png'))
    a_dcv(os.path.join(OUT, 'test_a2.png'), mode='\x06', main='-600.00', unit='mV', subs=[('\x05','53.63mV'),('\x04','-501.67mV')], badges=[('AUTO',FG,CHIP),('HOLD',BG,HOLD_BG),('REL',FG,SEL)])
    a_dcv(os.path.join(OUT, 'test_a3.png'), mode='', main='40.000', unit='KHz', subs=[None,('','50.0%')], badges=[('AUTO+',FG,CHIP),('A-HOLD',ACCENT,CHIP)])
    b_tile(os.path.join(OUT, 'test_b.png'))
