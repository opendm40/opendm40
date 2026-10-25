# Changelog

User-facing changes of opendm40 per release. The release workflow
([.github/workflows/release.yml](.github/workflows/release.yml)) uses the
section named after the tag as the release notes; how to cut a release:
[firmware/README.md, Releasing](firmware/README.md#releasing).

## Unreleased

### New

- Multimeter: a 60 mV range (on the range page, and first in the mV AUTO).
- Ohm and diode: the test current is shown next to the reading, from
  currents measured on the meter (e.g. 0.756 mA for the diode test).
- "Overload" is shown next to `0L`.
- Scope touch: the trigger-level marker sits on the graph's right edge and
  moves the trigger point; a press near a marker grabs the nearest one; a
  drag stays on one item and one axis; pinch modes with one step per 1.25x.
- Power meter: the wiring dialog draws the meter's jacks (VΩ / COM / A) and
  the current path through the meter.
- SCPI: IEEE 488.2 suffix multipliers, an error queue with codes
  (`SYSTem:ERRor?`), no reply to a query in a message with a bad set value;
  `LOG:FORMat ROWS|PACKed|DLG` for compact `LOG:DATA?` replies
  ([SCPI reference](docs/manual/scpi-reference.md)).
- The settings are kept in `_SETTING.BIN` on the internal disk.
- The version comes from one place, `firmware/Cargo.toml`: `*IDN?`,
  Settings > Device, the USB disk texts, the CSV headers and the install
  file `dm40_opendm40_v<version>.bin` show the same string.

### Changed

- The firmware is named opendm40 everywhere (device strings, image name).
- The "Leads in A jack? Move to V" reminder is removed.
- AUTO+ chosen on a mV range leaves mV at once for the 6 V range; AC mV steps
  up on a clipped input.
- Auto power-off: USB disk reads and writes count as host activity; the
  screen dims on input only.
- Wave view: the timebase starts at 1 ms/div. Scope: CH1 Type is removed.
- Wave view: V/div steps with one full-scale step per range (± the range on
  the 8 divisions: 1.5 V/div on 6 V, 15 V/div on 60 V, 2.5 A/div on 10 A)
  and 1-2-5 steps between that only rescale the screen; the range changes
  only where a step needs it. The pinch and AUTO work in the Wave view; 60 mV
  is in its list.

### Fixed

- Bluetooth: the module's start-up banner and its own text no longer reach
  the SCPI parser as commands.
- No false A-JACK warning from a voltage on V-COM in ohm, continuity and AC
  mV, nor from a source on V-COM in resistance.
- RES AUTO: no endless range hunt; `FETCh?` answers `+9.91E37` right after an
  autorange step.
- SCPI numbers: `nr3` and powers of ten print exact values
  (`VOLT:RES?` gives `+9.999999E-06`).
- `MMEM`: a directory prefix must be the file's own directory.
- First readings after a function change: every entry is checked; AC A too.
- Logger: one 76.8 kS/s nominal rate; no hole in the panel at the 76.8 kS/s
  start.
- The 6 M ohm test-current label is cleared on 60 M.

### Docs

- A user manual in one file with a picture in every section
  ([docs/manual/README.md](docs/manual/README.md)); the root README is the
  front page with install, backup and recovery ([README.md](README.md)).
- Measured ohm test currents and the HY3131 node behaviour
  ([hw/README.md](hw/README.md)); copying `DEBUG/SPIFLASH.BIN` on Linux.
- Finished logs and investigations moved to [docs/history/](docs/history).

### Tools

- [`firmware/tools/reload.sh`](firmware/tools/reload.sh): the first upload no
  longer fails on an earlier client's error.
- [`firmware/tools/dm40scpi.py`](firmware/tools/dm40scpi.py) `log --format
  rows|packed|dlg`; [`firmware/tools/srcmap.py`](firmware/tools/srcmap.py)
  sweeps the `DBG:SRC` taps into a CSV;
  [`firmware/tools/linkcheck.py`](firmware/tools/linkcheck.py) checks the
  Markdown links.
- CI and releases on GitHub Actions ([.github/workflows/](.github/workflows)).

## v0.1-pre1 (2026-10-24)

First pre-release ([release page](https://github.com/wirenboard/opendm40/releases/tag/v0.1-pre1)),
published by hand. Its `*IDN?` and Device page show `0.0.1` (the crate
version then). The version scheme of
[Releasing](firmware/README.md#releasing) starts with the next release.
