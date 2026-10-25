import sys; sys.argv = ['x']
sys.path.insert(0, __import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from an import *
h, t, v, pol, fl = get('sq5_16', skip=3, drop=False)
print('sq5_16: mean %.2f mV, p-p %.2f mV, sd %.2f mV' % (v.mean() * UV / 1e3, np.ptp(v) * UV / 1e3, v.std() * UV / 1e3))
h, t, v, pol, fl = get('sq5_rare20', skip=3, drop=False)
k = grid(t, 0.1); cm = clean_mask(fl, k)
for p in (0, 1):
    s = cm & (pol == p)
    print('rare clean %s: n %d mean %.2f mV p-p %.2f mV sd %.2f mV' % ('00' if p == 0 else '01', s.sum(), v[s].mean() * UV / 1e3, np.ptp(v[s]) * UV / 1e3, v[s].std() * UV / 1e3))
for W in (10, 30):
    tt, x, vos, _ = corrected(t, v, pol, fl, W)
    print('rare W%d: corrected mean %.2f mV, sd %.2f mV; VOS est mean %.2f mV sd %.2f mV (0 V: 22.23 mV)' % (W, x.mean() * UV / 1e3, x.std() * UV / 1e3, vos.mean() * UV / 1e3, vos.std() * UV / 1e3))
