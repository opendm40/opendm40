"""DM40 DMM front end, simplified schematic (facts: hw/README.md "DMM front end").

Render: python3 dmm_frontend.py  (needs schemdraw; writes dmm_frontend.svg)
"""
import os

import schemdraw
import schemdraw.elements as elm

schemdraw.use('svg')
schemdraw.config(unit=2, inches_per_unit=0.46, fontsize=13, lw=1.5, bgcolor='white',
                 font='Arial, Helvetica, sans-serif')

GREY = '#808080'     # inferred
RED = '#c03030'      # isolation barrier
FS = 13              # label size


def inf(e):
    """Inferred element: grey, dashed."""
    return e.color(GREY).linestyle('--')


def wire(d, *pts, inferred=False):
    for p, q in zip(pts, pts[1:]):
        ln = elm.Line().at(p).to(q)
        d += inf(ln) if inferred else ln


def text(d, xy, s, halign='center', color='black', size=FS):
    d += elm.Label().at(xy).label(s, halign=halign, color=color, fontsize=size).zorder(4)


def box(d, x0, y0, x1, y1, s, inferred=False, size=FS):
    col = GREY if inferred else 'black'
    d += elm.Rect(corner1=(x0, y0), corner2=(x1, y1), fill='white',
                  ls='--' if inferred else '-', color=col).at((0, 0)).theta(0).zorder(3)
    text(d, ((x0 + x1) / 2, (y0 + y1) / 2), s, color=col, size=size)


def jack(d, xy, name, halign):
    d += elm.Dot(open=True, radius=0.18).at(xy)
    dx = 0.35 if halign == 'left' else -0.35
    text(d, (xy[0] + dx, xy[1]), name, halign=halign, size=13)


