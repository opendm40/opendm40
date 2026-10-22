# HY3131 model for the DM40 emulator

`emu/hy3131/HY3131.cs` is the detailed model of the Hycon HY3131 DMM front end
(the file installed as `emu/peripherals/HY3131.cs` is a copy of it; keep them
identical). What it is based on: `RESEARCH.md` (datasheet, application note,
Hycon tool data, open-source drivers) and the stock firmware analysis in
`firmware/re/analysis/hy3131_v2_6.h` / `dmm_v2_6.h`.

```
emu/hy3131/HY3131.cs      the model: Hy3131Core (chip, no Renode types) + HY3131 (Renode peripheral)
emu/hy3131/DM40Frontend.cs the DM40 front end: jacks, relays, shunts, divider / ohm network (installed as emu/peripherals/DM40Frontend.cs)
emu/hy3131/tests/         host unit tests (dotnet 8): tests/run.sh
emu/hy3131/modetest.py    stock + fw-rs through every DMM mode in the emulator, results below
emu/hy3131/powtest.py     fw-rs power meter end to end over USB/IP (fw-rs/docs/power-meter-and-misconnect.md 3.11)
emu/hy3131/couptest.py    fw-rs AUTO coupling of V / A: DC -> AC -> mixed signals through the hysteresis band, taps, SCPI (fw-rs/docs/acdc-stock-vs-fwrs.md 11)
emu/hy3131/ad2streamtest.py fw-rs AD2 stream ISR (feature ad2-isr: TIM7 -> IRQ 55 -> SPI2 reads) end to end (fw-rs/docs/dmm-input-scope-feasibility.md 9)
emu/hy3131/wavetest.py    fw-rs DMM wave view (scope page, source DMM; feature ad2-isr) driven by DBG:KEY / TAP, checked over SCPI, --case current, --shots DIR (feasibility 10)
emu/hy3131/burdentest.py  fw-rs burden voltage readout: FETC:BURD? = READ? x R_path per shunt / AC / POW:BURD, DISP:BURD, screenshots (fw-rs/docs/power-meter-and-misconnect.md 3.15)
emu/hy3131/inrushtest.py  fw-rs inrush current (A jack, 10 A AC row on the AD2 stream): timed isrc steps (DC lamp-like, AC load), MF -> Inrush, INR:* SCPI, the saved FAST CSV; private disk copy + log per process, --shots DIR (fw-rs/docs/inrush-current.md)
emu/hy3131/btwaketest.py  fw-rs DMM rail (PE2) wake: BT link on the Scope page, link cycles, a chip POR (device check 2026-10-21 Bluetooth 1g / 1h)
emu/hy3131/runemu.py      emu/run.py with this directory's HY3131.cs compiled in (development)
emu/hy3131/RESEARCH.md    sources, facts, open points
emu/hy3131/ref/           transcribed register map and mux tables
```

## Terminal-level model (DM40 front end)

`DM40Frontend.cs` (`Dm40Frontend`) sits between the jacks and the chip. The
user says what is **connected** to V-ohm / COM / A; the firmware's own relay
pulses (PA9 / PA10 latching pair = V/A path or the ohm-mV network, PE0 / PE1 =
A / mA / none shunt, PC7 = frequency mode; `fw-rs/src/dmm.rs` relay_pa /
relay_pe / pc7, stock FUN_08012780 / FUN_080127be) and its HY3131 row decide
the route, so a fixed connection behaves like on the real meter when the
function changes:

| connected | DC V | DC A | ohm | diode | cap | temperature |
|---|---|---|---|---|---|---|
| voltage source V-COM | V | 0 (nothing in A) | KCL node of the ohm source against the source: clipped (OL) or a wrong value | OL / Vsource | no oscillation | reads it as thermocouple mV |
| current source A-COM | 0 (nothing on V) | I x relay-selected shunt | OL | OL | - | - |
| resistor | 0 | 0 | R (ratiometric) | 0.765 mA x R | no oscillation (R < 3 Rref) | 0 mV |
| diode | 0 | 0 | Vf / (Vs - Vf) x Rref | Vf (reversed: OL) | no oscillation | - |
| capacitor | 0 | 0 | charges through Rref: rising, then OL | OL | f = 1 / (2 ln2 R C) or the current ramp | - |
| thermocouple | E(T) - E(Tcj) | 0 | - | - | - | T (die sensor as cold junction) |
| voltage source in the A jack | 0 | V / (Rs + shunt) -> OL | OL | | | |

