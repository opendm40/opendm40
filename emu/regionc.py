"""Region C hooks for the emulator tests (fw-rs/link-native.x: .cold code in
0x08080000..0x080A0000, flash that is not zero-wait; a fetch from it while
the AD2 stream ISR owns the bus stalls the ISR's entry ~20 us on the device).

A Renode hook on every function symbol of region C writes the function's
address to a file on its first call while hy3131::HANDED is set (the ISR
owns the HY3131 bus) and removes itself (a Python hook per call of a busy
function slows the emulator to a crawl). Used by hy3131/wavetest.py and
fastlogtest.py:

    rc = regionc.Hooks(app_elf, out_dir, tag)   # rc.script: run.py --script
    rc.reset()                                  # before a scenario
    ...
    rc.calls()                                  # {function: 1} hit since
    rc.calls(every=True)                        # ... over the whole run

`exclude`: name parts of functions left without a hook. Every call of a
hooked function runs a Python hook, handed or not: code that is hot outside
the stream (the fast logger's Prep) slows the emulator to a crawl.

A function is reported once per run (its hook is gone after the first hit):
a scenario's check sees only functions no earlier step called while the
ISR owned the bus, so the tests also check the whole run at the end.
"""
import glob
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
C_LO, C_HI = 0x08080000, 0x080A0000


class Hooks:
    def __init__(self, app, out_dir, tag, exclude=()):
        self.syms = {}
        self.hits = os.path.join(out_dir, tag + '.chits')
        self.script = None
        open(self.hits, 'w').close()
        nm = sorted(glob.glob(os.path.join(ROOT, 'toolchain/rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm')))
        if not nm:
            return
        r = subprocess.run([nm[0], '-C', app], capture_output=True, text=True)
        handed = None
        for l in r.stdout.splitlines():
            p = l.split(None, 2)
            if len(p) < 3:
                continue
            a = int(p[0], 16)
            if p[1] in 'tT' and C_LO <= a < C_HI and not any(x in p[2] for x in exclude):
                self.syms[a & ~1] = p[2]
            elif p[2].startswith('dm40_fw::hy3131::HANDED'):
                handed = a
        if handed is None or not self.syms:
            return
        code = ("(machine.SystemBus.ReadByte(0x%x)!=0) and (open('%s','a').write('%%x 1'%%pc+chr(10)), self.RemoveHooksAt(pc))"
                % (handed, self.hits))
        self.script = os.path.join(out_dir, tag + '.cscript')
        with open(self.script, 'w') as f:
            for a in sorted(self.syms):
                f.write('1 monitor sysbus.cpu AddHook 0x%08x "%s"\n' % (a, code))

    def lines(self):
        """The event-script lines (run.py --script), '' without hooks."""
        return open(self.script).read() if self.script else ''

    mark = 0

    def reset(self):
        """Start a scenario: `calls` reports the hits after this point."""
        self.mark = os.path.getsize(self.hits) if os.path.exists(self.hits) else 0

    def calls(self, every=False):
        """{region C function: 1} called while the ISR owned the bus since
        the last reset (every: since the start of the run)."""
        got = {}
        if os.path.exists(self.hits):
            with open(self.hits, 'rb') as f:
                f.seek(0 if every else self.mark)
                for l in f.read().decode(errors='replace').splitlines():
                    p = l.strip('\0').split()
                    if len(p) == 2:
                        n = self.syms.get(int(p[0], 16), p[0])
                        got[n] = max(got.get(n, 0), int(p[1]))
        return got

    def cleanup(self):
        for p in (self.script, self.hits):
            if p and os.path.exists(p):
                os.remove(p)