with schemdraw.Drawing(show=False) as d:
    # ---- HY3131 --------------------------------------------------------
    hy = elm.Ic(size=(5, 7), leadlen=0.6, pins=[
        elm.IcPin(name='SPI', side='left', pos=0.85),
        elm.IcPin(name='VDD', side='left', pos=0.55),
        elm.IcPin(name='', side='left', pos=0.30, anchorname='X1'),
        elm.IcPin(name='', side='left', pos=0.08, anchorname='X2'),
        elm.IcPin(name='SENSE', side='right', pos=0.90),
        elm.IcPin(name='PA7', side='right', pos=0.78),
        elm.IcPin(name='PA6', side='right', pos=0.68),
        elm.IcPin(name='PA5', side='right', pos=0.58),
        elm.IcPin(name='PA4', side='right', pos=0.48),
        elm.IcPin(name='PB5', side='right', pos=0.29),
        elm.IcPin(name='PB6', side='right', pos=0.11),
        elm.IcPin(name='PB3', side='bottom', pos=0.2),
        elm.IcPin(name='PB1', side='bottom', pos=0.5),
        elm.IcPin(name='PB2', side='bottom', pos=0.8),
    ])
    d += hy.at((7.2, 0.0)).label('HY3131', loc='center', fontsize=15)

    # ---- isolation barrier (MCU side on the left) ----------------------
    BX = 4.0
    text(d, (BX - 0.3, 10.95), 'MCU side', halign='right', color=RED)
    text(d, (BX + 0.3, 10.95), 'DMM side (isolated)', halign='left', color=RED)

    W = 1.55

    def crossing(y, name, mcu_lines, inferred=False, size=FS):
        box(d, BX - W, y - 0.65, BX + W, y + 0.65, name, inferred, size)
        wire(d, (BX - W - 0.8, y), (BX - W, y))
        n = len(mcu_lines)
        for i, s in enumerate(mcu_lines):
            text(d, (BX - W - 0.9, y + 0.32 * (n - 1) - 0.64 * i), s, halign='right')

    RLY_Y = 9.9
    crossing(RLY_Y, 'optocouplers', ['PA9 / PA10'], inferred=True, size=FS)
    ys = hy.SPI[1]
    crossing(ys, 'CA-IS3741', ['SPI2', 'PC10..12, PD2'])
    yv = hy.VDD[1]
    crossing(yv, 'VPS8504B\n+ LT61', ['PE2 rail'], size=FS)
    GATE_Y = -5.3
    crossing(GATE_Y, 'optocouplers', ['PE0 / PE1'], inferred=True, size=FS)
    wire(d, (BX + W, GATE_Y), (BX + W + 0.7, GATE_Y), inferred=True)
    text(d, (BX + W + 0.8, GATE_Y), 'to the MOSFET gates', halign='left', color=GREY)

    # the barrier line, interrupted by the parts that cross it
    edges = [-5.9] + sorted(v for y in (GATE_Y, yv, ys, RLY_Y)
                            for v in (y - 0.65, y + 0.65)) + [10.6]
    for y0, y1 in zip(edges[::2], edges[1::2]):
        d += elm.Line(ls='--', lw=2.5, color=RED).at((BX, y0)).to((BX, y1))

    d += elm.Line(lw=3.5).at((BX + W, ys)).to(hy.SPI)
    wire(d, (BX + W, yv), hy.VDD)

    # crystal
    xc = hy.X1[0] - 0.9
    wire(d, hy.X1, (xc, hy.X1[1]))
    wire(d, hy.X2, (xc, hy.X2[1]))
    d += elm.Crystal().at((xc, hy.X1[1])).to((xc, hy.X2[1]))
    text(d, (xc - 0.45, (hy.X1[1] + hy.X2[1]) / 2), '4.9152\nMHz', halign='right')

    # reference and PB5
    wire(d, hy.PB6, (14.2, hy.PB6[1]), inferred=True)
    box(d, 14.2, hy.PB6[1] - 0.55, 18.0, hy.PB6[1] + 0.55, 'MAX6190 1.25 V',
        inferred=True)
    wire(d, hy.PB5, (13.8, hy.PB5[1]))
    text(d, (13.95, hy.PB5[1]), '?', halign='left', size=13)

    # ---- V / ohm input -------------------------------------------------
    VY = 7.9
    # divider legs on the HY3131 PA pins
    DX0, DX1 = 14.4, 18.4
    DY0, DY1 = hy.PA4[1] - 0.5, hy.PA7[1] + 0.5
    box(d, DX0, DY0, DX1, DY1,
        'divider legs\n1.111 M, 101.01 k,\n10.01 k, 1 k', inferred=True, size=FS)
    for p in ('PA7', 'PA6', 'PA5', 'PA4'):
        a = getattr(hy, p)
        wire(d, a, (DX0, a[1]), inferred=True)
    text(d, (DX1 + 0.15, hy.PA7[1]), '/10 PA7', halign='left', color=GREY, size=FS)
    text(d, (DX1 + 0.15, hy.PA6[1]), '/100 PA6', halign='left', color=GREY, size=FS)
    text(d, (DX1 + 0.15, hy.PA5[1]), '/1000 PA5', halign='left', color=GREY, size=FS)
    text(d, (DX1 + 0.15, hy.PA4[1]), '/10000 PA4 + PA5', halign='left', color=GREY, size=FS)

    # SENSE node and 10 M
    SX = 13.6
    wire(d, hy.SENSE, (SX, hy.SENSE[1]), (SX, VY), inferred=True)
    d += elm.Dot().at((SX, VY))
    wire(d, (SX, VY), (15.2, VY))
    d += elm.Dot().at((15.2, VY))
    wire(d, (15.2, VY), (15.2, DY1), inferred=True)
    r10m = elm.Resistor().at((15.2, VY)).to((18.6, VY)).label(
        '10 M, TC1746B6 (B / C)', loc='top', fontsize=FS)
    d += inf(r10m)

    # relay: common towards the input, one throw to the divider
    sw = elm.SwitchSpdt2().at((20.4, VY)).theta(180).anchor('c').flip()
    d += sw
    wire(d, (18.6, VY), sw.c)
    wire(d, sw.b, (sw.b[0] - 0.4, sw.b[1]), (sw.b[0] - 0.4, VY + 1.1))
    text(d, (sw.b[0] - 0.55, VY + 1.25), 'Ω / mV network', halign='right', size=FS)
    text(d, (sw.a[0] - 0.3, VY - 0.6), 'HFD4/3-LS', size=FS)
    # coil, driven through the optocouplers
    CX0, CX1 = 21.6, 23.6
    box(d, CX0, RLY_Y - 0.5, CX1, RLY_Y + 0.5, 'coil', inferred=True, size=FS)
    wire(d, (BX + W, RLY_Y), (CX0, RLY_Y), inferred=True)
    d += elm.Line(ls=':', color=GREY).at(((CX0 + CX1) / 2, RLY_Y - 0.5)).to(
        ((CX0 + CX1) / 2 - 1.6, VY + 0.25))

    # PTC, MOV, jack
    d += elm.Thermistor().at(sw.a).to((25.0, VY)).label('PTC', loc='bottom', fontsize=FS)
    d += elm.Dot().at((25.0, VY))
    wire(d, (25.0, VY), (26.2, VY))
    jack(d, (26.4, VY), 'V/Ω', 'left')
    d += elm.Thermistor().at((25.0, VY)).down().length(2.2).label(
        'MOV\n07D681K', loc='bottom', fontsize=FS)
    text(d, (25.0, VY - 2.65), '?', size=13)

    # ---- current path --------------------------------------------------
    IY = -2.1
    C, P1, NA, P2 = 8.5, 12.2, 16.2, 19.6
    jack(d, (6.8, IY), 'COM', 'right')
    wire(d, (7.0, IY), (C, IY))
    d += elm.Dot().at((C, IY))
    d += inf(elm.Resistor().at((C, IY)).to((P1, IY)).label('47.5 Ω', loc='top', fontsize=FS))
    d += elm.Dot().at((P1, IY))
    d += elm.Resistor().at((P1, IY)).to((NA, IY)).label('0.5 Ω', loc='top', fontsize=FS)
    d += elm.Dot().at((NA, IY))
    d += inf(elm.Resistor().at((NA, IY)).to((P2, IY)).label('10 mΩ', loc='top', fontsize=FS))
    d += elm.Dot().at((P2, IY))
    d += elm.Fuse().at((P2, IY)).to((22.4, IY)).label('fuse', loc='top', fontsize=FS)
    jack(d, (22.6, IY), 'A/mA/µA', 'left')

    # taps (README: PB2 top of 10 mohm, PB1 top of 47.5 ohm, PB3 low side)
    wire(d, hy.PB3, (C, IY), inferred=True)
    wire(d, hy.PB1, (hy.PB1[0], -1.05), (P1, -1.05), (P1, IY), inferred=True)
    wire(d, hy.PB2, (hy.PB2[0], -0.75), (P2, -0.75), (P2, IY), inferred=True)

    # MOSFETs across the lower shunts, to the COM node
    RAIL = -4.5
    for x, pe in ((NA, 'PE0'), (P1, 'PE1')):
        q = elm.NFet().at((x, IY - 0.9)).theta(0).anchor('drain')
        d += inf(q)
        wire(d, (x, IY), (x, IY - 0.9), inferred=True)
        wire(d, q.source, (x, RAIL), inferred=True)
        wire(d, q.gate, (q.gate[0] + 0.4, q.gate[1]), inferred=True)
        text(d, (q.gate[0] + 0.5, q.gate[1]), pe, halign='left', color=GREY)
    wire(d, (C, IY), (C, RAIL), (NA, RAIL), inferred=True)
    d += elm.Dot().at((P1, RAIL))
    text(d, (NA + 0.3, RAIL), 'MOSFETs RUH30J51M', halign='left', color=GREY)

    # room for the right-aligned MCU labels (their width is estimated low)
    d += elm.Line(color='white').at((-1.7, ys)).to((-1.6, ys))

    # legend
    text(d, (BX - 3.6, -6.3),
         'Grey dashed = inferred.  ? = not known.  Red dashed line = isolation barrier.',
         halign='left', size=13)

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'dmm_frontend.svg')
d.save(out)
