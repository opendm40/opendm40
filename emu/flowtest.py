#!/usr/bin/env python3
"""Headless check of the GTK signal-flow diagram (gtkpanel.FrontendView): the layout must not move when
the firmware switches an input inside one function / range (the owner: "скачет, некрасиво").

The fw-rs switches are built here from the firmware's own constants, independently of gtkpanel's
FrontendView.model_variants table (firmware/src/sentinel.rs, dmm.rs; power-meter-and-misconnect.md 4.4):
  * DC V 6 V: the comparator alternates PB1 (A-jack sentinel, R20 0x90) / SENSE (AC? hint, R20 0x10)
    every 100 ms window; after a comparator hit the A-PROT probe joins (probe_row: OP2 x1 on PB2, AD2 =
    OP2O - PB3 with refs PB6 / AGND (R27 0x6D), peak hold on AD2) and leaves again; DC mV the probe;
  * AC V: AD1 alternates PB1 - PB3 (R24 0x95) / SENSE - PB5 (R24 0x07) every window;
  * frequency: the counter source every 350 ms, comparator on PB4 (R20 0xF0) / the CNT pin (R20 0x08);
  * capacitance: the first counter read moves the comparator PB0 -> OP1O (R20 0x50);
  * thermocouple: SAD1I 3 / 2 / 0 (R33 0xAF / 0xAE / 0xAC), ~0.75 s each;
  * power meter: R34 cycles X·Y / Y·Y / X·Y / X·X (powermeter.rs CYCLE), the counter's comparator SENSE /
    PB4 (pw_cmp_select);
  * every peak-hold row: ENPKH 0 for a few ms at a restart.
Each sequence goes through FrontendView.update() (the history the panel keeps) and is rendered at each
width; height, every wire segment, every block and every pin label must be identical in all frames,
from the first one, and no two labels / blocks closer than 3 px. The state lines are fw-rs emulator
captures (HY3131 StateFile, conn= dropped).

  emu/flowtest.py [--png DIR] [--widths 1000,700,1400]
Exit status 0 = all passed. No emulator, no GTK application (only Gtk widgets offscreen, cairo).
"""
import argparse
import itertools
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.argv, _argv = sys.argv[:1], sys.argv[1:]
import gtkpanel as g  # noqa: E402
import cairo  # noqa: E402

