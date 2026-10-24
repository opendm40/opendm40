#!/usr/bin/env python3
"""Fetch and summarise the opendm40 touch log (DBG:TOUCH?, src/touch.rs log_text).

    touch-log.py                 query the device (USBTMC), print the summary
    touch-log.py --clear         clear the log on the device (DBG:TOUCH 0)
    touch-log.py --file F        summarise a saved reply instead
    touch-log.py --raw           also print the raw lines
    touch-log.py --target x0,y0,x1,y1   the rect the owner was told to tap:
                                 every touch's down point is checked against it

One block per touch (first pressed frame .. the gesture decision):
  frames      pressed frames read, their mean period, edge/level/bad counts
  down        ms from the first pressed frame to the debounced touch-down
              ("LOST" = pressed frames but no touch-down: the tap vanished)
  jitter      largest jump between consecutive raw points (px), and the
              farthest raw point from the down point
  release     ms from the first release frame to the debounced release
  result      the gesture decision: T tap, N nothing (1xx moved xx px,
              2 after a long press, 3 diagonal), L long press, U captured
              release, < > ^ v swipes, D down ...
"""
import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def fetch():
    import dm40dbg
    ln = dm40dbg.Link()
    ln.write('DBG:TOUCH?')
    d = ln.read()
    head = d.split(b'\n', 1)[0].split(b',')
    want = int(head[3]) + 1 if len(head) >= 4 else 1
    while d.count(b'\n') < want:
        d += ln.read()
    return d.decode()


def parse(text):
    head, rows = None, []
    for line in text.splitlines():
        p = line.strip().split(',')
        if not p or not p[0]:
            continue
        if p[0] == 'TOUCH':
            head = p
            continue
        rows.append(p)
    return head, rows


def summarise(rows, target):
    touches, cur = [], None
    for p in rows:
        t, k = int(p[0]), p[1]
        if k == 'F' and p[2] != 'x' and not p[3].startswith('k') and int(p[3]) > 0:
            if cur is None or cur.get('done'):
                cur = dict(frames=[], down=None, rel_frame=None, rel=None, result=[], src={})
                touches.append(cur)
            cur['frames'].append((t, int(p[4]), int(p[5]), p[2]))
            cur['rel_frame'] = None
        elif k == 'F' and cur is not None and not cur.get('done'):
            if p[2] == 'x':
                cur['src']['bad'] = cur['src'].get('bad', 0) + 1
            elif cur['rel_frame'] is None:
                cur['rel_frame'] = t
        elif k == 'E' and cur is not None:
            if int(p[2]) > 0 and cur['down'] is None:
                cur['down'] = (t, int(p[4]), int(p[5]))
            elif int(p[2]) == 0 and int(p[3]) == 0:
                cur['rel'] = t
        elif k == 'G' and cur is not None and p[2] not in 'Mc':
            cur['result'].append(p[2] + (p[3] if p[2] == 'N' else ''))
            if p[2] in 'TNU<>^vW':
                cur['done'] = True
        elif k == 'G' and cur is not None and p[2] == 'c':
            cur['result'].append('c')
    for i, c in enumerate(touches):
        fr = c['frames']
        t0 = fr[0][0]
        per = (fr[-1][0] - t0) / (len(fr) - 1) if len(fr) > 1 else 0
        src = {}
        for f in fr:
            src[f[3]] = src.get(f[3], 0) + 1
        src.update(c['src'])
        jump = max((max(abs(a[1] - b[1]), abs(a[2] - b[2])) for a, b in zip(fr, fr[1:])), default=0)
        print('touch %d at %d ms: %d pressed frames, period %.1f ms, %s' % (
            i + 1, t0, len(fr), per, ' '.join('%s%d' % kv for kv in sorted(src.items()))))
        print('  raw first %d,%d  last %d,%d  max jump %d px' % (fr[0][1], fr[0][2], fr[-1][1], fr[-1][2], jump))
        if c['down']:
            dt, dx, dy = c['down']
            far = max(max(abs(f[1] - dx), abs(f[2] - dy)) for f in fr)
            ok = ''
            if target:
                x0, y0, x1, y1 = target
                ok = '  IN target' if x0 <= dx <= x1 and y0 <= dy <= y1 else \
                    '  MISS target by %d,%d px' % (max(x0 - dx, 0, dx - x1), max(y0 - dy, 0, dy - y1))
            print('  down after %d ms at %d,%d; farthest raw point %d px%s' % (dt - t0, dx, dy, far, ok))
        else:
            print('  down: LOST (no debounced touch-down)')
        if c['rel'] is not None and c['rel_frame'] is not None:
            print('  release confirmed %d ms after the release frame' % (c['rel'] - c['rel_frame']))
        print('  result: %s' % (' '.join(c['result']) or '-'))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--file')
    ap.add_argument('--clear', action='store_true')
    ap.add_argument('--raw', action='store_true')
    ap.add_argument('--target')
    a = ap.parse_args()
    if a.clear:
        import dm40dbg
        dm40dbg.Link().write('DBG:TOUCH 0')
        return
    text = open(a.file).read() if a.file else fetch()
    head, rows = parse(text)
    if a.raw:
        print(text)
    if head:
        print('log: now %s ms, %s entries since clear, %s shown' % (head[1], head[2], head[3]))
    target = tuple(int(v) for v in a.target.split(',')) if a.target else None
    summarise(rows, target)


if __name__ == '__main__':
    main()
