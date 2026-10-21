#!/usr/bin/env python3
"""emu/run.py with the HY3131 model under development swapped in.

Runs emu/run.py unchanged (same arguments) but compiles emu/hy3131/HY3131.cs
instead of emu/peripherals/HY3131.cs when HY3131_MODEL=dev (the default here;
HY3131_MODEL=tree uses the working tree's peripheral). The merged compile
unit goes to emu/out/dm40-models-hy3131.cs so the regular runs are untouched.
"""
import glob
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
RUN = os.path.join(EMU, 'run.py')
DEV = os.environ.get('HY3131_MODEL', 'dev') == 'dev'


def merge_models():
    usings, bodies = [], []
    files = sorted(glob.glob(os.path.join(EMU, 'peripherals', '*.cs')))
    if DEV:
        dev = {'HY3131.cs', 'DM40Frontend.cs'}
        files = [os.path.join(HERE, os.path.basename(f)) if os.path.basename(f) in dev else f for f in files]
        for name in dev:  # not yet installed in peripherals/
            if not any(os.path.basename(f) == name for f in files):
                files.append(os.path.join(HERE, name))
    for f in files:
        body = []
        with open(f, 'rb') as fh:
            for l in fh.read().decode().splitlines():
                if re.match(r'^using [A-Za-z0-9_.]+;\s*$', l):
                    if l not in usings:
                        usings.append(l)
                else:
                    body.append(l)
        bodies.append('// ---- %s ----\n%s' % (os.path.relpath(f, EMU), '\n'.join(body)))
    out = os.path.join(EMU, 'out', 'dm40-models-hy3131.cs' if DEV else 'dm40-models.cs')
    text = '\n'.join(usings) + '\n' + '\n'.join(bodies) + '\n'
    os.makedirs(os.path.dirname(out), exist_ok=True)
    if not os.path.exists(out) or open(out).read() != text:
        with open(out, 'w') as f:
            f.write(text)
    return out


def main():
    src = open(RUN).read()
    ns = {'__name__': 'dm40_run', '__file__': RUN}
    exec(compile(src, RUN, 'exec'), ns)
    ns['merge_models'] = merge_models
    r = ns['main']()
    return r if isinstance(r, int) else 0  # --dry returns the .resc path


if __name__ == '__main__':
    sys.exit(main())
