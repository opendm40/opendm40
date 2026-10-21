# Power meter mode and wrong-connection detection: feasibility and design

Date 2026-10-21. Research + design only; no fw-rs source changed. Answers the two
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
  BOM-based conclusions from `docs/dm40-feature-request.md`, or chip behaviour
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
  cont / Hz / temp". The shunt MOSFETs (PE0 / PE1, "RUH30J51M dual N-MOSFET,
  automatic range switching in a single current jack", `dm40-feature-request.md`
  1.1) select 10 mohm (PE0 = 1), 0.5 ohm (PE1 = 1) or 47.5 ohm (both 0); V
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
(CA-IS3741 isolator + VPS8504B supplies, `dm40-feature-request.md` 1.2,
INFERRED there and consistent with the repo's ground-check instruction). The
voltage is measured V-COM, the current A-COM: **the load current flows through
COM**, and COM is at the potential of the conductor you break into.

```
 source (L / +) ---------------------+------------------- load (+)
                                     |
                                   [V/ohm]
                              DM40  [COM] -------------------- load (-)   (return conductor broken here)
                                    [A]  ---+
                                            |   fuse, MOSFETs, shunt (R_path)
 source (N / -) -----------------------------+
```

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
  charging" stays as a warning toast (the isolation makes it safe only if the
  INFERRED topology is right).
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
[SENSe:]POWer:BURDen <ohm> / ?                          R_path of the current range (compensation)
[SENSe:]POWer:PHASe <steps -127..127> / ?               SPHACAL (sign = lead/lag)
[SENSe:]POWer:PHASe:CALibrate RESistive|REACtive        self-cal (see 3.5)
[SENSe:]POWer:CYCle?                                    d and the phase period, for diagnostics
```

CONFigure? answers `"POW <vrange>,<irange>"`. The BT cmd 06/09 protocol has no
slot for a 16th mode; leave BT at the stock set (power only over SCPI/USBTMC and
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
  ohm might have survived); warn loudly instead.
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
| **A-JACK** (red) | mode not a current mode and the A-jack sentinel sees > 0.44 mA (comparator) / > 0.5 mA DC or peak (AD1) | assert after 2 consecutive polls >= 100 ms apart; clear after 1.0 s below | badge "A-JACK" (red, above HV in `strip_data` priority, below HOLD), toast "Current in A jack - leads in A while measuring V?" once per assertion, the main readout keeps measuring | 3 x 50 ms every 2 s while asserted (stock key beep = 50 ms; `App::beep`), honours the alert volume floor (user-feedback UF-28) |
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

---------------------------------------------------------------------------

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