Queries the chip model makes: `VJack(path)` (Thevenin of the V-COM elements
into 10 M, gated by the relay: 0 when the jack is routed elsewhere),
`ACurrent()` (through the shunt the relays selected; the row's OP1 gain and
tap scale it), `OhmNode(Vs, Rref)` (DC operating point: the OP3 source at VREF
through the reference resistor against everything on the jack; a plain
resistor gives `Vx / Vref = Rx / Rref` exactly, the continuity row's swapped
reference mux gives the negative code), `DiodeNode(0.765 mA, 3.3 V)`,
`CapOscC(R)` / `CapRampC(I, Vthr)` (0 or NaN when the network cannot
oscillate / ramp), `TcEmf()`. The calibration record is folded in physically:
Rref x gain, the ohm offset as lead resistance, the V / A gains on the
source. Cold junction (`cj`, the jacks) and HY3131 die (`die`, what SAD1I 10/11
report) are separate; the die follows the cold junction unless set
(`die:35`): a thermocouple at 100 C with cj 25 and die 35 reads ~110 C on both
firmwares, because they take the die sensor as the cold junction.

Unknowns stated once: the DM40 netlist (the per-row scale factors are the
stock's nominal constants), the ohm source open-circuit voltage (SFUVR table
at VDDA 3.6 V: 1.4 V on the normal ohm rows, 0.625 V on the "ON" variant,
-0.625 V continuity; the community measured ~0.85 / 0.27 mA short-circuit on
the real meter) which only the cross cases depend on, and PE2 (DMM rail /
power hold) which is not modelled as chip power.

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
  AD2 runs on its own grid from the last R26 change (as AD1): one AD2F and one
  fresh R03..R05 per period, the input at the grid point plus a per-result
  noise model (30 codes at OSR2 32, white scaling with OSR2; ENCHOPAD2 changes
  neither the rate nor the noise, as on the device:
  `Ad2NoiseCodes`; a model, not a measurement), an unread result overwritten;
  a read is never torn (the chip advances at CS low only). The row
  recognition also accepts an AC row with AD3 / RMS / LPF off and another
  AD2 rate (fw-rs data logger, LOG:SOURce AD2).
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
* fw-rs misconnect sentinel nodes (fw-rs/src/sentinel.rs): the comparator on
  SCMPI = PB1 sees the A current times the relay-selected shunt (DC V / mV /
  ohm / temperature roles), on SCMPI = SENSE in DC A the V jack / 1000 when SS5
  is closed (CMPHO / CMPLO instantaneous, DC + AC waveform); AD1 on PB1 - PB3
  in the AC V / mV roles, with the AD1 peak hold over every elapsed conversion.
  Row matching ignores AD1's fields in rows that leave AD1 off, PKHSEL /
  ENPKH, and SS5 in the current rows.
