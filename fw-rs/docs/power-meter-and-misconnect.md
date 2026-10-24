# Power meter mode and wrong-connection detection: feasibility and design

Date 2026-10-21 (implemented since: misconnect sentinel 4.6, power meter
3.11). Answers the two
owner ideas of `TODO.md` "Owner notes 2026-10-14" (power meter, misconnect
warnings) for the DM40 hardware as far as it is known without a netlist.

Evidence tags:

* **VERIFIED** = datasheet text (`docs/DS-HY3131_EN.pdf`, transcribed in
  `emu/hy3131/ref/registers.md` and `RESEARCH.md`), the stock configuration row
  bytes (`firmware/re/analysis/hy3131_v2_6.h`, byte-identical copies in
  `fw-rs/src/hy3131_tables.rs`), the stock formulas and relay table
  (`dmm_v2_6.h`, `hy3131_v2_6.h`) or a device measurement already in
  `fw-rs/docs/acdc-stock-vs-fwrs.md`.
* **INFERRED** = what the row bytes + the stock scale constants imply about the
  board wiring (RESEARCH.md open point 5: the DM40 PA/PB netlist is not known),
  BOM-based conclusions ([hw/README.md](../../hw/README.md)), or chip behaviour
  the datasheet does not state.
* **UNKNOWN** = needs the device or the schematic; listed with the test that
  settles it.

Nothing under `firmware/re/history/` was used.

Summary:

| idea | verdict | why |
|---|---|---|
| 1. Power meter (Vrms, Irms, P, S, PF, Wh) | **GO** (register-only; one hardware fact to confirm on the device) | V and I already use the same relay state; the chip has two fast ADCs with independent input muxes, two op amps with independent input muxes and a hardware multiplier (RMS = sum(X*Y)/N) with a phase trimmer (SPHACAL) made for this. The one real constraint: both stock paths go through OP1, so one quantity must move to OP2 (which the board has with its x10 network). |
| 1b. Time-multiplexed fallback | GO, 1-2 days, but gives **S only** (no P, no PF) | alternating the stock AC V and AC A rows like AC+DC yields Vrms and Irms without any phase relation. |
| 2. Misconnect detection | **GO for the dangerous case** (current in the A jack while in V / ohm / ...), **partial** for the rest | the resting shunt state (47.5 ohm) and the comparator on PB1 give a cheap sentinel in every non-current mode; "leads in V while in A" is detectable from 21 V (comparator) or from ~2 V (AD1 in AC A); a blown fuse and "mains across A-COM" cannot be detected in time. |

---------------------------------------------------------------------------

## 1. What the HY3131 offers (VERIFIED, datasheet)

* Two fast 19-bit sigma-delta ADCs, AD2 and AD3, same clock (AD2CLK = SYSCLK/2
  = 2.4576 MHz), same OSR2, independent input muxes:
  `SAD2IP / SAD3IP = OP1O, OP2O, PB4, PB7`; `SAD2IN / SAD3IN = RLU, AGND, PB3,
  PB5`; gains 0.5 / 1.0 / 1.5 / 2.0 each (AD2IG, AD3IG); common reference
  (SAD2RH/RL). Datasheet 13.2 note 8: "when AD2 and AD3 are used at the same
  time, connect AD2IN and AD3IN to AGND or to different PB<x>".
* Digital block after a high-pass filter on each channel: `RMS<37:0>` =
  `sum(X^2)/N` (SDO23 = 0, SVXI = 0), `sum(X*Y)/N` (SDO23 = 0, SVXI = 1),
  `sum(Y^2)/N` (SDO23 = 1), X = AD2, Y = AD3, N = OSR4 (LPFBW). One
  accumulator: the three statistics are **sequential**, not simultaneous. The
  LPF (DC mean) follows SDO23: LPF of AD2 (0) or of AD3 (1). RMS and LPF
  results arrive together at the LPF rate (device: every 106.7 ms with LPFBW =
  111, RESEARCH.md 3 / 8.1). Result = 38-bit signed in RMS<37:0>: `sum(X*Y)`
  can be negative.
* `SPHACAL<7:0>` (R36, HY3131 only): "This function is to measure Power, set
  AD2 and AD3 output in the same phase. Set SPHACAL first, then ENAD2 and
  ENAD3 = 1, otherwise the data would be stabilized later." Bit 7 = AD2 leads
  (1) / lags (0); bits 6:0 = delay in AD2CLK periods: step 1/2.4576 MHz =
  0.407 us, max 127 steps = 51.7 us (= 0.93 deg at 50 Hz, 1.12 deg at 60 Hz).
* Datasheet feature list: "Power Measurement can be achieved with combination
  of internal digital circuit (HY3131 only)"; block diagram labels AD3 "Power
  sigma-delta ADC (AD3)".
* Two op amps OP1, OP2 with input muxes `SOP1P = SENSE, FB, RLU, AGND, PB0,
  PB1, PB2, PB8`, `SOP2P = SENSE, FB, RLU, AGND, PB0, PB1, PB2, PB3`.
  `OPS<1>` / `OPS<2>` (R25 bits 0 / 1) select the negative input: the external
  OPxN pin (board gain network; the typical circuit has 10k / 90k = x10) or
  the output (unity buffer). Op amp common-mode input range 0.1 V .. VDDA -
  1.1 V (electrical table).
* Datasheet 13.1 note 7: "When AD1 and AD2/AD3 is on, users must not turn
  on/off AD to avoid different AD1 Offset problem" - toggling AD2/AD3 while
  AD1 converts shifts the AD1 offset. Relevant to the sentinels (section 5).
* Comparator: input `SCMPI = SENSE, FB, OP1O, PB0, PB1, RLD, PB3, PB4`,
  thresholds from the AGNDP/AGNDN taps (AGNDP<1> = AGND + 20.8 mV with REFO =
  1.25 V, AGNDN<1> = -20.8 mV, AGNDP<2> +-41.7 mV, <3> +-62.5 mV ...), outputs
  readable in CTSTA (ACPO bit 6, CMPHO bit 5, CMPLO bit 4).

## 2. What the stock does with these blocks (VERIFIED, rows)

Row bytes are R20..R35 (index 0..21). The decoded facts used below:

| row | R25 | R26 | R27 | R29 | R32 | R34 | R35 | meaning |
|---|---|---|---|---|---|---|---|---|
| f0 AC V r0 / r1..3 | 0x43 / 0x40 | 0x80 | 0x9d | 0xfc | 0x38 | 0x88 | 0x94 | AD2 = PB4 - AGND; AD3 = PB4 - AGND; ENOP1, OP1+ = SENSE; OPS1 = OPS2 = 1 in r0 (x10) else 0 (x1); RMS = sum(X*Y), LPF of AD2 |
| f2 AC mV r0 / r1 | 0x43 / 0x40 | 0x80 | 0x5d | 0xf8 | 0xaa | 0x88 | 0x54 | AD2 = OP2O - AGND; AD3 = OP2O - AGND; OP1+ = OP2+ = RLU; OPS2 = 1 in r0: **OP2 has a working x10 network on this board** |
| f4 AC A r0 / r1 | 0x43 / 0x40 | 0x80 | 0x2d | 0xf4 | 0x3e | 0x88 | 0x24 | AD2 = OP1O - PB3; AD3 = OP1O - PB3; OP1+ = PB2 (10 mohm shunt); OPS1 x10 / x1 |
| f5_6 AC mA/uA | 0x43 / 0x40 | 0x80 | 0x2d | 0xfc | 0x3d | 0x88 | 0x24 | as f4 with OP1+ = PB1 (0.5 ohm / 47.5 ohm shunt) |
| f1 DC V r0 | 0x40 | 0x00 | - | 0x00 | 0x38 | 0x00 | - | AD1 = OP1O - RLU, OP1+ = SENSE x1, tap PA7; AD2/AD3 off |
| f1 DC V r1..3 | 0x40 | 0x00 | - | 0x00 | 0x33 | 0x00 | - | AD1 = SENSE - PB5 (R24 0x07, R23 0x83 input buffers on), both op amps off; taps PA6 / PA5 / PA4+PA5 |
| f7 / f8_9 DC A | 0x01 / 0x00 | 0x00 | - | 0x00 | 0x3e / 0x3d | 0x00 | - | AD1 = OP1O - PB3, OP1+ = PB2 / PB1 |

* **The stock uses sum(X*Y) in every AC row, with AD3 on the same node as AD2**
  (PB4, OP2O or OP1O): a cross-correlation that cancels the uncorrelated ADC
  noise, never two different quantities. R36 (SPHACAL) is written 0x00 at every
  mode change (`hy3131_v2_6.h` "write 0x00 to R26, R34, R36"). The stock has no
  power function. The device experiment in acdc doc 10.1 (R34 cycled: sum X^2 =
  9919, sum Y^2 = 9930) also says AD2 and AD3 differ by ~0.1 % in gain at the
  same node - a calibratable product error.