STATES = {
    'dcv6': ('pa=0 shunt=uA pc7=0 role=DcV mode=1 range=0 ad1=1 ad2=0 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=32768 chop=1 sop1p=SENSE sad1fp=OP1O sad1fn=RLU sad1rh=PB6 sad1rl=AGND sad2ip=OP2O scmpi=PB1 '
             'sdio=0 sad1i=0 smode=0x20 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=0 ops2=0 sft1=3 srefo=1 sagnd=1 vrefsel=AGNDP0 pasw=9F,8P,7PS,0DF vjack=1.234 vjack_ac=0 '
             'vjack_hz=50 ijack=0.02 ijack_ac=0 vshunt=0.95 svxi=0 sdo23=0 osr2=32 osr4=128 ad1ig=0.9 ad2ig=1 ad3ig=1 '
             'pkh=0 pkhsel=0 sphacal=0 ad2rate=76800 lpfper=0.0008333 ad2code=0 pkmax=-262144 pkmin=262143 ctb=0 ctc=0'
             ' ctf=0 rd_vdc=1.21937 powered=1 ad1code=736473 lpf=0 rms=0 period=0.4 cj=25 die=25'),
    'dcmv': ('pa=1 shunt=uA pc7=0 role=DcMv mode=3 range=1 ad1=1 ad2=0 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=32768 chop=1 sop1p=RLU sad1fp=OP1O sad1fn=AGND sad1rh=PB6 sad1rl=AGND sad2ip=OP2O scmpi=PB1 '
             'sdio=0 sad1i=0 smode=0x11 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=0 ops2=0 sft1=3 srefo=1 sagnd=1 vrefsel=AGNDP0 pasw=8P vjack=0.1 vjack_ac=0 vjack_hz=50 ijack=0.02'
             ' ijack_ac=0 vshunt=0.95 svxi=0 sdo23=0 osr2=32 osr4=128 ad1ig=0.9 ad2ig=0.5 ad3ig=0.5 pkh=0 pkhsel=0 '
             'sphacal=0 ad2rate=76800 lpfper=0.0008333 ad2code=0 pkmax=-262144 pkmin=262143 ctb=0 ctc=0 ctf=0 '
             'rd_vdc=0.0993397 powered=1 ad1code=599991 lpf=0 rms=0 period=0.4 cj=25 die=25'),
    'acv': ('pa=0 shunt=uA pc7=0 role=AcV mode=0 range=0 ad1=1 ad2=1 ad3=1 lpf=1 rms=1 ctr=1 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=256 chop=0 sop1p=SENSE sad1fp=PB1 sad1fn=PB3 sad1rh=PB6 sad1rl=AGND sad2ip=PB4 scmpi=PB4 '
             'sdio=0 sad1i=0 smode=0x20 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=PB6 sad2rl=AGND sad3ip=PB4 sad3in=AGND '
             'ops1=1 ops2=1 sft1=3 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=9F,8P,6PS,1F vjack=0 vjack_ac=1 vjack_hz=50 '
             'ijack=0 ijack_ac=0 vshunt=0 svxi=1 sdo23=0 osr2=32 osr4=16384 ad1ig=3.6 ad2ig=1 ad3ig=1 pkh=1 pkhsel=1 '
             'sphacal=0 ad2rate=76800 lpfper=0.1067 ad2code=28682 pkmax=20 pkmin=-24 ctb=3 ctc=147456 ctf=50 '
             'rd_vrms=0.993181 powered=1 ad1code=322 lpf=0 rms=433827027 period=0.001563 cj=25 die=25'),
    'aca': ('pa=0 shunt=uA pc7=0 role=AcA mode=5 range=1 ad1=1 ad2=1 ad3=1 lpf=1 rms=1 ctr=1 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=256 chop=0 sop1p=PB1 sad1fp=SENSE sad1fn=AGND sad1rh=PB6 sad1rl=AGND sad2ip=OP1O scmpi=PB4 '
             'sdio=0 sad1i=0 smode=0x01 sfuvr=7 sop2p=AGND sad2in=PB3 sad2rh=PB6 sad2rl=AGND sad3ip=OP1O sad3in=PB3 '
             'ops1=0 ops2=0 sft1=3 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=8P,5S vjack=0 vjack_ac=0 vjack_hz=50 ijack=0 '
             'ijack_ac=0 vshunt=0 svxi=1 sdo23=0 osr2=32 osr4=16384 ad1ig=3.6 ad2ig=1 ad3ig=1 pkh=1 pkhsel=1 sphacal=0'
             ' ad2rate=76800 lpfper=0.1067 ad2code=-460 pkmax=24 pkmin=-32 ctb=0 ctc=0 ctf=0 rd_irms=3.28443E-05 '
             'powered=1 ad1code=-546 lpf=-1 rms=107045 period=0.001563 cj=25 die=25'),
    'dca': ('pa=0 shunt=uA pc7=0 role=DcA mode=8 range=0 ad1=1 ad2=0 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=32768 chop=1 sop1p=PB1 sad1fp=OP1O sad1fn=PB3 sad1rh=PB6 sad1rl=AGND sad2ip=OP2O scmpi=SENSE '
             'sdio=0 sad1i=0 smode=0x01 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=1 ops2=0 sft1=1 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=8P,5S vjack=0 vjack_ac=0 vjack_hz=50 ijack=0 '
             'ijack_ac=0.1 vshunt=0 svxi=0 sdo23=0 osr2=32 osr4=128 ad1ig=0.9 ad2ig=0.5 ad3ig=0.5 pkh=0 pkhsel=0 '
             'sphacal=0 ad2rate=76800 lpfper=0.0008333 ad2code=0 pkmax=-262144 pkmin=262143 ctb=0 ctc=0 ctf=0 '
             'rd_idc=-4.35707E-08 powered=1 ad1code=-125 lpf=0 rms=0 period=0.4 cj=25 die=25'),
    'ohm': ('pa=1 shunt=uA pc7=0 role=Ohm mode=10 range=1 ad1=1 ad2=0 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=32768 chop=1 sop1p=PB0 sad1fp=OP1O sad1fn=PB3 sad1rh=FB sad1rl=RLU sad2ip=OP2O scmpi=PB1 '
             'sdio=0 sad1i=0 smode=0x14 sfuvr=2 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=0 ops2=0 sft1=2 srefo=1 sagnd=1 vrefsel=VDS15 pasw=4DF,3D vjack=0 vjack_ac=0 vjack_hz=50 ijack=0 '
             'ijack_ac=0 vshunt=0 vsrc=1.4 rref=1002.7 vx=0.013889 itest=0.0013823 svxi=0 sdo23=0 osr2=32 osr4=128 '
             'ad1ig=0.9 ad2ig=0.5 ad3ig=0.5 pkh=0 pkhsel=0 sphacal=0 ad2rate=76800 lpfper=0.0008333 ad2code=0 '
             'pkmax=-262144 pkmin=262143 ctb=0 ctc=0 ctf=0 rd_r=10.0518 powered=1 ad1code=75682 lpf=0 rms=0 period=0.4'
             ' cj=25 die=25'),
    'cont': ('pa=1 shunt=uA pc7=0 role=Cont mode=11 range=0 ad1=1 ad2=1 ad3=0 lpf=0 rms=0 ctr=1 cmp=1 cmppin=0 op1=1 '
             'op2=1 osr1=32768 chop=1 sop1p=SENSE sad1fp=OP1O sad1fn=PB3 sad1rh=RLU sad1rl=FB sad2ip=OP2O scmpi=OP1O '
             'sdio=0 sad1i=0 smode=0x14 sfuvr=13 sop2p=PB1 sad2in=PB3 sad2rh=PB6 sad2rl=AGND sad3ip=OP2O sad3in=AGND '
             'ops1=0 ops2=0 sft1=2 srefo=1 sagnd=0 vrefsel=AGNDN7 pasw=8S,4DF,3D vjack=0 vjack_ac=0 vjack_hz=50 '
             'ijack=0 ijack_ac=0 vshunt=0 vsrc=-0.625 rref=1002.7 vx=-0.625 itest=-1.1072E-19 svxi=0 sdo23=0 osr2=256 '
             'osr4=128 ad1ig=0.9 ad2ig=2 ad3ig=0.5 pkh=1 pkhsel=0 sphacal=0 ad2rate=9600 lpfper=0.006667 ad2code=-3 '
             'pkmax=31 pkmin=-26 ctb=0 ctc=0 ctf=0 rd_r=1114.1 powered=1 ad1code=-8388608 lpf=0 rms=0 period=0.4 cj=25'
             ' die=25'),
    'diode': ('pa=1 shunt=uA pc7=0 role=Diode mode=12 range=0 ad1=1 ad2=1 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=1 osr1=2048 chop=1 sop1p=PB0 sad1fp=OP1O sad1fn=PB3 sad1rh=PB6 sad1rl=AGND sad2ip=OP2O scmpi=RLD '
             'sdio=1 sad1i=0 smode=0x06 sfuvr=2 sop2p=PB1 sad2in=PB3 sad2rh=PB6 sad2rl=AGND sad3ip=OP2O sad3in=AGND '
             'ops1=0 ops2=0 sft1=2 srefo=1 sagnd=2 vrefsel=VDS15 pasw=5P,4P,1P vjack=0 vjack_ac=0 vjack_hz=50 ijack=0 '
             'ijack_ac=0 vshunt=0 vx=3.3 itest=0.000765 compl=3.3 svxi=0 sdo23=0 osr2=256 osr4=128 ad1ig=0.9 ad2ig=2 '
             'ad3ig=0.5 pkh=1 pkhsel=0 sphacal=0 ad2rate=9600 lpfper=0.006667 ad2code=14 pkmax=31 pkmin=-30 ctb=0 '
             'ctc=0 ctf=0 powered=1 ad1code=1975904 lpf=0 rms=0 period=0.025 cj=25 die=25'),
    'cap': ('pa=1 shunt=uA pc7=0 role=CapOsc mode=13 range=3 ad1=0 ad2=1 ad3=0 lpf=0 rms=0 ctr=1 cmp=1 cmppin=0 op1=1'
             ' op2=1 osr1=16384 chop=1 sop1p=PB0 sad1fp=PB3 sad1fn=PB3 sad1rh=REFO sad1rl=AGND sad2ip=OP2O scmpi=OP1O '
             'sdio=0 sad1i=0 smode=0x1A sfuvr=7 sop2p=PB1 sad2in=PB3 sad2rh=PB6 sad2rl=AGND sad3ip=OP2O sad3in=AGND '
             'ops1=0 ops2=0 sft1=3 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=6P,5P vjack=0 vjack_ac=1 vjack_hz=1000 ijack=0 '
             'ijack_ac=0 vshunt=0 rcosc=10680 svxi=0 sdo23=0 osr2=256 osr4=128 ad1ig=0.9 ad2ig=2 ad3ig=0.5 pkh=1 '
             'pkhsel=0 sphacal=0 ad2rate=9600 lpfper=0.006667 ad2code=10 pkmax=42 pkmin=-23 ctb=14673 ctc=131072 '
             'ctf=275119 powered=1 ad1code=0 lpf=0 rms=0 period=0.2 cj=25 die=25'),
    'freq': ('pa=1 shunt=uA pc7=1 role=Freq mode=14 range=0 ad1=1 ad2=1 ad3=0 lpf=1 rms=1 ctr=1 cmp=0 cmppin=0 op1=1 '
             'op2=0 osr1=8192 chop=1 sop1p=RLU sad1fp=PB5 sad1fn=PB5 sad1rh=REFO sad1rl=AGND sad2ip=OP1O scmpi=SENSE '
             'sdio=1 sad1i=3 smode=0x11 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=0 ops2=0 sft1=3 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=8P vjack=0.003096 vjack_ac=0 vjack_hz=50 '
             'ijack=0 ijack_ac=0 vshunt=0 svxi=0 sdo23=0 osr2=32 osr4=16384 ad1ig=0.9 ad2ig=0.5 ad3ig=0.5 pkh=0 '
             'pkhsel=0 sphacal=0 ad2rate=76800 lpfper=0.1067 ad2code=342 pkmax=-262144 pkmin=262143 ctb=0 ctc=0 ctf=0 '
             'powered=1 ad1code=-27 lpf=325 rms=900 period=0.1 cj=25 die=25'),
    'temp_tc': ('pa=1 shunt=uA pc7=0 role=Temp mode=15 range=0 ad1=1 ad2=0 ad3=0 lpf=0 rms=0 ctr=0 cmp=1 cmppin=0 op1=1 '
             'op2=0 osr1=16384 chop=1 sop1p=PB0 sad1fp=OP1O sad1fn=AGND sad1rh=PB6 sad1rl=AGND sad2ip=OP2O scmpi=PB1 '
             'sdio=0 sad1i=3 smode=0x15 sfuvr=7 sop2p=AGND sad2in=AGND sad2rh=REFO sad2rl=AGND sad3ip=OP2O sad3in=AGND'
             ' ops1=1 ops2=0 sft1=3 srefo=1 sagnd=1 vrefsel=AGNDP0 pasw=8P,7DF,2DF vjack=0 vjack_ac=0 vjack_hz=50 '
             'ijack=0 ijack_ac=0 vshunt=0 svxi=0 sdo23=0 osr2=32 osr4=128 ad1ig=0.9 ad2ig=0.5 ad3ig=0.5 pkh=0 pkhsel=0'
             ' sphacal=0 ad2rate=76800 lpfper=0.0008333 ad2code=0 pkmax=-262144 pkmin=262143 ctb=0 ctc=0 ctf=0 '
             'powered=1 ad1code=319440 lpf=0 rms=0 period=0.2 cj=25 die=25'),
    'power': ('pa=0 shunt=A pc7=0 role=Power mode=16 range=13 ad1=1 ad2=1 ad3=1 lpf=1 rms=1 ctr=1 cmp=1 cmppin=0 op1=1 '
             'op2=1 osr1=8192 chop=1 sop1p=PB2 sad1fp=OP1O sad1fn=PB3 sad1rh=PB6 sad1rl=AGND sad2ip=OP1O scmpi=SENSE '
             'sdio=0 sad1i=0 smode=0x20 sfuvr=7 sop2p=SENSE sad2in=PB3 sad2rh=PB6 sad2rl=AGND sad3ip=OP2O sad3in=AGND '
             'ops1=0 ops2=0 sft1=3 srefo=1 sagnd=0 vrefsel=AGNDP0 pasw=9F,8P,5PS,1F vjack=0 vjack_ac=230 vjack_hz=50 '
             'ijack=0 ijack_ac=1 vshunt=0 svxi=1 sdo23=1 osr2=32 osr4=16384 ad1ig=0.9 ad2ig=1 ad3ig=1 pkh=0 pkhsel=0 '
             'sphacal=0 ad2rate=76800 lpfper=0.1067 ad2code=67793 pkmax=-262144 pkmin=262143 ctb=3 ctc=147456 ctf=50 '
             'powxs=2097.15 powys=209.715 rd_vrms=228.962 rd_vdc=0 rd_ad1idc=-0.00144045 powered=1 ad1code=-87 lpf=0 '
             'rms=2305616555 period=0.1 cj=25 die=25'),
}

