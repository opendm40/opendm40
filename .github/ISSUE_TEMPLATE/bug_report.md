---
name: Bug report
about: Something in opendm40 does not work as the manual says
labels: bug
---

**Meter**

- Model (DM40A / DM40B / DM40C):
- opendm40 version (Settings > More > More > Device, or `*IDN?`):
- Installed from: a release file / an own build (commit):

**What you did**

Steps, with the function, the range and what was connected to which jack.

**What you expected**

**What the meter did**

A screenshot helps: a three-finger swipe saves one to `SHOTS/` on the USB
disk.

**Remote control (if used)**

USB or Bluetooth; the program; the commands you sent and the reply to
`SYST:ERR?`.

**If the meter hung or restarted**

The output of `python3 firmware/tools/dm40dbg.py boot`.
