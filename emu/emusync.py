"""Input / clock markers from the Renode log: tests key their timing on them.

fw-rs's uptime (DBG:PING?) is the virtual time since its last DWT CYCCNT
write (fw-rs/src/clock.rs Clock::new in main). The boot before that write
is ~2.85 s of virtual time on a fresh disk image (pre_main's SPI-disk work:
fatdir, calbackup, vdisk::mirror_boot) and it changes whenever the boot
does, so a test must not place its windows by a guessed offset (couptest,
powtest and others did: 0.5 .. 1.8 s, while the real one had grown to
2.85 s). Two log lines give the exact relation (emu/README.md "Input
markers"):

  dwt: CYCCNT write 0x00000000 at 2.849631          (peripherals/DWT.cs)
  hy3131: input: open;dca:0.3 at 16.0000 fw 13.1504 (HY3131 SetInput)

The first is the clock marker (fw uptime = virtual - 2.849631 from then
on); the second is every input change with the firmware's uptime at that
moment (-1: before the firmware's clock started).

  m = Markers(log_path)
  m.wait_clock()             # after the USB attach: main's write is logged
  m.fw(16.0)                 # -> 13.1504 (virtual -> fw uptime)
  m.input_fw(16.0)           # the logged marker of the input at 16 s

`Timeline` turns a scenario's phases [(t, spec)] (t = script time, spec =
None: a phase that starts with an action, not an input change) into fw
uptime: a phase with an input starts at that input's marker (0 when the
input was there before the firmware's clock started); a phase without one
starts its script distance after the previous phase's start. Actions and
check windows are placed relative to those starts.

BOOT_S: the boot's virtual length on a fresh disk with run.py --fast (3.56 s on 7bd7f6f, 2.85 s
on ab7184c before mirror-bw's device-like SPI byte time; 1.90 s there at --mips 200). Scripts written in "fw uptime" terms put
their input changes BOOT_S later in virtual time (and run BOOT_S longer), so
the phases keep the lengths they were written with; no check depends on it
(a longer boot only shortens the phases, the markers keep every window on
its inputs).
"""
import os
import re
import time

RE_ZERO = re.compile(r'dwt: CYCCNT write 0x([0-9A-Fa-f]+) at ([0-9.]+)')
RE_INPUT = re.compile(r'hy3131: input: (.*) at (-?[0-9.]+) fw (-?[0-9.]+)\s*$')
HCLK = 200e6
BOOT_S = 3.56


def shifted(phases, shift=BOOT_S):
    """Virtual times of the phases' input changes for a script shifted by
    `shift` (t = 0, the connection at power-on, stays at 0)."""
    return [t + shift if t > 0 else t for t, *_ in phases]


class Markers:
    """Incremental reader of one run's Renode log (it is written as the run
    goes, a few s behind in wall time at most)."""

    def __init__(self, log):
        self.log = log
        self.pos = 0
        self.part = ''
        self.zeros = []   # virtual s at which the fw clock read 0
        self.inputs = []  # (spec, virtual s, fw uptime s or -1)

    def poll(self):
        try:
            with open(self.log, 'rb') as f:
                f.seek(self.pos)
                data = f.read()
        except OSError:
            return self
        if not data:
            return self
        self.pos += len(data)
        text = self.part + data.decode(errors='replace')
        lines = text.split('\n')
        self.part = lines.pop()
        for l in lines:
            m = RE_ZERO.search(l)
            if m:
                self.zeros.append(float(m.group(2)) - int(m.group(1), 16) / HCLK)
                continue
            m = RE_INPUT.search(l)
            if m:
                self.inputs.append((m.group(1), float(m.group(2)), float(m.group(3))))
        return self

    @property
    def zero(self):
        """Virtual time of fw uptime 0 (the last CYCCNT write), None before."""
        return self.zeros[-1] if self.zeros else None

    def wait_clock(self, timeout=60.0, after=0.5):
        """Wait (wall s) for the firmware's main-loop clock marker: the last
        CYCCNT write at or after `after` virtual s (the shim's own write at
        ~0 s is not the main loop's). The USB/IP server starts after it, so
        once a test has attached it is normally there already."""
        t_end = time.time() + timeout
        while True:
            self.poll()
            if self.zeros and self.zeros[-1] >= after:
                return self.zeros[-1]
            if time.time() >= t_end:
                return self.zero
            time.sleep(0.2)

    def fw(self, v):
        """fw uptime at virtual time v (clamped at 0: the fw clock had not
        started); None without a clock marker."""
        z = self.zero
        return None if z is None else max(0.0, v - z)

    def input_fw(self, v, spec=None):
        """The logged fw uptime of the input change at virtual v (0 when it
        came before the firmware's clock started), None if not logged."""
        for s, iv, ifw in self.inputs:
            if abs(iv - v) < 5e-4 and (spec is None or s == spec):
                if self.zeros and iv < self.zeros[-1]:
                    return 0.0  # before the clock the firmware runs on now
                return max(0.0, ifw)
        return None

    def at(self, v):
        """fw uptime of the input change at virtual v: its marker once
        logged, else predicted by the clock marker (the same value: the
        relation is exact; None without either). For a live harness."""
        self.poll()
        got = self.input_fw(v)
        return got if got is not None else self.fw(v)

    def restarts(self, after=0.5):
        """CYCCNT writes after the first main-loop one (a reset in the run)."""
        z = [x for x in self.zeros if x >= after]
        return z[1:]


