"""Analysis of the CHOPSEQ runs. an.py sw|noise|lvl|sq|all"""
import sys, os
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cs import load, OUT
UV = 1.6755  # uV per code, 6 V range (device-check-2026-10-21 "AD1 OSR characterization")

GLITCH = 3000  # codes (5 mV): a clean conversion this far from its polarity's median

def glitches(v, pol, fl):
    """Isolated wild reads (pairs of +-X, device 12cbb3a1; not chopper
    related): far from the median of their polarity, not right after a write."""
    g = np.zeros(len(v), bool)
    after = np.r_[False, fl[:-1] == 1]
    for p in np.unique(pol):
        s = (pol == p) & ~after
        if s.sum():
            g |= s & (np.abs(v - np.median(v[s])) > GLITCH)
    return g

def get(name, skip=2.0, drop=True):
    h, a = load(name)
    t, v, pol, fl = a[:, 1], a[:, 2], a[:, 3].astype(int), a[:, 4].astype(int)
    k = t - t[0] >= skip
    t, v, pol, fl = t[k], v[k], pol[k], fl[k]
    g = glitches(v, pol, fl)
    if drop and g.any():
        print('  [%s: %d glitch records dropped at %s s]' % (name, g.sum(), ', '.join('%.1f' % x for x in (t[g] - t[0])[:6])))
        # a dropped read is a hole in the grid: the next one counts as after a gap
        t, v, pol, fl = t[~g], v[~g], pol[~g], fl[~g]
    return h, t, v, pol, fl

def timing(t, P=0.1):
    dt = np.diff(t)
    return 'dt median %.2f ms, min %.2f, max %.2f, >1.5P %d of %d' % (np.median(dt) * 1e3, dt.min() * 1e3, dt.max() * 1e3, (dt > 1.5 * P).sum(), len(dt))

def levels(v, pol, fl, settle=3):
    """Median code per polarity over records >= settle after the last flip."""
    since = np.zeros(len(v), int); n = 99
    for i in range(len(v)):
        since[i] = n
        n = 0 if fl[i] else n + 1
    since = np.roll(since, 0)
    good = since >= settle
    return {p: np.median(v[good & (pol == p)]) for p in (0, 1)}, since

def sw(name):
    h, t, v, pol, fl = get(name)
    L, since = levels(v, pol, fl)
    span = L[1] - L[0]
    print('%s  seq %s  cs %s' % (name, h['seq'], h['cs']))
    print('  %s' % timing(t))
    print('  levels: 00 %.0f, 01 %.0f codes; 01 - 00 = %.0f codes = %.2f mV' % (L[0], L[1], span, span * UV / 1e3))
    idx = np.where(fl == 1)[0]
    for j in (1, 2, 3, 4):
        f, d = [], []
        for i in idx:
            if i + j < len(v) and all(fl[i + 1:i + j] == 0):
                old = L[pol[i]]; new = L[1 - pol[i]]
                f.append((v[i + j] - old) / (new - old)); d.append(t[i + j] - t[i + j - 1])
        if f:
            f = np.array(f); d = np.array(d)
            print('  read %d after the write: fraction of the new level %.4f (sd %.4f, min %.4f, max %.4f), dt %.2f ms (min %.2f max %.2f), n %d' % (
                j, f.mean(), f.std(), f.min(), f.max(), d.mean() * 1e3, d.min() * 1e3, d.max() * 1e3, len(f)))
    return L

def allan(x, dt, taus):
    out = []
    for tau in taus:
        m = int(round(tau / dt))
        if m < 1 or len(x) < 3 * m:
            out.append(np.nan); continue
        n = len(x) // m
        y = x[:n * m].reshape(n, m).mean(1)
        out.append(np.sqrt(0.5 * np.mean(np.diff(y) ** 2)))
    return np.array(out)

if __name__ == '__main__':
    what = sys.argv[1:] or ['sw']
    if 'sw' in what:
        for n in ['off_blk8_d0', 'off_blk8_d25', 'off_blk8_d50', 'off_abab_short']:
            if os.path.exists(os.path.join(OUT, n + '.csv')):
                sw(n)

TAUS = [0.2, 0.4, 1, 2, 4, 10, 20, 40]

def grid(t, P):
    return np.round((t - t[0]) / P).astype(int)

def clean_mask(fl, k):
    """Records not mixed by a polarity switch: no write after the previous read,
    and the previous conversion was not lost (a lost read skips its write)."""
    m = np.ones(len(fl), bool)
    m[0] = False
    m[1:] = (fl[:-1] == 0) & (np.diff(k) == 1)
    return m

def corrected(t, v, pol, fl, W, P=0.1):
    """Clean conversions corrected by VOS estimated from the clean 00 / 01
    conversions in the causal window (t - W, t]: (mean 00 - mean 01) / 2."""
    k = grid(t, P); cm = clean_mask(fl, k)
    tc, vc, pc = t[cm], v[cm], pol[cm]
    out_t, out_v, vos = [], [], []
    ja = jb = 0
    for i in range(len(tc)):
        lo = tc[i] - W
        sel = (tc > lo) & (tc <= tc[i])
        a = vc[sel & (pc == 0)]; b = vc[sel & (pc == 1)]
        if len(a) < 3 or len(b) < 2:
            continue
        e = (a.mean() - b.mean()) / 2
        out_t.append(tc[i]); vos.append(e)
        out_v.append(vc[i] - e if pc[i] == 0 else vc[i] + e)
    return np.array(out_t), np.array(out_v), np.array(vos), cm.mean()

def adev_line(label, x, dt):
    a = allan(x, dt, TAUS) * UV
    return '%-28s ' % label + ' '.join('%6.1f' % y if not np.isnan(y) else '     -' for y in a)

def noise():
    print('Allan deviation, uV, tau (s): ' + ' '.join('%6s' % x for x in TAUS))
    for n, P in [('off2_06', 0.1), ('off2_16', 0.2)]:
        h, t, v, pol, fl = get(n, skip=5)
        print(adev_line('%s sd %.1f uV' % (n, v.std() * UV), v - v.mean(), P))
        x = v - v.mean(); print('   drift over the run: first/last 30 s mean %.1f / %.1f uV' % (x[t - t[0] < 30].mean() * UV, x[t - t[0] > t[-1] - t[0] - 30].mean() * UV))
    for n in ['off2_blk8', 'off2_rare20']:
        h, t, v, pol, fl = get(n, skip=5)
        k = grid(t, 0.1); cm = clean_mask(fl, k)
        print('%s: clean %.1f %% of conversions (%.2f /s), 00 %d, 01 %d' % (n, cm.mean() * 100, cm.sum() / (t[-1] - t[0]), (cm & (pol == 0)).sum(), (cm & (pol == 1)).sum()))
        for W in (10, 30, 60):
            tt, x, vos, _ = corrected(t, v, pol, fl, W)
            # Allan on the clean 00 readings only, resampled to the 0.1 s grid by holding (approx.)
            kk = grid(tt, 0.1); y = np.interp(np.arange(kk[-1] + 1), kk, x)
            print(adev_line('  W=%d s: sd %.1f uV' % (W, x.std() * UV), y - y.mean(), 0.1), ' VOS est sd %.1f uV' % (vos.std() * UV))
        a = v[cm & (pol == 0)]; b = v[cm & (pol == 1)]
        print('   VOS (00 - 01)/2 = %.1f uV; residual offset of the corrected readings %.1f uV' % ((a.mean() - b.mean()) / 2 * UV, x.mean() * UV))

if __name__ == '__main__' and 'noise' in sys.argv[1:]:
    noise()