PROBE = dict(op2='1', sop2p='PB2', ops2='0', ad2='1', sad2ip='OP2O', sad2in='PB3', sad2rh='PB6', sad2rl='AGND',
             pkh='1', pkhsel='0')


def toggles(st):
    """-> [(name, [states 100 ms apart])]: the switches fw-rs makes in this row"""
    r, role = [], st.get('role')
    cm = st.get('scmpi') if st.get('cmp') == '1' else None
    if role == 'DcV' and st.get('range') == '0' and cm in ('PB1', 'SENSE'):
        a, b = dict(st, scmpi='PB1'), dict(st, scmpi='SENSE')
        r.append(('cmp PB1/SENSE', [a, b] * 4))
        pa, pb = dict(a, **PROBE), dict(b, **PROBE)
        r.append(('cmp + probe', [a, b, a, pa, pb, pa, dict(pb, pkh='0'), pa, pb, a, b, a, b]))
    if role == 'DcMv' and cm == 'PB1':
        r.append(('probe', [st, st, dict(st, **PROBE), dict(st, **PROBE), dict(st, **dict(PROBE, pkh='0')), st, st]))
    if role == 'AcV' and st.get('ad1') == '1' and (st.get('sad1fp'), st.get('sad1fn')) in (('PB1', 'PB3'), ('SENSE', 'PB5')):
        a, b = dict(st, sad1fp='PB1', sad1fn='PB3'), dict(st, sad1fp='SENSE', sad1fn='PB5')
        r.append(('AD1 PB1/SENSE', [a, b, a, b, dict(a, pkh='0'), b, a, b]))
    if role == 'Temp' and st.get('ad1') == '1':
        r.append(('TC phases', [dict(st, sad1i=x) for x in '3203203']))
    if role == 'Freq':
        a, b = dict(st, cmp='1', scmpi='PB4'), dict(st, cmp='0', scmpi='SENSE')
        r.append(('counter source', [a, b] * 4))
    if role == 'CapOsc' and cm in ('PB0', 'OP1O'):
        r.append(('cap CMP', [dict(st, scmpi='PB0'), dict(st, scmpi='OP1O'), dict(st, scmpi='OP1O')]))
    if role == 'Power':
        r.append(('power statistic', [dict(st, svxi=x, sdo23=y) for x, y in ('10', '11', '10', '00', '10', '11')]))
        if cm in ('SENSE', 'PB4'):
            r.append(('power CMP', [dict(st, scmpi=x) for x in ('SENSE', 'PB4', 'PB4', 'SENSE')]))
    if st.get('pkh') == '1':
        r.append(('ENPKH restart', [st, dict(st, pkh='0'), st, st, dict(st, pkh='0'), st]))
    return r