* fw-rs power meter rows (`fw-rs/docs/power-meter-and-misconnect.md` 3.1,
  assignment B; role `Power`, mode 16, `Power vr<V range> iu<I unit> ir<I
  range>`, Range = vr * 6 + iu * 2 + ir): the 16 distinct rows of the design's
  builder (`Hy3131Core.PowerRow`; iu 1 / 2 share the bytes, PE tells mA from
  uA) with R34 SVXI / SDO23, R35 AD3IN / AD3IG, R20, R22, R36 and the I row's
  R2E / R2F = 08 81 free. X = AD2 = A current x relay-selected shunt x OP1 (x10
  with OPS1) x AD2IG; Y = AD3 = V jack / kV[vr] (10 .. 10000) x OP2 (x10 with
  OPS2) x AD3IG / AD3RG; codes of 1.25 V / 2^18, both DC coupled. RMS block:
  SDO23 -> sum(Yac^2)/N, else SVXI -> sum(Xac*Yac)/N (signed: 38-bit two's
  complement in RMS<37:0>, bits 39:38 sign copies), else sum(Xac^2)/N; LPF =
  DC of Y with SDO23, else of X; R03..R05 = Y with SDO23, else X. The cross
  term is Xac * Yac * mean(u_V(t) u_I(t - ph)) over one period of the unit
  waveforms (sine: cos ph; square / triangle in phase: 1; different V / I
  frequencies: 0); XY noise is the uncorrelated residue (floor x
  sqrt((X^2 + Y^2 + floor^2) / N)), XX / YY keep the 30-code floor. SPHACAL
  (R36) shifts only the cross term: <6:0> AD2CLK periods (0.407 us) of the I
  channel, bit 7 = 1 advances I (phase lag reduced by 360 f n / 2.4576 MHz
  deg), bit 7 = 0 delays it. An R34 statistic change in the middle of a block
  makes that block (RMS and LPF) the time-weighted mix of the selections it
  saw; the next block is clean (`powmix:0`: no mixing; the real chip's
  behaviour is unknown, design 3.4 / device test T3). Calibration: X carries
  the stock AC A gains folded in (AC part / cal(0xC4 + 4 ci), DC part /
  cal(0xF4 + 4 ci) or, while that is the default 1.0, / cal(0xC4 + 4 ci);
  ci = iu * 2 + ir, no offsets); Y carries the factory-derived tap gain
  (6 V: DC V r0 x AC V r1 / DC V r1, else AC V r1..r3; fw-rs
  pw::v_gain_factory) times `powvg:` (a deviation for the V self-cal tests).
  `powz:1` adds the device's LPF zeros (Y +1080 codes, X +888 with OP1 x10,
  +155 with x1; doc 3.11 T1 / T3). The comparator / counter: SCMPI = SENSE
  sees the V tap DC-coupled (dc + AC against VRH / VRL: a unipolar signal
  never crosses a negative VRL), SCMPI = PB4 the current's AC at OP1O. The
  fw-rs DC row (powermeter.rs `dc_row`: the power row at YY plus AD1 = OP1O
  - PB3 with the DC A row's AD1 bytes) is a power row with AD1 on the
  current, the stock DC A calibration folded in (`powz:1`: +300 codes of
  AD1 zero). The f8_9 DC A rows fold the uA calibration when PE selects the
  47.5 ohm shunt (stock mode 9).
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
| power (fw-rs, mode 16) | AD2 = I (stock AC A path), AD3 = V tap through OP2, RMS = XX / XY / YY by R34, LPF | `X = I * shunt * OP1 * AD2IG`, `Y = V / kV[vr] * OP2 * AD3IG` in 1.25 V / 2^18 codes; XY signed (above) |

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
| `RailModel true/false`, `PowerUpMs ms` | PE2 power gate (default on, 2 ms): PE2 low = unpowered (writes ignored, DO low); a PE2 rise = power-on reset (registers cleared, BORF) and the bus ignored for `PowerUpMs`. Until the firmware first raises PE2 the chip counts as powered (a ROM-raised rail, the unit tests). `RailUps`, `DeafFrames`: rises seen, frames ignored |
| `Status`, `Role`, `Mode`, `Range`, `Intf`, `Ad1`, `CmpPin`, `Transfers` | state for scripts and tests |

Connection spec (`emu/run.py --dmm SPEC[@T]`, script lines `T dmm SPEC` or
`T monitor sysbus.hy3131 SetInput "SPEC"`, the GTK panel). Elements add up
(several with `;`), `open` clears everything, `KEY:none` removes one:

| spec | connected |
|---|---|
| `vsrc:1.234[,ac=1.0,f=50,shape=sine|square|triangle,duty=0.3,rs=4700]` | voltage source V-COM: DC, AC rms, source resistance |
| `isrc:0.0123[,ac=,f=,shape=,duty=,compl=10,ph=DEG]` | current source A-COM (compliance V); `ph`: phase of its AC part behind the V source's AC part (positive = current lags, inductive; PF = cos ph) |
| `pow:230@50,i=1[,pf=0.8\|ph=DEG,dc=VDC,idc=IDC,f=HZ,shape=]` | power meter shorthand: V source (AC rms 230 at 50 Hz + dc) on V-COM and current source (AC rms 1 A at the same frequency + idc) into A, the current lagging by `ph` or `acos(pf)` (`pf < 0` = energy flowing out, P < 0; `ph` wins over `pf`); negative `idc` = negative DC power. Replaces only the V and A sources (other elements stay; prefix `open;` for a clean connection). With `emu/run.py --dmm` the last `@` is the time: write `pow:230,f=50,i=1` or `pow:230@50,i=1@0` |
| `r:4700`, `short` (= `r:0`), `c:100e-9`, `diode:0.65[,rev=1]` | resistor, short, capacitor, diode (anode at V unless `rev`) between V and COM |
| `tc:100`, `tcmv:4.096`, `tcopen` | type K thermocouple: hot junction C, or its mV, or connected but open |
| `cj:25`, `die:35` (`die:none` = follows cj) | cold junction (jacks) and HY3131 die temperature |
| `avsrc:0.5[,rs=]`, `isrcv:0.001`, `aload:100` | wrong-jack cases: voltage source across A-COM, current source across V-COM, resistor across A-COM |
| `dcv:`, `acv:V@Hz`, `hz:F,duty=,amp=`, `dca:`, `aca:`, `ohm:`, `cap:`, `temp:T,cj=`, `chip:` | the older per-function keys: each replaces the whole connection with the equivalent one |
| `noise:0`, `cal:0`, `beat:1`, `powmix:0` | model options (`powmix:0`: an R34 switch in the power role does not mix the block in progress) |

State: `sysbus.hy3131 Frontend` returns one line of `key=value` tokens (relay
and shunt state, role / mode / range, which converters and switches are on,
the V-jack voltage, the A current and shunt drop, the ohm node, the
connection); `StateFile @path` writes the same line to a file on change (the
GTK panel's schematic reads it).

## Tests

`emu/hy3131/tests/run.sh` (dotnet 8; `DOTNET=` to point at an SDK) compiles
the real `HY3131.cs` with a 60-line Renode shim and drives it through the SPI
pins like the firmware: 297 checks of the register semantics, the frame,
conversion timing, DC / AC / A / ohm / continuity / diode / capacitance /
frequency / temperature physics, calibration consistency, the AC coupling
transient, the AC+DC cadence, the (optional) enable-order beat, the misconnect
sentinel and the fw-rs power rows (recognition, PF 1 / 0.5 / 0 / -1, DC, the
38-bit sign, SPHACAL, statistic-switch mixing, the counter, the stock AC V /
AC A rows with both jacks driven).

`emu/hy3131/modetest.py` runs the stock firmware and fw-rs in the emulator:
the input is scripted, the stock is switched with Bluetooth command 06 and
polled with command 09 (the values the LCD shows, `bt_meas_v2_6.h`), fw-rs
with `CONF:<f>` / `READ?` over the same link. Results (last reported value
after settling; stock Bluetooth is enabled by patching settings byte 0x115 in
the emulated flash with a recomputed CRC):

| scenario | input | expected | stock shows (BT cmd 09) | fw-rs READ? |
|---|---|---|---|---|
| dcv | `dcv:1.234` | 1.234 V | 1.2340 | 1.234 |
| dcv-neg | `dcv:-12.5` | -12.5 V | -12.500 | -12.5 |
| dcv-hv | `dcv:230` | 230 V | 230.01 | 230.0 |
| dcmv | `dcv:0.2345` | 234.5 mV (auto -> mV) | 234.5 | 234.494 |
| acv | `acv:1.0@50` | 1.0 V rms | 1.0000, 50.00 Hz | 0.999982 |
| acv-hv | `acv:230@50` | 230 V rms | 230.0, 50 Hz | 229.998 |
| acdcv | `acv:1.0@50,dc=0.5` | 1.1180 V (AC+DC) | 1.1181 | 1.11806 |
| dca | `dca:0.5` | 0.5 A (auto mA -> A) | 0.5001 | 0.499997 |
| dcma | `dca:0.0123` | 12.3 mA | 12.30 | 12.2999 |
| aca | `aca:0.1@60` | 0.1 A rms | 99.99 mA | 99.9989 mA |
| ohm | `ohm:4700` | 4700 ohm | 4700 | 4699.99 |
| ohm-1m | `ohm:1e6` | 1 Mohm | 1.0001 M | 999987 |
| cont | `ohm:10` | 10 ohm | 10.00 (beeper: CMP pin) | 10.005 |
| diode | `diode:0.65` | 0.65 V | 0.6501 | 0.650016 |
| cap | `cap:100e-9` | 100 nF (RC oscillator) | 100.0 nF | 100.002 nF |
| cap-big | `cap:100e-6` | 100 uF (current ramp) | **104.0 uF** (+4 %, see below) | 102.03 uF (+2 %) |
| hz | `hz:1000,duty=0.25,amp=1.0` | 1000 Hz, 25 % | 1000.0 Hz, 25.0 % | 1000.02 |
| temp | `temp:100,cj=25` | 100 C, CJ 25 C | 100.3 C, CJ 25.0 | 100.29 |

(`modetest.py -j 6`, final run on commit d07fedc + the final model; the fw-rs
ohm row was rerun alone after a port collision of the harness.) All within the
meter's specification except the capacitance ramp ranges: the
stock computes C = I * dt / dV with dt taken from its own millisecond timer at
the poll (every 255 ms + 10 ms granularity) while dV spans whole AD1
conversions (50 ms at the ramp row's OSR 4096 with the chopper), so dt / dV is
quantised to 250 / 300 ms steps and the trimmed mean lands ~4 % high; the
factory calibration leaves the r5..r7 gains at 1.0. Whether the real meter
shows the same bias is unknown (no device test). fw-rs's +2 % has the same
origin with its own timing. Run time per scenario: 8..18 virtual seconds,
25..80 s wall at `-j 6`.


### Fixed connections x functions (cross table)

`modetest.py --cross`: one connection, every function, both firmwares
(readings as reported over Bluetooth cmd 09 / SCPI READ?; OL = no valid
reading; cells are the last value after the settle; blank = not run):

| connection | fw | dcv | acv | dca | aca | ohm | cont | diode | cap | hz | temp |
|---|---|---|---|---|---|---|---|---|---|---|---|
| V 1.234 `vsrc:1.234` | stock | 1.234 | 0.00012 | -2.71e-06 | 7.2e-06 | OL | 0 | 1.234 | 0 | 0 | OL |
| V 1.234 `vsrc:1.234` | fwrs | 1.23399 | 0.017731 | -2.71462e-06 | 7.20722e-06 | OL | -665.907 | 1.23402 | 3.14321e-15 | 0 | OL |
| V 1.234 + I 12.3 mA `vsrc:1.234;isrc:0.0123` | stock | 1.234 | 0.00012 | 0.012301 | 2.2e-05 | OL | 0 | 1.234 | 0 | 0 | OL |
| V 1.234 + I 12.3 mA `vsrc:1.234;isrc:0.0123` | fwrs | 1.23399 | 0.017731 | 0.0123003 | 2.19533e-05 | OL | -665.908 | 1.23401 | 3.14321e-15 | 0 | OL |
| I 12.3 mA `isrc:0.0123` | stock | 0 | 0.00012 | 0.0123 | 2.2e-05 | OL | OL | OL | 0 | 0 | 24.7 |
| I 12.3 mA `isrc:0.0123` | fwrs | -1.55273e-06 | 0.00083487 | 0.0122997 | 2.19349e-05 | OL | OL | OL | 3.52156e-15 | 0 | 24.6698 |
| R 4.7 k `r:4700` | stock | 0 | 0.00012 | -2.71e-06 | 7.2e-06 | 4700 | OL | OL | 0 | 0 | 24.7 |
| R 4.7 k `r:4700` | fwrs | 2.00805e-06 | 0.000830868 | -2.70869e-06 | 7.20722e-06 | 4699.99 | OL | OL | 3.14321e-15 | 0 | 24.6942 |
| 0.1 V behind 4.7 k `vsrc:0.1,rs=4700` | stock | 0.09995 | 0.00012 | -2.71e-06 | 7.2e-06 | 4.213e+06 | OL | OL | 0 | 0 | OL |
| 0.1 V behind 4.7 k `vsrc:0.1,rs=4700` | fwrs | 0.0999482 | 0.00144192 | -2.70207e-06 | 7.20592e-06 | 779228 | OL | OL | 3.14321e-15 | 0 | OL |
| diode 0.65 `diode:0.65` | stock | 0 | 0.00012 | -2.71e-06 | 7.2e-06 | OL | OL | 0.65 | 0 | 0 | 24.7 |
| diode 0.65 `diode:0.65` | fwrs | -1.55273e-06 | 0.000843113 | -2.70032e-06 | 7.20592e-06 | 4.31845e+07 | 3.01894e-06 | 0.650005 | 3.14321e-15 | 0 | 24.6698 |
| C 100 nF `c:100e-9` | stock | 0 | 0.00012 | -2.69e-06 | 7.2e-06 | 3.8684e+07 | OL | OL | 1e-07 | 0 | 24.7 |
| C 100 nF `c:100e-9` | fwrs | -1.55273e-06 | 0.000833298 | -2.7066e-06 | 7.20592e-06 | 5.77772e+07 | OL | OL | 1.00002e-07 | 0 | 24.6698 |
| TC 100 C (cj 25, die 25) `tc:100;cj:25` | stock | 0.00309 | 0.00012 | -2.71e-06 | 7.2e-06 | 12.2 | 4.68 | 0.0108 | 0 | 0 | 100.3 |
| TC 100 C (cj 25, die 25) `tc:100;cj:25` | fwrs | 0.00309109 | 0.000834009 | -2.70207e-06 | 7.20592e-06 | 12.196 | 4.68485 | 0.0107508 | 3.52156e-15 | 0 | 100.314 |
| TC 100 C, die 35 `tc:100;cj:25;die:35` | stock | 0.00309 |  |  |  |  |  |  |  |  | 110.3 |
| TC 100 C, die 35 `tc:100;cj:25;die:35` | fwrs | 0.00309109 |  |  |  |  |  |  |  |  | 110.315 |
| 0.5 V in the A jack `avsrc:0.5` | stock | 0 |  | 8.197 | 0.0011 | OL |  |  |  |  |  |
| 0.5 V in the A jack `avsrc:0.5` | fwrs | -1.55273e-06 |  | 8.19661 | 0.000255177 | OL |  |  |  |  |  |

Reading the table: a voltage source alone in ohm gives OL (the ratiometric
node clips); in continuity the stock shows "0.00" (it clamps the negative
code to 0) and fw-rs the negative value: a firmware difference, not the
model; the same source in diode mode reads its own voltage (1.234 V < 3 V
full scale); in AC V the stock settles at 0 while fw-rs keeps ~18 mV of the
coupling transient on a DC input (the device-measured stock autoranges to mV
and reads 1 mV, acdc doc 6a); 0.5 V across the A jack reads 8.2 A on the
auto-selected 10 A range (no fuse in the model); a thermocouple with the die
10 C above the cold junction reads 110 C on both (they take the die sensor as
the cold junction); 1 V behind 4.7 k in ohm is OL, 0.1 V behind 4.7 k shows
12 k instead of 4.7 k.

## Open points

See RESEARCH.md 8: LPF / RMS output rate (modelled with the measured value),
the empirical AC-node and enable-order parts, the temperature sensor constants
(stock vs datasheet), the DM40 CNT pin and netlist.