EVENT = '<event>'


class Timeline:
    """Phase starts of a scenario in fw uptime, keyed on the markers.

    phases: [(t, spec)] in script order (t = virtual s of the input change;
    spec None = no input change, the phase starts with an action; spec
    EVENT = a script event the model does not log (a touch): the phase
    starts at its virtual time on the fw clock, m.fw(vt));
    run_for: the run's virtual length; vt: the virtual times of the phases'
    input changes when the script shifts them from t (default: t; e.g.
    senttest puts its inputs a fixed time later than its phase times)."""

    def __init__(self, phases, run_for, markers, vt=None):
        self.phases = [(float(t), s) for t, s in phases]
        self.vt = [float(v) for v in vt] if vt is not None else [t for t, _ in self.phases]
        self.run_for = float(run_for)
        self.m = markers

    def start(self, i, predict=False):
        """fw uptime at which phase i starts, None if not known yet.
        predict: an input not logged yet is placed by the clock marker
        (the same value its marker will have: fw = virtual - zero), so a
        live harness need not wait for the log; the checks after the run
        use the logged markers only."""
        t, spec = self.phases[i]
        v = self.vt[i]
        if spec == EVENT:
            return self.m.fw(v)
        if spec is not None:
            got = self.m.input_fw(v)
            if got is not None:
                return got
            if v == 0:
                return 0.0
            return self.m.fw(v) if predict else None
        if i == 0:
            return t
        prev = self.start(i - 1, predict)
        return None if prev is None else prev + (t - self.phases[i - 1][0])

    def at(self, ts, predict=False):
        """A script time inside the scenario -> fw uptime: its distance from
        the start of the phase it falls in (None if that start is unknown)."""
        i = max([k for k, (t, _) in enumerate(self.phases) if t <= ts] or [0])
        s = self.start(i, predict)
        return None if s is None else s + (ts - self.phases[i][0])

    def missing(self):
        """Labels of the phases whose input marker is not in the log."""
        return ['%g %s' % (t, s) for i, (t, s) in enumerate(self.phases) if s not in (None, EVENT) and self.vt[i] > 0 and self.m.input_fw(self.vt[i]) is None]

    def end(self):
        """fw uptime at the run's end (virtual run_for)."""
        return self.m.fw(self.run_for)

    def stop(self, i):
        """fw uptime at which phase i ends: the next phase's start or the run's end."""
        return self.start(i + 1) if i + 1 < len(self.phases) else self.end()

    def window(self, i):
        """Phase i's check window (fw s), back from its end: before the next
        phase's input marker 2.7 .. 0.7 s (where couptest / powtest's windows
        sat with the ~1.8 s offset they assumed), before a next phase that
        starts with an action 4.5 .. 2.5 s (as they were: such a boundary
        was always on the fw clock), before the run's end 2.7 .. 0.3 s."""
        e = self.stop(i)
        if e is None:
            return None
        if i + 1 == len(self.phases):
            return (e - 2.7, e - 0.3)
        if self.phases[i + 1][1] is None:
            return (e - 4.5, e - 2.5)
        return (e - 2.7, e - 0.7)

    def describe(self):
        return ' '.join('%g->%s' % (t, '?' if self.start(i) is None else '%.3f' % self.start(i))
                        for i, (t, _) in enumerate(self.phases))


def log_path(out_dir, name):
    """run.py's Renode log for --name `name` (no --log)."""
    return os.path.join(out_dir, name + '.log')


def check_sync(markers, timeline, name=''):
    """After the run: print the phase starts; -> failure strings (missing
    markers, a restarted fw clock)."""
    markers.poll()
    print('  sync: fw clock zero at %s s virtual; phase starts (script -> fw) %s' % (
        '?' if markers.zero is None else '%.4f' % markers.zero, timeline.describe()), flush=True)
    bad = []
    if markers.zero is None:
        bad.append('no clock marker (dwt: CYCCNT write) in %s' % markers.log)
    if timeline.missing():
        bad.append('input markers missing: %s' % ', '.join(timeline.missing()))
    if markers.restarts():
        bad.append('fw clock restarted at %s s virtual (a reset?)' % ' '.join('%.3f' % z for z in markers.restarts()))
    return bad


def summary(markers, timeline):
    """For the result JSON."""
    return {'zero': markers.zero, 'inputs': markers.inputs,
            'starts': [timeline.start(i) for i in range(len(timeline.phases))], 'end': timeline.end()}
