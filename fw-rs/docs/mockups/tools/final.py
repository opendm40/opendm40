# Renders every mockup PNG for docs/combined-view-design.md.
#   /usr/bin/python3 final.py <out_dir> <shots_dir>
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mock import *
import scene

OUT = sys.argv[1]
SHOTS = sys.argv[2] if len(sys.argv) > 2 else ''
os.makedirs(OUT, exist_ok=True)
P = lambda n: os.path.join(OUT, n)

SINE = sine(125, 140, 35 * 2.2)

def bubble(l, cx, cy, t, fg=FG):
    w = l.width('ASCII26', t) + 20
    r = (cx - w // 2, cy - 17, w, 34)
    l.rbox((r[0] - 1, r[1] - 1, r[2] + 2, r[3] + 2), 7, S_FRAME, S_FILL)
    l.rbox(r, 6, 0x18C3, S_FRAME)
    l.glyphs('ASCII26', r, t, fg, 0x18C3, 8, 0)

# ---------------- current (HEAD) ----------------
# The current-state images need the device screenshots (argument 2); without
# them the committed cur-*.png are kept as they are.
if os.path.exists(os.path.join(SHOTS, '16-combined.png')):
    for n in ['16-combined.png', '32-combined-pwm-acv.png', '33-combined-pwm-freq.png']:
        Image.open(os.path.join(SHOTS, n)).save(P('cur-' + n.split('-', 1)[1]))
    # HEAD (40d9a76) AC+DC compact layout over the 16-combined shot
    l = load_png(os.path.join(SHOTS, '16-combined.png'))
    l.fill(40, 22, 440, 34, BG)
    scene.compact(l, True, '\x06', 'mV', ' 504.53', [('53.63', 'mV'), ('501.67', 'mV')])
    save(l, P('cur-combined-acdc-head.png'))

# ---------------- proposed A ----------------
a_dcv(P('a1-dcv.png'))
a_dcv(P('a2-acdc-hold-rel.png'), mode='\x06', main='-600.00', unit='mV',
      subs=[('\x05', '53.63mV'), ('\x04', '-501.67mV')],
      badges=[('AUTO', FG, CHIP), ('HOLD', BG, HOLD_BG), ('REL', FG, SEL)])
a_dcv(P('a3-hz-duty.png'), mode='', main='40.000', unit='Hz',
      subs=[None, ('', '50.0%')], badges=[('AUTO+', FG, CHIP), ('A-HOLD', ACCENT, CHIP)])
a_dcv(P('a4-acv-hv.png'), mode='\x05', main='230.12', unit='V',
      subs=[('', '50.00Hz'), ('', '50.0%')], badges=[('AUTO', FG, CHIP), ('HV', 0xFFFF, 0xC9A0)],
      hv=True, trace=SINE, ac=True, meas=['Freq:50.0Hz', 'Vpp:651V', 'Vavg:0.12V', 'Vrms:230V'])
a_dcv(P('a5-temp.png'), mode='', main='0L', unit='℃', subs=[('', '26.5℃'), ('', '0L℉')],
      badges=[('TC-K', FG, CHIP)])

def picker(l):
    fill(l, 0, 274, 480, 46, S_BAR)
    fill(l, 0, 274, 480, 1, S_FRAME)
    import mock as M
    # the DMM page's own six function buttons (ui.rs dmm_button), y 278
    for i in range(6):
        x = 2 + 80 * i
        c = SEL if i == 0 else BTN
        l.rbox((x, 278, 76, 40), 8, c, S_BAR)
    gl = [('LETTERS', 'V', 'BTN_GLYPH', '\x04'), ('LETTERS', 'A', 'BTN_GLYPH', '\x04')]
    for i, (f1, t1, f2, t2) in enumerate(gl):
        x = 2 + 80 * i; c = SEL if i == 0 else BTN
        l.glyphs(f1, (x, 278, 36, 40), t1, STATUS_FG, c, 4)
        l.glyphs(f2, (x + 40, 278, 30, 40), t2, STATUS_FG, c, 1)
    l.glyphs('BTN_GLYPH', (162, 277, 76, 40), 'Ω', STATUS_FG, BTN, 8)
    l.glyphs('BTN_GLYPH', (242, 278, 76, 40), '\x11', STATUS_FG, BTN, 8)
    l.glyphs('BTN_GLYPH', (326, 277, 33, 40), '\x08', STATUS_FG, BTN, 8)
    l.glyphs('BTN_GLYPH', (364, 278, 33, 40), '\x07', STATUS_FG, BTN, 1)
    l.glyphs('BTN_GLYPH', (405, 276, 36, 40), '\x10', STATUS_FG, BTN, 8)
    l.glyphs('BTN_GLYPH', (442, 278, 30, 40), '℃', STATUS_FG, BTN, 8)
    # strip shows it is the active (pressed) tile
    l.rbox((2, 21, 476, 2), 1, ACCENT, S_BAR)
a_dcv(P('a6-function-picker.png'), extra=picker)

def trig_drag(l):
    G_A.draw(l, PWM, trig_row=140 - 50, dashed=True)
    offset_marker(l, G_A, 140)
    trig_marker(l, G_A, 140 - 50)
    tpos_marker(l, G_A, 200)
    y = G_A.row(140 - 50)
    w = l.width('ASCII26', 'T 3.57 V') + 16
    l.rbox((404 - w, y - 34, w, 28), 6, 0x18C3, S_FILL)
    l.glyphs('ASCII26', (404 - w, y - 34, w, 28), 'T 3.57 V', S_ORANGE, 0x18C3, 8, 0)
a_dcv(P('a7-trigger-drag.png'), extra=trig_drag)

def pinch(l):
    G_A.draw(l, square(50, 140 - 35 * 1.9, 140 + 35 * 0.1, phase=20), trig_row=140 - 35)
    offset_marker(l, G_A, 140); trig_marker(l, G_A, 140 - 35); tpos_marker(l, G_A, 200)
    pill(l, (65, 2, 58, 16), '5ms', 0x18C3, FG, S_BAR, rad=4)
    bubble(l, 213, 188, '5 ms/div')
    for cx in (120, 306):     # finger hints
        circle(l, cx, 188, 13, 0x7BEF, th=2)
a_dcv(P('a8-pinch-timebase.png'), extra=pinch)

# ---------------- B ----------------
b_tile(P('b1-floating-tile.png'))

# ---------------- F39 specimen ----------------
l = sim.LCD(BG)
text(l, 'F14', (6, 4, 200, 14), 'F78 (stock, DMM page)', FG, BG, 0)
l.label('F78', (6, 20, 330, 80), '-12.345', RED, BG, 1)
text(l, 'F14', (6, 110, 300, 14), 'F39 = F78 2x2 box filter (proposed)', FG, BG, 0)
l.label('F39', (6, 126, 300, 40), '-12.345 0L', RED, BG, 1)
text(l, 'F14', (6, 176, 300, 14), 'LETTERS (unit / sub-readings)', FG, BG, 0)
l.label('LETTERS', (6, 192, 300, 30), '53.63mV 50.0% 26.5℃', RED, BG, 1)
text(l, 'F14', (6, 232, 300, 14), 'F14 (chips, badges, meas bar)', FG, BG, 0)
l.label('F14', (6, 248, 300, 14), 'AUTO 10ms 5V DC /2.50V HOLD REL', FG, BG, 1, 1)
text(l, 'F14', (6, 272, 300, 14), 'ASCII26 (transient gesture bubbles only)', FG, BG, 0)
l.label('ASCII26', (6, 288, 300, 28), '5 ms/div  T 3.57 V', FG, BG, 1)
save(l, P('fonts-hierarchy.png'))
print('ok')