def render(v, w, png=None):
    h = int(v.layout(None, w)) + 1
    surf = cairo.ImageSurface(cairo.FORMAT_RGB24, w, h)
    cr = cairo.Context(surf)
    cr.set_source_rgb(*g.rgb('#1E1F22'))
    cr.paint()
    v.ink, v.segs = [], []
    v.layout(cr, w)
    ink, segs = v.ink, v.segs
    v.ink = v.segs = None
    if png:
        surf.write_to_png(png)
    return ink, segs, h


PINS = set(sum(g.FrontendView.MUX.values(), [])) | {'FB/RLU', '–'}
BLOCKS = ('box', 'op', 'mul', 'jack', 'part', 'com', 'chip', 'board')


def geometry(ink, segs, h):
    """what must not move: the height, blocks (box texts may change, not their boxes), pin labels, wires"""
    def name(b):
        return ' '.join(b[0].split(' ')[:2]) if b[0].startswith(('op ', 'box AD', 'box CMP')) else b[0]
    bx = sorted((name(b),) + tuple(round(c, 1) for c in b[1:]) for b in ink if b[0].split(' ')[0] in BLOCKS)
    pl = sorted((b[0], round(b[1], 1), round(b[2], 1)) for b in ink if b[0] in PINS or b[0].startswith('PA'))
    sg = sorted(tuple(round(c, 1) for c in s[1:]) for s in segs)
    return h, bx, pl, sg


