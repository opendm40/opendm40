# HY3131 model for the DM40 emulator

`emu/hy3131/HY3131.cs` is the detailed model of the Hycon HY3131 DMM front end
(the file installed as `emu/peripherals/HY3131.cs` is a copy of it; keep them
identical). What it is based on: `RESEARCH.md` (datasheet, application note,
Hycon tool data, open-source drivers) and the stock firmware analysis in
`firmware/re/analysis/hy3131_v2_6.h` / `dmm_v2_6.h`.

```
emu/hy3131/HY3131.cs      the model: Hy3131Core (chip, no Renode types) + HY3131 (Renode peripheral)
emu/hy3131/tests/         host unit tests (dotnet 8): tests/run.sh
emu/hy3131/modetest.py    stock + fw-rs through every DMM mode in the emulator, results below
emu/hy3131/runemu.py      emu/run.py with this directory's HY3131.cs compiled in (development)
emu/hy3131/RESEARCH.md    sources, facts, open points
emu/hy3131/ref/           transcribed register map and mux tables
```

## What is modelled

Chip (datasheet DS-HY3131-V10):

* Register file R00..R3F with the datasheet semantics: data registers read
  only; INTF cleared when read, writing 0s clears bits, 1s cannot be written;
  INTE and the IRQ on DO while CS is high; CTSTA (ACPO in bit 6, PCNTI, CTBOV
  cleared by reading); CTA<23:8> writable only while ENCTR = 0, CTA<7:0> held
  at 0; ENCTR = 0 clears CTB / CTC / CTBOV; ENAD1 / ENAD2 / ENLPF / ENRMS = 0
  clear their data; ENPKH = 0 presets PKHMAX / PKHMIN; R37 = 0x60 resets the
  chip; reserved "0" bits read 0, "-" bits are kept; auto-increment reads and
  writes, the read period clock, DO changing on the falling edge (`HY3131`).
* Conversion timing: AD1 period = `OSR1 * 30 / 4.9152 MHz`, doubled with
  AD1CHOP = 1x (R22 0x13 / 0x15 / 0x17 -> 25 / 100 / 400 ms; RESEARCH.md 3);
  AD2 / AD3 at `OSR2 * 2 (or 4) / SYSCLK` (13.02 us); LPF and RMS results every
  `AD2 period * OSR4 * 0.5` (the measured 106.7 ms for LPFBW = 111); AD1F,
  AD2F, LPFF, RMSF, CTF set at those times and accumulated until INTF is read.
  Conversions are evaluated lazily when the firmware touches the bus (no
  Renode timers): `Hy3131Core.Update(now)`.
* AD1 transfer: `code = IG * Vin / (RG * VR) * 2^23 + DCOS / RG * 2^23`, IG /
  RG / DCOS from R25 / R23 / R22, clipped to +-8388607; AD2 / LPF 19-bit, RMS
  38-bit = sum(X^2)/N of the AC part (HPF) in AD2 codes, `SDO23` / `SVXI`
  selection; peak hold of AD2 / AD1<23:5> / LPF.
* Frequency counter: gate opens on the first input edge after ENCTR = 1, CTA
  counts SYSCLK from the preset, after the overflow the count stops on the next
  edge: CTB whole cycles, CTC high time, CTF; no edges -> CTA keeps running,
  CTB = 0, no CTF; CTBOV on CTB overflow. Source ACPO (comparator) or the CNT
  pin (ENCNTI).
* Comparator: VRHCMP / VRLCMP from the R21 mux tables and the reference divider
  (VDSC<n> = K1 * VDDA, AGNDP/N<n> = AGND +- K2 * REFO), latch -> ACPO, CMPO on
  the CMP pin when ENPCMPO = 1 (`CmpPin`: the continuity beeper).
* Temperature sensor on SAD1I = 10 / 11 (differential pair, sum removes the
  offset), with the stock's constants (RESEARCH.md 7).
* Noise: Gaussian, 25 AD1 counts at OSR 32768 scaled by 1/sqrt(OSR) (acdc doc
  6c), RMS floor 30 counts; `noise:0` turns it off (deterministic seed).
* Empirical, optional: the AD2/AD3 enable-order beat of
  `fw-rs/docs/acdc-stock-vs-fwrs.md` 10 (`beat:1`; off by default: the stock's
  emulated SPI skew is not the real one).

Board (DM40, from the stock rows and formulas): the input terminals are a
`Hy3131Input` (V/COM waveform: DC + AC RMS at a frequency, sine / square /
triangle, duty; A terminal current; R / C / diode on the ohm path; thermocouple
and die temperature). Each stock configuration row (hy3131_v2_6.h, 42 rows) is
recognised by a masked compare of R20..R35 (R21 / R22 ignored, enable bits,
SFUVR, SOP1P / ENOP1 and SAD1I masked, so fw-rs's enable-order variant and the
run-time R22 / R32 / R33 writes still match) and gives the analog path:

| role (stock mode) | converter and path | chain used by the model |
|---|---|---|
| DC V 1 (r0..r3) | AD1, ref 1.25 V | `code = V / (k[r] * 1.25/0.9 / 2^23)`, k = 10 .. 10000; negative-side gains and the r0 correction of the stock inverted |
| DC mV 3, DC A 7..9 | AD1 through OP1 (x10 / x1) | stock constants: mv_div, shunt * opgain * 0.9 |
| AC V 0 | AD2 + AD3 on PB4 (AC coupled, tau 0.3 s), RMS + LPF | `Vnode = V / (k[r] * AD2IG)`; LPF = remaining DC step on the coupling capacitor (charges to the OP1O level, -0.6 V while OP1 is off: the stock's entry transient and the AC+DC behaviour), RMS = AC + 1.5 x that step |
| AC mV 2, AC A 4..6 | AD2 through OP1, DC coupled | LPF sees the full DC |
| ohm 10 (r0..r6), continuity 11 | AD1 ratiometric to Rref[r] (0.1 k .. 63 M, 1 k) | `code = 0.9 * Rx / Rref * 2^23` (negative in continuity: reference order), clipped (open = OL); comparator window 41.7 mV / 0.625 mA = 67 ohm -> CMP pin |
| diode 12 | AD1 on the /10 divider (SDIO) | `code = Vf / 10 / 1.389 * 2^23`, open = 3.3 V (OL) |
| capacitance 13 r0..r4 | RC oscillator into the counter | `f = 0.72135 / (R[r] * (C + stray))`, R = 1020 k .. 1.5 k |
| capacitance 13 r5..r7 | constant current ramp on AD1 | `dV/dt = I[r] / C` (4.06 / 41 / 400 uA at the AD1 node) between the R21 thresholds, ACPO toggles, discharge row -> 0 V |
| frequency 14 | comparator on the input (+-20.8 mV) or CNT pin | counter; AD2 RMS of the level |
| temperature 15 | AD1: sensor phases (R33), thermocouple on the mV x10 path | type K EMF (NIST ITS-90) of (hot - cold junction) |

The real PA / PB wiring of the DM40 is not known (hy3131_v2_6.h); the chain
constants are the stock's nominal scale factors, i.e. the design intent of the
AFE + board, applied per recognised row. A configuration that matches no stock
row reads 0 (role `None`); `sysbus.hy3131 Role` shows the match.

### Calibration consistency

The meter multiplies every raw reading by the gains of its calibration record
(flash 0x080C0168; e.g. DC V r0 gain 1.012 on the dumped unit). The model
therefore produces the raw codes of *this* board: at the first SPI transfer it
reads the record from the emulated flash and inverts the stock's correction
(`raw = (x + offset) / (gain * scale)` per path), so the stock and fw-rs
display the scripted input. `SetCalConsistent false` (or `cal:0` in the input
spec) gives the ideal chain instead (the stock then shows 1.234 V as 1.2488 V).

## C# API (Renode monitor: `sysbus.hy3131 <method> args`)

| call | meaning |
|---|---|
| `SetInput "spec"` | the inputs; several specs separated by `;` (below) |
| `SetVin V` | DC voltage on V / COM (old scripts) |
| `SetAd1 code` / `ClearAd1` | force a raw AD1 code |
| `SetAd2 code` | force a raw AD2 code |
| `SetCalConsistent true/false` | apply the meter's calibration deviations (default true) |
| `SetNoise scale` | converter noise scale, 0 = off |
| `SetCntPin true/false` | level of the CNT pin (when the firmware selects it) |
| `BrownOut` | chip reset with BORF set |
| `Status`, `Role`, `Mode`, `Range`, `Intf`, `Ad1`, `CmpPin`, `Transfers` | state for scripts and tests |

Input spec keys (`emu/run.py --dmm SPEC[@T]` or `--script` lines
`T monitor sysbus.hy3131 SetInput "SPEC"`):

| spec | input |
|---|---|
| `dcv:1.234` | 1.234 V DC on V / COM (`noise=` V rms white noise) |
| `acv:1.0@50` | 1.0 V rms sine at 50 Hz; options `dc=0.5`, `shape=square|triangle`, `duty=0.3` |
| `hz:1000,duty=0.25,amp=1.0` | square wave for the frequency / duty functions |
| `dca:0.5`, `aca:0.1@60` | current into the A terminal (A rms for AC; `dc=`, `shape=`) |
| `ohm:4700`, `short`, `open` | resistance on the ohm path (also continuity) |
| `diode:0.65` | forward voltage; `open` = no diode |
| `cap:100e-9` | capacitance (F); `cap:0` = open |
| `temp:100,cj=25` | thermocouple hot junction and chip (cold junction) temperature, C |
| `tcmv:4.096`, `tcopen`, `chip:25` | thermocouple mV directly, open thermocouple, die temperature |
| `noise:0`, `cal:0`, `beat:1` | model options |

## Tests

`emu/hy3131/tests/run.sh` (dotnet 8; `DOTNET=` to point at an SDK) compiles
the real `HY3131.cs` with a 60-line Renode shim and drives it through the SPI
pins like the firmware: 133 checks of the register semantics, the frame,
conversion timing, DC / AC / A / ohm / continuity / diode / capacitance /
frequency / temperature physics, calibration consistency, the AC coupling
transient, the AC+DC cadence and the (optional) enable-order beat.

`emu/hy3131/modetest.py` runs the stock firmware and fw-rs in the emulator:
the input is scripted, the stock is switched with Bluetooth command 06 and
polled with command 09 (the values the LCD shows, `bt_meas_v2_6.h`), fw-rs
with `CONF:<f>` / `READ?` over the same link. Results (last reported value
after settling; stock Bluetooth is enabled by patching settings byte 0x115 in
the emulated flash with a recomputed CRC):

RESULTS_TABLE

## Open points

See RESEARCH.md 8: LPF / RMS output rate (modelled with the measured value),
the empirical AC-node and enable-order parts, the temperature sensor constants
(stock vs datasheet), the DM40 CNT pin and netlist.
