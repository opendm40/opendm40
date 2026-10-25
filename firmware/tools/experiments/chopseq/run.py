"""Device run (see cs.py). Usage: run.py [group ...]; groups: noise, sw, lvl, sq."""
import sys, os, time, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cs import *
m = M(); c = Cal()
st = {q: m.q(q) for q in ['*IDN?', 'SYST:ALER:ALARm?', 'SYST:BEEP:VOL?', 'CONF?', 'VOLT:RANG:AUTO?', 'SENS:RATE?', 'LOG:DEST?', 'LOG:SOUR?', 'LOG:OSR?', 'LOG:CHOP?', 'MMEM:CAT?']}
p = os.path.join(OUT, 'start-%d.json' % int(time.time())); json.dump(st, open(p, 'w'), indent=1)
print({k: v[:80] for k, v in st.items()}, flush=True)
m.w('CONF:VOLT:DC'); time.sleep(1); m.w('VOLT:RANG 6'); m.w('LOG:DEST HOST'); m.w('DBG:AD2:DRAW 0'); time.sleep(1)
print('CONF?', m.q('CONF?'), m.q('SYST:ERR?'), flush=True)
G = sys.argv[1:] or ['sw', 'noise', 'lvl', 'sq']
T = time.time()
def run(*a, **k):
    stream(m, *a, **k); print('  t=%.0f s' % (time.time() - T), flush=True)
try:
    if 'sw' in G:
        c.dc(0.0); c.off(); time.sleep(3)
        run(6, 0, (1, 8, 0), 60, 'off_blk8_d0')
        run(6, 0, (1, 8, 50), 60, 'off_blk8_d50')
        run(6, 0, (1, 8, 25), 60, 'off_blk8_d25')
        run(6, 0, (1, 1, 0), 60, 'off_abab_short')
    if 'noise' in G:
        c.dc(0.0); c.off(); time.sleep(3)
        for name, osr, chop, seq in [('off_06', 6, 0, None), ('off_16', 6, 1, None), ('off_abab', 6, 0, (1, 1, 0)),
                                     ('off_prbs', 6, 0, (2, 0, 0)), ('off_rare10', 6, 0, (3, 10, 0))]:
            run(osr, chop, seq, 300, name)
    if 'noise2' in G:
        c.dc(0.0); c.off(); time.sleep(3)
        for name, osr, chop, seq in [('off2_rare20', 6, 0, (3, 20, 0)), ('off2_06', 6, 0, None), ('off2_16', 6, 1, None), ('off2_blk8', 6, 0, (1, 8, 0))]:
            run(osr, chop, seq, 300, name)
    if 'lvl2' in G:
        c.dc(1.0); time.sleep(3)
        run(6, 1, None, 40, 'v1_16')
        run(6, 0, (3, 20, 0), 90, 'v1_rare20')
        run(6, 1, None, 40, 'v1_16b')
    if 'sq2' in G:
        c.sq(5.0); time.sleep(3)
        run(6, 1, None, 60, 'sq5_16')
        run(6, 0, (3, 20, 0), 150, 'sq5_rare20')
    if 'lvl' in G:
        c.dc(1.0); time.sleep(3)
        run(6, 1, None, 40, 'v1_16')
        run(6, 0, (1, 1, 0), 60, 'v1_abab')
        run(6, 0, (3, 10, 0), 60, 'v1_rare10')
        c.dc(5.0); time.sleep(3)
        run(6, 1, None, 40, 'v5_16')
        run(6, 0, (1, 1, 0), 60, 'v5_abab')
    if 'sq' in G:
        c.sq(5.0); time.sleep(3)
        run(6, 1, None, 60, 'sq5_16')
        run(6, 0, None, 60, 'sq5_06')
        run(6, 0, (1, 1, 0), 120, 'sq5_abab')
        run(6, 0, (2, 0, 0), 120, 'sq5_prbs')
        run(6, 0, (3, 10, 0), 120, 'sq5_rare10')
        # memory: 5 Hz square for 60 s, then the output off for 60 s
        run(6, 0, (1, 1, 0), 120, 'sq5off_abab', events=[(60, c.off)])
        c.sq(5.0); time.sleep(3)
        run(6, 0, (2, 0, 0), 120, 'sq5off_prbs', events=[(60, c.off)])
finally:
    c.off(); c.close()
    m.w('DBG:AD2:CHOPSEQ 0,0,0'); m.w('DBG:AD2:DRAW 1')
    m.w('LOG:OSR %s' % st['LOG:OSR?']); m.w('LOG:CHOP %s' % ('ON' if st['LOG:CHOP?'] == '1' else 'OFF')); m.w('LOG:DEST %s' % st['LOG:DEST?'])
    m.w('LOG:SOUR %s' % st['LOG:SOUR?'])
    m.w('CONF:VOLT:DC'); time.sleep(0.5)
    m.w('VOLT:RANG:AUTO ON' if st['VOLT:RANG:AUTO?'] == '1' else 'VOLT:RANG 6'); time.sleep(1)
    print('end', m.q('CONF?'), m.q('SYST:ERR?'), m.q('SYST:ALER:ALARm?'), 'disk same:', m.q('MMEM:CAT?') == st['MMEM:CAT?'], flush=True)