def describe(a, b):
    out = []
    if a[0] != b[0]:
        out.append('height %d -> %d' % (a[0], b[0]))
    for i, n in ((1, 'blocks'), (2, 'pins'), (3, 'wires')):
        x, y = set(a[i]), set(b[i])
        if x != y:
            out.append('%s -%s +%s' % (n, sorted(x - y)[:2], sorted(y - x)[:2]))
    return '; '.join(out)


def close(ink):
    for a, b in itertools.combinations(ink, 2):
        d = max(b[1] - a[3], a[1] - b[3], b[2] - a[4], a[2] - b[4])
        if d < 3:
            yield '%.1f px %r %r' % (d, a[0], b[0])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--png', help='write the frames as PNG into this directory')
    ap.add_argument('--widths', default='1000,700,1400')
    a = ap.parse_args(_argv)
    widths = [int(x) for x in a.widths.split(',')]
    if a.png:
        os.makedirs(a.png, exist_ok=True)
    bad = n_seq = 0
    for name, line in STATES.items():
        st0 = g.parse_state(line)
        for tn, frames in toggles(st0):
            n_seq += 1
            for w in widths:
                v = g.FrontendView()
                ref = None
                for i, fr in enumerate(frames):
                    v.update(fr)
                    png = os.path.join(a.png, '%s-%s-%d-f%02d.png' % (name, tn.replace(' ', '_').replace('/', '-'), w, i)) \
                        if a.png else None
                    ink, segs, h = render(v, w, png)
                    gm = geometry(ink, segs, h)
                    for m in close(ink):
                        bad += 1
                        print('FAIL %s / %s / %d px frame %d: labels too close: %s' % (name, tn, w, i, m))
                    if ref is None:
                        ref = gm
                    elif gm != ref:
                        bad += 1
                        print('FAIL %s / %s / %d px frame %d moves: %s' % (name, tn, w, i, describe(ref, gm)))
                        break
            print('%-8s %-16s %d frames x %d widths' % (name, tn, len(frames), len(widths)))
    print('%d sequences, %d problems' % (n_seq, bad))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
