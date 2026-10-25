"""AD1 manual-chop experiment helpers (DBG:AD2:CHOPSEQ, branch exp/ad1-chopseq).
DC V 6 V manual, LOG:SOURce AD1, LOG:DEST HOST, logger panel off (DBG:AD2:DRAW 0)."""
import json, os, sys, time
TOOLS = '~/work/board/alientek-dm40/firmware/tools'
sys.path.insert(0, TOOLS)
import dm40dbg, dm40scpi
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')
VMAX = 24.0

class M:
    def __init__(self):
        self.l = dm40dbg.Link()
    def q(self, c):
        return self.l.query(c).decode(errors='replace').strip()
    def w(self, c):
        self.l.write(c)

class Cal:
    def __init__(self):
        import mr9270
        self.c = mr9270.MR9270('/dev/ttyACM1').connect()
    def dc(self, v):
        assert 0 <= v <= VMAX
        self.c.set_output('voltage', v, enable=True)
    def off(self):
        self.c.disable()
    def sq(self, hz):
        self.c.set_output('frequency', hz, enable=True)
    def close(self):
        self.c.close()

def untag(raw):
    """(code, pol, flip, tagged) of a CHOPSEQ record raw."""
    u = raw & 0xFFFFFFFF
    tagged = bool(u >> 30 & 1)
    if not tagged:
        return raw, None, None, False
    v = u & 0xFFFFFF
    if v & 0x800000:
        v -= 1 << 24
    return v, u >> 28 & 1, u >> 29 & 1, True

def stream(m, osr, chop, seq, dur, name, meta=None, events=()):
    """seq = (mode, param, delay_ms) or None. CSV seq,t_dev,code,pol,flip,t_host."""
    path = os.path.join(OUT, name + '.csv')
    if os.path.exists(path):
        print('skip (exists)', path); return path
    m.w('*CLS'); m.w('LOG:SOUR AD1'); m.w('LOG:CHOP %s' % ('ON' if chop else 'OFF')); m.w('LOG:OSR %d' % osr)
    mode, param, dl = seq or (0, 0, 0)
    m.w('DBG:AD2:CHOPSEQ %d,%d,%d' % (mode, param, dl))
    e = m.q('SYST:ERR?'); assert e.startswith('0'), e
    ev = sorted(events, key=lambda x: x[0]); evlog = []
    t0 = time.monotonic(); m.w('LOG:STAT ON'); recs = []
    try:
        pre = m.q('LOG:PRE?')
        while time.monotonic() - t0 < dur:
            while ev and time.monotonic() - t0 >= ev[0][0]:
                th = time.monotonic() - t0; ev[0][1](); evlog.append((ev[0][0], th, time.monotonic() - t0)); ev.pop(0)
                meta = dict(meta or {}, events=evlog)
            raw = m.l.query('LOG:DATA?'); th = time.monotonic() - t0
            for r in dm40scpi.log_records(raw):
                recs.append((r[0], r[1], r[2], th))
            time.sleep(0.05)
        cs = m.q('DBG:AD2:CHOPSEQ?')
    finally:
        m.w('LOG:STAT OFF')
    cnt = m.q('LOG:COUN?'); isr = m.q('DBG:ISR?'); err = m.q('SYST:ERR?')
    for r in dm40scpi.log_records(m.l.query('LOG:DATA?')):
        recs.append((r[0], r[1], r[2], time.monotonic() - t0))
    with open(path, 'w') as f:
        f.write('# osr,%d\n# chop,%d\n# seq,%s\n# pre,%s\n# count,%s\n# isr,%s\n# cs,%s\n# err,%s\n# meta,%s\n' % (
            osr, chop, json.dumps(seq), pre, cnt, isr, cs, err, json.dumps(meta or {})))
        f.write('seq,t_dev,code,pol,flip,t_host\n')
        for s, ts, raw, th in recs:
            v, p, fl, tg = untag(raw)
            f.write('%d,%.6f,%d,%s,%s,%.3f\n' % (s, ts / 1e6, v, '' if p is None else p, '' if fl is None else fl, th))
    print(name, 'n', len(recs), 'cs', cs, 'count', cnt, 'err', err, flush=True)
    return path

def load(name):
    import numpy as np
    hdr = {}; rows = []
    for l in open(os.path.join(OUT, name + '.csv')):
        if l.startswith('#'):
            k, v = l[2:].rstrip('\n').split(',', 1); hdr[k] = v
        elif l[0].isdigit():
            x = l.rstrip('\n').split(',')
            rows.append([float(x[0]), float(x[1]), float(x[2]), float(x[3] or -1), float(x[4] or -1), float(x[5])])
    return hdr, np.array(rows)
