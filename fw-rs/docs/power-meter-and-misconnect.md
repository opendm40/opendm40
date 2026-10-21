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
| burden | R_path per shunt (default the shunt: 10 mohm / 0.5 ohm / 47.5 ohm; POW:BURD sets the current range's), compensation ON by default: P_L = P - I^2 R, V_L^2 = V^2 - 2 R P + R^2 I^2 (exact for any waveform: v_L = v - R i), S_L = V_L I, Q, PF from those; the burden voltage I R in its own box (amber when the compensation is off and it exceeds 1 % of V) |
| limit | continuous-current limit (default 5 A / 30 s, POW:LIM <A>,<s>, persisted; the owner should set the manual's numbers): above it a countdown in the side column, at expiry a toast and the A-JACK beep pattern while it stays above; the auto power-off (and its warning) is held while the energy integrates with I > 50 uA, the DMM rail stays on in every view |
| 4 SCPI | `CONFigure:POWer [<V>|AUTO[,<A>|AUTO]]`, `MEASure:POWer?` (P; waits through the range hunt, at most 15 s), `READ?` / `FETCh?` = P, `FETCh:POWer:ALL?` = `P,S,Q,PF,Vrms,Irms,F,E_Wh,seconds,Vdc,Idc,VAh,burden_V` (NaN = +9.91E37: the fallback's P / Q / PF / Vdc), `MEASure:POWer:APParent? / :PFACtor? / :ENERgy? / :REACtive? / :VOLTage? / :CURRent?` (latest values), `[SENSe:]POWer:VOLTage:RANGe` / `:CURRent:RANGe <value>|AUTO` / `?` (`AUTO,+6.000000E+02`), `:ENERgy:RESet`, `:ENERgy:STATe`, `:BURDen <ohm>|DEFault` / `?`, `:BURDen:STATe`, `:PHASe <-127..127>` / `?`, `:PHASe:CALibrate [REACtive]` (RESistive: "Settings conflict", see 3.5: the cos optimum is flat), `:CYCle?` (`d,P period ms,full set ms`), `:METHod SIMultaneous|ALTernate` / `?`, `:LIMit <A>[,<s>]` / `?`; `CONFigure?` = `"POW AUTO,AUTO"` / `"POW +6.0E+01,+6.0E+00"`, `FUNCtion?` = `"POW"`. BT keeps the stock protocol (cmd 06 / 09 report the AC A byte and P as the value) |
| 5 UI | see below |
| 6 settings | p[48..52]: SPHACAL, method, compensation, discards, I limit, time limit, R_path x 3 (0 = default); the function itself is **not** restored: a restart after a power session comes up in DC A on the 10 A shunt (a load current may still flow) |
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
BIG (> 70 % AC: the 6 V tap clips at +0.7 V; > 100 % DC), UNSTABLE (the two
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
(`sent.configured(Kind::Off)`), so R20 / R21 are the counter's alone; the
fixes agent's sentinel.rs changes do not touch this path.

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

**DC method (owner: "DC power meter too").** `POW:COUP AC|DC|AUTO` /
Power Settings coupling button (AC / DC / AC/DC = AUTO), persisted; the W
button enters with the last coupling.

| | AC (method B) | DC (AD1 method) |
|---|---|---|
| rows | the power row (RMS / LPF, R34 P V P I) | stock DC V row of the V range / DC A row of the I range (f7 / f8_9), alternately |
| relays | PA 0, PE = the I range's shunt | the same: DC V and DC A both use PA state 0 (`write_config`: modes 1 and 7..9), PE kept on the I range's shunt in both rows -> no relay click per phase |
| V | OP2 -> AD3, 19-bit LPF, factory-derived gain, POW:ZERO | AD1 24 bit, OSR 8192 chopped, stock DC V cal (0x04 / 0x14, DCV_NEGK) |
| I | OP1 -> AD2 LPF, AC A RMS gain, POW:ZERO | AD1, stock DC A cal (0x94 / 0xAC) |
| P | P_ac + V_dc I_dc | V I (signed), the same `totals` (burden compensation, S = |P|, PF +-1, Q 0) |
| cadence | full set 1.28 s | phase = 150 ms settle + 1 dropped + 2 taken (100 ms each) = ~0.45 s; a reading after each phase |
| shown | PF / S / Q / f | PF / S / Q / f "--", the DC glyph above W, Vb |
| expected accuracy (manual specs of the stock functions) | V: AC V +-(0.3 % + 15 d) class, gain residual ~0.1 %; I: AC A +-(0.5..0.75 % + 15 d); P ~+-1 % at PF >= 0.5; DC parts after POW:ZERO ~0.1..0.2 % (V), I_dc as AC A | V: DC V +-(0.03 % + 5 d); I: DC A +-(0.15..0.2 % + 5 d); P ~+-0.25 % + digits, no zero needed (factory offsets) |

AUTO: starts in B; after a full set whose AC parts are both below 1 % of
their DC parts plus ~100 codes of floor (`pw_dc_like`), the DC method.
In the DC method the V row's comparator watches SENSE with a window +-3 mV
(nearest taps) around the tap's DC; CMPHO and CMPLO both seen during one V
phase (an AC swing of >= ~20 mV at the tap: 0.2 V on 6 V, 2 V on 60 V, 20 V
on 600 V) -> back to B. Predictable: a DC source with less ripple than that
stays DC; mains goes to B within one V phase. `POW:COUP?` answers `AUTO,DC`
/ `AUTO,AC` in AUTO. The fallback (VA only) is ignored while DC is in force.

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
reset, `pw_enter`); BT still reports the AC A byte with P as the value.

**Persist v7** (58 words): the power block grows to 10 words (p[48..58]: the
4 V gains as u16, the Y zero + flags (zeros valid, wiring dialog seen,
coupling), the 6 X zeros); a v6 record loads zero-extended (= factory gains,
no zeros, dialog not seen, AC).

**Emulator** (both HY3131.cs / DM40Frontend.cs copies identical): Y gain =
the factory-derived tap gain (inverse, so fw-rs reads true) x `powvg:`;
`powz:1` = the device's LPF zeros (Y +1080, X +888 x10 / +155 x1); the
power role's counter on SENSE (dc + AC against the window) and PB4 (the
current); DC V rows with SCMPI = SENSE drive CMPHO / CMPLO from the tap
waveform; the f8_9 DC A rows fold the uA calibration when PE selects the
47.5 ohm shunt (the stock mode 9; before: always mA, +3 % on the uA ranges).
Host tests 306 OK (unipolar square f = 0 at 0xDD / 40 Hz at 0xBF, PB4 50 Hz,
powz zeros, powvg). `emu/hy3131/powtest.py` 17 scenarios OK:

| scenario | result |
|---|---|
| zero | no POW:ZERO: V 0.0526 V, I 8.92 uA (= the device's phantoms); after: V 0.0015 (noise), zeros 1080 / 888,155,... ; DC 5 V 0.4 A 1.99997 W; -5 V: P -1.99997 W, Vdc -4.9999 |
| vcal | powvg 0.99: V 29.700 -> POW:VOLT:CAL (AC ref) -> 29.9999; powvg 0.98 DC 20 V -> POW:VOLT:CAL (DC ref) -> 19.9999; 0.2 V on 60 V: SMALL |
| freq | unipolar 40 Hz square: F 40.0007; 12 V DC + 3 A AC 50 Hz: F 50.0008 (PB4) |
| dcad1 | 12 V 2 A: 24.004 W; reversed -24.0025 W; 5 V 4 mA on 6 mA: 0.02000 W; 5 V 200 uA on 600 uA: 1.000 mW |
| dcburden | 5 V 0.4 A, R 0.5: P_L 1.92006 W, V_L 4.8001, Vb 0.200 |
| auto | 12 V 2 A -> AUTO,DC 23.999 W; mains 230 V 1 A -> AUTO,AC 230.00 W 50 Hz |
| the 11 of 3.11 | unchanged (mains, dc, small, burden, manual, alt, phasecal, energy, t1-yy, t3-cycle, scpi) |

UI (ui-render: 0 violations over 22996 cases incl. the 7-button rows in
every function, the power page in the DC method, the ranging texts, the
settings status line in 13 job states; touch-test 789 cases 0 failures):
renders p07 (settings + status), p09 (I ranging), p10 (refused V cal), p11
(DC method).

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
   / coupling buttons and the status line; a reboot comes back in W.
5. `POW:COUP DC` with the calibrator's DC on V-COM: Vdc = DC V reading,
   no relay clicks per phase, ~0.45 s per phase; compare at steady state
   *and* the first readings after the alternation starts (DC_SETTLE_MS = 150
   is a guess against the stock's 450 ms: raise it if the first V of each
   phase is off); `POW:COUP AUTO` with the square: back to AC within a V
   phase.
6. With current through A (T2 / T3 / T4 of 3.11): Idc of B vs DC A (the
   RMS gain now applied to I_dc), the DC method's P vs V x I of the stock
   functions, XY / XX switch cost with a real current (DISC 2 kept), PB4 f
   with a current-only AC signal.

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

Not done (as allowed by 4.2-4.4): AC+DC (both phases keep the stock rows:
`freq_secondary` rewrites R20), diode / continuity / capacitance /
frequency / AC A A-jack or V-jack sentinels (their comparators / converters
are busy; AD2 + OP2 variants need T8 first), the DC? / AC? / FUSE? hints, the
OVERLOAD shunt rule, the fuse-check dialog, STAT:QUES bit 9. The MOSFETs are
never switched by the sentinel.

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
LIVE), `SYSTem:ALERt:ENABle <0..7> | ON | OFF` / `?`, `DBG:SENTinel?`
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
  (a uA/mA-range overload heats the 47.5 ohm resistor long before the fuse opens).