* **Relay states**: V modes 0/1 and A modes 4..9 all use the PA9/PA10 latching
  relay in state 0 (`dmm_v2_6.h` "PA 0", `dmm.rs write_config` `pa = 0` for
  0 | 1 | 4..=9); the relay only separates "V/A" from "mV / ohm / cap / diode /
  cont / Hz / temp". The shunt MOSFETs (PE0 / PE1, RUH30J51M dual N-MOSFET,
  [hw/README.md Main parts](../../hw/README.md#main-parts)) select 10 mohm (PE0 = 1), 0.5 ohm (PE1 = 1) or 47.5 ohm (both 0); V
  modes write "both 0" (`relay_pe(2)`). So **in every V mode the divider and
  the shunt network are connected at the same time** - nothing new is energised
  by a power mode; it is only a question of which converter looks at which node.
* **Both stock AC paths run through OP1** (R32 = 0x38 SENSE in AC V, 0x3e /
  0x3d PB2 / PB1 in AC A), and the AC V node PB4 is *downstream* of OP1O through
  an external coupling capacitor (acdc doc 5 / 6a: the AD2 node charges to the
  OP1O level with tau ~0.3 s; README "Analog path": PB4 AC-coupled from OP1O).
  So "V on AD2 and I on AD3" is not a mux-only change: OP1 can buffer either
  the divider tap or the shunt, not both. One of them must move to OP2.

## 3. Feasibility on the DM40: the register-only power row

### 3.1 Node assignment

Two assignments are possible; both need no board change.

**B (recommended): I on the stock path, V through OP2, DC-coupled.**

| block | setting | source of confidence |
|---|---|---|
| I path | OP1+ = PB2 (A) or PB1 (mA / uA), OPS1 = 1 (x10, low range) / 0 (x1), **AD2 = OP1O - PB3**, AD2IG 1.0 | byte-identical to the stock AC A rows f4 / f5_6 (VERIFIED) |
| V path | divider tap on SENSE through the switch bytes R2A..R2F of a stock V row (4 V: f1 r0 `06 00 00 90 28 a0` = PA7 /10; 40 V: f0 r1 `20 00 00 09 28 a0` = PA6 /100; 400 V: f0 r2 `20 00 90 00 28 a0` = PA5 /1000; 1000 V: f0 r3 `20 00 99 00 28 a0` = PA4+PA5 /10000; all SMODE 0x20 = no power bus, capacitor array on), **OP2+ = SENSE** (SOP2P = 000), OPS2 = 0 (x1), ENOP2, **AD3 = OP2O - AGND** (SAD3IP = 01, SAD3IN = 01; RLU or PB5 are the alternatives if AGND shows an offset), AD3IG 1.0 (1.5 possible) | SOP2P = SENSE exists (VERIFIED mux table); OP2 -> AD2/AD3 is the stock AC mV path (VERIFIED f2); the tap levels are the ones the stock already puts on OP1 in AC V r1..r3 (VERIFIED: same taps, same op amp common-mode range). That OP2 behaves like OP1 on SENSE: INFERRED. **UNKNOWN**: the stock A rows run the power bus as SGND (R2F 0x81) with PA8 P closed, i.e. PA8 grounded, while the V rows leave the bus floating (0xa0) and use it to tie the tap to PA8. The combined row takes the V row's bytes; if the current channel misbehaves in T1/T2, PA8's role is the first suspect (what PA8 is wired to is not known) |
| references | SAD2RH/RL = PB6 / AGND (R27 low nibble 0x0d, as every AC row) | VERIFIED |
| note 8 | AD2IN = PB3, AD3IN = AGND: different | VERIFIED guidance |
| RMS / LPF | R29 = 0xf8 (ENRMS, ENLPF, LPFBW 111, no peak hold); R34 = 0x88 (ENAD3, SVXI) for P, 0x80 for Irms, 0x8c for Vrms (SDO23 also moves the LPF to AD3 = V) | VERIFIED register semantics |
| AD1 | off (R23 = 0x03) as in the stock AC rows: no note-7 exposure | VERIFIED |
| relay / MOSFET | PA state 0; PE per current range (0 = A, 1 = mA, 2 = uA) | VERIFIED |
| comparator / counter | SCMPI = SENSE (000), R21 = 0xdd (+-20.8 mV), ENCMP, ENCTR: frequency of the voltage (tap >= 23 mV at any range for mains) | VERIFIED mux; level INFERRED |
| capacitor array | SMODE<5> + ACC as the f0 row (0xa0 / R30 0x80): the divider's AC compensation | VERIFIED bytes |

Why B: no coupling capacitor in either path, so the only V-to-I phase
difference is the op amps' (2 MHz bandwidth: negligible at mains); DC is
measured too (the LPFs give I_dc and V_dc), so the mode also works as a DC
wattmeter (PSU, battery, PV); the current path keeps the stock's AC A
calibration constants.

**A (alternative): V on the stock AC V path, I through OP2.** OP1+ = SENSE,
OPS1, OP1O -> coupling C -> PB4 -> AD2 (f0 bytes), OP2+ = PB2 / PB1 with OPS2
x10 / x1, AD3 = OP2O - PB3. Keeps the stock AC V calibration for Vrms but the
V channel is AC-coupled (no V_dc without AD1, and the 0.3 s coupling adds
~0.6 deg of V lead at 50 Hz; see 3.5). Use A only if B's OP2-on-SENSE turns
out noisy on the device.

Neither is "V and I on AD2 and AD3 by mux alone": the owner's premise holds
for the ADCs but the op amps are the shared resource.

### 3.2 Ranges

Independent (different hardware): any V range with any I range.

| V range | tap (PA) | tap level at full scale (1.05x) | AD3 at IG 1.0 |
|---|---|---|---|
| 4 V | PA7 (/10) | 0.42 V | 34 % FS |
| 40 V | PA6 (/100) | 0.42 V | 34 % |
| 400 V | PA5 (/1000) | 0.42 V (0.59 V peak for a sine) | 34 % |
| 1000 V | PA4 + PA5 (/10000) | 0.105 V | 8 % (AD3IG 2.0 -> 17 %) |

The AC V r0 trick (x10 on the /100 tap) is not needed: the /10 tap exists (DC V
r0 uses it with OP1 x1). Op amp common-mode: AGND = 0.5 VDDA (R31 SAGND 00),
range 0.1 V .. VDDA - 1.1 V = -1.7 .. +0.7 V around AGND; 0.59 V peak fits
(the stock AC V r2 puts the same 0.59 V on OP1). Asymmetric headroom means
positive half-waves clip first on gross over range: autorange must act on the
RMS, as the stock does.

| I range | shunt (PE) | op gain | OP1O at full scale | stock row |
|---|---|---|---|---|
| 4 A | 10 mohm (PE0) | x10 | 0.42 V | f4 r0 |
| 10 A | 10 mohm | x1 | 0.105 V | f4 r1 |
| 40 mA | 0.5 ohm (PE1) | x10 | 0.21 V | f5_6 r0 |
| 400 mA | 0.5 ohm | x1 | 0.21 V | f5_6 r1 |
| 400 uA / 4000 uA | 47.5 ohm (none) | x10 / x1 | 0.2 V | f5_6 (mode 6) |

Autorange each quantity independently with the stock thresholds (up above
1.05 x limit, down below 0.1 x limit / 1.05, acdc doc 7 item 7) and settle
times (AC V 450 ms, AC A 550 ms); a range change restarts the phase machine
(3.4). For energy integration offer range lock (manual ranges are the stock's
`m+3 = 0x80`).

### 3.3 Scaling and sign

With X = AD2 = I path, Y = AD3 = V path (both codes in units of 1.25 V / 2^18
at the converter input, IG 1.0):

```
V_tap  = Y * 1.25 / 2^18                 Vin = V_tap * kV[r]      kV = 10, 100, 1000, 10000
V_sh   = X * 1.25 / 2^18                 I   = V_sh / (shunt[u] * opgain[r])   (stock constants)
P_ac   = RMSxy * (1.25 / 2^18)^2 * kV[r] / (shunt[u] * opgain[r])     [W]
Vrms_ac = sqrt(RMSyy) * 1.25 / 2^18 * kV[r]
Irms_ac = sqrt(RMSxx) * 1.25 / 2^18 / (shunt[u] * opgain[r])
I_dc   = LPF(X) * ... same scale as I ;  V_dc = LPF(Y) * ... same scale as V
P      = P_ac + V_dc * I_dc        (the HPF removes DC from X and Y; the DC product is added back)
Vrms   = hypot(Vrms_ac, V_dc)      Irms = hypot(Irms_ac, I_dc)      (as the stock's AC+DC)
S      = Vrms * Irms               PF = P / S (signed with P)        Q = sqrt(S^2 - P^2) (magnitude only)
E     += P * dt                    [W s] -> Wh; dt from the ms timer between accepted P results
```

Check: 230 V on the /1000 tap = 48.2k counts rms, 1 A on 10 mohm x1 = 2097
counts rms, PF 1 -> RMSxy = 1.01e8 -> P = 1.01e8 * 2.27e-11 * 1000 / 0.01 =
230 W. Resolution at that range pair ~0.1 W per count of the I channel;
5 digits are available at mains levels.

Calibration: the stock's AC V RMS gains (cal struct 0x34..0x44) apply to Vrms
only if the V channel stays on the stock path (assignment A). In B the V
channel is a new path (OP2 x1 + AD3): start with gain 1.0 and the DC V gains
(0x04..0x14, same divider) as the best prior; the I channel takes the stock AC A
RMS gains (0xc4 / 0xdc); P takes the product of the two channel gains (the RMS
gains act on sqrt(RMS), i.e. linearly on each channel). A user "power
calibration" against a known resistive load is one multiplier.

**Sign**: `poll_ac` (dmm.rs) and the stock read `(b[13] & 0x1F) << 32`, i.e. 37
bits unsigned - right for sum(X^2), wrong for sum(X*Y), which is negative when
the current flows the other way (load-side wiring, or a source under test).
The power read path must sign-extend from bit 37 (`b[13] & 0x3F`, bit 5 =
sign). Also drop the `sq > 262144` overload guard for the XY statistic (it is a
magnitude guard for a square; for P use |RMSxy| > 2^36 as the garbage guard).

### 3.4 Update rate: the three statistics are sequential

One accumulator (3.1): P = sum(X*Y), Irms = sum(X^2), Vrms = sum(Y^2) need
three R34 settings. A result every 106.7 ms (measured, RESEARCH.md 3). After a
mode switch the block in progress is mixed and must be discarded; acdc doc 10.1
experiment 3 saw "4 results to recover after an XY block" in the beat state -
with the AD3-first enable order the count is UNKNOWN (device test T3 below):
budget d = 1..4 discarded results per switch.

Proposed phase machine (like the AC+DC machine in dmm.rs, `switch_phase`):

```
P (XY) -> V (YY, LPF = V_dc) -> P (XY) -> I (XX, LPF = I_dc) -> ...
each phase: d discarded + 1 accepted result, 106.7 ms each
```

| d | P period | V, I period | full set |
|---|---|---|---|
| 1 | 0.43 s | 0.85 s | 0.85 s |
| 2 | 0.64 s | 1.3 s | 1.3 s |
| 4 | 1.07 s | 2.1 s | 2.1 s |

(The stock AC+DC cycle is 1.9 s, acdc doc 3, so even d = 4 is the stock's own
cadence.) Each accepted value goes through the stock EMA (`ema(thr, 0.25, ..)`
with the AC V / AC A windows). If T3 shows the chip restarts the block on an
R34 write (d = 0), P comes every 213 ms. Energy uses the last accepted P over
the wall time since the previous accepted P (gaps during range changes hold the
last P; a range change of either channel marks the integral "interrupted" in
the UI if it lasts > 1 s).

An alternative that keeps P continuous (XY always) and gets Vrms from AD1
running unchopped at OSR 256 (640 sps, software RMS) was considered: it needs a
sinc-response correction at 50/60 Hz (|H| = 0.97 at 50 Hz for a 3rd-order comb
at 640 Hz) and still leaves Irms without a converter. Not worth it; the phase
machine is the stock's own pattern.

### 3.5 Phase, bandwidth, accuracy

* Phase sources in B: op amps (2 MHz, 130 dB DC gain: < 0.002 deg at 60 Hz),
  the divider with its ACC compensation (set as the stock f0 row), the shunt
  (resistive), AD2/AD3 sample alignment. In A add the coupling capacitor: tau
  ~0.3 s (INFERRED from the 6a transient) -> V leads by atan(1 / (2 pi 50 *
  0.3)) = 0.61 deg at 50 Hz, 0.51 deg at 60 Hz. Error in P = S * sin(phi) *
  phi_err: at PF 0.5, 0.61 deg = 0.9 % of S = 1.8 % of P; at PF 0.1 it is 10 %
  of P.
* SPHACAL covers it: 0.61 deg at 50 Hz = 34 us = 83 steps of 0.407 us (fits
  127). Datasheet order: write R36 before enabling AD2/AD3. fw-rs's `configure`
  enables AD3 then AD2 (the beat fix, acdc doc 10) - compatible: R36 is part of
  the 22-byte row, written before both enables. R36 is byte 22 of the row
  region (R20..R36 = 23 bytes): extend `configure` or write R36 separately
  before the enables.
* Self-calibration of SPHACAL (menu item "Phase cal", resistive load, mains or
  a transformer): maximise `RMSxy / sqrt(RMSxx * RMSyy)` (should reach 1.000
  at PF 1); the optimum is flat (cos), so use the zero-crossing method instead:
  swap to a capacitor/inductor load (PF ~0) and null P, or fit the three
  statistics at two SPHACAL values. Store the result in fw-rs settings (not the
  stock cal record).
* The AD2/AD3 "beat" (acdc doc 10): sum(X*Y) exceeded the Cauchy-Schwarz bound
  when AD2 was enabled before AD3, i.e. the chip paired misaligned samples; for
  V*I that is a PF-dependent error. Keep the AD3-first order; verify with the
  resistive-load test that P = Vrms * Irms within the channel errors.
* Bandwidth: AD2/AD3 at 76.8 kHz output rate (OSR2 32) with sinc decimation;
  the stock's AC accuracy at 40 Hz..kHz applies. Harmonics to the 20th at 50 Hz
  are well inside. Block length 8192 samples = 106.7 ms = 5.33 cycles at 50 Hz
  (6.4 at 60 Hz): the non-integer cycle count gives a per-result ripple on P of
  order (1 / (2 pi 5.33)) * cos(2 phi) ~ +-3 % worst case before the EMA; the
  EMA over 4+ results and the random phase of the block start average it to
  < 1 %. For a utility-grade reading accumulate 2 blocks per phase.
* Expected accuracy after a one-point resistive calibration: V +-0.5 %, I
  +-0.5 % (stock AC specs), phase after cal < 0.05 deg -> P +-1 % at PF >= 0.5,
  energy the same. Without a new calibration (stock gains only, B assignment):
  P +-2 % (the OP2/AD3 channel gain is uncalibrated: +-0.1 % AD3 vs AD2 plus
  the OP2 offset/gain, INFERRED).
* Noise floor: the stock sees 31..33 RMS counts floor at AC V 4 V (acdc doc 6a)
  = sqrt(sum X^2), i.e. ~1000 counts^2 of noise power; in sum(X*Y) the two
  channels' noise is uncorrelated and averages toward zero over N: the P
  floor is far below the V or I floors. Zero-power reading with no load: V_rms
  * I_floor (~0.5 mA * 230 V = 0.1 VA of S, ~0 W of P).

### 3.6 Wiring, burden and safety (COM is shared)

The meter has V/ohm, COM and one A jack, isolated from the USB/MCU ground
(CA-IS3741 isolator + VPS8504B supplies,
[hw/README.md Isolation boundary](../../hw/README.md#isolation-boundary)). The
voltage is measured V-COM, the current A-COM: **the load current flows through
COM**, and COM is at the potential of the conductor you break into.

```
 source (L / +) ---------------------+------------------- load (+)
                                     |
                                   [V/ohm]
                              DM40  [A]   -------------------- load (-)   (return conductor broken here)
                                    [COM] ---+                  fuse, MOSFETs, shunt (R_path) between A and COM
                                             |
 source (N / -) ------------------------------+
```

(Corrected 2026-10-22: an earlier version of this sketch had A and COM
swapped, which contradicts the sign paragraph below; the load current must
enter A and leave through COM to read positive. The fw-rs entry dialog draws
this version.)

Wiring 1 (above, "V on the source side"): V-COM = V_load + I * R_path. The
meter's own burden is inside the voltage reading: P_meas = P_load + I_rms^2 *
R_path. R_path = shunt + fuse + MOSFET + traces:

| range | shunt | R_path (INFERRED; fuse and MOSFET UNKNOWN) | burden at full scale | error example |
|---|---|---|---|---|
| 10 A | 10 mohm | ~20..50 mohm | 0.2..0.5 V, 2..5 W | 0.1..0.2 % of 2.3 kW at 230 V |
| 400 mA | 0.5 ohm | ~0.6..1.5 ohm (mA fuse) | 0.24..0.6 V | 5 V x 0.4 A = 2 W load: 5..12 % |
| 4000 uA | 47.5 ohm | ~48 ohm | 0.19 V | 3.3 V x 4 mA: 6 % |

So: `P_load = P_meas - Irms^2 * R_path` with an R_path setting per shunt
(default = shunt value; the user can measure the rest with the ohm function
between the A jack and COM in the chosen range, a guided step). Not perfectly
knowable, document it; for low-voltage DC loads advise the A ranges (burden
0.1 V at 10 A) and show the burden voltage I*R_path on screen so the user sees
it.

Wiring 2 ("V on the load side"): V/ohm to the load's return terminal is
impossible without a second COM - the load's other terminal is the A jack. If
the user puts V/ohm on load(-) (the A-jack conductor) the reading is just the
burden voltage. Keep wiring 1 and compensate.

Sign: in wiring 1 the load current enters the A jack and leaves through COM,
which is the stock's positive direction (emulator README: `dca:0.5` = "current
into the A terminal", the stock shows +0.5001), so a consumer reads P > 0 with
no sign games. Define the power mode's positive direction as "into the A jack,
out of COM" and say so in the entry dialog; sources (PV, battery discharge)
then read negative, which is why the signed RMS read of 3.3 matters.

Safety:

* The A input's fuse and CAT rating of the DM40 are UNKNOWN here (the manual
  has them); the stock clamps the 10 A range at 10 A and has no time limit.
  Shunt dissipation at 10 A is 1 W in 10 mohm plus the fuse; typical handheld
  limits are 10 A for 30 s. Proposal: a configurable continuous-current limit
  with a countdown warning (default 10 A / 30 s, 5 A unlimited) - the owner
  must set the real numbers from the manual.
* The MOSFET range switch is rated 30 V (RUH30J51M): it only ever sees the
  voltage across the shunt path, fine as long as the fuse is intact.
* Power mode is the one configuration the safety note of
  `dm40-feature-request.md` ("never close the current shunt while in V mode")
  forbids by reflex. It is nevertheless the same hardware state as plain V
  mode (the shunt network is always in circuit; PE only selects which shunt).
  Make the mode explicit: no autorange path leads into it, the function picker
  item shows the wiring diagram and asks for confirmation, and the chosen
  current range is shown with the fuse rating.
* Long sessions (energy): inhibit auto-dim / auto-off while the integrator
  runs and the current is above the floor; the stock's "do not measure while
  charging" stays as a warning toast (the DMM side is isolated; the toast is for the scope / generator, which share the USB
  ground).
* Mains: the usual DMM rules (CAT rating of the leads, both leads rated,
  never break the neutral of a circuit you cannot de-energise). The entry
  dialog should state the current rating and that COM carries the load current.

### 3.7 Time-multiplexed fallback (if the simultaneous row fails on the device)

Alternate the stock AC V row (f0, chosen V range) and the stock AC A row (f4 /
f5_6, chosen I range) exactly as the AC+DC machine alternates mode 0 and 1
(`switch_phase`, settle 450 / 550 ms, 3-4 results per phase): Vrms and Irms
with the stock's calibration, S = Vrms * Irms, VAh. **No active power, no PF**
(no phase relation between phases), so it is a different product ("VA meter").
Cost 1-2 days on the existing AC+DC code; worth doing first as a UI/SCPI
skeleton, then swap the row.

### 3.8 Device tests that settle the UNKNOWNs (in order)

1. **T1 OP2 on SENSE**: write the B row for 40 V / 400 mA with R34 = 0x8c (Y^2):
   sqrt(RMS) vs a known AC source on V-COM must match the AC V reading within
   0.5 %; LPF(Y) vs DC V. Noise floor with the input shorted. This is the
   go/no-go for B; A is the fallback (T1a: OP2 on PB1/PB2 vs the AC A reading).
2. **T2 two different nodes**: R34 = 0x88 with a resistive load (e.g. 230 V
   lamp or a 12 V transformer + resistor): RMSxy / sqrt(RMSxx * RMSyy) = 1.000
   +- 0.005; also with the current reversed (sign). Capacitor load (or
   transformer unloaded): P near 0, PF small, repeat over SPHACAL 0..127 to
   find the null (phase cal).
3. **T3 switch cost**: toggle R34 among 0x88 / 0x80 / 0x8c every k results,
   log sqrt(RMS) per result (the `DBG:ACDC?`-style ring): how many results after
   a switch deviate -> d of 3.4. Also whether writing R34 restarts the block.
4. **T4 DC**: PSU + resistor on wiring 1: V_dc * I_dc from the LPFs vs DC V and
   DC A readings; P_ac ~ 0.
5. **T5 ranges**: every V range x every I range on suitable sources: scale
   errors vs the stock single-quantity readings; op amp clipping at 1.05x FS.
6. **T6 burden**: 5 V / 0.4 A load on the mA range: P_meas - Irms^2 * R_path vs
   a reference; measure R_path with the ohm function to confirm the default.

### 3.9 Proposed mode: readings, UI, SCPI

Readings: P (W, signed, main), Vrms (V), Irms (A), S (VA), PF (signed, 3
digits), f (Hz, from the counter on SENSE), E (Wh, signed accumulation; also
VAh), elapsed time, I_dc / V_dc on demand (DC wattmeter view), burden voltage.

UI, mapped on the stock DMM page slots (480x320; `ui.rs`: F78 main readout at
(40,68) with the unit at x 401, two LETTERS 30 px aux rows, badge slot, strip
chart, six bottom buttons; `strip_data` for the combined view):

```
 +------------------------------------------------------------------------+
 | POWER  AUTO 400V / 10A        [badge: HOLD/HV/REC/AUTO]   bt bars 12:34 |  status / range line
 |   Vrms  229.98 V                      Irms   0.9872 A                  |  aux1 / aux2 (LETTERS)
 |                                                                        |
 |              226.84          W                                         |  F78 main = P (signed)
 |                                                                        |
 |   PF 0.998   S 227.3 VA   50.00 Hz   burden 0.02 V                     |  3rd row (LETTERS, new)
 |   E 0.1234 Wh   00:12:34   [RESET]   [LOCK RANGE]                      |  energy row (replaces MIN/MAX row in this mode)
 |   [ strip chart: P (W) ]                                               |
 | [ V ][ A ][ ohm ][ cap ][ cont ][ Hz ]    ...  [W] via function picker |  bottom buttons unchanged
 +------------------------------------------------------------------------+
```

* Tap on the main value cycles P -> S -> PF -> E as the main readout (the
  other three move to the 3rd row). Long press on E resets the integrator.
  A tap on Vrms / Irms cycles the range menu of that channel (manual ranges
  + AUTO), as the stock MF range popup.
* The badge slot gets "PHASE?" (amber) until a phase cal exists for this
  SPHACAL, "BURDEN" when I^2 R_path > 2 % of P, "RNG" while a range settles
  (integrator holding), and the HV logic of the stock (>= 30 V red, >= 100 V
  glyph) from Vrms.
* Entry: function picker item "W" -> wiring dialog (diagram of 3.6, current
  rating, "I positive into A") -> OK. Default ranges AUTO / AUTO; both channels
  autorange independently; energy integration starts on the first accepted P
  after the ranges are stable.
* No-overlap check and contact sheet before showing (memory rule) - the sketch
  above only names the slots; the layout work is in the implementation plan.

SCPI (style of `scpi.rs`: `CONFigure:<f>`, `MEASure:<f>?`, `[SENSe:]...`):

```
CONFigure:POWer [<vrange>|AUTO[,<irange>|AUTO]]      select the mode (ranges in V / A)
MEASure:POWer?                                          P in W (one accepted P result)
READ? / FETCh?                                          P in W while the mode is active
FETCh:POWer:ALL?                                        "P,S,Q,PF,Vrms,Irms,F,E_Wh,seconds,Vdc,Idc"
MEASure:POWer:APParent? | :PFACtor? | :ENERgy?
[SENSe:]POWer:VOLTage:RANGe <V>|AUTO / ?                V channel range
[SENSe:]POWer:CURRent:RANGe <A>|AUTO / ?                I channel range
[SENSe:]POWer:ENERgy:RESet                              zero Wh / VAh / time
[SENSe:]POWer:BURDen <ohm> / ?                          R_path of the current range (compensation; in a DMM A function: its shunt, 3.15)
[SENSe:]POWer:PHASe <steps -127..127> / ?               SPHACAL (sign = lead/lag)
[SENSe:]POWer:PHASe:CALibrate RESistive|REACtive        self-cal (see 3.5)
[SENSe:]POWer:CYCle?                                    d and the phase period, for diagnostics
```

CONFigure? answers `"POW <vrange>,<irange>"`. The BT cmd 06/09 protocol has no
slot for a 16th mode (superseded by 3.13: the DC V byte with P); leave BT at the stock set (power only over SCPI/USBTMC and
BT SCPI).

### 3.10 Implementation plan (idea 1)

1. `hy3131.rs`: `read_rms_signed()` (38-bit sign), `configure23()` that writes
   R20..R36 with R36 (SPHACAL) before the enables while keeping the AD3-first
   order; `set_rms_stat(Stat::XY | XX | YY)` writing R34 only.
2. `hy3131_tables.rs` / new `power_rows.rs`: `fn power_row(vr: u8, ir: u8) ->
   [u8; 23]` built from f0 (PA / SMODE / ACC bytes of the V range) and f4 /
   f5_6 (R25 OPS1, R32 SOP1P, R27) with R32 |= ENOP2 | SOP2P=SENSE, R35 = 0x54
   (OP2O - AGND, IG 1.0), R23 = 0x03, R20 = 0x12 | SCMPI SENSE, R21 = 0xdd,
   R29 = 0xf8. Host test: every byte either equals the stock source row or is
   one of the listed overrides (no accidental bits).
3. `dmm.rs`: mode 16 `Power` with a `PowerState` (phase, d, EMA states, E, t0,
   ranges, lock) driven from `poll` like the AC+DC machine; autorange per
   channel with the stock thresholds; `Reading` gains `power: Option<PowerVals>`
   (P, S, PF, Vrms, Irms, E, secs, Vdc, Idc) so the UI and SCPI do not grow
   nine aux fields. Burden compensation and the continuous-current limit here.
4. `scpi.rs`: the commands of 3.9; `CONFigure?` "POW".
5. `ui.rs` / `meter.rs`: the page of 3.9 (3rd LETTERS row, energy row, badge
   rules), function picker item + wiring dialog (`dialog.rs`), strip chart of P.
   Overlap check + contact sheet (`tools/ui-render`).
6. Settings (`persist.rs`): SPHACAL, R_path per shunt, current limit, last
   ranges.
7. Emulator: the HY3131 model recognises stock rows only and reads 0 for any
   other row (README "A configuration that matches no stock row reads 0"), so it
   cannot validate the power row today. Add to `HY3131.cs` a `Power` role
   (OP2 <- SENSE -> AD3, AD2 <- OP1 <- shunt, SDO23/SVXI statistics on two
   different inputs, LPF follows SDO23, signed RMS) and the terminal-level input
   `pow:230@50,i=1.2,pf=0.8` - this is a slice of the owner's terminal-level
   model TODO. Host tests for the scaling and the phase machine run against it.
8. Device: T1..T6 of 3.8; commit the exact tree before each flash (memory rule).

Effort: 3.7 skeleton 1-2 days; B row + phase machine + SCPI 3-4 days; UI 2
days; phase cal + tests 2 days; emulator role 2 days.


### 3.11 Implemented (fw-rs, 2026-10-22; follow-ups in 3.12)

`fw-rs/src/powermeter.rs` (a child of dmm.rs: row builder, phase machine,
scaling, energy, autorange, burden, limit; host tests), `src/ui_power.rs`
(page slots, wiring dialog, Power Settings page), `src/app_power.rs`
(taps, settings words, SCPI), hooks in `dmm.rs` / `hy3131.rs` / `app.rs` /
`scpi.rs` / `meter.rs` / `persist.rs` (v6, 4 words) / `stockfont.rs`.
Against the plan of 3.10:

| plan item | as built |
|---|---|
| 1 hy3131.rs | `configure_ph(row, r36)`: the row with ENAD2 / ENAD3 cleared, INTE, **R36 (SPHACAL)**, then R34 (AD3) and R26 (AD2) - datasheet order + the AD3-first beat fix; `rms_signed` (bit 37 sign, bits 39:38 ignored), `lpf19`; the statistic switch is a one-byte R34 write |
| 2 row | `power_row(vr, k, stat)` exactly as 3.10 (6 V: the DC V r0 /10 tap bytes; 60 / 600 / 1000 V: AC V r1..r3); host test `power_row_bytes`: every byte = the stock I row's, the V row's R2A..R30 or a listed override, plus the decoded intent (OP1 on PB2 / PB1, OP2 on SENSE x1, AD2 = OP1O - PB3, AD3 = OP2O - AGND, AD1 off). Relays: PA state 0, PE = the current range's shunt in **every** write (also the fallback's AC V row) |
| 3 dmm.rs | function code **18** (16 / 17 were AC+DC already); internally the HY mode stays the AC A mode of the current range (`hy_mode` 4..6) with `pw.on`, so the button, relay and catch-all code paths stay AC A; any `set` (a function button, BT cmd 06, SCPI CONF of another function) leaves. Cycle P -> V -> P -> I, **2 results discarded** per R34 switch (`DISC_DEFAULT`, DBG:POW:DISC), each accepted part through the stock EMAs (V: AC V windows, I: AC A windows, P: 5 % relative jump, a 0.25); P = P_ac + V_dc I_dc etc. as 3.3. Calibration: I channel the stock AC A RMS gain (0xC4 + 4 ci) and LPF gain (0xF4 + 4 ci), no offsets; V channel gain 1.0 (no stock cal applies to OP2 / AD3). Readings once P, V and I were measured; every accepted result gives one. The state (`Pw`, ~0.6 KB with the T3 ring) lives in SRAM (.sram_bss), not in the App on the TCM stack |
| autorange | per channel after its own result, stock thresholds (up > 1.03 x limit, down < 0.1 x limit / 1.03); V from 600 V, I from 10 A (least burden); the I floor follows the A key's AUTO / AUTO+ (AUTO+ never enters the 47.5 ohm shunt). A range change rewrites the row, settles 550 ms, restarts the cycle; manual ranges per channel (page / SCPI), AUTO key = both auto |
| energy | Wh and VAh from the totals of the previous reading over the time since it (a range-change gap holds the last P), i64 mJ + f32 remainder (host test: 1 mW s steps on top of 1 kWh); RUN / PAUSE (energy box tap, POW:ENER:STAT), reset (long press on the side column, page, POW:ENER:RES); average P = E / t |
| burden | R_path per shunt (default the shunt: 10 mohm / 0.5 ohm / 47.5 ohm; POW:BURD sets the current range's; in a DMM A function that function's shunt, 3.15), compensation ON by default (**OFF since 3.16**, "At load" opt-in): P_L = P - I^2 R, V_L^2 = V^2 - 2 R P + R^2 I^2 (exact for any waveform: v_L = v - R i), S_L = V_L I, Q, PF from those; the burden voltage I R in its own box (amber when the compensation is off and it exceeds 1 % of V) |
| limit | continuous-current limit (default 5 A / 30 s, POW:LIM <A>,<s>, persisted; the owner should set the manual's numbers): above it a countdown in the side column, at expiry a toast and the A-JACK beep pattern while it stays above; the auto power-off (and its warning) is held while the energy integrates with I > 50 uA, the DMM rail stays on in every view |
| 4 SCPI | `CONFigure:POWer [<V>|AUTO[,<A>|AUTO]]`, `MEASure:POWer?` (P; waits through the range hunt, at most 15 s), `READ?` / `FETCh?` = P, `FETCh:POWer:ALL?` = `P,S,Q,PF,Vrms,Irms,F,E_Wh,seconds,Vdc,Idc,VAh,burden_V` (NaN = +9.91E37: the fallback's P / Q / PF / Vdc), `MEASure:POWer:APParent? / :PFACtor? / :ENERgy? / :REACtive? / :VOLTage? / :CURRent?` (latest values), `[SENSe:]POWer:VOLTage:RANGe` / `:CURRent:RANGe <value>|AUTO` / `?` (`AUTO,+6.000000E+02`), `:ENERgy:RESet`, `:ENERgy:STATe`, `:BURDen <ohm>|DEFault` / `?`, `:BURDen:STATe`, `:PHASe <-127..127>` / `?`, `:PHASe:CALibrate [REACtive]` (RESistive: refused with the reason, see 3.5 / 3.13: the cos optimum is flat; `:PHASe:CALibrate?` = the outcome since 3.13), `:CYCle?` (`d,P period ms,full set ms`), `:METHod SIMultaneous|ALTernate` / `?`, `:LIMit <A>[,<s>]` / `?`; `CONFigure?` = `"POW AUTO,AUTO"` / `"POW +6.0E+01,+6.0E+00"`, `FUNCtion?` = `"POW"`. BT keeps the stock protocol (cmd 06 / 09 reported the AC A byte and P as the value; since 3.13 the DC V byte with P) |
| 5 UI | see below |
| 6 settings | p[48..52]: SPHACAL, method, compensation (bit 9 = OFF; since 3.16 bit 10 = ON, older records load OFF), discards, I limit, time limit, R_path x 3 (0 = default); the function itself is **not** restored: a restart after a power session comes up in DC A on the 10 A shunt (a load current may still flow) |
| 7 emulator | Power role in `emu/hy3131/HY3131.cs` (both copies; X = I x shunt x OP1 gain, Y = V / k, signed XY with the phase, SPHACAL on the cross term, mid-block R34 mixing, LPF and the counter as the hardware): 88 host checks; `emu/hy3131/powtest.py` end to end over USB/IP |
| phase cal | `POW:PHAS:CAL` on a reactive load: P / S averaged over 8 results at the current SPHACAL and 48 steps away, then the Newton step that nulls it (works for C and L loads; the sign of the slope comes from the measurement) |

UI (stock DMM page slots and faces, ui-render matrix: 6 worst cases x 4
hold states x 4 alerts on the full page, every case as an update of every
other one equal to its fresh draw, the dialog at 3 limit settings, the
settings page at 18 states, 0 violations; renders p01..p08, c12):

* status bar: AUTO (both channels auto) / MANU; HOLD / A-HOLD / alerts as
  everywhere. aux1 = Vrms (the load's with the compensation), aux2 = Irms in
  the AC V / AC A formats of the ranges. Main: P in the F78 cells, unit `W`
  (`mW` below 1 W of full scale, `kW` for 1000 V x 10 A) in the stock UNIT
  face - it has no W, so `stockfont::glyph_or_twin` draws two of its V
  overlapped (also in LETTERS / HIST / the meter strip); the AC+DC glyph
  above it and on the A button; red from 30 V on the V channel as the stock V
  functions. Fallback: S in `VA`.
* history row: five boxes PF | S (VA / mVA) | Q (var) | f (Hz, the counter on
  SENSE) | Vb (burden V); F14 tags, HIST values, units the HIST face lacks
  (var) in F14. No history in this function (taps / long press do nothing).
* chart: P (the main value). Side column: Wh, VAh, the energy box (E RUN /
  PAUSE, h:m:s; tap = run / pause, long press on the column = reset), AVG
  (W), the ranges (`600V 10A`) or the countdown `I>LIM 23s` (amber) /
  `I>LIM!` (red).
* entry (the stock 6 function buttons unchanged): **long press on the A
  button** or **"Power W" on the Current Settings page** (MF in an A
  function) -> the wiring dialog: source / load boxes, the + wire with V/ohm
  on it, the return conductor broken through A (load side) and COM (source
  side), current arrows, "COM carries the load current; P > 0: I into A",
  the limit; Cancel / OK. Any function button leaves: A to DC A on the
  power meter's current range (its shunt kept), the others as the stock (the
  47.5 ohm resting shunt; a current still flowing then trips A-JACK). MF in the power meter
  -> Power Settings: V ranges + AUTO, I ranges + AUTO, P + PF / VA only
  (the fallback), Burden ON / OFF, Reset Wh, Wiring (the dialog again), OK.

Emulator results (`emu/hy3131/powtest.py`, fw-rs native over USB/IP, the
model's Power role; burden compensation off unless stated; all 11 OK):

| scenario | input | fw-rs (FETC:POW:ALL?) |
|---|---|---|
| resistive | `pow:230@50,i=1,pf=1` | P 230.02 W, S 229.95 VA, PF 1.000, 230.00 V, 0.9998 A, 50.001 Hz, 600 V / 10 A |
| PF 0.5 | `pf=0.5` | P 115.00, S 230.02, Q 199.2, PF 0.49995 |
| reactive | `ph=90` | P -0.014 W, PF -0.00006, Q 230.0 |
| reversed | `pf=-1` | P -230.01, PF -0.99993 |
| DC | `vsrc:12;isrc:2` | P 23.998, PF 0.99998, Vdc 12.000, Idc 2.000 |
| DC source | `isrc:-2` | P -23.998 |
| DC autorange | `vsrc:5;isrc:0.4` | 2.0000 W on 6 V / 600 mA |
| small AC | `pow:5@50,i=0.01` / `i=0.002,pf=0.8` | 0.05000 W on 6 V / 60 mA; 8.000 mW PF 0.800 on 6 V / 6 mA (47.5 ohm shunt) |
| burden | `vsrc:5;isrc:0.4`, POW:BURD 0.5 | P_L 1.9200 W, V_L 4.8000 V, Vb 0.200 V |
| manual | CONF:POW 60,6 | 0.500 W held on 60 V / 6 A |
| fallback | POW:METH ALT, `pf=0.5` | S 230.5..231.0 VA (+0.2 %: the AC V coupling node after each phase switch, 4 results dropped), PF n/a |
| phase cal | 1 kHz, `ph=90`, POW:PHAS 40 | PF 0.102 (6 deg) -> POW:PHAS:CAL -> PF 0.000006, SPHACAL ~0 |
| energy | 10 W DC | Wh x 3600 / s within 5 % of 10 W (also VAh) |
| SCPI | MEAS:POW? from DC V, PF 0.6 at 60 Hz | 29.88 W after the range hunt; MEAS:POW:PFAC? 0.600, READ? 30.00, CONF? `"POW AUTO,AUTO"`, POW:CYC? `2,642,1284` |
| T1 / T3 hooks | DBG:POW:STAT YY / DBG:POW:DISC 0 | ring all YY with R34 = 0x8C; ring cycling XY / YY / XY / XX every 106 ms (with the model's mid-block mixing the d = 0 values are visibly wrong: what the discards are for) |

What the emulator cannot tell (the model implements the doc's assumptions):
OP2 on SENSE, the PA8 / power-bus question, the real R34 switch cost, the
AD2 / AD3 gain match, the true phase error. That is the device session.

#### Device test plan (power build; commit the exact tree before flashing)

Tools: `tools/dm40dbg.py scpi '<cmd>'` (USBTMC). Rows: `DBG:POW:ROW?` = the
22 row bytes R20..R35 + R36 in hex and ON / OFF. Ring: `DBG:POW:RAW?` =
`n;STAT,discarded,rms,lpf,dt_ms;...` (last 16 RMS / LPF results, raw: rms =
signed RMS<37:0>, sqrt(rms) in AD codes of 1.25 V / 2^18 for XX / YY; lpf =
19-bit LPF). Overrides: `DBG:POW:POKE <index 0..21>,<decimal byte>` (R20 +
index; up to 4, `DBG:POW:POKE CLEAR`), `DBG:POW:STAT XY|XX|YY|AUTO` (freeze
R34), `DBG:POW:DISC <0..8>`. All rewrite the row (550 ms settle).

* **T1 OP2 on SENSE (go / no-go of method B), ~10 min.** AC source (e.g.
  12 V AC transformer or the generator through an amplifier) on V-COM,
  nothing in A: `CONF:POW`, `POW:VOLT:RANG 60`, `DBG:POW:STAT YY`; read
  `DBG:POW:RAW?` a few times: sqrt(rms) x 100 x 1.25 / 2^18 = Vrms, compare
  with `CONF:VOLT:AC` / `READ?` of the same source (expect within 0.5 %);
  lpf x 100 x 1.25 / 2^18 = DC part (a DC source: compare with DC V). Shorted
  leads: noise floor (sqrt(rms) in codes; the stock AC V floor is ~31 codes).
  Repeat at `POW:VOLT:RANG 6` and `600` (mains only with the usual care).
  If T1 is bad: try `DBG:POW:POKE 14,8` + `DBG:POW:POKE 15,129` (R2E / R2F
  of the I row: power bus as SGND, PA8 P), then AD3IN = RLU / PB5 (`DBG:POW:POKE
  21,68` / `21,116`); if nothing helps, `POW:METH ALT` (the fallback, VA only)
  and note it for the default.
* **T2 two different nodes, resistive and reactive, ~15 min.** 12 V AC
  transformer + power resistor (~1 A) wired as the dialog: `CONF:POW`,
  `FETC:POW:ALL?`: PF = 1.000 +- 0.005 and P = Vrms x Irms within the channel
  errors; compare V and I with the AC V / AC A functions. Reverse the current
  (swap A / COM at the load): P negative, same magnitude. Then a capacitor
  (or the transformer unloaded): PF near 0, run `POW:PHAS:CAL REAC` (~5 s)
  and read `POW:PHAS?` (the step count; expected small at 50 Hz, the doc
  estimates < 10). DC: PSU + resistor (T4): `Vdc,Idc` of FETC:POW:ALL? vs
  DC V / DC A.
* **T3 switch cost, ~5 min.** Same resistive setup: `DBG:POW:DISC 0`, read
  `DBG:POW:RAW?` repeatedly: per statistic, how many results after a switch
  differ from the steady value (XY vs the steady XY, XX vs XX, YY vs YY), and
  `dt` (106.7 ms expected). Then set the default discards to that count
  (`DISC_DEFAULT` in powermeter.rs, or `DBG:POW:DISC n`, persisted) and note
  `POW:CYC?`.
* T5 ranges / T6 burden as 3.8 (every V range x the I ranges on suitable
  sources; 5 V / 0.4 A with `POW:BURD` set to the R_path measured with the
  ohm function between A and COM). UI on the device: long press on A ->
  dialog -> OK, the page with a lamp load, MF -> Power Settings, a function
  button leaves; with a 5 A+ load (if available) the countdown and the alarm.

#### Device results (2026-10-22, build 7dce69f; T1, T3 at zero current, UI)

Setup: the MR9270S+ calibrator (no sine) on V-COM and the scope probe; the A
jack **not wired**, so T2 / T4 / T5 / T6 (and the XY / XX parts of T3) were
not possible. AC source = calibrator func 2 mode 0 value 40000, the 40 Hz
square (stock AC V 4.8068 V, DC V 4.8109 V: about 0..9.6 V on this unit, not
10.4); DC levels func 1 (1 V, 5 V, 0 V; it cannot source negative). Scale as
above: code x k x 1.25 / 2^18, k = 10 / 100 / 1000 for 6 / 60 / 600 V.
`DBG:POW:STAT YY`, last 8..16 ring entries each, sd of sqrt(rms) 0.1..1 code.

**T1: OP2 on SENSE works (method B is GO); the V channel needs a per-range
gain and an LPF zero.** Default row `12 DD 15 03 52 40 80 2D 31 F8 20 00 00
09 28 A0 80 C7 8E AC 8C 54 00` (R2E 0x28, R2F 0xA0, R35 0x54).

| range | source | sqrt(rms) | lpf | V (AC part) | vs stock | V_dc from lpf - 1080 | vs DC V |
|---|---|---|---|---|---|---|---|
| 60 V | square 40 Hz | 9916.3 | 11005 | 4.7285 | **-1.63 %** | 4.7328 | -1.62 % |
| 60 V | DC 1 V / 5 V | 53 / 42 (floor) | 3144 / 11389 | - | - | slope 2062.2 codes/V (ideal 2097.2) | **-1.67 %** |
| 6 V | square 40 Hz | 98522 | 99855 | 4.6979 | -2.27 % (the square's 0.96 V peak at the /10 tap is past the +0.7 V op amp headroom of 3.2) | - | - |
| 6 V | DC 1 V / 5 V | 43 / 60 | 21620 / 103759 | - | - | slope 20542 codes/V (ideal 20972) | **-2.05 %** |
| 600 V | square 40 Hz | 1004.7 | 2085.7 | 4.7907 | **-0.33 %** | 4.7956 | -0.32 % |

* The literal 0.5 % criterion with the channel on gain 1.0 is met only on
  600 V. But the AC part (after the HPF) and the offset-corrected DC part
  agree to ~0.1 % on every range, 1 V and 5 V DC give the same slope, and
  the result is the same with the square at the calibrator's other
  frequency settings: a constant per-range gain of the divider tap + OP2 x1
  + AD3 (0.9795 / 0.9833 / 0.9967 for 6 / 60 / 600 V), the per-range V cal
  the plan expected (3.3: "start with gain 1.0"), not a wrong node.
* **LPF zero offset ~1080 codes** on Y (0 V and open leads, the same on 6 /
  60 / 600 V, i.e. after the divider: ~5.15 mV at AD3) = a phantom V_dc of
  0.052 / 0.515 / 5.15 V. It is not compensated: on the 40 Hz square the page
  shows Vrms 7.062 V = hypot(4.728, 5.246), the true AC+DC rms is ~6.80 V
  (+3.9 %); with the leads open Vrms 0.051 V on 6 V. The X LPF has its own
  ~888 codes: the page's I = 8.9 uA with nothing in A is exactly that offset
  (888 x 1.25 / 2^18 / 475 ohm on the 600 uA range), and V_dc x I_dc gives the
  P = 0.047 mW. Both LPF zeros need subtracting (a zero at entry with the
  inputs at rest, or a fixed per-unit constant). The LPF polarity was not
  checked (no negative source).
* Noise floor, sqrt(rms) of YY: 47 codes on 60 V (22 mV rms), 50 codes on 6 V
  (2.4 mV), the same with 0 V (low impedance) and open leads; the stock AC V
  floor is ~31 codes.

POKE fallbacks (60 V, the square; each 3.5 s after the write; `POKE CLEAR` restored
the default bit for bit):

| override | sqrt(rms) | lpf | V (AC part) | verdict |
|---|---|---|---|---|
| none | 9916.9 | 11000 | 4.7288 | reference |
| `14,8` + `15,129` (R2E 0x08, R2F 0x81) | 47.1 | 1090 | 0.022 | **kills the signal** (floor): the stock I row's R2E / R2F are required |
| `21,68` (R35 0x44, AD3IN = RLU) | 9910.4 | 10825 | 4.7257 | same as SENSE (-0.07 %), LPF offset ~ -175 codes |
| `21,116` (R35 0x74, PB5) | 9859.6 | 10864 | 4.7014 | -0.58 % |

Keep the default row; no override fixes the gain (it is not a node problem).

**T3 (zero current, 60 V, the square; `DBG:POW:DISC 0`, then the default 2).**
`dt` 105 / 110 ms alternating (one 215 ms). With d = 0, `POW:CYC?` =
`0,214,428`; with 2 `2,642,1284`.

* YY right after an XY block: the first result equals the steady value
  within 0.003 % (98.34 M +- 4 k); the switch contamination is a few k counts
  absolute, invisible against 98 M. So no proportional block mixing: the R34
  write appears to restart the block (or the result straddling it is clean).
* XX right after XY (at the floor, steady `STAT XX` = 2200..2350): the 1st
  and 2nd results are off and often negative (-5188, -969, -3129, 137; 1866,
  -1758, 691, -898; impossible for a sum of squares = the mixed-block mark),
  the 3rd matches (1920..2275). So **d = 2 is needed** at least at the floor;
  whether the transient matters with a real current (XX >> 5 k) and for XY
  is not assessable without current: keep `DISC_DEFAULT` = 2 until T3 with a
  load. Side effect of d = 0: the garbage XX made the I autorange hunt (the X
  LPF flipping 155 <-> 880 codes = shunt changes, 640 ms gaps = the 550 ms
  settle). An artefact of d = 0 only; `DBG:POW:DISC 2` restored (persisted).

**UI** (screenshots in the session scratchpad, `dev-pow/01..09`): power page
on the square: AUTO RUN, aux 7.062 V / 8.95 uA, P 0.047 mW (synthesised W
face), PF 0.742 (noise / offset ratio, meaningless at the floor) | S 0.06 mVA
| Q 0.04 mvar | **F `-- Hz`** | Vb 0.000 V, Wh / VAh 0.0000, E RUN with the
time, AVG, `60V 600uA`. **f is 0 in power mode** (FETC:POW:ALL? field 7 =
0) while AC V on the same signal shows 40.00 Hz 50.0 %: the counter on SENSE
does not run with the power row (the emulator shows f): open. Long press on
the A button (DBG:DRAG hold 1.2 s) -> the wiring dialog (source / load, COM
V A, "Max 10 A; above 5.0 A: 30 s"); OK -> `CONF? "POW AUTO,AUTO"`; the page
stays empty (no value, no AUTO / RUN, the DC V side column) for a few
seconds after the entry while the I channel walks down from 10 A (550 ms per
range) - cosmetic. MF -> Power Settings (V / A ranges + AUTO, P + PF / VA
only, Burden ON, Reset Wh, Wiring, OK); OK -> back to the page. CONF:VOLT:DC
leaves (DC V AUTO+).

Not done here (need current through A): T2 (PF, sign, phase cal), T4 DC
power, T5 ranges, T6 burden, the limit countdown, XY / XX switch cost.
Follow-ups for the build: per-range V gain (or a user V cal), LPF zero on X
and Y, f in power mode.

### 3.12 Follow-ups (fw-rs, 2026-10-22): V gain, zeros, f, DC method, 7th button

Built on 853f945 against the device results of 3.11. Parts of 3.11 this
replaces: the entry (now the W button), "the function is not restored",
"V channel gain 1.0", "f on SENSE", "DC parts from the LPFs only".

**V gain per range (factory-derived default + self-cal).** The V channel's
raw gain error is the divider tap's (plus OP2 / AD3), and the factory record
already measured the same taps through the stock AC V path (OP1 x1 -> AD2):

| range | tap | default gain (`pw::v_gain_factory`) | dumped unit | T1 needs (1 / raw) | residual |
|---|---|---|---|---|---|
| 6 V | PA7 /10 (DC V r0's) | DC V r0 gain (cal 0x04) x AC V r1 / DC V r1 (0x38 / 0x08: the AD2-path / AD1-path ratio on a tap both measure) | 1.02065 | 1.0209 | -0.03 % |
| 60 V | PA6 /100 | AC V r1 (0x38) | 1.01614 | 1.0170 | -0.08 % |
| 600 V | PA5 /1000 | AC V r2 (0x3C) | 1.00453 | 1.0033 | +0.12 % |
| 1000 V | PA4+PA5 | AC V r3 (0x40) | 0.99815 | - | (not measured) |

(The AC V / DC V ratio is 1.00849 on both r1 and r2 of the dump: a constant
AD2-vs-AD1 converter factor, which is why the 6 V derivation works.) The
dump (`private/memrw-kit-test/quick.img`, 2026-10-13) is assumed to be the
T1 unit: its AC V r1 gain reproduces the T1 ratio stock / raw (1.0166) to
0.04 %. So most units need no self-cal; the gains are applied as
`raw x gain` in `pw_take` (Vrms and V_dc). A missing record or a derived
gain outside 0.9..1.1 gives 1.0.

`POW:VOLT:CAL` / Power Settings "V cal": steady AC or DC on V-COM, the V range
fixed during the job (~8 s AC, ~3 s DC): power row (R34 frozen YY, 1 dropped
+ 3 averaged) -> the stock reference row of the same range -> power row ->
reference; the kind follows the larger part of the first round: **DC** = the
stock DC V row (AD1, `poll_dcv` scaling with cal 0x04 / 0x14 and DCV_NEGK)
vs V_dc (zero applied); **AC** = the stock AC V row (cal 0x34 / 0x44, 2 s
settle: the AC-coupled node starts from the power row's OP1O level, the
emulator showed +0.46 % with 0.6 s) vs V_ac. gain = mean(ref / raw) of the
two rounds, stored per range (persisted). Refusals (`POW:VOLT:CAL?` =
`<result>,<range V>,<gain in force>,SELF|FACT`): SMALL (< 10 % of the range),
BIG (> 70 % AC: the 6 V tap clips at +0.7 V; > 100 % DC; or, either kind,
|DC| + sqrt(2) x AC RMS above the range: the peak clipped, device f3aea2f
calibrated a 0..9.6 V square on 6 V by its DC part), UNSTABLE (the two
rounds' ratios or raw levels differ > 0.3 %), RANGE (gain outside 0.9..1.1),
METHOD (the VA-only fallback), ABORT (a range / function change).
`POW:VOLT:CAL DEF` returns the range to the factory-derived gain,
`POW:VOLT:GAIN?` lists the 4 gains in force.

**LPF zeros.** Internal zero (datasheet mux tables): `SOP2P = AGND` (011) and
`SOP1P = AGND` exist, so OP2 + AD3 (and OP1 + AD2) offsets *can* be measured
without the leads. Not used as the default: AD2IN is PB3 (not AGND) in the
I row, and the T1 POKE test showed the reference nodes themselves differ by
~0.7 mV (AD3IN = RLU: LPF -175 codes, PB5: -136), so an AGND-input zero
leaves a residual of that order (~150 codes = 0.07 V phantom on 60 V, 0.7 V
on 600 V) - and the X zeros differ per range (155 / 880 codes seen in T3)
where the internal zero would need a row rewrite + settle per range change.
Built instead: **`POW:ZERO`** / "Zero" (leads open, nothing in A, ~6 s):
the six current ranges in turn (R34 XX: the X LPF; 1 dropped + 3
averaged) and the V channel once (YY; the zero is after the divider, the
same on every V range), refused (SIGNAL) when sqrt(RMS) > 400 codes (AC
present) or |LPF - old zero| > 4000 codes (DC present); stored per unit
(persisted), subtracted from V_dc / I_dc only (the RMS statistics are after
the HPF). `POW:ZERO?` = `<result>,SET|NONE,<Y>,<X 600 uA..10 A>`,
`POW:ZERO CLE` clears. Polarity: `lpf19` sign-extends bit 18; the emulator
runs -5 V / 0.4 A to P = -2.000 W, Vdc -5.000 (scenario `zero`).
Device hook to settle the internal-zero question later: `DBG:POW:STAT YY`,
`DBG:POW:POKE 18,<0x80 | 0x30 | R32 low nibble>` (SOP2P = AGND) and compare
the LPF with the open-lead zero of `POW:ZERO?`.

**I_dc gain.** The DC part of the current runs through the same shunt / OP1
/ AD2 as the AC part; the record's LPF gains (0xF4..) are the default 1.0 on
the dumped unit while its AC A RMS gains are 0.954..1.036, i.e. never
calibrated. B now applies the RMS gain to I_dc when the LPF gain is exactly
1.0 (INFERRED: device check T4 = Idc of B vs the DC A function).

**Frequency.** Cause of f = 0 in T1: the comparator window R21 0xDD is
+-20.8 mV around AGND; SENSE is DC-coupled, and the calibrator's 40 Hz
square is unipolar (0..9.6 V = 0..96 mV on the 60 V tap): it crosses +20.8
mV but never -20.8 mV, the latch never resets (AC V counts on PB4, the
AC-coupled node). Real mains (bipolar) would have counted. The emulator
missed it because its counter tested only the AC peak; it now tests dc +-
peak against both thresholds on DC-coupled nodes (power role). Fix
(`pw_cmp_select`, after each V / I result): `cmp_window(V_dc / k, V_ac / k)`
picks the nearest R21 thresholds around the tap's DC (AGNDP / AGNDN taps,
the VDSC taps where those have no level; at least 6 mV off the DC - an
un-zeroed unit's ~5 mV Y phantom keeps 0xDD -, inside the swing); a bipolar
signal keeps 0xDD; the T1 square gets 0xBF (+62.5 / +20.8 mV). No window (pure DC or too small an AC part) but >= 25 mV rms of
current AC at OP1O: the counter on PB4 = the current's frequency
(INFERRED from the stock AC A rows' R20 0xF2). Else "--". A V range change
returns to SENSE / 0xDD. The sentinel is off in the power meter
(`sent.configured(Kind::Off)`), so R20 / R21 are the counter's alone;
sentinel.rs changes do not touch this path.

**Entry ranging.** `pw_apply` dropped every part on any range change, and a
range change restarted the cycle at P: each I autorange step cost a whole
P V P I set (~1.8 s; the T1 page stayed blank ~9 s from 10 A down to 600
uA). Now a change of one channel keeps the other channel's parts and starts
with its own statistic (XX / YY; DC method: its own row): ~0.9 s per step.
Until the first full set the page shows the V reading in the aux1 slot and
"I ranging..." in the main area (`ui::pw::ranging`, also "Zeroing: leads
open" / "V calibration..." during the jobs).

**Discards.** `DISC_DEFAULT` stays 2 (T3: YY clean from the 1st result, XX
at the floor needs 2). XY and XX switch costs with a real current are
untested (T3 with a load, device list below).

**DC coupling (owner: "DC power meter too"; round 3: one fixed row).**
`POW:COUP DC|ACDC|AUTO`, the Settings row, or the active W button; the W
button enters with the last coupling. The first build alternated the stock
DC V and DC A rows (AD1 for both); the owner asked why the row keeps being
rewritten, so it was replaced (the alternating code is gone: the fixed row
is faster, simultaneous and needs no relay or row writes; nothing it did
better remains):

| | AC+DC (method B) | DC (`dc_row`) |
|---|---|---|
| row | the power row, R34 P V P I | **one row**: the power row frozen at YY plus AD1 as the stock DC A row of the current range sets it (R22 OSR 8192 chopped, R23 ENAD1, R24 AD1 = OP1O - PB3, R28 refs PB6 / AGND, R33 OP1 chopper / pre-filter); host test `dc_row_bytes`: every byte is the power row's or the DC A row's |
| legality | - | AD2 = OP1O - PB3 (as in B), AD3 = OP2O - AGND: AD2IN != AD3IN (note 8); OP1 on the shunt is read by AD1 and AD2 at once (two converters on one node); written by `configure_dc` with all three converters off, then AD3, AD2 and **AD1 last**, so AD2 / AD3 never switch while AD1 converts (note 7); nothing is written while measuring (a range change rewrites the row, 550 ms settle) |
| relays | PA 0, PE = the I range's shunt | the same (no relay moves at all) |
| V | OP2 -> AD3, RMS (AC) + LPF (DC), factory-derived gain, POW:ZERO | the same channel: Y LPF = V_dc (gain, Y zero), Y RMS = the ripple (AUTO only) |
| I | OP1 -> AD2 LPF, AC A gain, POW:ZERO | AD1 24 bit, the `poll_dca` scaling with the factory DC A gain (0x94); offset: the row's own AD1 zero of POW:ZERO (note 7: the stock's factory offset 0xAC was taken with AD2 / AD3 off; it is used only until POW:ZERO ran) |
| P | P_ac + V_dc I_dc | V_dc I_dc (signed), `totals` (burden compensation, S = |P|, PF +-1, Q 0) |
| cadence | full set 1.28 s | a reading per AD1 result (100 ms), V updated every 107 ms, simultaneous |
| shown | PF / S / Q / f | PF / S / Q / f "--", the DC glyph above W and on the button |

Expected accuracy (DC; manual specs of the stock functions and the T1
numbers): **I** = the stock DC A path: +-(0.15..0.2 % + 5 d), plus whatever
offset note 7 adds (removed by POW:ZERO). **V via the AD3 LPF**: resolution
1 LPF code = 1.25 V / 2^18 at the tap = 0.48 mV on 6 V, 4.8 mV on 60 V, 48
mV on 600 V (the DC V function on AD1 resolves 0.1 mV / 1 mV / 10 mV); gain
the factory-derived per-range gain (T1 residual <= 0.12 %) or a V cal (~0.05
%); zero 1080 codes (0.5 V on 60 V) until POW:ZERO, then ~1 code; so ~+-(0.1
.. 0.2 % + 2 LPF codes) against the DC V function's +-(0.03 % + 5 d) on AD1.
P ~+-0.3 % + digits. The AD1 method of the first build had the DC V accuracy
for V too, at the price of row writes every 0.45 s and V / I not at the
same instant. AUTO: B until a full set is DC-like (AC parts < 1 % of the DC
parts + ~100 codes); in the DC row back to B when the voltage's ripple (the
Y RMS) exceeds 3 % of V_dc + ~100 codes (an AC current with a pure DC
voltage is not seen in the DC row). `POW:COUP?` answers `AUTO,DC` /
`AUTO,ACDC`. POW:ZERO now also takes the DC row's AD1 zero per current
range (~12 s in all); persist v9 (62 words: p[59..62] the 6 AD1 zeros).

Emulator: the DC row is a power row with AD1 on the current (the stock DC A
calibration folded in; `powz:1` adds 300 codes of AD1 zero). powtest
`dcad1`: 12 V 2 A 23.992 W, reversed -23.992 W, 5 V 4 mA on 6 mA 20.000 mW,
5 V 200 uA on 600 uA 1.0000 mW; `dcburden` 1.9200 W, V_L 4.8000; `auto` DC
-> AUTO,DC 23.994 W, mains -> AUTO,ACDC 230.00 W; `zero` with the AD1 zeros.

**7th function button.** Owner request: the power meter as a full function.
The DMM row (and the combined view's band) has 7 buttons of 64 px at a 68 px
pitch from x 4 (4 px gaps, 4 px to both edges); every button centres its
glyph run (V / A + the DC / AC / AC+DC glyph 3 px apart, ohm (+ ON tag),
cap, diode + continuity, Hz + degC 3 px apart - the stock 76 px offsets do
not fit), W = the LETTERS V twinned (`glyph_or_twin`) as on the page. In the
power meter the V / A buttons show their plain DC glyph. Hit boxes: the 4 px
gaps split 2 / 2 (no dead pixels between buttons; touch-test 0 failures).
The W button opens the wiring dialog until it was confirmed once
(persisted), then enters directly; Power Settings > Wiring shows it again,
the long press on A stays as a shortcut, the "Power W" button of the Current
Settings page is gone. A reboot in the power meter restarts it (energy
reset, `pw_enter`); BT still reports the AC A byte with P as the value
(3.13: now the DC V byte).

**Persist v7** (58 words): the power block grows to 10 words (p[48..58]: the
4 V gains as u16, the Y zero + flags (zeros valid, wiring dialog seen,
coupling), the 6 X zeros); a v6 record loads zero-extended (= factory gains,
no zeros, dialog not seen, AC).

**Emulator** (both HY3131.cs / DM40Frontend.cs copies identical): Y gain =
the factory-derived tap gain (inverse, so fw-rs reads true) x `powvg:`;
`powz:1` = the device's LPF zeros (Y +1080, X +888 x10 / +155 x1); the
power role's counter on SENSE (dc + AC against the window) and PB4 (the
current); the DC row (round 3) has AD1 on the current; the f8_9 DC A rows fold the uA calibration when PE selects the
47.5 ohm shunt (the stock mode 9; before: always mA, +3 % on the uA ranges).
Host tests 306 OK (unipolar square f = 0 at 0xDD / 40 Hz at 0xBF, PB4 50 Hz,
powz zeros, powvg). `emu/hy3131/powtest.py` 17 scenarios OK:

| scenario | result |
|---|---|
| zero | no POW:ZERO: V 0.0526 V, I 8.92 uA (= the device's phantoms); after: V 0.0015 (noise), zeros 1080 / 888,155,... ; DC 5 V 0.4 A 1.99997 W; -5 V: P -1.99997 W, Vdc -4.9999 |
| vcal | powvg 0.99: V 29.700 -> POW:VOLT:CAL (AC ref) -> 29.9999; powvg 0.98 DC 20 V -> POW:VOLT:CAL (DC ref) -> 19.9999; 0.2 V on 60 V: SMALL |
| freq | unipolar 40 Hz square: F 40.0007; 12 V DC + 3 A AC 50 Hz: F 50.0008 (PB4) |
| dcad1 | (first build, alternating rows) 12 V 2 A: 24.004 W; reversed -24.0025 W; 5 V 4 mA on 6 mA: 0.02000 W; 5 V 200 uA on 600 uA: 1.000 mW; the DC row: see "DC coupling" below |
| dcburden | 5 V 0.4 A, R 0.5: P_L 1.92006 W, V_L 4.8001, Vb 0.200 |
| auto | 12 V 2 A -> AUTO,DC 23.999 W; mains 230 V 1 A -> AUTO,AC(DC) 230.00 W 50 Hz |
| the 11 of 3.11 | unchanged (mains, dc, small, burden, manual, alt, phasecal, energy, t1-yy, t3-cycle, scpi) |

UI (ui-render: 0 violations over 22996 cases incl. the 7-button rows in
every function, the power page in the DC method, the ranging texts, the
settings status line in 13 job states; touch-test 789 cases 0 failures):
renders p07 (settings + status), p09 (I ranging), p10 (refused V cal), p11
(DC method).

**Power page, owner round 2 (2026-10-22).**

* **W glyph**: the stock DMM faces have no W; the two overlapped V read
  badly. `src/stockfont_w.rs` (generated by `tools/mkwglyph.py`) holds a
  real W for UNIT, LETTERS and SC_DIGITS, rasterised from FreeSans Bold
  (of the installed faces the closest to the stock Helvetica-style bold: mean
  alpha error 0.77..0.98 / 15 over A H K M N V) through each font's own V
  ink box, 4 bpp, advance = width + 1; HIST (1 bpp pixel font) gets its M
  mirrored top to bottom. `glyph_or_twin` uses it; the twin stays only as
  the fallback for other fonts.
* **Coupling, the stock way** (owner, round 3): a tap on the active W
  button steps the coupling as a tap on the active V / A button steps DC ->
  AC -> AC+DC: **DC -> AC+DC -> AUTO -> DC** (superseded by 3.14: DC -> AC
  -> AC+DC, AUTO gone) (`pw::next_coupling`; also
  the locked-key path and the combined band). "AC+DC" is method B (the RMS
  of AC and DC; the old name "AC" stays a `POW:COUP` alias), "DC" the DC
  method, AUTO the last state. The page shows the stock glyph of the method
  in use above the unit (DC / AC+DC) with "AUTO" left of it in AUTO; the W
  button carries the same glyph as the V / A buttons carry theirs. The
  Power Settings page has a DC / AC+DC / AUTO row in the Current Settings
  page's DC / AC / AC+DC style; Burden is a cell in the third row, the
  VA-only fallback (`POW:METH ALT`) is SCPI only (shown in the status line).
  `POW:COUP DC|ACDC|AUTO` (AC = ACDC), `POW:COUP?` = `DC` / `ACDC` /
  `AUTO,DC` / `AUTO,ACDC`. The separate chip of round 2 is gone.
* **Energy** (owner, round 3: "V and I on top, the Wh counter below"): the
  aux row is V and I again; the energy is the first, wider box of the lower
  row (E | PF | S | Q | f: 116 / 78 / 90 / 90 / 84 px): Wh with mWh below 1
  Wh and kWh from 1000 Wh, 5 digits (`energy_format`; as many decimals as
  the box holds), VAh in the fallback. A tap on the main value cycles P ->
  Wh -> VAh (display only: `READ?`, BT and the Reading's SI value stay P /
  S; the chart follows the main readout, cleared on the switch); with the
  energy in the main readout the box shows P. The energy box in the side
  column (RUN / PAUSE, the time) stays: tap = run / pause; a long press on
  the energy box (lower row) or the side column opens "Reset energy?" (No /
  Yes, cancels after 10 s); Power Settings "Reset Wh" and `POW:ENER:RES`
  reset directly. The burden voltage moved into the side column.
* **Energy over a power cycle**: not persisted; `pw_enter` resets it, so a
  reboot (the power meter is restored) starts at 0 Wh. Persisting it would
  need the backup-SRAM record written per accepted reading - only if the
  owner wants it.
* Renders p12 (main = kWh, AUTO), p13 (main = VAh, DC), p14 (the reset
  confirm); ui-render 0 violations over 24899 cases; touch-test 0
  failures.

#### Device re-check list (this build)

1. Open leads, nothing in A: `CONF:POW`, then `POW:ZERO`, `POW:ZERO?` -> OK,
   Y ~1080, X per range (expect ~880 on the x10 ranges, ~155 on x1); page:
   V ~0, I ~0, P ~0.
2. 1 V and 5 V DC on 6 V (`POW:VOLT:RANG 6`): Vdc vs DC V within 0.1 %
   before any cal (factory-derived 1.02065); `POW:VOLT:CAL`, `POW:VOLT:CAL?`
   -> stored gain vs 1.0209; same on 60 V with 5 V (1.0170) and the 40 Hz
   square (AC reference).
3. The 40 Hz square on 60 V: f = 40 Hz (R21 0xBF in `DBG:POW:ROW?`); on the
   current 7dce69f build the theory can be checked first: `DBG:POW:POKE
   1,191` (R21 0xBF; f in `FETC:POW:ALL?` field 7 should become 40).
4. Entry from the W button: the dialog once, then direct; the page shows V
   within ~1 s and "I ranging..." until P; MF > Power Settings: Zero / V cal
   / coupling buttons and the status line; a reboot comes back in W (energy
   0). On the page: the W glyphs, a tap on the active W button cycles DC /
   AC+DC / AUTO (glyph on the button and above the unit), a tap on the value
   P / Wh / VAh, a long press on the E box asks "Reset energy?".
5. `POW:COUP DC` with the calibrator's DC on V-COM (A open): Vdc vs DC V
   (expect 0.1..0.2 % + 2 LPF codes after POW:ZERO), I ~0 after POW:ZERO
   (`POW:ZERO?` fields 9..14 = the DC row's AD1 zeros: the note-7 offset);
   `DBG:POW:ROW?` = the DC row, a reading every ~100 ms; `POW:COUP AUTO`
   with the square: back to AC+DC (the ripple). With current: Idc vs the
   DC A function on the same range (T4).
6. With current through A (T2 / T3 / T4 of 3.11): Idc of B vs DC A (the
   RMS gain now applied to I_dc), the DC method's P vs V x I of the stock
   functions, XY / XX switch cost with a real current (DISC 2 kept), PB4 f
   with a current-only AC signal.

### 3.13 The TODO remainder (fw-rs, 2026-10-22): BT, W limits, phase cal

**Bluetooth: the stock phone protocol.** The protocol (docs/bt-protocol.md,
`firmware/re/analysis/bt_meas_v2_6.h`) has no unit field: the phone app
derives the unit from the mode byte's group (V group: base mV, A group:
base uA, ohm, nF, Hz, ...) and the format byte's x1000 prefix, and the mode
byte has no free group for a 16th function (groups 6 / 7 are undefined: the
stock runs cmd 06 with an uninitialised argument for them). Options:

| mapping | what the app shows for 230.02 W | verdict |
|---|---|---|
| the AC A byte with P as the value (3.11..3.12) | P in uA ("230.02 uA": the A group's base, prefix 0 for "W"), V as uA too | wrong unit and wrong magnitude |
| AC A with Irms (a right unit) | 1.0000 A; value 2 = f | no power at all: a phone log of a power session loses its point |
| AC V with Vrms, value 3 = PF x 100 as "duty %" | 230.00 V, 50.00 Hz, 99.8 % | no P; PF disguised as duty |
| **DC V byte, P as a V-group value** (chosen) | "230.02 V" | the digits, decimals, sign (a source reads negative) and the m / k scale of P are right; only the letter is V |

Built: `Dmm::mode_byte` in the power meter = the DC V byte with the V
channel's range field (5 = auto, 1..4 = manual 6 / 60 / 600 / 1000 V:
0x28 auto); cmd 09's main value = `pw::bt_main` (P, S in the VA-only
fallback; mW -> "mV", W -> "V", kW -> W with 0 decimals; at most 65534
counts, a decimal dropped when it does not fit; OL -> 0xFFFF), value 2 / 3
= 0xFFFF as in a stock DC V frame. The value follows HOLD (the shown
reading) and stays P while the readout shows Wh / VAh. cmd 06 with that
same byte (the app re-sending the mode it was told) keeps the power meter
(`apply_mode_byte`); any other byte leaves it like a function button. BT
SCPI (bt_scpi.rs) runs the full USBTMC command set except `DBG:*` (and the
file / recording commands main.rs keeps USB only), so `CONF:POW`,
`FETC:POW:ALL?`, `MEAS:POW...?`, `[SENS:]POW:...` (incl. `POW:PHAS:CAL`,
`POW:ZERO`, `POW:VOLT:CAL`) and `CALC:LIM` work over BT unchanged; powtest
`bt` checks FETC:POW:ALL? / POW:VOLT:RANG? over BT next to cmd 09 / 06.

**Limits in W.** `meter::LIM_SLOTS` 9 -> 10: slot 9 "Power" (`LIM_POWER`,
`lim_slot(18)`), compared with the Reading's SI value = P (S in the
fallback), also while the main readout shows Wh / VAh. `CALC:LIM:UPP / LOW /
STAT / CLE / FAIL?` work in the power meter (the "Settings conflict"
refusal is gone); the Hold & Limits page opens from Power Settings
("Limits", the bottom row is now Reset Wh / Wiring / Limits / OK at 110 px)
and is titled "Hold & Limits: Power", values typed / shown in the unit of P
(W / mW / kW), "Upper / Lower = reading" take P. The fail beep and the
OK / HI / LO badge are the other functions'; the badge takes the slot left
of the coupling glyph that shows "AUTO" with the AUTO coupling (the method
glyph above the unit still shows DC / AC+DC). Persist v10: the slot's two
values at p[62..64] (`persist::W_LIM_W`), its bits next to the others' in
p[18] / p[19]. Energy limits (Wh) are not offered: the energy only grows
in a session, so a "limit" there is a one-shot budget alarm rather than a
pass / fail band; worth a separate setting if the owner wants it. A
channel over range counts as OL for the limits (HI with an upper limit)
also while the readout shows Wh / VAh.

**Phase calibration on a resistive load: stays refused.** With PF = 1, P /
S = cos(phi) and the residual phase to find is phi ~ 0, where the slope of
cos is ~0. One SPHACAL step is 0.407 us = 0.0073 deg at 50 Hz: at the
optimum it moves P / S by ~1e-8, and the whole +-127 span (+-0.93 deg)
moves it by ~3e-5 around the optimum. The alternatives of 3.5 / the TODO:

* maximise P / S over the steps: the flat top above;
* a symmetric two-point or least-squares fit over the steps (the linear
  term of cos(phi0 + k s) is -k phi0 s): to resolve phi0 = 0.05 deg the
  fit must see P / S differences of ~3e-5, but every 107 ms block carries
  a ~1e-2 ripple (5.33 cycles per block, 3.5) and V, I, P come from
  different blocks (the mains amplitude moves ~1e-3 between them):
  ~1e5..1e6 results = hours. Not usable;
* derive it from the AD2 / AD3 OSR group delay: both decimators run the
  same OSR, so their group delays are equal and cancel; what SPHACAL
  corrects is the analog paths (OP1 on the shunt vs OP2 on the divider and
  its ACC compensation), which only a measurement shows;
* a known PF = 1 load gives |phi| through 1 - PF = phi^2 / 2 = 4e-5 at 0.5
  deg: below the channel gain errors (~1e-3).

The reactive method stays: at PF ~ 0 the slope is the largest (d(P/S) /
dphi = 1), 48 steps move P / S by 6e-3 at 50 Hz. `POW:PHAS:CAL RES` answers
`-100,"Settings conflict; RES: PF 1 optimum is flat, use REACtive (C or L
load, PF ~0)"` in SYST:ERR?. The reactive run now refuses instead of
storing garbage: |P / S| > 0.32 at the start (sin 18.6 deg, the SPHACAL span
at 1 kHz: a resistive or mixed load) -> NOTREACTIVE, a Newton null outside
+-127 -> RANGE, no slope -> FLAT, SPHACAL kept in each; it needs method B
(the DC method / VA-only fallback refuse with "Settings conflict; phase
cal needs ..."). New query `POW:PHAS:CAL?` = `<NONE|RUN|OK|FLAT|NOTREACTIVE|RANGE>,<SPHACAL>`.

**The rest of the TODO block.** "Tap on P cycles P / S / PF / E" is
superseded by the owner's round 3 (3.12: the tap cycles P / Wh / VAh; S,
PF, Q, f are in the boxes below). Left, all needing a device: the re-check
list of 3.12 (item 6: T2 / T3 / T4 with current through A), T5 / T6 of
3.8, and the checks below.

Emulator (`emu/hy3131/powtest.py`, new scenarios): `limits` (CALC:LIM:UPP
100 / LOW 10: FAIL 1 at 230 W, 0 at 50 W), `phasecal-ref` (RES refused with
the reason; REAC on a resistive load -> NOTREACTIVE, SPHACAL 0; then on a
1 kHz reactive load -> OK), `bt` (run.py --bt-port: cmd 09 = 0x28 and P 115
"V" at PF 0.5, value 2 / 3 FFFF, BT SCPI FETC:POW:ALL? = the same P;
cmd 06 0x28 keeps FUNC? "POW", -24 W after it). ui-render: the badge in
every power case x 2 couplings x 3 verdicts, every verdict change as an
update equal to its fresh draw, Hold & Limits: Power (p17 / p18 renders),
Power Settings with the new bottom row (p07); 0 violations. All 21 powtest
scenarios OK (the 18 earlier ones unchanged). Build: stack headroom 24.8 KB,
region B free 23.0 KB (`App::scpi` is now `#[inline(never)]`: with the new
arms LLVM folded its ~10 KB into main.rs `run`), SRAM free 12.9 KB.

#### Device check list (this build)

1. Phone app (stock DM40 app) in the power meter: the main value shows P
   with a "V" unit (sign for a source, mW as mV); value 2 / 3 empty; the
   app does not leave the power meter by itself (if it does: it sends a
   cmd 06 other than the reported byte - capture it; also try with a manual
   V range on the meter first: the byte is then 0x08..0x20, and an app that
   re-sends an older 0x28 would leave the power meter). HOLD freezes it.
2. MF > Power Settings > Limits: Upper 100 W with a ~200 W load (or 1 W
   with a small one): HI badge (in place of "AUTO" with AUTO coupling),
   the fail beep once, `CALC:LIM:FAIL?` 1; the load below: OK / 0; the
   limit survives a reboot (persist v10) and the other functions' limits
   are unchanged after the update from v9.
3. `POW:PHAS:CAL RES` -> SYST:ERR? text; `POW:PHAS:CAL` on the resistive
   T2 setup -> `POW:PHAS:CAL?` NOTREACTIVE, `POW:PHAS?` unchanged; on a
   capacitor -> OK,<steps> (expected small at 50 Hz).
4. Over BT SCPI (tools/dm40bt_scpi.py): `CONF:POW`, `FETC:POW:ALL?`,
   `CALC:LIM:UPP 100`, `CALC:LIM:FAIL?`.

### 3.14 Owner round 4 (fw-rs, 2026-10-22): no false OVERLOAD, DC / AC / AC+DC, a readable settings page

**False OVERLOAD with ~3 A (owner).** The I channel autoranges across all
three shunts (it enters on 10 A and walks down), but the doc 4.3 OVERLOAD
rule (`sentinel::overload_trips`: the uA / mA shunt over range, manual or
the shunt's top range in auto) also ran on it: after a low current had left
the channel on the mA shunt, 3 A on 600 mA tripped the red OVLD + beep +
"Overload: 10 A shunt selected" instead of the normal step up. Reproduced in
the emulator (senttest `ovl-pow-auto-ma`: OVERLOAD from 14 s on the old
build). Now (`pw_autorange`):

| I range | over range on the uA / mA shunt (\|I\| > 1.03 x the range) |
|---|---|
| AUTO | a normal up-range: **one hop to the A shunt's 6 A range** (ik 4) at the first over-range result, from any uA / mA range (also r0: the 47.5 ohm / 0.5 ohm parts have no fuse of their own, so no intermediate step on them); no latch, no alert, no beep; the walk down follows as usual (> 6.18 A: one more normal step to 10 A) |
| manual | OVERLOAD as before (red, latched on 10 A manual, released by a user choice or 5 s below half the manual range) |

Timing: in method B the I channel is judged on its XX result, so the hop
comes at most one P V P I set (1.28 s) after the current step, then the
550 ms settle; in DC coupling every AD1 result (100 ms) is judged. The
same latency as the 4.7 rule accepted (thermal protection). Measured in the
emulator (`istep-ua`, 200 uA -> 3 A scripted at 22 s): the range read 6 A at
fw uptime 21.96 s and 3 A at 22.93 s (the fw clock lags the script clock by
0.5 s or more, so the hop came <= ~1.5 s after the step, the first full
reading ~1 s after the hop). Emulator: powtest `istep` (10 mA -> 0.1 -> 1 ->
3 A: 60 mA, 600 mA, 6 A, 6 A, SYST:ALER? NONE in every checked sample), `istep-ua` (200 uA on 600 uA -> 3 A: 6 A, NONE), `iman-ovl`
(manual 600 mA, 3 A: OVERLOAD on 10 A); senttest `ovl-pow-auto-ma` /
`-ua` / `-dc` (NONE in every sample, READ? = P of 3 A) next to `ovl-pow`
(manual: OVERLOAD).

**Coupling: DC, AC, AC+DC (owner: "why only DC and AC+DC? what is AC+DC
there?").** The stock V / A trio, a tap on the active W button steps DC ->
AC -> AC+DC (`pw::next_coupling`); AUTO is gone:

| | DC | AC | AC+DC |
|---|---|---|---|
| row | the DC row (`dc_row`, 3.12) | method B (power row, R34 P V P I) | method B |
| P | V_dc I_dc | P_ac (sum X Y after the HPF) | P_ac + V_dc I_dc |
| V / I | V_dc / I_dc | Vrms / Irms of the AC parts | hypot(AC, DC) |
| S / PF / Q / f | "--" | V I, P / S, sqrt(S^2 - P^2), f | as AC, of the totals |
| Vdc / Idc (FETC:POW:ALL?) | measured | NaN (9.91E37): not used | measured |
| glyph above W / on the button | stock DC | stock AC | stock AC+DC |

AC drops the DC parts entirely, so it does not depend on the LPF zeros
(POW:ZERO) at all; a DC source reads ~0 (powtest `ac`: 12 V 0.4 A -> P 7e-9
W, V 0.015 V; the 2 V ripple on 12 V -> V 2.000, P ~0 with a DC current).
AC <-> AC+DC switch without a row write (the same method B row); only DC
<-> the others rewrite. The burden compensation uses the parts shown (AC:
I_ac^2 R). Energy integrates the P shown. Phase cal works in AC and AC+DC.
SCPI `POW:COUP DC|AC|ACDC`, `POW:COUP?` = `DC` / `AC` / `ACDC`; **`AUTO` is
deprecated**: still accepted, it selects ACDC (the old `AC` alias of ACDC is
now the real AC). Persist: the 2-bit field keeps its values (0 = AC+DC, 1 =
DC), the former AUTO (2) loads as AC+DC, AC takes the free value 3
(`pw::coup_of`): no layout change, no version bump. An older build reading
3 would take it as its AUTO. BT cmd 09 unchanged (P as the V-group value). Entry: since 3.16 every entry from another function starts
in DC (as V / A); a restart in W keeps the persisted coupling.

**Power Settings page made readable (owner: "WHAT IS BURDEN HERE?").**

* "Burden" -> **"At load"** (selected = on): P and V of the load, the
  meter's own drop (I^2 R_path, I R_path) taken off; off = at the jacks. A
  tap shows its meaning in the help line: "At load ON: P, V at the load
  (meter drop off)" / "At load OFF: P, V at the jacks, meter drop in".
* **Zero** and **V cal** open a dialog first (the fuse-check box, Cancel /
  Start; `Overlay::PowJob`): "Zero the power meter / Leads open, nothing in
  A / Takes about 12 s"; "V cal of this V range / Steady V, 10-70 % of range
  / Gain vs DC V / AC V, 8 s". `POW:ZERO` / `POW:VOLT:CAL` start directly.
* The coupling row **DC / AC / AC+DC**; the help line (y 302) says what the
  selected one does ("DC: P = Vdc x Idc; no f, PF, S, Q", "AC: AC parts only
  (DC removed); P, PF, S, Q, f", "AC+DC: total; P = Pac + Vdc x Idc, rms of
  both") until "At load" is tapped.
* The status line: "V gain 6 V: 1.0207 factory, zero: not set" (was "6V
  gain 1.02066 (factory), no zero"); jobs and refusals unchanged.
* Help 7/7 "Power meter": the W tap cycle, the three couplings, At load,
  Zero, the value tap, the energy reset.

`sf::label` lays out at most 48 glyphs: the help lines are <= 47 (ui-render
guard). Renders p19 (AC selected), p20 (At load help), p21 / p22 (the
dialogs), p23 (help page); ui-render 0 violations.

#### Device check list (this build)

1. W with a load on the mA shunt (e.g. 10..50 mA, I AUTO): the I range
   settles on 60 mA / 600 mA; then ~3 A through A: no red OVLD, no beep,
   `SYST:ALER?` NONE, `POW:CURR:RANG?` 6 A within ~2 s, P = V x 3 A. Same
   from 200 uA (600 uA range) straight to 3 A.
2. Manual I 600 mA (Power Settings), > 0.62 A: red OVLD + beep + 10 A
   (unchanged); back to AUTO ends it.
3. Tap the active W button: DC -> AC -> AC+DC, glyph above the unit and on
   the button each time; `POW:COUP?` DC / AC / ACDC; `POW:COUP AUTO` ->
   ACDC.
4. AC on mains (resistive / PF < 1): P, PF, S, Q, f as AC+DC within the
   noise; `FETC:POW:ALL?` Vdc / Idc 9.91E37. AC with a DC supply + load:
   P ~0, V ~0 (with POW:ZERO not run too).
5. Power Settings: "At load" tap -> help line text toggles; Zero -> dialog
   -> Start runs the 12 s zero; V cal -> dialog -> Cancel leaves; the
   status line text; Settings > Help 7/7.
6. A reboot keeps the coupling (AC persists as 3; a unit last in AUTO comes
   back in AC+DC).

### 3.15 Burden voltage in the A functions (fw-rs, 2026-10-22)

As the EEVblog 121GW: the DMM page shows how much the meter drops in the
circuit it measures, `I x R_path` of the shunt in use (3.6), so the user
sees the error a current measurement adds to a low-voltage circuit (uA:
47.5 ohm, 0.19 V at 4 mA; mA: 0.5 ohm and up, 0.3 V at 600 mA; 10 A:
10 mohm and up, 0.1 V at 10 A).

**Decisions** (the open points of the TODO item):

* **One R_path setting.** The power meter's per-shunt `r_path`
  (powermeter.rs, persisted in its words, default `SHUNT_OHM` = the bare
  shunt) is used; no second setting. `POW:BURDen <ohm>|DEFault` / `?` now
  acts on the shunt of the DMM A function in use when one is (DC / AC /
  AC+DC A: its own shunt, followed through the autorange), else (power
  meter, any other function) on the power meter's current range's shunt as
  before. The default (the shunt alone) is a **lower bound**: fuse, MOSFET
  and traces are UNKNOWN until measured (3.6).
* **Slots.** DC A (A, mA, uA): aux1 (empty there in the stock; the DC mA
  loop percentage keeps aux2). AC A: aux2, instead of the duty cycle (the
  frequency stays in aux1 where the stock has it; a duty cycle of a current
  is the least useful readout of the two). Each value carries an F14
  caption "Burden" right before it, as the 4-20 mA percentage's caption.
  **AC+DC:** both aux rows hold the AC / DC parts, so no readout there
  (FETC:BURD? still answers: the true RMS x R_path). Power meter: none (it
  has its own "At load" burden line, 3.12). **AUTO coupling** (f864be8)
  runs the AC+DC alternation with the parts in the aux rows: no readout
  either; FETC:BURD? answers the total RMS x R_path as in AC+DC, not the
  part shown (a 200 mA DC + 100 mA AC current shows 200 mA DC but drops
  223.6 mA x R_path; emu/hy3131/burdentest.py checks both couplings).
* **Value.** DC keeps the sign (current into A = positive, as the
  reading); AC is the RMS. From the raw current: REL and the display
  smoothing do not change it (HOLD freezes it with the reading). The unit /
  decimals follow the range's burden at the OL limit (fixed while the
  reading moves, <= 4 digits): < 0.1 V mV x.xx, < 1 V mV x.x, < 10 V
  V x.xxx, < 100 V V x.xx. Defaults: 6 A 61.80 mV max, 10 A 103.0 mV, 60 mA
  30.90 mV, 600 mA 309.0 mV, 600 uA 29.36 mV, 6000 uA 293.6 mV. An OL
  reading shows "0L" there too.
* **Toggle.** Settings 3/3 "Burden voltage": OFF (stock) / ON (A, mA, uA),
  persisted (p[38] bit 14, `meter::EXT_BURDEN`; 0 in every older record =
  off). Default OFF: the stock has no such readout and keeps the duty cycle
  in AC A (stock-look baseline).
* **SCPI.** `FETCh:BURDen?`: V of the latest reading of a current function
  (pre-REL), whether the readout is on or not; +9.9E37 when over range;
  +9.9E37 and "Data stale" outside the A functions. `DISPlay:BURDen ON|OFF`
  / `?`: the readout (the Settings row). Not `POW:BURD:STAT`, which is the
  power meter's compensation switch.
* **Bluetooth.** The burden is not sent to the phone app (it would read it
  in the function's unit: mA, A); with the readout on, AC A sends no value 3
  (the duty cycle) either.
* **The guided R_path step (3.6) is not built.** With the DM40's own ohm
  function only the uA chain is reachable (one lead V/ohm -> A, the resting
  PE state: the fuse check, `SYST:FUSE?` = OK,<ohm>, ~48 ohm); the mA and
  10 A paths need a known current and a second meter across A-COM (R_path =
  V / I). So: measure as in the device check below and enter it with
  `POW:BURD` in that A range (or in the power meter's Settings). A later
  step could take the uA value from the fuse check.

Code: `Dmm::burden_val` (dmm.rs; per reading, region B), `Reading::burden`
(the slot, recognised by the V / mV unit), `ui::aux_shown` / `BURDEN_CAP`,
the strip prefix "Bd ", `App::burden_scpi` / `burden_shunt` / `Dmm::
burden_of` (.cold). Region B 20 328 -> 19 376 bytes free (on f3aea2f),
stack headroom 21 460 -> 21 404 bytes, SRAM unchanged (10 904 free).

**Tests.** ui-render: 8 burden cases in the DMM page / strip matrix
(widest texts per decimals class, OL, with the loop percentage, AC with V)
incl. every A -> B update, 0 violations; renders d21..d24, c16..c18, m10.
Emulator `emu/hy3131/burdentest.py` (one run, isrc in A): DC 50 mA ->
25.00 mV (0.5 ohm), -2 mA (AUTO: 6000 uA) -> -95.0 mV (47.5 ohm), 3 A ->
30 mV (10 mohm), AC 100 mA -> 50.0 mV, `POW:BURD 0.6` -> 60.0 mV (the mA
shunt), `POW:BURD DEF` + `DISP:BURD OFF` -> FETC:BURD? still 50 mV, DC V
-> +9.9E37; FETC:BURD? = READ? x R_path within 0.2 % in every phase.

#### Device check list (burden voltage)

Bench supply with a series resistor into A (or mA / uA), a second meter
(DC mV) across A-COM; never more than 30 V across A-COM (3.6).
1. Settings > More > More > "Burden voltage" ON. DC 4 mA (uA range): aux1
   "Burden" ~190 mV; the second meter reads the real drop (shunt + fuse +
   MOSFET). Note the ratio: that is R_path / 47.5.
2. 400 mA (mA range) and 3..5 A (10 A range): the same comparison; expect
   the meter's value low by the fuse / MOSFET / trace part.
3. Enter R_path = V_second / I in each range (`POW:BURD <ohm>` while in
   that A range; `POW:BURD?` reads it back): the aux value now matches the
   second meter; a reboot keeps both the toggle and the R_paths.
4. Reverse the current: the DC burden turns negative.
5. AC A (a transformer or the generator through a resistor): aux1 Hz,
   aux2 "Burden" = RMS x R_path; the second meter in AC mV agrees.
6. REL on: the main value zeroes, the burden does not. Toggle OFF: the
   duty cycle comes back in AC A, aux1 empty in DC A, no stale caption.
7. Phone app in AC A with the readout on: value 3 empty, no mV shown as mA.

### 3.16 Owner round 5 (fw-rs, 2026-10-22): "At load" opt-in and marked, DC at entry, AC floors, coupling hints

**"At load" was invisible (owner).** With 1.234 V on V-COM the top V read
1.2240 V at 1 A and 1.2140 V at 2 A: the readout was V - I R_path (3.11,
R_path 10 mohm), and nothing on screen said so.

* **Off by default.** `Pw::comp` / `PowerVals::ZERO.comp` = false: V and P
  are what the jacks measure. `POW:BURD:STAT?` answers 0 after a fresh
  start. With it off the side column's Vb turns amber above 1 % of V
  (3.11): that is the designed cue that the meter's drop is inside P.
* **Migration.** The settings word kept "compensation OFF" in bit 9, so 0
  meant both "the user chose ON" and "never touched" (the old default): an
  explicit ON cannot be told apart. New bit 10 = "At load ON" (opt-in);
  bit 9 is still written (= !ON) so an older build reading a newer record
  behaves as before. Every older record has bit 10 = 0 and loads **OFF**:
  an old explicit OFF stays OFF, an old ON (default or chosen) becomes OFF;
  the user turns it on again where wanted. No record version bump (bit 10
  was 0 in every record).
* **"@load" marks** (F14, the value colour) while it is on: after the top
  V's unit ("229.98 V @load", inside the aux1 unit box, keyed with the
  unit) and at the main readout's top left (x 4..39, y 66..79) for P and
  whatever the main readout shows (Wh / VAh are integrals of the
  compensated P / S). The right column next to the coupling glyph
  (403..436) was the first choice; F14 "@load" needs 36 px at the letter
  spacing the overlap check demands, 3 px more than that column has. The
  other compensated values (S, Q, PF of the lower row; AVG; the energy box)
  carry no mark of their own; the help line and Help 7/7 say what the
  switch touches: V_L = V - I R, P_L = P - I^2 R, and S, Q, PF, Wh, VAh,
  AVG from those. The combined view strip and BT (cmd 09 P as V) are not
  marked (no room / no field).
* **When to turn it on**, the Power Settings help line after a tap on "At
  load": ON "At load ON: V at source, so load sees V - I*R", OFF "OFF: V,
  P at the jacks. ON if V leads at source" (V/ohm on the source side of the
  broken conductor and the meter in series, wiring 1 of 3.6). Help 7/7:
  "MF: At load (off)" / "@load: V - I*R".

**DC at entry (owner: as V / A).** Entering W from another function
(the W button, the wiring dialog's OK, `CONF:POW`, `MEAS:POW?`, BT cmd 06)
starts in DC, as the stock V / A start in DC; the tap cycle on the active
W button stays DC -> AC -> AC+DC -> DC. A restart while in W comes back in
its coupling (`App::restore` re-applies the persisted one after `select`),
as V / A come back in theirs through the function code in p[2]. The
persisted 2-bit field is unchanged (0 = AC+DC, 1 = DC, 3 = AC). SCPI:
`POW:COUP` still works anywhere, but `CONF:POW` from another function
resets it to DC, so set the coupling after `CONF:POW` (powtest does).

**AC floors (owner: AC coupling on pure DC showed V 0.0015 V, I 0.014 A,
PF -0.092, S 0.02 mVA, Q 0.02 mvar).** The stock AC V / AC A only clamp a
negative RMS (after the offset calibration) to 0; fw-rs's existing "no AC
here" level is the AUTO coupling's "to DC" floor (dmm.rs AUTO_FLOOR_V / _A
[1], acdc doc 9: V 0.5 %, A 2 % of the range; A's 2 % is sized for the uA
path's ~1.5 % RMS residue). `pw::FLOOR_V` reuses V's 0.5 %. `FLOOR_I` is
0.5 % too, not 2 %: with 2 % the first test run blanked a real 0.1 A AC
on the manual 6 A range (powtest `manual`, 1.7 % of the range); the
10 mohm path's pure-DC residue is 0.14 % in the emulator, and on the uA
range a residue above 0.5 % still shows, as in the stock AC A. Applied in
`pw_vals` (method B only; the raw parts and the autorange are untouched):

| coupling | V_ac or I_ac below its floor |
|---|---|
| AC | that part reads 0 (V, I), P 0 (no P_ac without both), PF / S / Q "--" (NaN; SCPI +9.91E37 as every "no value" of FETC:POW:ALL?), f "--" when the counter's channel (V tap, or the current on PB4) is below its floor |
| AC+DC | the part leaves the totals (V = hypot(0, V_dc)), P_ac 0, Q 0; S, PF of the totals stay; f as AC |

f is now NaN (9.91E37) whenever no frequency is measured, also in DC
coupling (was 0).

**Q of noise (owner: AC+DC on pure DC showed Q 0.017 var).** Uncorrelated
noise on the RMS of V and I adds to S but not to P. First order:
S^2 - P^2 = (V_n I)^2 + (V I_n)^2, so a resistive load shows Q_noise =
hypot(V_n I, V I_n) (the owner's 1.234 V / 1 A with the emulator residues
V_n 1.5 mV, I_n 14 mA: 0.0173 var, the observed 0.017). The coordinator's
sqrt(2 S (V_n I + V I_n)) is the first-order upper bound of the same; with
the floors as V_n / I_n either form zeroes real Q up to PF ~0.99 (230 V /
1 A on 600 V / 6 A with the 2 % I floor: 28 var). So V_n / I_n are a
separate noise estimate, `pw::NOISE_V` 0.1 % / `NOISE_I` 0.25 % of the
range (a few x the emulator's pure-DC residues 0.025 % / 0.14 %; the
device's are a check item), and Q below hypot(V_n I, V I_n) reads 0 in AC
and AC+DC. At 230 V / 1 A on 600 V / 6 A that is 3.5 var: Q reads 0 above
PF ~0.9999 there; at full scale (600 V x 6 A, 9.7 var) above PF
~0.999996.

**Coupling hints** (doc 4.4 style: grey badge DC? / AC?, silent, a toast
once, assert after 2 s, clear 1 s after, the hints bit of
SYST:ALER:ENAB / Settings 3/3; SYST:ALER? DC / AC):

* **AC+DC with no AC part**: V_ac and I_ac both below their floors and a
  DC part above one (`HINT_NO_AC`) -> DC? "No AC: DC is more precise" (DC
  reads I on AD1 with the factory DC A calibration, AC+DC through the AD2
  LPF).
* **DC with an AC part on V** (`HINT_AC`) -> AC? "AC present: use AC+DC".
  The register question: the DC row is the power row frozen at R34 = YY
  with the RMS / LPF on (`dc_row`), and `pw_poll_dc` already reads that
  RMS (sum Y^2 after the HPF = V_ac) on every INTF_RMS and used to drop it.
  So V_ac costs no register write: no note-7 exposure (AD2 / AD3 never
  switch while AD1 converts). I_ac would need R34 at XX, a write while AD1
  converts: not done. V_ac is the right condition anyway: the AC power DC
  misses is mean(v_ac i_ac), zero without v_ac. A ripple on V with a pure
  DC current (P exact in DC) still raises the hint (it cannot be told
  apart without I_ac).
* The power meter's row writes reset the hint timers (`sent.configured`
  with reset): a range change restarts the 2 s. The toasts are the power
  meter's own (`pw::hint_toast`); the badges and SYST:ALER? names are the
  DC V / AC V hints'.

Code: `pw::FLOOR_*`, `NOISE_*`, `HINT_*`, `hint_toast`, `Pw::vac_dc`,
`PowerVals::hint` (powermeter.rs, `pw_vals` / `pw_poll_dc` per reading in
region B); `Sentinel::pw_hints`; `ui::LOAD_CAP`, `ui::pw::LOAD_MARK`
(the mark in `coupling`, cleared with the mode glyph); app_power.rs word
bit 10; `App::restore` coupling; ui_power.rs help lines, no units next to
"--".

**Tests.** ui-render: the power cases with "At load" on (all but worst /
dc-ua / ac-floor, so every update pair crosses on <-> off), the ac-floor
case, both hint toasts; 0 violations; renders p01 (marks on), p24 (off),
p25 (AC floor + DC? badge), p20 (the help line). powtest (every old
scenario now sets `POW:COUP ACDC` after `CONF:POW`, burden / dcburden
`POW:BURD:STAT ON`, dcad1 F = 9.91E37) plus `entry`, `ac-floor`,
`acdc-floor`; senttest ovl-pow / -auto-ma / -auto-ua set `POW:COUP
ACDC`. All 29 powtest scenarios and the 4 senttest ovl-pow* pass (with
3.17 in the image; ereset long-presses the parts layout's lower row).
New ones (FETC:POW:ALL?): `entry` CONF:POW -> DC, BURD:STAT 0, 1.234 V /
1 A: P 1.2345 W, V 1.234, F 9.91E37; 12 V + 2 V AC in DC: SYST:ALER? AC,
gone 1 s after the ripple; `ac-floor` 1.234 V / 1 A in AC: V 0, I 0, P 0,
PF / S / Q / F 9.91E37; mains PF 0.95: Q 69.3 var (model PF 0.953);
`acdc-floor` the same DC in AC+DC: Q 0, PF 1, SYST:ALER? DC; PF 0.98: Q
43.4 var, NONE; PF 1: Q 0; "At load" on: V 1.224, P 1.2240 W. `manual`
(0.1 A AC on the manual 6 A range) failed with the 2 % I floor (I 0):
hence 0.5 %.

#### Device check list (round 5)

1. After the update (an older record): W comes up with "At load" off
   (`POW:BURD:STAT?` 0), no "@load" on the page. A bench supply 1.234 V
   through A at 1 A / 2 A: the top V stays 1.234 V (the drop is in Vb,
   amber above 1 % of V).
2. Power Settings: tap "At load": selected, help line "At load ON: V at
   source, so load sees V - I*R"; the page shows "@load" after the V unit
   and at the main readout's top left; V reads 1.224 / 1.214 V. Tap again:
   marks gone, "OFF: V, P at the jacks. ON if V leads at source". Reboot
   keeps the choice.
3. From V (or any function) press W: DC glyph, `POW:COUP?` DC. Tap W:
   AC, AC+DC, DC. Reboot while in AC+DC: comes back in AC+DC.
4. AC coupling, pure DC source + load: V 0, I 0, P 0, PF / S / Q / f
   "--" with no units; `FETC:POW:ALL?` 9.91E37 in those fields. Note the
   raw residues first (AC+DC: `FETC:POW:ALL?` V vs Vdc, I vs Idc) against
   the floors 0.5 % / 0.5 % of the ranges and NOISE 0.1 % / 0.25 %.
5. AC+DC, the same DC: Q 0, PF 1.000; after ~2 s the grey "DC?" badge and
   the toast "No AC: DC is more precise"; `SYST:ALER?` DC. Mains through a
   resistive load: Q small but not forced to 0 above the noise; with a
   motor / capacitor (PF < 0.98): Q as before.
6. DC with a rippled V (a supply with a transformer-like ripple, or the
   generator AC-coupled onto a DC): "AC?" badge, toast "AC present: use
   AC+DC", `SYST:ALER?` AC; Settings 3/3 hints off: none.

### 3.17 AC+DC: the DC and AC parts on the page (fw-rs, 2026-10-22; owner's variant V2)

The owner picked V2 of the mockups (scratchpad pwmock: V1 a table in the
chart's place, V2 sub-lines, V3 a tap cycle total / DC / AC): in **AC+DC
coupling only** small F14 sub-lines show the parts, "DC x V AC y V" under
the top V, "DC x A AC y A" under I (y 61, right-aligned to the V unit's
end 201 / to 423), "DC x W + AC y W" under the big P (y 155, to 394,
only while the main readout shows P; with Wh / VAh there it is empty).
"DC" / "AC" / "+" dim, the values in the status colour, each in the
readout format of its range (`reading_vals`). DC and AC coupling keep the
3.14 layout.

**Values** (`PowerVals::vp` / `ip` / `pp`, [DC, AC], `pw_vals`, method B):
V_dc / I_dc the LPFs, V_ac / I_ac the RMS statistics, P_dc = V_dc I_dc,
P_ac the X*Y RMS statistic; none derived from the totals by hypot. The AC
floors of 3.16 apply (a part below its floor reads 0; P_ac 0 unless both
V_ac and I_ac are above). "At load" applies per part exactly as to the
totals (v_L = v - R i): V_dc - R I_dc, sqrt(V_ac^2 - 2 R P_ac + R^2
I_ac^2), P_dc - R I_dc^2, P_ac - R I_ac^2; so P = P_dc + P_ac and V^2 =
V_dc^2 + V_ac^2 hold for what is shown, with or without "@load". I is
never compensated. SCPI is unchanged: `FETC:POW:ALL?` Vdc / Idc stay the
measured (uncompensated) DC parts.

**Geometry** (`ui::pw_parts`, set with the static draw of the DMM page
when the power meter is in AC+DC with method B; `App::pw_tick` redraws
the page when that changes; false on every other page and function, so
nothing else moves: ui-render's 135 non-power renders are pixel-identical
to the previous commit's):

| item | stock / DC / AC | AC+DC parts |
|---|---|---|
| main F78 cells, unit, coupling glyph, limit badge, "@load" mark | y 68 / 114 / 76 / 72 / 66 | +7 (`PARTS_DY`) |
| lower row (E, PF, S, Q, f) | 153..171 | 170..188 (`PARTS_ROW_DY` 17) |
| chart frame | 176, 97 rows | 192, 81 rows; axis labels 192 / 227 / 262, timebase chip 208, rate chip 243 |
| side column | Vb 176, VAh 188, energy box 205, AVG 245, ranges 259 | Vb 192, no VAh (the main value tap still shows VAh), box 208, AVG 249, ranges 263 |
| taps | value 64..150, row 153..171, box 205..242 | value 64..167, row 170..188, box 208..245, the chart column from 192 |

The aux row's value / unit boxes (y 36..65) overlap the sub-lines' slots
(61..74): a repaint of an aux box invalidates its sub-line
(`PwCache::sub_invalidate`). The HV lightning's box (0, 80, 40, 64, V
functions) can overlap the moved "@load" mark: its toggle invalidates the
mark.

Code: `ui::set_pw_parts` / `pw_parts` / `main_dy` / `chart_geom` /
`chart_rows`, `ui::pw::sublines` / `side_geom` / `row_hit` (per reading:
region B), `App::pw_parts_want`. Region B 23 852 -> 22 640 bytes free
(with 3.16's I floor change), region C 9 936 -> 9 768, stack headroom
21 396 -> 21 252, SRAM 10 848 (unchanged).

**Tests.** ui-render: cases parts-mixed / parts-dc / parts-mw plus every
AC+DC case (worst: the 1 kV / 10 A widest sub-lines) in the page x hold x
alert matrix and the limit-badge matrix with the parts regions, every
update pair within one layout (the app redraws across), 0 violations;
renders p26 (mixed, "@load"), p27 (pure DC, DC? hint), p28 (mW ranges,
negative parts, HI badge), p29 (Wh in the main readout: no P sub-line),
p02 (1 kV / 10 A). touch-test: the power page's targets (energy box,
lower row, rate chip, chart column, function row) in both layouts.

#### Device check list (parts layout)

1. W, tap to AC+DC: the page redraws, sub-lines under V, I and P; a DC
   supply + load: "AC 0.0000 V", "AC 0.0000 A", "+ AC 0.000 W"; DC parts
   = DC V / DC A of the same source.
2. Mains through a resistive load: DC parts ~0 (the LPF zeros), AC parts =
   the totals; a DC supply with ripple: both parts, P = DC + AC.
3. Tap W to DC: the 3.14 layout back (VAh row in the side column, chart
   from 176), no stale sub-line pixels. Tap the main value: Wh / VAh, the
   P sub-line empties, V / I sub-lines stay.
4. "At load" on in AC+DC: "@load" marks, the DC part of V drops by
   R I_dc, P parts by R I^2 each.

---------------------------------------------------------------------------

## 4. Wrong-connection detection: what the signals allow

### 4.1 The resting state is the key fact

In every non-current mode the stock writes PE0 = PE1 = 0 (`relay_pe(2)`, VERIFIED),
which is the uA state: both MOSFETs off, the A jack sees 10 mohm + 0.5 ohm +
47.5 ohm ~ 48 ohm to COM (INFERRED from the mode table; a user confirms the idle
input "rests on the 47 ohm mA shunt", user-feedback #59918). Consequences:

* A voltage source across A-COM while in V / ohm / ... mode draws V / 48 ohm:
  5 V -> 0.1 A (0.5 W in the 47.5 ohm resistor), 12 V -> 0.25 A (3 W), 24 V ->
  0.5 A (12 W), mains -> 4.8 A (1.1 kW) until the fuse opens. Whether the 47.5
  ohm resistor and the mA fuse (rating UNKNOWN) survive the 12..24 V cases is
  the hardware's business; the firmware can see every one of these currents
  immediately on the shunt node PB1 (47.5 ohm * I: 1 mA = 47.5 mV).
* A current flowing legitimately in the A jack while the user thinks he is in
  V mode (leads in A/COM, function V) is the same signal: current in PB1 while
  mode != current. That is the Fluke "Input Alert" case; the DM40 has no jack
  sensors (the stock DMM GPIO list is PA9/PA10, PE0/PE1, PC7 only, VERIFIED
  absent in `hy3131_v2_6.h`), so the shunt node is the only way.

### 4.2 Sentinel per mode (what is free, what to use)

The comparator is preferred wherever it is idle: it needs no converter and
therefore avoids datasheet note 7 (enabling AD2/AD3 while AD1 converts shifts
the AD1 offset and would touch the stock DC calibration). Thresholds are the
rows' own R21 = 0xdd (+-20.8 mV at the comparator input, VERIFIED): 0.44 mA on
47.5 ohm.

| mode (row) | what is busy | A-jack sentinel | V-jack / live sentinel |
|---|---|---|---|
| DC V (f1), DC mV (f3), ohm (f10), temp (f15) | AD1; comparator enabled but idle (R20 0xf0 / 0xe0: SCMPI = PB4, ENCMP, no ENCTR, no ENPCMPO) | **comparator on PB1**: R20 SCMPI = 100 (PB1), read CTSTA CMPHO / CMPLO every poll; DC current of either sign or AC -> either bit set / toggling. Cost: one R20 byte in the row, one CTSTA read per poll | - |
| diode (f12) | AD1; comparator = the beeper (R20 0xb0 / 0xb4, SCMPI = RLD, ENPCMPO toggled) | AD2 + OP2 are free (R32 0x3c: OP1 on PB0 only) -> OP2+ = PB1, AD2 = OP2O - PB3, LPF/RMS, enabled with the row and never toggled; note 7 exposure (the diode reading is a 3 V scale, a small offset shift is tolerable) | - |
| cap r0..4 (f13 RC rows) | comparator + counter (the oscillator), AD1 off, OP1 on PB0, OP2 off | AD2 free, OP2 free -> as the diode row (AD1 is off here: no note-7 issue) | - |
| AC V (f0), AC mV (f2) | AD2/AD3/RMS/LPF; comparator busy with the frequency readout on PB4 (ENCTR) | **AD1, which is off in these rows**: ENAD1, SAD1FP = PB1 (1001), SAD1FN = PB3 (101), IG 3.6, ref PB6/AGND, unchopped OSR 256 (640 sps) with peak hold PKHSEL = 01 (AD1<23:5>): |PKHMAX|, |PKHMIN| over ~100 ms catch DC and 50/60 Hz AC. AD1 is enabled with the row, never toggled (note 7 is about AD1's own offset: irrelevant for a detector). Alternative: time-share the comparator between PB4 gates (the counter restarts every 350 ms; a 20 ms look at PB1 between gates) | AD1 alternates to SENSE - PB5 (the DC V r1..r3 input) every other window: DC present while the AC reading is ~0 -> hint "DC voltage: use DC V?" (acdc doc 6a: AC V on 0.5 V DC autoranges into mV and shows 1 mV) |
| continuity (f11) | AD1; comparator = the beeper (R20 0x56: SCMPI = OP1O, ENPCMPO); OP1 on SENSE (R32 0x38) | AD2 + OP2 free -> as the diode row, or **none** (continuity sessions are short) | - |
| cap r5..7 ramp | AD1, comparator (ramp reversal), **both** op amps on PB0 (R32 0xcc) | **none**: no amplifier left to reach PB1; AD1 could read PB1 - PB3 directly between ramp readings only by stealing the ramp converter. Accept the gap (seconds per large capacitor) | - |
| DC A (f7 / f8_9) | AD1, OP1; comparator idle (R20 0xf0) | - (the reading is the current) | **comparator on SENSE** with the /1000 tap closed in the row: **SS5 only** (the A rows run the power bus as SGND, R2F 0x81, so PS5 would ground the tap); the /100 tap would put 230 V = 2.3 V at the tap, outside the op amp / switch common-mode range. Trips at 20.8 mV * 1000 = 21 V (AGNDP/N<1>), 42 V (<2>), 62 V (<3>). Below 21 V: AD2 via OP2 <- SENSE (free in DC A, R32 0x3e / 0x3d) with LPF/RMS - enabled with the row, never toggled; **T8**: check that the DC A zero / gain are unchanged with AD2 on (note 7) |
| AC A (f4 / f5_6) | AD2/AD3, OP1, comparator = frequency readout (R20 0xf2, SCMPI = PB4: in AC A the PB4 node carries the current signal coupled from OP1O, INFERRED) | - | **AD1 free** (R23 0x03): SENSE - PB5 with the /1000 tap (SS5), chopped OSR 2048 (25 ms) for DC, peak hold for AC: ~2 V detection |
| frequency (f14) | counter, AD1 (PB5 - PB5 zero reading), AD2 | comparator busy -> AD1 (PB1 - PB3) replaces the zero-reading input | - |

AC+DC caveat: in AC+DC the stock and fw-rs (`freq_secondary(now, 0xF0)`, acdc
doc 7 item 1) rewrite R20 = 0xf0 / 0xf2 every 350 ms in **both** phases, which
points the comparator back at PB4. The DC-phase A-jack sentinel must therefore
fold its SCMPI = PB1 into the `src` byte that `freq_secondary` writes (and
accept that the counter then measures the current node's frequency, or run the
frequency readout from the AC phase only), or be disabled in AC+DC.

Decode note: byte 18 of each 22-byte row is R32 (`ENOP2 SOP2P<2:0> ENOP1
SOP1P<2:0>`); the values quoted above are from the rows in
`hy3131_tables.rs` (f1 r0 / f0 / f11 0x38 SENSE, f3 0x3a RLU, f4 / f7 0x3e PB2,
f5_6 / f8_9 0x3d PB1, f10 / f12 / f15 / f13 r0..4 0x3c PB0, f13 r5..7 0xcc
both on PB0, f2 0xaa both on RLU, f1 r1..3 0x33 both off). The free-text OP
column of `hy3131_v2_6.h` disagrees in places; each sentinel row must be
checked byte by byte at implementation time (host test: only the intended
fields differ from the stock row).

### 4.3 What cannot be detected (and what helps instead)

* **A voltage source across A-COM while in a current mode** (the fuse-blower):
  in the 10 A range the shunt path is ~20..50 mohm; a 5 V supply current-limits
  to its own limit and reads as a plausible current; mains blows the fuse in
  < 1 ms. No signal distinguishes it before the fuse opens; after it opens the
  A jack side of the fuse has no path into the chip (the ohm source reaches
  only V/ohm through the relay, INFERRED from the rows), so a **blown fuse is
  invisible**: the meter reads 0 A forever. Mitigation: a guided "Fuse check"
  (ohm mode, lead between V/ohm and A: expect ~48 ohm (PE = 2) or ~0.05 ohm
  (PE0 = 1, MOSFET bypass); OL = blown) and a hint after 60 s of exactly-zero
  current in an A mode: "0.000 A for a minute - check the fuse (Menu > Fuse
  check)".
* **uA/mA shunt over-current** (47.5 ohm resistor under a gross over range):
  the stock autoranges mA -> A only when AUTO+ (`m+7 bit0`) is set and after a
  650 ms settle (dmm_v2_6.h 1). A protective rule is possible: in the uA / mA
  shunt state, an OL reading (first conversion, 25..100 ms) switches PE to the
  A shunt at once regardless of AUTO+, shows OL + "OVERLOAD - A range
  selected", and stays there until the current drops. Whether the resistor's
  power rating makes this necessary is UNKNOWN (BOM); the rule is cheap and
  only moves the reading up a range. In V modes the sentinel of 4.2 cannot
  quantify the current (comparator) - do **not** auto-switch the MOSFETs there
  (a voltage source on A-COM would then see 10 mohm and blow the fuse where 48
  ohm might have survived); warn loudly instead. **Superseded by 4.8** (owner
  2026-10-22): the probe on PB2 quantifies the current, and a source that
  would burn the 47.5 ohm resistor is moved onto the 10 mohm shunt (a weak
  one then flows harmlessly, a stiff one blows the 10 A fuse instead of the
  resistor / the 30 V MOSFETs).
* **Live circuit in ohm / diode / continuity / cap**: the ohm input node PB0
  sits at the OP3 source voltage when open, so a positive external voltage
  cannot be told from "open" by level; a negative one (CMPLO on PB0 below
  AGNDN<1>) or an AC one (CMPHO/CMPLO toggling) can - in the ohm rows the
  comparator is idle (R20 0xe0) and can watch PB0 (SCMPI = 011). Cap mode
  entry: a 25 ms AD1 read of PB0 - AGND before the first cap row: |V| > 0.1 V
  -> "Discharge the capacitor". The 900 k / PTC input network (typical
  circuit) limits the current, the clamp level is still visible. Partial, P3.
* **Stock warnings**: none for connections. The analysis headers list only the
  V-mode colour (red from 30 V) and the HV glyph (from 100 V, dmm_v2_6.h 7)
  plus "Low battery"; fw-rs has them (`ui.rs hv_warning`, `strip_data` badge).

### 4.4 Warning design

Alert states (one at a time, priority order), computed in `dmm.rs` per poll and
carried in `Reading.alert`:

| alert | condition | hysteresis | UI | sound |
|---|---|---|---|---|
| **A-JACK** (red) | mode not a current mode and the A-jack sentinel sees > 0.44 mA (comparator) / > 0.5 mA DC or peak (AD1) | assert after 2 consecutive polls >= 100 ms apart; clear after 1.0 s below | badge "A-JACK" (red, above HV in `strip_data` priority, below HOLD), toast "Current in A jack - leads in A while measuring V?" once per assertion, the main readout keeps measuring | 3 x 50 ms every 2 s while asserted (stock key beep = 50 ms; `App::beep`), honours the alert volume floor (user-feedback UF-28); **since 2026-10-24 the danger alarm of 4.10** (loud two-tone, whatever the volume) |
| **OVERLOAD** (red) | current mode, uA/mA shunt, OL on the first conversion | immediate | OL + badge "OVERLOAD", PE -> A shunt (4.3, if adopted) | 1 x 300 ms |
| **V-JACK?** (amber) | current mode, |I| < 0.5 % of range for > 1 s **and** the V sentinel sees >= 21 V (comparator) / >= 2 V (AD1 in AC A) | assert 1 s, clear 0.5 s | badge "V-JACK?", toast "Voltage on V/ohm, no current: leads in the V jack?" | none (informational) |
| **LIVE?** (amber) | ohm / diode / cont: PB0 below -20.8 mV or toggling for > 0.3 s | 0.3 s / 1 s | badge "LIVE?", toast "Voltage on the circuit - power it off" | 1 x 100 ms |
| **DC?** / **AC?** (grey hint) | AC V with AC ~ 0 and |DC| > 0.5 V (AD1 sentinel); DC V with ripple... (DC V has no free converter without note-7 exposure: skip) | 2 s | small hint text in the aux2 slot "DC 0.50 V present" | none |
| **FUSE?** (grey hint) | A mode, exactly 0 (< 1 count rms) for 60 s | - | hint "0 A for 60 s - Menu > Fuse check" | none |

Thresholds come from the hardware: the comparator's +-20.8 mV is fixed by the
AGNDP/N<1> taps (0.44 mA on 47.5 ohm; 0.21 mA if the resting shunt were 0.5 +
47.5 + ... - the exact resting resistance is INFERRED); the AD1 sentinels use
> 10x the observed path offsets (uA path LPF offset 8.7 uA = 0.4 mV, acdc doc
6b, VERIFIED) and > 5x the stock's DC mV noise (sd 4 uV, 6c). SCPI:
`SYSTem:ALERt?` -> `NONE|AJACK|OVERLOAD|VJACK|LIVE|DC|FUSE`, `SYSTem:ALERt:ENABle
<mask>`, and bit 9 of `STATus:QUEStionable?` while any red alert is on. The
sentinel readings are also exposed for the emulator/device tests:
`DBG:SENTinel?` -> raw CTSTA / AD1 peak values.

### 4.5 Implementation plan (idea 2)

1. Row builder: `sentinel_row(mode, range) -> [u8; 22]` applying the per-mode
   overrides of 4.2 to the stock row (R20 SCMPI; or R23/R24/R28/R29 for the AD1
   sentinel; PA5 PS+SS for the A-mode V sentinel). Host test per row: diff vs
   the stock row is exactly the intended fields.
2. `dmm.rs`: `Sentinel` state (mode, kind, last CTSTA / peak, timers) polled
   in `poll` before the mode's own read (CTSTA read = 1 byte; AD1 peak = 6
   bytes every 100 ms; the AD1 input alternation for the DC? hint); `Alert`
   enum in `Reading`.
3. `app.rs` / `ui.rs`: badge + toast + beep pattern; alert priority in
   `strip_data`; settings toggle "Connection alerts" (default on) and per-alert
   mask.
4. `scpi.rs`: `SYSTem:ALERt?`, `:ENABle`, `DBG:SENTinel?`.
5. Menu: "Fuse check" guided dialog (ohm mode, PE state chosen by the dialog,
   expected values, verdict).
6. Emulator: the terminal-level model (owner TODO) is what makes these
   testable: a current source into the A terminal while the firmware is in V
   mode must appear on PB1 (47.5 ohm) and on the comparator; a voltage on
   V/ohm in A mode must appear on SENSE through the /1000 tap. Until then,
   `SetAd1` / forced CTSTA hooks for the host tests.
7. Device tests: **T7** DC V zero and 1 V reading with the comparator
   repointed to PB1 (expect no change); **T8** DC A zero / gain with AD2 + OP2
   enabled in the row (note 7) - if the zero shifts, the DC A V-sentinel stays
   comparator-only (>= 21 V); **T9** A-JACK trip with 1 mA / 10 mA DC and 10 mA
   AC into A-COM while in DC V, DC mV, ohm, AC V; latency and no false trips
   with the A jack open over 10 minutes; **T10** V-JACK? with 5 V / 24 V on
   V-COM in DC A / AC A.

Effort: rows + sentinel state + A-JACK alert 2 days; V-JACK / LIVE / hints 1
day; fuse check dialog 0.5 day; tests 1 day.

### 4.6 Implemented (fw-rs, 2026-10-21)

`fw-rs/src/sentinel.rs` (row builder, alert state machine, host tests),
hooks in `dmm.rs` (`write_config`, `poll`, `poll_ac`, ohm / continuity /
diode), `app.rs` / `ui.rs` (badge, toast, beep, Settings 3/3), `scpi.rs`.
What was built, against the plan of 4.5:

| sentinel | modes | row change (byte-diff host test vs the stock rows) | detection |
|---|---|---|---|
| A-JACK, comparator on PB1 | DC V r0..r3, DC mV, ohm (both variants, also the resistive temperature sensors), thermocouple | R20 -> 0x90 only (SCMPI PB1, ENCMP; no counter, no CMP pin) | CTSTA read (1 byte) on every `poll`, ORed into 100 ms windows; 2 hit windows in a row assert, 1 s without a hit clears |
| A-JACK, AD1 peak hold on PB1 - PB3 | AC V r0..r3, AC mV | R22 0x00 (OSR 256 unchopped), R23 \|= ENAD1, R24 0x95, R25 AD1IG 3.6, R29 ENPKH + PKHSEL 01 | PKHMIN / PKHMAX come with `poll_ac`'s 20-byte read; > 17 900 (0.5 mA on 47.5 ohm) is a hit; ENPKH toggled after each harvest (`PKH_RESTART`, T9) |
| V-JACK?, comparator on SENSE via /1000 (SS5 only) | DC A / mA / uA | R20 -> 0x10, R2C \|= 0x10 (SS5; PS5 never) | hit windows for 1 s while \|I\| < 0.5 % of the range; clears 0.5 s after the last hit or when current flows |
| LIVE? (no hardware) | ohm, continuity, diode | none | ohm AD1 code < -20 000 (sign the formula otherwise drops), continuity < -2 ohm before the clamp, diode < -0.05 V; 0.3 s to assert, 1 s to clear |

Known limit: in the temperature function `poll` gets past its settle
deadline only ~1.4 times a second (the sensor phases), so the comparator is
sampled that rarely there: DC trips as elsewhere, an AC current trips and
clears more slowly and may flicker.

Not done in this step (all done since, section 4.7): AC+DC, diode /
continuity / capacitance / frequency / AC A sentinels, the DC? / AC? / FUSE?
hints, the OVERLOAD shunt rule, the fuse-check dialog, STAT:QUES bit 9. The
MOSFETs are never switched by the sentinel (only the OVERLOAD rule does, in
the current functions) - until 4.8 (A-jack protection: PE0 = 1 on a current
that would overheat the 47.5 ohm resistor).

UI (stock visual language, `tools/ui-render` ink / overlap check over every
alert x hold x verdict x rate-chip case, renders d12-d14, c11, t03, m10):
the DMM status slot (where RUN / HOLD / A-HOLD sit) shows "A-JACK" white on
red or "V-JACK?" / "LIVE?" in the value amber when no HOLD / AutoHold is on
(HOLD wins; the alert box runs 119..201); the combined-view strip badge has
priority HOLD > alert > HV > REC > REL; one toast per assertion ("Current in
A jack: check leads" / "Voltage on V jack, no current" / "Voltage on circuit
- power off"); A-JACK beeps 3 x 50 ms every 2 s at the key / alert volume (0 %
= silent), LIVE? once 100 ms, V-JACK? silent. Settings 3/3 "Jack alerts": ON
(all) / A-JACK only / OFF, persisted in p[38] bits 11..13; a disabled
sentinel writes the stock row. SCPI: `SYSTem:ALERt?` (NONE | AJACK | VJACK |
LIVE), `SYSTem:ALERt:ENABle <0..7> | ON | OFF` (0..15 since 4.9: bit 3 the "Leads in A?" reminder) / `?`, `DBG:SENTinel?`
(kind,alert,mask,ctsta,pkhmin,pkhmax,streak,hit_windows,samples).

Emulator: the HY3131 model got the sentinel nodes (comparator on PB1 = the
A current times the relay-selected shunt, instantaneous, in the DC V / mV /
ohm / temperature roles; SENSE = V jack / 1000 with SS5 in DC A; AD1 on PB1 -
PB3 in the AC roles with the peak hold over every elapsed conversion; row
matching ignores the AD1 fields of rows with AD1 off, PKHSEL / ENPKH, and SS5
in the current rows): 199 host checks. fw-rs end to end over USB/IP
(`emu/hy3131/senttest.py`, 18 scenarios, all as expected): 1 mA DC / -1 mA / 5 mA AC 50 Hz / 5 V across
A-COM in DC V, 2 mA in DC mV, 2 mA in ohm, 5 mA with a thermocouple, 10 mA AC
and 1 mA DC in AC V, 5 mA AC 60 Hz in AC mV -> AJACK, cleared ~1 s after the
source goes; 0.2 mA in DC V -> NONE; 24 V DC and 230 V AC on V-COM in DC A ->
VJACK, 5 V or 24 V with 12.3 mA flowing -> NONE; -1 V in ohm and diode, 1.234
V in continuity -> LIVE; ENAB 6 -> NONE. The readings stayed those of the
stock rows (1.2340 V, 100.3 C, 1.0000 V AC, 4699.98 ohm), and the
`modetest.py --cross` table of fw-rs (86 cells) is unchanged against the parent
commit within the model noise (all ok; largest moves: the AC V floor 0.84 ->
0.89 mV, diode on a thermocouple 10.79 -> 10.76 mV).

After the device check of 9dd1222 (docs/device-check-2026-10-21.md, "Fixes
after 9dd1222"): the AC V sentinel row also holds OP1's chopper off (R33
OP1CHOP = 00; with AD1 on, the 2 kHz chopper put a ~37 mV RMS floor on the 6 V
range), and DC V 60 / 600 / 1000 V carry no comparator sentinel (those rows
drive the power bus from OP3; A-JACK tripped at 10 V with the A jack open).
`DBG:SENTinel:VARiant` restores either for a device check.

Device test plan (sentinel build; commit the tree before flashing):

* **T7** DC V / DC mV zero (shorted leads) and 1 V reading, ohm 100 ohm / 10
  kohm, thermocouple: equal within noise with Jack alerts ON and OFF
  (Settings 3/3), i.e. comparator on PB1 vs the stock row. `DBG:SENT?` with
  the A jack open over 10 minutes in each: alert NONE, hit_windows 0.
* **T8** (not needed by this build: no AD2 sentinel) DC A zero / gain with AD2
  + OP2 enabled, before any AD2-based V-JACK? in AC A.
* **T9** A-JACK: 1 mA and 10 mA DC (both polarities) and 10 mA AC 50 Hz into
  A-COM while in DC V, DC mV, ohm, temperature, AC V, AC mV: badge within
  ~0.3 s, toast once, 3-beep pattern every 2 s (volume 50 % and 0 %),
  cleared ~1 s after removal; trip threshold (expect ~0.44 mA DC on the
  comparator, ~0.5 mA on AD1); `DBG:SENT?` pkhmin / pkhmax in AC V. AC V
  reading of 1 V / 230 V 50 Hz with alerts ON vs OFF (AD1 running + the R29
  ENPKH toggle every ~107 ms must not move the RMS / LPF / Hz readouts; if it
  does, `PKH_RESTART = false`). No false trips in any of those modes with the
  A jack open (10 min each) and during autoranging. Temperature: expect a
  slower trip / clear with AC (sparse CTSTA samples).
* **T10** V-JACK?: 5 V (no alert expected, below 21 V), 24 V DC and mains on
  V-COM with nothing in A while in DC A / mA / uA: amber badge after ~1 s,
  cleared 0.5 s after removal; DC mA reading with SS5 closed unchanged
  (12.3 mA reference); no alert while a current flows.

### 4.7 Implemented (fw-rs, 2026-10-22): OVERLOAD, the other sentinels, hints, fuse check

What the rest of 4.2-4.5 became, with the manual's answers of section 5 (ONE
10 A ceramic fuse in front of all shunts: the 0.5 ohm mA shunt and the 47.5
ohm uA resistor have no protection of their own, so the OVERLOAD rule
matters). Code: `sentinel.rs` (pure rules, row overrides, host tests),
`dmm.rs` / `powermeter.rs` (the latch, the hooks, the fuse job), `app.rs` /
`dialog.rs` / `ui.rs` (badge, beep, toast, dialog, the Current Settings
button), `scpi.rs`.

**OVERLOAD (doc 4.3 rule, red, not maskable).**

| | |
|---|---|
| where | DC A / AC A / AC+DC A (and the AUTO coupling) on the mA (0.5 ohm) or uA (47.5 ohm) shunt with a MANUAL range; the power meter's I channel on ik 0..3 with a MANUAL I range only. In AUTO (DMM since 2026-10-23, power meter since 3.14) the shunt's top range over is a normal up-range, no alert (below). Never in V / ohm / continuity / diode / cap / Hz / temperature (no shunt switching there: host test `overload_rule`) |
| trigger | a reading over its range (\|I\| > 1.03 x the range, i.e. what shows OL; AC+DC: the total) when the range is manual (any of the shunt's two). AUTO: the shunt's top range (6000 uA / 600 mA) over is a **quiet hop** in one step to the A shunt's 6 A range (DC: mode 7 r0, 250 ms; AC: mode 4 r0, 650 ms; AC+DC / AUTO coupling: its DC phase), no latch, beep, toast or badge, and the walk down as usual (> 6.18 A: 10 A). Owner 2026-10-22: A AUTO coupling enters on DC mA r0 (stock), so AC 1 A raised the red OVERLOAD at once (couptest `iac`) |
| action | manual: the 10 A range at once (DC: mode 7 r1, settle 250 ms; AC: mode 4 r1, 650 ms; AC+DC: its DC phase; power: ik 5, the V parts kept); `relay_pe(0)` through the normal `write_config` |
| latch | manual before: 10 A stays manual (the DMM has no auto latch any more; the code path for `back` = None stays for the power meter) |
| release | any user range / function / AUTO choice (`press`, `set_range`, `auto_key`, `select`, `set_sub`, BT mode byte, `pw_set_ranges`, `pw_enter`), or \|I\| below `max(0.85 x the full scale it returns to, 2 mA)` for 5 s (OL readings never count; `OVL_RELEASE_FRAC`, was 0.5 until 2026-10-23: manual 60 mA with 40 mA flowing never came back, senttest `ovl-dcma-man`): back to the manual range it left. The trip is at 1.03 x, so 0.85 x leaves a margin; a current flapping around the range re-trips within 30 s and the new latch is sticky (user choice only). 2 mA is what the 10 A range can tell from zero (1 mA digits; the model's noise ~0.5 mA); manual 600 uA is the case where 2 mA exceeds the scale it returns to (the 30 s sticky rule covers its trip / release cycle) |
| timing | the first conversion after the settle: DC ~0.35 s after the overload, AC ~0.75 s; one 300 ms beep (key / alert volume, also with the key beep off), toast "Overload: 10 A shunt selected", badge OVLD (TEXT22: "OVERLOAD" does not fit the 83 px slot) red for `OVL_RED_MS` = 3 s, alternating with HOLD as A-JACK (decision #5), strip "OVLD"; then, while the latch holds, an amber "10A" badge / strip (`Alert::Ovl10A`, display only: `Sentinel::shown`; no new toast or beep): the meter measures fine on 10 A, the badge says the range is not the user's (owner 2026-10-22: the red OVLD stayed while the reading was fine) |
| SCPI | `SYST:ALER?` OVERLOAD; `STAT:QUES:COND?` 514 (bit 9 + bit 1), for the whole latch (red and amber) |

The protective value is thermal (the 47.5 ohm resistor and the 0.5 ohm shunt
under amps for seconds), so detection after the settle is enough; reading AD1
inside the settle would save ~0.3 s at most and was left out. It cannot help
against a voltage source across A-COM in a current function (4.3: the fuse's
business).

**Sentinels added** (all A-JACK unless noted; host tests: byte diff vs the
stock rows; `DBG:SENTinel:VARiant +8` writes the stock rows again for a device
comparison):

| mode | block (free in the stock row) | row override | threshold |
|---|---|---|---|
| AC+DC V / A | the plain DC and AC phases' own sentinels (comparator PB1 / SENSE in the DC phase, AD1 peak in the AC phase); the DC phase's R20 goes through `acdc_src` because `freq_secondary` rewrites it every 350 ms (the counter then counts that node; AC+DC shows no frequency). The history belongs to the function and the sentinel family (A-JACK: comparator + AD1 / AD2 peak; V-JACK?: comparator + AD1 peak), so a phase switch keeps an alert; the comparator is also read during the phase settles | as the plain modes | as the plain modes; V-JACK?'s "no current" = < 5 % of the range (the total carries the AC floor) |
| AC A (4..6), V-JACK? | AD1 (off in the row) on SENSE - AGND (until f0d21eb: SENSE - PB5, see 4.11) with the /1000 tap (SS5 only, as DC A), IG 3.6, OSR 256 unchopped, peak hold; OP1's chopper held off (AD1 on starts it: the 9dd1222 AC V floor) | R22 00, R23 \|= ENAD1, R24 03 (was 07), R25 \|= 30, R29 PKH AD1, R2C \|= 10, R33 OP1CHOP 00 | since 4.11: the swing PKHMAX - PKHMIN > 4 270 codes = 2 V rms AC on V/ohm (was: \|peak\| > 1 510 codes = 2 V, DC included); "no current" = < 5 % of the range (the uA path's RMS floor is ~1.5 % of 600 uA, acdc doc 6b) |
| frequency (14) | **none since the f3aea2f check** (was: AD1's zero reading PB5 - PB5 moved to PB1 - PB3 with the AC rows' peak settings). The frequency row leaves its input divider open (R2C / R2D 0) and PB1 - PB3 stands at +41 100 codes (~54 mV) with 0..2 V or nothing on V-COM, ~3 100 at 5 V, -850 at 10 V: a false A-JACK, and no fixed threshold or offset tells a current from it | stock row | - |
| continuity (11), diode (12), cap r0..4 (13, RC oscillator) | AD2 + OP2 (unused there): OP2 x1 (OPS2 0) on PB1, AD2 = OP2O - PB3, refs PB6 / AGND, IG 2.0, OSR 256, peak hold on AD2 (PKHSEL 00); continuity / diode have AD1 on: the row goes out with AD1 off and ENAD1 last (note 7: AD2 never starts while AD1 converts) | R25 AD2IG 11 + OPS2 0, R26 83, R27 6D, R29 ENPKH, R32 D0 \| OP1 | 9 900 codes = 0.5 mA |

Gaps (no fake coverage): frequency (the row's PB1 level follows the input,
above), capacitance r5..7 (the ramp: both op amps on PB0,
AD1 and the comparator busy; asserted by the `cap-ramp-gap` scenario), DC
V 60 / 600 / 1000 V (the 9dd1222 false trip, 4.6) and therefore AC+DC V
above 6 V in both phases (an AC-phase-only A-JACK would be wiped by every
phase switch and re-asserted: a toast and beeps each cycle; asserted by
`acdc-v-hi-gap`). In the temperature
function the comparator is still sampled only ~1.4 times a second (4.6).

**Hints (doc 4.4, grey in the status slot, silent, one toast; on with Jack
alerts ON, off with "A-JACK only" / OFF; `SYST:ALER?` DC / AC / FUSE; below
every alert, never alternating with HOLD):**

* **DC?** (AC V, not AC+DC): AD1 alternates per 100 ms window between PB1 -
  PB3 (A-JACK) and SENSE - PB5 (the range's divider tap: AC V r0 / r1 use DC
  V r1's /100 tap - INFERRED from R2C / R2D - r2 /1000, r3 /10000), IG 3.6
  both. The V-tap window's (PKHMIN + PKHMAX) / 2 is the DC; hint when |DC| >
  max(0.5 V, 2 % of the range) and the AC reading < 10 % of it, 2 s to
  assert, 1 s to clear. ENPKH goes off with the input switch and back on 3 ms
  later (`pkh_due`), so the one 1.6 ms conversion of mixed inputs never
  reaches the hold (else a V-jack DC would be an A-JACK hit). A-JACK now sees
  every other window: it still asserts on two PB1 windows in a row (~0.3 s).
* **AC?** (DC V 6 V only: the other ranges have no sentinel): the comparator
  alternates per window between PB1 and SENSE (V / 10, the model's and the
  stock formula's ratio for r0); both CMPHO and CMPLO in one SENSE window =
  the input swings both ways by > 0.21 V around zero; 2 s / 1 s. The first
  CTSTA after each switch is dropped. Since 2026-10-23 (false AC? after
  switches and on 5 V DC, TODO "False AC? / A-JACK / A-PROT after switches"):
  a window counts only while the row's DC V reading is below 2 V
  (AC_HINT_DC_MAX) and none before its first reading; DC? and AC? (not the
  power meter's) need the condition in every window for the 2 s. All
  sentinels: no sample for 150 ms after a row write and 100 ms after its
  settle, the peak hold cleared at the settle's end (sentinel.rs BLANK_*).
  V-JACK? after leaving ohm or a chip restart (device ad74e93: CTSTA 0x70 to
  ~4.4 s / 5.2 s, then 0x50):
  within 7 s a both-bit read is no hit, nor CMPLO alone within 2.5 s of the exit /
  the last 0x70 read (OHM_EXIT_*); CMPHO alone counts at once. AC?'s V-tap
  window never counts a both-bit read.
* **FUSE?** (DC / AC / AC+DC A, not the power meter): 60 s with |I| below 0.1
  % of the range (DC; 60 of 60000 counts) or 5 % (AC, AC+DC: their RMS
  floor), not OL; any reading above clears it at once. It is a question:
  with the fuse intact and nothing connected the meter reads the same zero.
* `DBG:SENTinel:VARiant +16` turns the two alternations off (the 4.6 rows).

**Fuse check** (Current Settings page, the "Fuse check" button in the
middle column above DC / AC / AC+DC, where the V page has its note line;
`SYST:FUSE:TEST`, `SYST:FUSE?`): a dialog "Lead from
V/ohm jack to A, nothing else connected" -> Start -> ohm 600 ohm, manual,
the stock row with every sentinel off (the check drives the ohm current
through the A shunt on purpose), PE = 2 as every ohm row: V/ohm -> lead -> A
-> fuse -> 10 mohm + 0.5 + 47.5 ohm -> COM ~ 48 ohm. The first final
reading of the 600 ohm row decides, one whose ohm vet saw two conversions
agree (dmm.rs OHM_VET_*, fastswitch2; until then the third raw reading
decided, which after the fast ohm entry could still be charging: owner on
10242c3, 18 ohm instead of 48, "Check leads"; senttest fuse-ok-lag): 44..54 ohm FUSE OK (green), OL FUSE OPEN (red, "Replace: 10 A
ceramic fuse"), anything else "Check leads" (amber; a short V-COM, the lead
in COM); no reading in 8 s: "No ohm reading". Then the function before comes
back (a manual DMM range too; from the power meter the energy restarts);
Cancel / a key aborts and restores; "Again" repeats; a function choice
from SCPI / BT while it runs ends it without a restore and closes the
dialog. The 0.05 ohm step with
PE0 = 1 of 4.3 is left out: it tests the MOSFET, not the fuse. `SYST:FUSE?`:
IDLE | BUSY | OK,<ohm> | OPEN | FAIL,<ohm> | FAIL,NONE; ~1 s in the emulator.

**STATus:QUEStionable**: `:CONDition?` bit 9 while a red alert (A-JACK,
OVERLOAD) is on, bit 1 while OVERLOAD (SCPI-99 CURRent; the 34401A's
"current overload"; note: in the 34401A bit 9 means "ohms overload", hosts
using SYST:IDN:EMUL 34401A may read it so); `[:EVENt]?` the bits latched
since the last read / `*CLS`. No enable / transition registers; `*STB?`
stays 0.

**Emulator** (terminal-level model, `emu/hy3131`, both copies identical):
the AD1 sentinel input in the Freq role, SENSE - PB5 through the /1000 tap
in AC A and through the range's tap in AC V, AD2 = OP2(PB1) - PB3 in the
continuity / diode / cap-oscillator roles (the peak hold now sees every
AD2 result between two bus accesses), the comparator on SENSE (V / 10) in
DC V r0; row matching frees exactly those fields; front-end keys `valead`
(a lead V/ohm -> A: fuse + shunt chain in parallel with V-COM) and
`fuseopen` (no current reaches the shunts; kept by `open`). Model checks:
362 pass. `emu/hy3131/senttest.py` 46 scenarios (27 new: ovl-dcma, ovl-dcua-man,
ovl-acma, ovl-acdc, ovl-pow, acdc-v, acdc-vac, acdc-v-hi-gap, acdc-a, aca-v, aca-vac,
aca-vi, freq-a, cont-a, diode-a, cap-a, cap-ramp-gap, acv-dchint, acv-dc-ac,
acv-dc-ajack, dcv-achint, dcv-ac-ajack, fuse-hint, fuse-ok, fuse-ui, fuse-open,
fuse-odd); every row also checks `STAT:QUES:COND?` against `SYST:ALER?`.
OVERLOAD reached 10 A ~0.5 s after 2 A appeared on the 60 mA AUTO range;
the fuse check read 47.50 ohm.

**Device test plan (this build; commit the exact tree before flashing):**

* **T8** first (gates the AD2 rows): diode 0.65 V (a 1N4148), continuity 10
  ohm, cap 100 nF / 1 uF: readings with `DBG:SENT:VAR 0` vs `8` equal within
  noise (note 7: AD2 running beside AD1). If the diode / continuity reading
  moves, keep +8 behaviour for those modes (rows back to stock).
* **T11 OVERLOAD** (current-limited bench supply in series with a load; the
  10 A fuse protects nothing below 10 A, so start at 0.2 A): DC 60 mA manual
  with 0.2 A -> OVLD + beep + toast within ~0.4 s, reads 0.200 A on 10 A;
  remove the current -> back on 60 mA after ~5 s; 0.1 A again within 30 s ->
  sticky (stays on 10 A at 0 A). DC AUTO: 1 A -> 10 A, then 50 mA -> 600 mA
  (never the uA shunt) until 0 A for 5 s. AC mA 0.5 A 50 Hz, AC+DC, the power
  meter with I fixed on 600 mA and 1.5 A: the same. `STAT:QUES:COND?` 514
  while on, `STAT:QUES?` 514 once after. V modes with current in A: A-JACK,
  never a shunt change (`DBG:SENT?`, PE stays 2).
* **T12 AC A V-JACK?**: 5 V, 24 V DC and mains on V-COM, nothing in A, AC mA
  / uA / A: amber V-JACK? after ~1 s; 10 mA AC flowing with 24 V on V-COM:
  none; AC mA 10.000 mA reading with alerts ON vs OFF equal (AD1 on + OP1's
  chopper held off must not move the RMS).
* **T13 A-JACK** in AC+DC V (1 mA DC, 10 mA AC: no flicker over the phase
  switches), Hz (1 kHz on V, 1 mA into A), continuity, diode, cap 100 nF (1
  mA and 10 mA AC); cap 100 uF (ramp): no alert (gap). No false trips over 10
  min with the A jack open in each, also while autoranging.
* **T14 fuse check**: lead V/ohm -> A: FUSE OK ~48 ohm (note the value: it
  is the real resting resistance, INFERRED so far); leads V-COM shorted:
  Check leads; lead removed: FUSE OPEN; from AC+DC / the power meter / a
  manual range: the function comes back.
* **T15 hints**: AC V with 5 V DC -> DC? within ~3 s, 1 V AC -> none; 230 V
  mains AC V reading with `VAR 0` vs `16` equal (the AD1 input switch every
  100 ms must not move the RMS / LPF / Hz); DC V with 2 V AC -> AC?, 1 V DC
  -> none, DC V zero / 1.0000 V with `VAR 0` vs `16` equal; DC A with
  nothing connected for 60 s -> FUSE?.
* **T16**: `SYST:ALER?` names, `STAT:QUES?` cleared by the read and by
  `*CLS`, the Help 5/5 rows, the Settings 3/3 masks (A-JACK only: no hints).

---------------------------------------------------------------------------

### 4.8 A-jack protection (fw-rs, 2026-10-22): the hybrid

Owner decision (TODO "A-jack protection hybrid"): keep the resting state of
4.1 in every non-current function (PE0 = PE1 = 0, ~48 ohm A-COM: the
sensitive A-JACK from ~0.44 mA), but when a current through A would
overheat the 47.5 ohm resistor, move the A jack to the 10 mohm shunt (PE0 =
1): a weak source then flows harmlessly, a stiff one blows the 10 A fuse
instead of burning the resistor. This replaces the "never switch the
MOSFETs in V modes" rule of 4.3 / 4.6: that rule assumed the current could
not be quantified there; the probe below does it.

**What it costs the resistor** (owner: a ~2512 package, assume 1 W, derate
to 0.5 W): I^2 x 47.5 ohm. 5 V across A-COM = 104 mA = 0.52 W, 12 V = 250 mA
= 3 W, 24 V = 0.5 A = 12 W.

**The probe** (`sentinel::probe_row`, `probe_amps`). PB1 (the resistor's
top, what the comparator watches) is far outside any converter range at
these currents (100 mA = 4.75 V). PB2 is the top of the 10 mohm element,
which every A-jack current passes in every PE state (INFERRED from the
shunt chain 10 mohm + 0.5 ohm + 47.5 ohm): I x 10 mohm, 0.25 A = 2.5 mV.
OP2 (x1, unused in DC V / DC mV / ohm / temperature) on PB2, AD2 = OP2O -
PB3, IG 2.0, OSR 256, the AD2 peak hold read every 100 ms: 4 194 codes per
A, the larger of |PKHMAX| / |PKHMIN| (DC either sign; AC: the peak, >= the
RMS, so conservative). The probe goes into the row at the first comparator
hit window (one rewrite, AD1 last: note 7; the DC V reading skips ~120 ms)
and leaves when the run of hits ended and nothing is latched. After every
row write with the probe in (its entry, a range / function change while it
stays) the peak hold is cleared `PROBE_SETTLE_MS` (30 ms) later and the
first window counts from there: AD2 restarts with the row and its first
results are not the input (device f3aea2f: A-PROT latched with the A jack
open on the first window, the probe read 15 mA afterwards). Normal
measuring (no A-jack current) never has AD2 on: the stock DC V calibration
conditions stay untouched.

**The rule** (`sentinel::aprot_trips`), only while the comparator run is
still going (a real current through the 47.5 ohm chain is far above its
window): >= 300 mA (4.3 W) at the first probe window (~130 ms after the
first hit window), or >= 125 mA for 400 ms counted from the first hit window
(which lags the onset by <= one 100 ms window, so ~0.5 s from the onset).
125 mA instead of the owner's ~100 mA (0.5 W): 5 V across A-COM (104 mA,
0.52 W) is to stay a warning (A-JACK only, owner), and 125 mA = 0.74 W is
still below the assumed 1 W rating; 12 V (250 mA) is well above it.

**Action and latch** (`Dmm::aprot_poll`): PE0 = 1 at once (a GPIO write, no
row rewrite), red **A-PROT** alert (badge "A-PROT", toast "A jack current:
10 A shunt on", the A-JACK beep pattern, `SYST:ALER?` APROT, STAT:QUES bit
9; above A-JACK, below OVERLOAD; not maskable). While latched every
function keeps PE0 = 1 (`write_config`) and DC V (all ranges, also 60 V+
which has no comparator) / DC mV keep the probe in the
row, now measuring on the 10 mohm path. The latch ends when **both** the
user acted (any function / range / AUTO choice: the `ovl_clear` hooks) and
the probe read < 50 mA for 1 s (in either order); then the row is written
again with the resting PE state. A current function (DC / AC A, AC+DC A)
or the power meter ends it at once: their own shunt selection and the
OVERLOAD rule take over. In a function without the probe (AC V / mV, Hz,
ohm, temperature, continuity, diode, cap) the latch holds (the A jack stays on 10 mohm) until
the user goes back to one with it or to a current function.

**Not in ohm / temperature (device 1eb4d7e).** With the A jack open and the
calibrator on V-COM, the probe read the V input in those rows: ~0.34 A per
volt (5 V DC: 1.7 A, 10 V: 3.1 A, the 0..9.6 V 40 Hz square: up to 37 A)
and latched A-PROT within ~140 ms (the f3aea2f latch was most likely this:
the run had the calibrator on while it went through ohm / temperature).
The ohm and temperature rows connect the V input to the node the probe
reads (the PB2 inference holds for DC V / mV only), so the probe is in DC V
and DC mV only; ohm / temperature keep the comparator's A-JACK, gated by
the row's own V side since 2026-10-23 (V_GATE below).

**V_GATE: A-JACK in ohm / temperature (2026-10-23).** Devices 1eb4d7e /
973df07, A jack open: 5 / 10 V DC or the 0..9.6 V square on V-COM raised
A-JACK in RES and TEMP (comparator hits on PB1), 1 V did not. The
comparator gives two bits per window (above VRH / below VRL): a PB1
excursion that comes from V-COM and one from a current in A look the same,
and the size cannot separate them either (the pickup grows with the
voltage; the probe on PB2 read ~0.34 A per volt there), so no threshold or
offset fixes it - the same finding as Hz, which has no sentinel at all. The
difference from Hz: these rows measure the V jack themselves. Ohm: a
passive Rx never gives a negative AD1 code; a source on V-COM above the ohm
source's open-circuit voltage (~1.4 V) reverses the drop across Rref, and
any negative source drives the node below COM: both give the negative code
that LIVE? already uses (`LIVE_CODE`). Temperature: the thermocouple input
reads |V| > 80 mV (`T_VDRV_MV`; type E is 76.4 mV at 1000 C, the readout is
OL above 1000 C K = 41.3 mV; an open TC read ~212 C = ~7.6 mV on the
device); the RTD / NTC sensors use the ohm path and its LIVE? sign. The case
A-JACK is for there - the leads in A / COM, V-COM open - shows neither. So
in modes 10 / 15 A-JACK asserts only when a V-side result since the hit run
began said V-COM is free (ohm: the next conversion; TC: up to one ~2.2 s
CJ / TC cycle later), and an asserted A-JACK drops when the V side turns
driven; ohm then shows LIVE?. `DBG:SENT?` field 12: -1 not gated / no
result yet, 0 free, 1 driven. Senttest `ohm-v5` / `temp-v5` with the
model's `Pb1VjackMv 10` (PB1 picks up 10 mV per volt on V-COM in those rows:
INFERRED, any k with 1 V below and 5 V above the 23.75 mV threshold fits
the device): HEAD 70572bb A-JACK, the gate LIVE? / NONE, and A-JACK with a
current in A and V-COM open.
Residual gaps: a source between the pickup threshold (~1-2.4 V by the
device) and ~1.4 V in ohm (no negative code yet) would still trip if the
real k is near the top of the range; a current in A WITH a source on V-COM
at the same time is not reported in ohm / temperature (LIVE? is); an open
TC input that read > 80 mV would hide A-JACK in TC mode (not seen: 212 C).
Device retest: TODO "False A-JACK in ohm / temperature".

**Limits (not protectable by switching).**
* **> 30 V on A-COM** (mains, a battery pack): the MOSFETs are 30 V parts
  (RUH30J51M, 3.6); the drain-source voltage of the off MOSFETs at rest is
  the source voltage minus nothing, so they may break down before (or
  while) PE0 switches. The fuse (10 A ceramic) is the only protection; the
  firmware can only warn and switch as fast as it can.
* **Jack alerts OFF** (or A-JACK masked) turns this off too: the comparator
  hit is the probe's trigger.
* **AC+DC** (its phases rewrite the row every cycle) carries no probe:
  A-JACK only.
* **AC V / mV, frequency, continuity, diode, capacitance** have no free
  op amp / converter pair for the probe (AD1 peak on PB1 saturates at ~7 mA,
  OP2 / AD2 are busy or the AD2 sentinel watches PB1): A-JACK only there.
* The probe's numbers are INFERRED (PB2 node, OP2 x1 path); the model's
  peak-hold noise is ~30 mA. Device check below.

**Emulator** (both HY3131.cs copies): `Pb2Volts` = I x 10 mohm, AD2 on PB2
when R32's OP2 nibble is 0xE (`Ad2OnPb2Probe`) in the DC V / mV / ohm /
temperature roles; the role matcher frees AD2IG / R26 / R27 / OP2 there.
senttest (A-PROT latency = DBG:SENT? field 11, ms from the first hit window):

| scenario | result |
|---|---|
| aprot-dcv-5 (avsrc 5 V, DC V) | A-JACK only (never A-PROT) |
| aprot-dcv-12 | A-PROT after 409 ms (454 ms with the settle); the source removed: stays latched; the AUTO key (DBG:KEY 2, a user choice; was CONF:RES: ohm has no probe now) -> released, NONE |
| aprot-dcv-24 | A-PROT after 154 ms (the first probe window after the 30 ms settle; 105 ms before it) |
| aprot-glitch | the model's AD2 start-up off by 3000 codes (0.72 A, `Ad2StartGlitch`), 1 V across A-COM (21 mA): A-JACK only (a60f8ce: A-PROT) |
| aprot-ohm-12, aprot-temp-12 | A-JACK only (no probe in ohm / temperature since 1eb4d7e's device check; was A-PROT after 409 / 406 ms) |
| aprot-ack-early | the user acts while the source is still there: latched until 1 s after it went |
| aprot-to-dca | CONF:CURR:DC ends the latch (DC A's own shunt / OVERLOAD) |

After PE0 = 1 the model has no fuse to blow: a stiff `avsrc` drives
V / 60 mohm (the probe clips at 62.5 A).

#### Device check list (A-jack protection)

Bench supply with current limit, leads V-COM open, the supply across A-COM:
1. DC V, 5 V / 1 A limit: A-JACK (red, beeps), never A-PROT; `DBG:SENT?`
   field 10 (probe mA) ~100.
2. DC V, 12 V: A-PROT within ~0.5 s, the supply goes into its current limit
   (10 mohm path); `DBG:SENT?` field 11 ~400 ms. Supply off: still A-PROT;
   tap another function: released, A-JACK gone.
3. Ohm and temperature: A-JACK only (no probe there, above).
4. DC V reading with nothing on A: unchanged vs the previous build (the
   probe is not in the row); with the probe in (a small current in A),
   compare the DC V offset of a known source (note 7).
5. Do **not** test above 30 V across A-COM.

### 4.9 "Leads in A?" reminder (fw-rs, 2026-10-22)

TODO "Leads in A?" (after the UNI-T UT197 comparison in TODO.md): the UT197
senses a plug in its current jacks and flashes "InErr" in any non-current
position, before the probe touches a circuit. The DM40 has no jack sense
(4.1), so an unconnected lead in the A jack is invisible; the closest
firmware stand-in is to remind at the moment the UT197 would warn: the
function change **away from a current function**.

| | |
|---|---|
| fires on | a change from a function whose leads use the A jack (DC / AC / AC+DC A, any range or shunt; the power meter) to one that does not (V, mV, ohm, continuity, diode, cap, Hz, temperature, V AC+DC), by any path: the function buttons, the PD6 click, SCPI CONF / FUNC, the BT mode byte, the power meter's exit. `sentinel::Leads` (pure, host test `leads_reminder`) fed per drawing pass by `App::leads_tick` with `Dmm::uses_a_jack` (HY3131 mode 4..=9 or the power meter; mode 255 = a row change in progress, ignored) |
| never on | V -> A, A -> A (DC <-> AC <-> AC+DC A, autorange / OVERLOAD shunt changes, A <-> power meter), the first function after power-on / restore, the fuse check (it moves a DC A meter to ohm with a lead in A on purpose and comes back: the state is held while `fuse_state()` is Busy; a check ended by a V choice counts as A -> V) |
| what | one 200 ms beep at once (key / alert volume, also with the key beep off, as OVERLOAD's; 0 % = silent) and one toast "Leads in A jack? Move to V" (red, `TOAST_MS` = 1.5 s) when the new function's page has its first reading (the toast holds the repaint: over an empty page it looked broken in the emulator), at the latest 1 s after the change; dropped if the function went back to a current one meanwhile; not over the settings pages (the alert toasts' rule) |
| repeat / dismiss | **neither**: one beep, one toast that goes by itself (a tap closes it early, as every toast). The firmware cannot tell whether the lead was moved, so a repeating or must-acknowledge reminder would nag every time a careful user already moved it; the A-JACK sentinel (4.6) takes over with its red badge and repeating beep the moment a current does flow through A. If owners find one toast too easy to miss, the next step is a second beep, not a modal |
| HOLD / decision #5 | unaffected: the reminder is no alert state (`SYST:ALER?` stays NONE, no badge in the status slot, no STAT:QUES bit), so it never alternates with HOLD; a real A-JACK after it shows its own toast (replacing this one), badge and beeps |
| option | Settings 3/3 row "A-lead reminder" ON (default) / OFF, separate from "Jack alerts" (whose ON / A-JACK only / OFF cycle keeps it); `SYST:ALER:ENAB` bit 3 (`sentinel::M_LEADS`; ON = 15, `?` answers 15 by default, 0..15 accepted); persisted in p[38] bit 15 `EXT_LEADS_OFF` (0 = on: older records get it on). Changing only bit 3 does not rewrite the HY3131 row |
| host scripts | a script that alternates A and V functions with leads in both jacks (the power-meter wiring) gets a beep per change: `SYST:ALER:ENAB 7` |
| SCPI debug | `DBG:LEADs?` -> `n,from,to` (reminders since power-on, `Dmm::func` codes of the last change that fired; 255 = none) |

Toast text: the TODO's "Leads in the A jack? Move to V" leaves 1 px to the
toast frame in TEXT22 with the toast's letter spacing (ui-render margin
rule), so it reads "Leads in A jack? Move to V".

Emulator: `emu/hy3131/leadstest.py` (DC A -> a tap on the DMM page's V
button -> 1; DC A -> AC A no; AC A -> AC V 2; ENAB 7: DC A -> ohm silent;
power meter -> DC V 3 from 18; fuse check from DC A (`valead`, OK 47.50
ohm, back in DC A) no; A AC+DC -> V AC+DC 4 from 17 to 16; SYST:ALER? NONE
and STAT:QUES:COND? 0 throughout); screenshots `emu/out/leads-before.png`,
`leads-toast.png` (over the DC V page with its first reading),
`leads-after.png` (gone by itself). ui-render: the toast (t05-toast-leads),
Settings 3/3 with the new row (m10-settings3, all value combinations, the
pressed rows), Help 5/7 "Jack/A-lead alerts".

#### Device check list ("Leads in A?")

1. DC A (any range) -> V button: one 200 ms beep at once, toast "Leads in A
   jack? Move to V" over the DC V reading, gone after ~1.5 s with no tap.
2. AC A -> ohm, A AC+DC -> V (the AC+DC cycle of the V key does not count:
   only a change of the jack), power meter -> Hz: the same. DC A -> AC A,
   V -> A, A -> power meter: nothing.
3. PD6 click while locked (sub-mode cycle): nothing (V stays V, A stays A).
4. Key / alert volume 0 %: no beep, the toast still shows; key beep OFF in
   Settings: the reminder still beeps.
5. Settings 3/3 "A-lead reminder" OFF: nothing on A -> V; the setting
   survives a power cycle; "Jack alerts" cycling leaves it alone;
   `SYST:ALER:ENAB?` 7 then, 15 after ON.
6. Fuse check from DC A (Current Settings > Fuse check, lead V -> A): no
   reminder during it or when it returns to DC A.
7. With the lead really in A and a current flowing after A -> V (a 1 mA
   source): the reminder, then A-JACK (red badge, its toast, 3-beep
   pattern) within ~0.3 s: A-JACK wins.

### 4.10 Danger alarm (fw-rs, 2026-10-24): loud, whatever the volume

Owner request 2026-10-24: a wrong connection that is actually dangerous
also beeps at **maximum** volume, overriding the key / alert volume (the
owner keeps it at 0 %). Code: `src/danger.rs` (pattern, event rules, the
10 A rule; host tests), `App::alert` / `App::alert_beep` / `App::alarm_hit`
(`src/app.rs`). `sentinel.rs` is unchanged: the alarm reads the alert of
each reading and the pub `Sentinel::probe_a`.

**The stock has nothing to copy.** The V2.6 RE notes
(`firmware/re/analysis/lcd_buzzer_v2_6.h`, `dmm_v2_6.h`) show no misconnect
alarm at all (no jack sensing, 4.1); the buzzer is TIM14 CH1 (PA7) at
3.57 kHz (PSC 4, ARR 11200), loudness = duty, `min(v/3, 0.99) x 0.5`, so
the stock's loudest is 49.5 % duty at volume 100 %; it beeps from a 2-entry
queue in the 1 kHz TIM7 ISR (50 ms per key, 150 / 300 ms in the DMM).
Stock keys (MENU, RUN, AUTO, MF; the PD6 power key) always beep and act;
there is no cancel / mute key.

**Classification.**

| alert / cue | dangerous? | why |
|---|---|---|
| **A-PROT** (4.8) | **yes**, while its current flows | a source across A-COM pushed >= 125 mA (0.74 W in the 47.5 ohm resistor) for 0.4 s or >= 300 mA at once: the firmware moved the A jack to the 10 mohm shunt, so a stiff source now runs into the 10 A fuse, and above 30 V the MOSFETs themselves are at risk (4.8 "Limits"). Latched until the user acts and the current is gone; it trips only in DC V / mV, which keep the probe in the row while latched, so the alarm follows the probe (`probe_a` >= `APROT_GONE_A` = 50 mA); the latch without current is only waiting for the user (badge, no alarm). In a function without the probe (the latch holds there too, 4.8) its last reading stands: the source removed first, then another function (the user doing the right thing) stays quiet; a function change with the source still there keeps the alarm (the cap below applies) |
| **A-JACK** (4.6) | **yes**, where confirmed (next row but one) | the leads are in A / COM while a V / ohm / ... function is selected: the meter is a ~48 ohm path in series with whatever the probes touch. Probing a supply "for volts" is the classic way meters blow fuses and resistors; in AC V / mV, ohm, temperature, Hz, continuity, diode, cap and AC+DC there is **no probe** (4.8 "Limits"), so A-JACK is the only warning before a 12-24 V source cooks the 47.5 ohm resistor: A-PROT can never fire there. Also: a reading acted on is wrong (the meter shows the V function's idea of a circuit it is in series with), and a function / range change made to "fix" it can switch the A jack to a 10 mohm or 0.5 ohm shunt across a live source |
| **current above the 10 A rating** | **yes** (new condition, no badge of its own) | the DMM's A functions reading > 10 A (or OL on the 10 A range itself: DC mode 7 / AC mode 4 range 1; AC+DC: the total), the power meter's I channel on its 10 A range (ik 5) reading > 10 A or OL, for >= 1 s (`I10_ASSERT_MS`: a motor / lamp / capacitor inrush is the fuse's normal life). The manual's limit is 10 A behind one 10 A ceramic fuse (4.7): above it the shunt and the fuse heat for as long as it lasts, and a fuse carries 110 % for a long time, so the meter overheats before anything opens. OL on a lower range (manual 6 A, the uA / mA shunts) says nothing about the amps and is not counted |
| **A-JACK in the ohm-network rows** (ohm, continuity, diode, capacitance, the resistive temperature sensors; branch sentinel2, 2026-10-24) | **no** (badge, toast and log as before; `Sentinel::soft`, DBG:SENT? last field 1) | its only evidence there is PB1 (the comparator in ohm, the AD2 peak in continuity / diode / cap), and PB1 follows a voltage on V-COM in those rows: the RES 5 V A-JACK (35fce2a 7b, E4) and the continuity 5 V one (f0d21eb item 4) are false, and on 612c45c the RES one sounded this loud alarm (6 events of ~3 s in the E1 subset, at night). The V side cannot tell (5 V on V-COM reads OL, as open leads do), and the PB2 probe is no help (it follows V-COM there too, ~0.34 A per volt: off in those rows, 4.8). So the alarm keys on `Sentinel::ajack_alarm()` = A-JACK in a row whose evidence V-COM cannot fake (DC V 6 V, DC mV, AC V / mV, the thermocouple with its V_GATE, AC+DC V), never on the alert alone. A real current in A while in RES shows the same badge without the alarm; the switch to DC V (or any non-soft row) sounds it within ~1 s. A-PROT keeps its own rule (probe-confirmed, DC V / mV). The owner's decision on the RES A-JACK badge is still open; this changes only the sound. Emulator: loudtest `ajack-res` (RES, 5 V on V-COM with the model's PB1 pickup, then a real 2 mA in A: badge, quiet; CONF:VOLT:DC with the current in A: loud), `ajack-cont` (continuity, 2 mA in A: badge, quiet) |
| OVERLOAD / OVLD, amber 10A (4.7) | no | the meter protected itself (uA / mA shunt -> 10 A shunt) and measures fine: its one 300 ms beep at the key / alert volume stays |
| power meter current limit expired ("Current too high", 3.13) | no | a user-set thermal budget (default 6 A for 30 s), not a hardware limit: its 3 x 50 ms every 2 s at the key / alert volume stays (above 10 A the rule above sounds anyway) |
| V-JACK? (amber), LIVE? (amber), DC? / AC? / FUSE? (grey) | no | advisories: no current flows through the meter (V-JACK?, hints) or the source is in the circuit under test, not in the meter (LIVE?: 100 ms beep as before) |
| "Leads in A jack?" reminder (4.9), fuse check (4.7) | no | reminders / a guided test: their beeps are unchanged |

**Pattern** (`danger::tone_at`): a burst of four 125 ms tones, high / low /
high / low (3.57 kHz = the stock ARR 11200, and 2.86 kHz = ARR 14000, a
major third under it), then 500 ms quiet: one burst per second, 50 % of the
time sounding, unlike any other beep of the meter. Duty 49.5 % of each
tone's period (CCR1 5544 / 6930) = the stock's 100 % volume, whatever the
key / alert volume. ARR and CCR1 are preloaded (CR1 ARPE, CCMR1 OC1PE), so
a tone change lands whole at the next 0.3 ms period.

**Event rules** (`danger::Alarm`):

* an event starts when the condition turns on (the first reading that
  carries A-JACK / A-PROT with current, or the 10 A rule after 1 s) and
  sounds at once;
* **silence** for this event: a tap on the alert badge (the DMM status slot
  119..202 x 1..33, the combined view's strip badge; that tap does nothing
  else), a tap on the alert's toast (which closes, as any toast tap), or
  any key (MENU / RUN / AUTO / MF, which still do their job, as stock keys
  always do; the PD6 power key too). This works also with the touch lock
  on, a dimmed screen or a screenshot running;
* **re-arm**: the condition gone and back is a new event: it sounds again,
  also after a silenced one;
* **stop**: the pass after the condition clears (A-JACK: its own 1 s
  clear; A-PROT: the probe below 50 mA, ~100 ms windows, in DC V / mV; the
  10 A rule:
  the first reading at or below 10 A), and when the DMM stops measuring
  (the scope / generator views switch its rail off: no readings, no claim);
* **cap**: the full pattern for 30 s (`FULL_MS`), then one burst every 10 s
  (`REMIND_MS`, 5 % sounding) until the event ends or is silenced: a latch
  that outlives its cause (A-PROT outside DC V / mV) cannot hold the
  buzzer on for good, and a forgotten danger still reminds;
* while it sounds the alarm owns the buzzer: it re-asserts its tone every
  main-loop pass (written only when TIM14 differs: one MMIO read of ARR /
  CCR1 per pass), so a key / tap beep in between cannot leave it muted;
  at its end CCR1 = 0 and ARR = 11199 (the stock pitch for every other
  beep).

**Setting.** Settings 3/3 row "Danger alarm" (third row, after "A-lead
reminder"): **Loud** (default) -> **Volume** (the same pattern at the key /
alert volume; 0 % = silent) -> **Off** (no sound; badge and toast as
before). Persisted in p[38] bits 16..17 (`meter::EXT_DANGER_SHIFT`, 0 =
Loud: older records get Loud). SCPI `SYSTem:ALERt:ALARm LOUD | VOLume |
OFF` / `?`, `SYSTem:BEEPer:VOLume <0..100>` / `?` (the key / alert volume,
for the tests and host scripts), `DBG:ALARm?` -> `<sounding>,<silenced>,
<events>,<setting>`. The "Jack alerts" mask still decides whether A-JACK
exists at all (OFF / A-JACK only: no A-JACK, no A-PROT, so no alarm).

**Measurements, ISR, memory.** The drive is the stock's own: TIM14 PWM on
PA7, the same <= 49.5 % duty a stock key beep at volume 100 % uses, from
the main loop only (no ISR touches TIM14; the AD2 stream ISR and the HY3131
SPI are unrelated pins and peripherals). The only new thing is how long it
sounds (seconds instead of 50-300 ms) and the 2.86 kHz tone. Whether the
buzzer's supply current couples into the HY3131 front end cannot be
checked in the emulator (no analog supply model): device check 4 below.
Code size: region B -1.5 KiB (39 772 B free), region C and SRAM free
unchanged, stack headroom 21 152 B (stackdepth --check 20480 passes).

**Emulator** (`emu/hy3131/loudtest.py`; `run.py --buzzer-log` makes the
TIM14 model, `emu/peripherals/PwmTimer.cs` `LogWrites`, log every change of
ARR / CCR1 in virtual ms; key / alert volume 0 % via SCPI):

| scenario | script (virtual s) | result |
|---|---|---|
| aprot | 10 s: 12 V across A-COM (DC V); 15.45 tap on the badge; 18 source off; 21 AUTO (the user acts); 25 12 V again; 31 off | quiet before; first tone 0.25 s after the source (at A-JACK, A-PROT follows), CCR1 5544 / 6930 only (ARR 11199 / 13999), steps 125 ms (one 95: a ~30 ms late pass), bursts 1.00 s apart; the tap mid-burst: off 0.15 s later (the tone in progress ends at its step), nothing until 25; the 2nd event loud 0.33 s after the source; source off at 31: off at 31.40 s **while A-PROT stays latched** (SCPI `APROT` with `DBG:ALARm?` 0,0,2,0); ARR 11199 / CCR1 0 after every stop |
| ajack | 10 s: 1 mA into A (DC V); 15.45 RUN key; 18 off / 21 on; 26 off; 29 on; 29.5 tap on its toast | loud at 10.28 s, 125 / 1000 ms exact; RUN mid-burst: off at once, silent until 21; re-armed at 21.21; current off at 26: the last burst at 26.55 (A-JACK clears 1 s after the current), none from 27.2; 3rd event 29.15, toast tap 29.5: off at 29.65, silent to the end |
| vjack-off | DC A, 24 V on V/ohm (V-JACK?) 10..19 s; then Off + DC V, 1 mA into A at 26; then Volume at 40 % | no alarm write in 6..33 s (V-JACK?, then A-JACK with Off: `DBG:ALARm?` 0,0,1,2); Volume: two-tone at CCR1 746 / 933 (40 %) from 33.2 s, nothing louder |
| aprot-ohm | 10 s: 12 V across A-COM (DC V); 15 source off; then CONF:RES (the user acts in a function without the probe) | loud, quiet after 15 s, and no new alarm in ohm while A-PROT stays latched there (`APROT` with `DBG:ALARm?` 0,0,1,0). The first build (afb6304) took the latch in ohm as dangerous and sounded again: fixed |
| i10 | DC A, 5 A, 12 A at 16 s (OL on the 10 A range), 5 A at 24 | quiet at 5 A; loud from 17.35 s (the range walk + 1 s); back to 5 A at 24: the burst due at 24.35 s never comes |

Not run in the emulator: the 30 s -> one burst per 10 s cap (host test
`pattern_two_tone_then_reminders` only), the combined view's strip-badge
tap, the power meter's 10 A rule (host test `over_rating_rule`).
senttest `aprot-dcv-12`, `dcv-adc`, `dca-v` unchanged (OK). Host tests
`tools/meter-test/run.sh danger` (pattern, event rules, the 10 A rule);
ui-render: Settings 3/3 with every value (m10-settings3, the matrix, the
pressed rows), 0 violations.

#### Device check list (danger alarm)

The sound of A-PROT / A-JACK needs a current in the A jack: only with a
bench supply (current limit!) or a 1 mA source across A-COM. Key / alert
volume 0 % (the owner's setting), Settings 3/3 "Danger alarm" Loud.

1. DC V, 1 mA into A-COM (or the supply at 5 V with a 1 kohm resistor in
   series): red A-JACK and at once the two-tone burst every second, loud.
   Tap the A-JACK badge (top bar, middle): silent, the badge stays. Remove
   the current, apply it again: loud again. Remove it: silent within ~1 s.
2. Again, and press RUN instead of tapping: silent (and HOLD toggles, as
   the key always does). The power key and the toast tap silence too.
3. DC V, 12 V across A-COM through the supply's current limit (4.8 item
   2): A-JACK -> A-PROT, loud; supply off: silent at once while A-PROT
   stays latched (red badge); tap another function: the latch ends.
4. Measurement coupling: DC V on a stable reference (e.g. 1.000 V), a 1 mA
   source in A (A-JACK, loud), note READ? / the display for 20 s; silence
   it (tap) and note 20 s more: the mean and the scatter should not change
   by more than a count. If they do, the alarm needs a gap per reading.
5. Is the low tone (2.86 kHz) about as loud as the high one? If it is
   much weaker (off the buzzer's resonance), the pair moves to 3.57 /
   4.17 kHz (`BUZZER_ARR_LO`).
6. Settings 3/3 "Danger alarm" Volume with the volume at 0 %: no sound;
   at 50 %: the same pattern, quieter. Off: no sound, badge and toast as
   before. The setting survives a power cycle.
7. V-JACK? (DC A, 24 V on V-COM, no current): no alarm. OVERLOAD on a
   manual mA range: one 300 ms beep only (at the key volume: silent at 0 %).
8. Above 10 A (if the bench allows it, briefly): > 10 A for 1 s in DC A ->
   the alarm; back below 10 A -> silent.

### 4.11 Device f0d21eb follow-up (fw-rs branch sentinel2, 2026-10-24)

Device check "f0d21eb" in `device-check-2026-10-21.md` (meter on f0d21eb, A jack
not wired, the calibrator on V-COM only). Raw data: the session scratchpad's
`chk3/out/` (`ctsta_summary.txt`, `E_*_raw.json`, `acarange*.txt`).

**What CTSTA means (from the device, not the datasheet).** Over all `E_*_raw.json`
rows of chk2 / chk3 (~233 000 DBG:SENT? reads in comparator and other kinds)
CTSTA took 0x00, 0x10, 0x50 and 0x70 only, never 0x20 or 0x60. CMPHO never
comes without CMPLO, so the two bits are not "above VRH" / "below VRL" (that
pair would give 0x20 for a node above the window and never 0x70): the fit is
CMPHO = node above VRH, CMPLO = node above VRL, ACPO (bit 6) the latch, set
above VRH and reset below VRL (a Schmitt pair). Read that way 0x70 = above the
window, 0x50 = inside it coming from above, 0x10 = inside it coming from
below, 0x00 = below it. It fits DC V's V-tap window (+1 V: 0x10, +5 V: 0x70).
INFERRED, and the emulator's model keeps the older reading (CMPLO = below
VRL); the firmware rules were not changed on this basis (a reading with any
bit set is a hit as before). Two consequences for the A rows: with V-COM open
the DC A comparator reads 0 = **below** VRL, i.e. SENSE sits more than ~21 mV
below the window; and the 24 V V-JACK? case is 24 mV through the /1000 tap
against a 20.8 mV threshold, a 3 mV margin, so a history-dependent offset of
tens of mV decides whether 24 V is seen at all.

**1. False V-JACK? in every AC A range (2a). Fixed.** The AC A sentinel put AD1
on SENSE - PB5. PB5 as an AD1 reference exists in the stock only in DC V
60 V+ (their power bus driven by OP3, SMODE 0100, PA8 P); the A rows run the
bus as SGND and never reference PB5, so in AC A it is an undefined node: the
peak ran from ~-117 000 at 0.7 s to the negative rail (-262 144) by ~2 s with
V-COM open, 1, 5, 10 or 24 V alike. Now AD1 = SENSE - AGND (R24 0x03): AGND is
what the DC A comparator's window hangs on. Because the A rows' SENSE offset
is tens of mV (above), a DC level threshold of 2 V (1 510 codes = 2 mV) is
meaningless, so AC A counts the hold's swing only: PKHMAX - PKHMIN > 4 270
codes = 2 V rms AC on V (`PKV_SWING_THR`). Mains on V in an AC current
function is the hazard and is still caught; **DC on V in plain AC A is no
longer flagged** (DC A's comparator and the DC phase of AC+DC A / AUTO A
cover DC). A hold at a rail with no swing is no hit. In AC+DC / AUTO A the
AC phase's swing-less windows carry the DC phase's comparator run instead of
ending it (`Sentinel::bridge`; before, the AC phase's level kept it going), so
a DC source there keeps its V-JACK? timing. `DBG:SENT:VAR +32` puts the old
row back for a device comparison.

**2. False OVERLOAD in AC A, 60 mA falling back to 10 A. Fixed (by a hold).**
Not the AD1 drift: the OVERLOAD rule reads the AC A reading (RMS and LPF of
AD2 on OP1O - PB3), and only 10 A -> 60 mA trips (600 uA -> 60 mA and DC mA ->
AC 60 mA hold): the shunt (10 mohm -> 0.5 ohm), OP1's input (PB2 -> PB1) and
gain (x1 -> x10) change at once, and the first block(s) carry that step. Plain
AC A now holds over-range results back for 500 ms after the first result of a
row (`AC_OVER_HOLD_MS`; no reading, range move, OVERLOAD or AUTO hop). A real
over range still reads over after the hold and acts then (OVERLOAD / the AUTO
hop ~0.5 s later after a switch; the protection is thermal, seconds). One that
starts later acts at once. The step's length on the device is not known: if
the retest still trips, `AC_OVER_HOLD_MS` is the knob.

**3. A real 24 V on V-COM missed in DC A (after DC V 60 V, RES, a chip restart).
Not fixed; cause outside the HY3131 registers.** `write_config` writes all 22
bytes R20..R35 from the row every time, zeroes R26 / R34 / R36 before
(`quiesce`), and nothing writes R37. `DBG:SENTinel:REG?` (new) reads R20..R37
back: in the emulator DC A after DC V 60 V, RES, TEMP and AC V reads the same
24 bytes (`10DD1783 35010055 31000000 10000881 80C73DA4 00500000`, mode 9 r0).
So no register carries the history; what TEMP sets and DC V 60 V+ / the ohm
rows / a power-up clear is board state (the divider and its capacitors, the
power bus / PA8 node, the relay, which is pulsed only on a change). With the
3 mV margin above, a history offset of tens of mV is enough. What the firmware
can offer without the netlist is an experiment, default off:
`DBG:SENTinel:PRECond <ms>` runs the thermocouple row (relay to the ohm side,
OP3 on PA7) for that long before a DC A / mA / uA row is entered after DC V
60 V+, an ohm-network row or a chip restart (`a_blind` in `DBG:SENT:REG?`).
In the emulator (`vjblind:1`, a replay of the device table) it brings V-JACK?
back at 2.2-2.3 s in every case; on the device it tests the hypothesis. It
blocks the main loop for <ms> and pulses the relay twice. The RES-exit mask
(OHM_EXIT_*) is unchanged: RES -> A at 24 V ends at 0 after the 0x70 tail, so
a shorter cap buys nothing until the blindness is understood.
DBG:SENT?'s ctsta field is now -1 in kinds that sample no comparator: it was
the last comparator row's value, and the f0d21eb check's "AC V reads 0x70
after TEMP" was TEMP's stale read.

**4. A-JACK in continuity at 5 V DC. Badge unchanged, no danger alarm (4.10).** The
continuity row's AD2 peak on PB1 read -97 000..+123 000 at 5 V (~+-230 mV at
PB1, an oscillation: both signs) and +-400 at 1 V (LIVE?, right). It is the
same premise as the RES 5 V owner item (PB1 follows V-COM in the ohm-network
rows): at 5 V the continuity reading shows no live source (no LIVE?), so no
V-side gate could tell it from a current. Owner decision with the RES case;
since 612c45c's danger alarm the ohm-network rows' A-JACK is badge only (4.10,
`Sentinel::soft`), so neither case sounds the loud alarm.

Emulator (opt-in knobs, `emu/hy3131/README.md`): `pb5drift:0.6;pb5tau:2.2`
(AC A AD1 SENSE - PB5 to the rail), `acastep:200000;acastepms:600` (AC A 10 A
-> mA: the first blocks over range; length chosen, not measured),
`vjblind:1` (DC A SENSE comparator sees V only after a temperature row).
switchtest: `aca-drift`, `aca-ovl`, `real-vjack-ac`, `real-vjack-blind`,
`real-vjack-blind-prec`.

### 4.12 Device 0774e70 follow-up (fw-rs branch sentinel3, 2026-10-24)

Device check "0774e70" in `device-check-2026-10-21.md` (raw: the session
scratchpad's `chk7/out/sp_s*.json`, `sent5_*.json`).

**The DC A precondition is on by default (100 ms).** The device showed
that no register carries the 24 V history (`DBG:SENT:REG?` was the same 48
hex digits in every DC A entry), and that 100 / 300 / 1000 ms of the
temperature row before the DC A row bring V-JACK? back at 1.9 / 2.1 / 2.8 s
after DC V 60 V and after RES, with no false hit at 5 V or with the source
off. `Dmm::precond_ms` now defaults to `PRECOND_MS` = 100. It runs only
when a DC A / mA / uA row is entered as a function change while `a_blind`
(set by DC V 60 V+, an ohm-network row or a chip restart; cleared by a
temperature row or by the precondition itself), with V-JACK? enabled and no
fuse check. `DBG:SENT:PREC 0` turns it off. **Cost** (emulator,
switchlat `--dc-only`, FAST): the first fresh DC A reading after ohm went
from 0.34 s to 0.55 s. That is the blocking 100 ms, the relay pulse there
and back (2 x 10 ms), and the AD1 grid. Entries that are not blind (from DC
V 6 V, AC V, the other A rows, a second entry) pay nothing.

**AC A: SENSE itself is the drifting node, not PB5.** With AD1 on SENSE -
AGND the device hold still sat at the negative rail (below -347 mV at IG
3.6) from ~0.9 s at 1 V and with the source off, and at 24 V DC it ramped
there over ~3 s. `DBG:SENT:VAR 32` (the old PB5 row) looked the same. So the
/1000 tap on SENSE through SS5 is not a defined node in the A rows: they run
the power bus as SGND with PA8 P, and the divider's reference is not
driven. This is the same node that the DC A comparator reads as below its
window with V-COM open (4.11). What the firmware can use is an AC swing.
A ramp gave 9 000-12 000 codes per 107 ms hold, above the 4 270-code swing
threshold: a false V-JACK? at 1.5-3.5 s in 60 mA / 600 uA. **The swing rule
now also needs a reversal.** The hold's range must overlap the previous
hold's range by more than `PKV_SWING_THR`. Consecutive holds of a ramp
barely overlap; an AC signal fills the same span in every window, and so
does one clipped at the rail on one side. Cost: one hold more (~107 ms) to
the first hit. `DBG:SENT:VAR +64` runs this AD1 at IG 0.9 (full scale
1.39 V) to see where SENSE sits on the device. The swing threshold is not
rescaled for that diagnostic.

**No reading crosses a range change.** Device: for ~0.9 s after
`CONF:CURR:AC 10` -> `0.06`, FETC? answered the 10 A row's value, and the
first 60 mA reading was 1 % of it (1.902e-2 -> 1.925e-4, 4.92e-2 ->
4.92e-4). That is the AC EMA's 0.99 step from the old value. FETC? now
answers +9.91E37 when the shown reading is not from the row the chip runs
(`Dmm::reading_current`: mode, AC+DC either phase, and range; not in Hz,
whose readings carry the counter's range, nor in the power meter, nor under
HOLD / AutoHold). The AC filters (`f_ac`, `f_lpf`, `ac_raw`) restart on
every plain range step, not only on a mode change. The emulator does not
reproduce the device's stale FETC? (its first 60 mA reading is fresh), so
the guard is checked by reasoning and on the device.

Emulator: `senseneg:0.75;sensetau:5` (AC A SENSE falls V x (1 -
exp(-t / tau)) from the first AC A row: the 24 V ramp, ~10 k codes per hold
at 1 s, the rail by ~3.5 s); switchtest `aca-ramp` (24 V DC through AC A /
mA / uA / 10 A: FAIL on e7d5382, OK now); `real-vjack-blind` passes with the
default precondition (it failed on e7d5382).

## 5. Open points for the owner

1. The DM40 manual's A-input fuse rating(s), CAT rating and 10 A time limit
   (3.6, 4.3): needed for the entry dialog and the current-limit default.
2. Whether a single fuse protects both the 10 mohm and the 47.5 ohm path
   (single current jack): decides how much the OVERLOAD rule of 4.3 matters.
3. T1 (OP2 on SENSE) is the only hardware gate for assignment B; T2/T3 set the
   accuracy and cadence numbers of 3.4/3.5; T7/T8 decide which sentinels may
   use a converter.
4. BT protocol: no mode slot for power; SCPI-only unless the BT cmd set is
   extended (bt-protocol.md).

Answers from the DM40 user manual (EN v1.x, emu/out/tmp/manual/manual.txt, section 2.4 and
Table 2.3.1; checked 2026-10-22):
- Points 1-2: ONE current jack "A mA uA", "10A (Max)", "Built-in 10A ceramic fuse. Replace with the
  same model if burned" -> a single 10 A ceramic fuse in front of all shunts (10 mohm and 47.5 ohm
  alike). The manual gives NO time limit for 10 A and no mA/uA fuse. Current ranges (DM40C):
  600 uA / 6000 uA / 60 mA / 600 mA / 6 A / 10 A; AC V max 750 V, DC V 1000 V.
- CAT: the manual does not state one for the DMM inputs (only the scope probe: x1 150 Vrms CAT II,
  x10 300 Vrms CAT II); the case print reads "CAT IV 600V CAT III 1000V" (owner's photos, emulator
  panel). Treat the case print as the vendor's claim, unverified.
- Defaults chosen: hard limit 10 A (manual max), a 30 s countdown above 6 A (the top regular range;
  no vendor duty-cycle figure, so the common hand-held DMM practice of >= 30 s limits at the top
  range), the 47.5 ohm path protected only by the 10 A fuse -> the OVERLOAD rule of 4.3 matters
  (a uA/mA-range overload heats the 47.5 ohm resistor long before the fuse opens). Implemented with
  these answers: section 4.7.

### HPF bypass probe (device 7889e6c, 2026-10-22)

Question: can the RMS block's high-pass filter be turned off so X·Y includes DC (hardware DC power)?
The datasheet (13.2) defines X / Y as AD2 / AD3 "after the High Pass Filter" and documents no control
bit for it. Probed the reserved bits of the related registers on the device: power mode, AC method,
6 V range, 1.000 V DC on V-COM, R34 frozen at YY (DBG:POW:STAT YY), each bit set alone with
DBG:POW:POKE, 2 results each. Baseline: RMS (ΣY²/N) ≈ 1830 codes (noise), LPF = 21592 codes (the DC).
With the HPF bypassed RMS would be ≈ 21592² ≈ 4.7e8.

| poke | register / bit | RMS | LPF |
|---|---|---|---|
| 20,204 | R34 bit 6 | 1820 / 1863 | 21595 |
| 20,142 | R34 bit 1 | 1803 / 1825 | 21600 |
| 20,141 | R34 bit 0 | 1830 / 1822 | 21593 |
| 6,192 | R26 bit 6 | 1814 / 1826 | 21595 |
| 21,85 | R35 bit 0 | 1816 / 1821 | 21596 |
| 21,86 | R35 bit 1 | 1828 / 1831 | 21589 |

No reserved bit bypasses the HPF: DC power stays a software product (LPF means or AD1), the hardware
multiplier covers the AC part only. Overrides cleared afterwards (POKE CLEAR), meter back on DC V.
