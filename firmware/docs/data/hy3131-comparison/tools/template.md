# HY3131 configuration in eight firmwares: a per-mode comparison

How the Alientek DM40 stock firmware and seven other sources set up the Hycon
[HY3131](../../hw/datasheets/hycon-hy3131-datasheet-v10.pdf) in each
measuring mode, field by field (R20..R36), and what each mode does on the chip.
Field names and option tables follow
[`emu/hy3131/ref/registers.md`](../../emu/hy3131/ref/registers.md).

## Sources

| meter / source | MCU | how the rows were obtained | where |
|---|---|---|---|
| Hycon reference | — | (a) application note APD-DMM003 "HY313X Configurations": block diagrams with the active switches and mux inputs drawn in red, read by eye (lines starting "APD p.N" in the Hycon column; where the drawing is ambiguous, e.g. whether the AD1 reference buffers are on, nothing is listed); (b) register images of Hycon's "HY313x ENOB V1.4" evaluation tool (`Config/*/HYCONFIG-n.txt`, rows R20..R33 or R20..R36) | [`hycon-hy313x-configurations-apd-dmm003.pdf`](../../hw/datasheets/hycon-hy313x-configurations-apd-dmm003.pdf); ENOB rows: [`RESEARCH.md`](../../emu/hy3131/RESEARCH.md) §5 (the tool archive is not in the repository) |
| Alientek DM40 stock V2.6 | GD32F427VGT6 | tables in the stock image (22 bytes, R20..R35); the open firmware carries them unchanged | [`src/hy3131_tables.rs`](../src/hy3131_tables.rs), two capacitance rows in [`src/dmm.rs`](../src/dmm.rs) (`CFG_F13_R7`, `CFG_F13_DISCHARGE`) |
| EEVblog 121GW v2.05 | STM32L152ZD (schematic; image linked at 0x08006000) | static analysis of the plain update image: row tables (20 bytes, R20..R33) and the code that selects and edits them; v1.02 differences against the community IDA tables ([notes](data/hy3131-comparison/eevblog-121gw.md)) | EEVblog `EEVBlog2_05.zip`; [tpwrules/121gw-re](https://github.com/tpwrules/121gw-re) (v1.02 IDA database, `tables/hy_regtbl.h`), [121gw-88mph](https://github.com/tpwrules/121gw-88mph) |
| Digilent DMM Shield | Arduino Uno / Mega 2560 / Due | source table (INTE + R20..R36) | [Digilent/Arduino-DMMShield-Library](https://github.com/Digilent/Arduino-DMMShield-Library) `dmm.cpp` |
| Temco T3 | STM32F103 (tables), ESP32 (driver) | source tables (20 bytes, R20..R33). The STM32 project's `main.c` excludes the HY3131 task; which rows a shipped firmware writes is unknown | [temcocontrols/T3-programmable-controller-on-ESP32](https://github.com/temcocontrols/T3-programmable-controller-on-ESP32) `STM32/Multimeter/USER/HY3131_FunTable.h`, `main/hy3131_sw.c` |
| OWON XDM2041 V1.7.2 | GD32F103CBT6 | SPI captures (bulk write R00..R37 per mode change) plus a static decompilation of the stock image ([notes](data/hy3131-comparison/owon-xdm2041.md)) | [PetteriAimonen/owon-xdm2041-info](https://github.com/PetteriAimonen/owon-xdm2041-info) `spi_captures/all_modes.txt` |
| Keysight U1281A / U1282A v1.05 | NEC 78K0 (*inferred*) | static decode of the update image (nibble-swapped, byte-reversed): a 13-row table (R20..R33) at 0x18EA plus per-range overlays; the mode-to-function mapping is *inferred* ([notes](data/hy3131-comparison/keysight-u128x.md)) | "Keysight U1281A U1282A Firmware Update Software (Version 1.05)", file `u1281a_82a_v105.ag` |
| Metrix MTX3292(B) / MTX3293(B), CA 5292 / 5293 (Chauvin Arnoux ASYC IV) v1.02; rows identical in the v1.08 images | Kinetis K70 (`MK70FN1M0`, *inferred*) | static analysis of the plain update image (linked at 0x8000): 64 rows of 25 bytes (R1E..R36) at flash 0xB4C64, one per mode / coupling / range; the function names of the mode numbers are *inferred* ([notes](data/hy3131-comparison/metrix-mtx3293.md)) | Chauvin Arnoux "Logiciel embarqué ASYC IV", loader v1.09 (`loader_asyc4_v1.09.zip`), file `X04877C01_P_CODE_V1_02_ASYC4B_GB_FR.BIN` |

[Project Phoenix](https://github.com/aziemer/Project-Phoenix) (open firmware
for the XDM2041) copies the OWON stock rows; its differences are listed under
the OWON runtime notes.

**Method.** One decoder turns every row (R20..R36, missing registers marked
"not written") into named fields; every table below is generated from it, 379
rows in all ([`tools/`](data/hy3131-comparison/tools/README.md)). The decoder output agrees with the hand decodes of DM40
`CFG_F3[0]` (DC 60 mV, [`hy3131-frontend-2026-10-26.md`](hy3131-frontend-2026-10-26.md) §4),
DM40 `CFG_F13[0]` (R21 = VDSC11 / VDSC5, SMODE 0x1A), Keysight row 0 and row 4
and OWON `RES 5E6`. The power-bus row of SMODE uses the truth table in the
datasheet's switch-network figure (§12.2): SMODE 0110 = SVDD + SVSO2 + SCP,
0111 = SVSS + SVSO2 + SCN (`registers.md` lists 0110 as SVDD + SCP + SCN).

**Reading the tables.** A cell with several values gives the value of the most
common rows bare and the others with their row labels in brackets. "AD1 off"
/ "off": the block is disabled in that row, so its other fields are inert.
"not written": the firmware does not write that register (it keeps the reset
or previous value). PA switch notation: `DF4` = PA4 has DS4 and FS4 closed, `PS7`
= PA7 has PS7 and SS7 closed. Pins PA0..PA9 and PB0..PB8 are the HY3131's
analog pins; what each board connects there differs and is given only where it
is documented (DM40: [`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131);
Hycon: the APD diagrams); elsewhere it is unknown. Ranges are named as each
source names them (Hycon/121GW/Digilent/OWON 5/50/500 V-style, DM40/Temco
6/60/600 V-style). The tables leave out rows that are duplicates or never used:
Temco "Auto" rows (identical to the manual ones), the DM40 ohm rows r0 and
row 7 (never selected) and the DM40 ohm "P8SET" variant (see the ohm section),
Keysight base rows 1 / 4 / 7 (replaced by base row + per-range overlay), the
Metrix AC+DC and mode 9 rows (byte-identical to its AC rows) and its all-zero
Ω row 6. Metrix ranges are named by index (r0 = lowest); its V r0 is listed as
mV (*inferred*). All
rows are in the [appendix](#appendix-raw-rows).

## How the HY3131 measures

The input reaches the chip through board resistors on the switch pins PA0..PA9
and on the analog pins PB0..PB8. Each PA pin has four switches: PS to a power
bus (SVDD / SVSS / SGND / SVSO, chosen by SMODE<3:0> and, in the capacitance
modes, by the comparator output CMPO), DS to the output of OP3 (the ohm / diode
/ capacitance source, reference VREF chosen by SFUVR), FS to the node FB and SS
to the node SENSE; so a resistor string on PA pins becomes a divider tap (PS +
SS), a reference resistor driven by OP3 (DS + FS) or a current path. OP1 and OP2
buffer or amplify a selected node (SOP1P / SOP2P); OPS1 / OPS2 close their loop
through the OPxN pin, where the datasheet figure (§12.3) draws a 10 k / 90 k
network to AGND (x10). OP1 has a chopper (OP1CHOP 1 / 2 kHz). AD1, the 24-bit
sigma-delta converter (AD1CLK = SYSCLK / 30), takes its input either from the
filter muxes SAD1FP / SAD1FN, whose positive leg passes the pre-filter resistor
(SFT1: 100 k, 10 k, 0 or open) into the external FTP–FTN capacitor, or directly
from FB / RLU or the die temperature sensor (SAD1I). Its reference comes from
SAD1RH / SAD1RL; the result is IG · VIN / (RG · VR) of full scale (2²³ codes,
plus DCOS / RG), decimated by OSR1 = 256..32768 and, with AD1CHOP = 1x,
chopped against its own offset at half the rate. AD2 and AD3 are 19-bit fast
converters (AD2CLK = SYSCLK / 2 or / 4, OSR2 32..1024) whose samples feed a
digital low-pass filter (LPF, OSR4 128..16384) and the RMS block (ΣX²/N, or
ΣXY/N of AD2 and AD3 with SVXI = 1; the square root is taken by the MCU), and a
peak-hold on AD2, AD1 or LPF. The window comparator (CMPH / CMPL, input SCMPI,
thresholds SCMPRH / SCMPRL from the VDSC or AGNDP / AGNDN taps) gives ACPO and
CMPO; ACPO or the CNT pin drives the reciprocal counter (CTA gate time, CTB
input cycles, CTC high time), and CMPO can drive the CMP pin. AGND is K3 · VDDA
(SAGND), REFO is the band gap or the PB6 pin buffered (SREFO), ACM is the AD
common mode (SACM). Sources: datasheet §11–14, [`RESEARCH.md`](../../emu/hy3131/RESEARCH.md) §2.

## DC mV

The input (on the DM40 the mV node RLU, *inferred*; on the Hycon board PB0) is
read by AD1 against the PB6 reference (an external 1.2..1.25 V reference
buffered as REFO). The low range (50 / 60 mV) goes through OP1 with OPS1 = 1
(x10), chopped; the high range either reads the pin directly or through OP1 at
x1. The pre-filter and AD1CHOP set the noise and the offset; DCOS is 0
everywhere.

{{TABLE:dcmv}}

- DM40 is the only source with AD1IG 0.9 on the 60 mV range; every other source
  uses 1.8 there (121GW drops to 0.9 only on 500 mV). The DM40 window at the
  input is ±139 mV on 60 mV ([frontend doc](hy3131-frontend-2026-10-26.md) §4).
- DM40 reads the 600 mV range through OP1 at x1 (OPS1 = 0), as 121GW and Temco
  do; Hycon, Digilent and OWON read PB0 directly.
- DM40 leaves SFT1 open on both ranges; Hycon and Digilent use 100 k on the x10
  range, OWON 0. DM40 AGND is 0.3 · VDDA here (as on DC V 6 V, ohm and
  temperature; 0.5 on DC V ≥60 V, AC and DC A), and its
  comparator stays enabled on PB4 in every DC row.
- 121GW sets AD1CHOP = 11 on 50 mV and OSR 256 on 500 mV in the table and
  never rewrites R22; it averages up to 10 readings in software instead
  ([notes](data/hy3131-comparison/eevblog-121gw.md#runtime-logic)).

## DC V

A 10 M input resistor and tap resistors on PA4..PA7 form the divider; the tap of
the range is connected to the power bus and to SENSE (PS + SS), so SENSE carries
the divided input and AD1 reads it against RLU or another low node. The Hycon
network (APD-DMM003 §3) is 10 M / 1.111 M / 101.01 k / 10.01 k / 1 k for /10
.. /10000; the DM40 taps follow the same ratios (*inferred*,
[`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131)). FS9 / FS0 / FS1
connect the input and divider foot to FB.

{{TABLE:dcv}}

- DM40 uses AD1IG 0.9 on every range; the others use 1.8 except at 1000 V
  (OWON, Keysight) and the 121GW (0.9 everywhere).
- DM40 buffers SENSE through OP1 on the 6 V range (AD1 = OP1O − RLU, OP1
  chopped) and reads SENSE − PB5 on the higher ranges with SMODE 0x24 (SVSO1 on
  the bus); Hycon, Digilent and OWON read SENSE − RLU directly on every range,
  121GW SENSE − PB3, Temco and Keysight OP1O − RLU.
- DM40 1000 V closes PS5 and PS4 together (two taps in parallel); Hycon and
  121GW close PS4 alone. DM40 also closes DS0 with FS0, and DS9 with FS9 on
  the ranges from 60 V.
- SFT1: DM40 and Keysight open, OWON 0, Hycon / Digilent / Temco 100 k.
- Hycon's two references differ: the APD diagram (p16) reads SENSE − PB3, the
  ENOB tool rows SENSE − RLU (the 121GW uses SENSE − PB3).
- Metrix reads OP1O − PB5 against PB6 − PB5 (alternate r1 row: PB2 − PB5) with OP1 chopped at
  1 kHz and SFT1 100 k; its taps are PS3 / PS8 / PS7 / PS6, with P9 closed in
  every V and A row.

## AC mV

The low-level input is amplified by OP1 (and on the DM40 also OP2) and digitised
by AD2; the LPF gives the DC part and the RMS block the mean square. AD1 is off
in every source except the 121GW's normal AC rows. The comparator watches the
same signal for the frequency readout (ENCTR = 1).

{{TABLE:acmv}}

- DM40 enables OP1 and OP2 both on RLU (OPS1 = OPS2 = 1 on 60 mV) and reads
  AD2 (and AD3) on OP2O − AGND with IG 1.0 and reference PB6; what OP1O drives
  on the board is unknown. Hycon, Digilent and Temco use OP1 only, AD2 on OP1O − PB3 with
  IG 0.5 and reference REFO.
- DM40 enables AD3 on the same node with SVXI = 1, so its RMS is ΣXY/N of AD2
  and AD3 rather than ΣX²/N; no other source enables AD3.
- DM40 keeps OP1 chopped at 2 kHz in AC (OWON also on 500 mV); the others turn
  the chopper off.
- 121GW: the normal AC mV rows read AD1 (unchopped, OSR 2048) on PB7 with AD2,
  LPF and RMS off. PB7 is the DC output of an AD8436 true-RMS converter fed
  from OP1O (the 121GW circuit diagram); the LPF key routes it through a
  passive RC network on the OP2O pin first (OP2 is off; the 1 kHz corner is
  *inferred* from the net name). AD2 + RMS + peak run only in the "1 ms peak"
  table, which uses only the peak-hold result.
  Keysight runs AD2 on PB7 with the LPF on and ENRMS = 0 in every row.
- Metrix has AD2, LPF and RMS off in every row: AD1 reads PB1 − PB5 (OSR
  16384) while the comparator on PB3 drives the counter; the AC value comes
  from a circuit outside the HY3131 on PB1 (*inferred*). The same holds for
  AC V, AC A and Hz.

## AC V

The divider of DC V is used with the tap on the power bus and SENSE; OP1
buffers SENSE, and the signal reaches AD2 either straight from OP1O or through
an external coupling network (DM40: OP1O → capacitor → PB4, τ = 0.34 s, the
capacitor *inferred*, [frontend doc](hy3131-frontend-2026-10-26.md) §2). SMODE5
connects the ACC capacitor array (0.2 pF steps up to 25.4 pF) for the divider's
high-frequency compensation. The RMS block gives the AC value, the LPF the DC
part.

{{TABLE:acv}}

- DM40 feeds AD2 from PB4 (the external coupling network) with IG 1.0 and
  reference PB6; Hycon, Digilent and Temco use OP1O − PB3, IG 0.5, reference
  REFO; OWON OP1O − AGND.
- ACC: DM40 0 on every range (measured: no effect on the DM40, [frontend
  doc](hy3131-frontend-2026-10-26.md) §3); Hycon 25 / 48 (5.0 / 9.6 pF) on the
  two low ranges, Digilent 127, Keysight 64 (127 on one range, and a calibrated
  value on another); OWON loads it per unit from calibration (E9 / FF in the
  capture are one unit's values). 121GW uses the Hycon values in its 1 ms peak
  rows only, never writes R30 at run time, and corrects the frequency response
  in software instead: per-range tables of percent against frequency scale two
  calibrated coefficients
  ([notes](data/hy3131-comparison/eevblog-121gw.md#ac-frequency-response-correction)).
- DM40 compensates with FS9 + FS1 (no FS0) and the tap on the bus (PS6 on 6 V
  and 60 V, OP1 x10 on 6 V); Hycon and OWON close FS0 / FS1 / FS9 and drive
  the tap with DS as well on the low range (PDS7).
- DM40 enables AD3 + SVXI (ΣXY) and the peak-hold on AD2 (ENPKH = 1); 121GW
  normal rows use AD1 on PB7 (the AD8436 output) with AD2 off, as in AC mV.
  121GW Low-Z in AC sends the 1000 V row with PA9 moved from the bus to FB and
  FS1 added.
- Metrix: ACC 0 in the rows; at run time a 16-bit calibration word sets ACC,
  P0..P2 and three MCU-switched parts (see the runtime table).

## DC A

The current flows through a shunt chain; the shunt voltage is read on PB1 (or
PB2) against PB3 or AGND, either directly or through OP1 at x10. The DM40 shunts
are 10 mΩ (A, PB2), 0.5 Ω (mA) and 47.5 Ω (µA) on PB1, switched by MOSFETs from
MCU pins PE0 / PE1 ([`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131));
the mA and µA ranges share one row. The Hycon board has 0.01 / 0.99 / 99 Ω
(APD-DMM003 §9).

{{TABLE:dca}}

- DM40 routes every range through OP1 (x10 on the low range of each shunt, x1 on
  the high one) and reads OP1O − PB3; Hycon, Digilent, Temco and OWON alternate
  OP1 x10 and the bare shunt pin; 121GW reads PB1 directly and changes AD1IG
  (1.8 / 3.6) and AD1RG (1 / 1/3) instead. Its v2.05 DC A rows use SFT1 100 k;
  the VA / burden rows (and v1.02 for both) leave it open.
- DM40 AD1IG is 0.9 on every range (others 1.8, 0.9 only on 10 A for OWON /
  Temco). DM40 sets SFT1 = 10 k on the low mA / µA range (60 mA, 600 µA) and
  open elsewhere.
- OWON references AD1 to REFO − AGND here, the others to PB6 − AGND
  (Keysight PB6 − PB3 with RG 1/3).
- Metrix buffers PB8 through OP1 x10 on every range (OP1O − PB5) and changes
  AD1IG to 2.7 on r4 / r5.

## AC A

The shunt voltage of DC A goes to AD2 through OP1 (x10 or x1); RMS and LPF as in
AC V.

{{TABLE:aca}}

- DM40: AD2 on OP1O − PB3 with IG 1.0, reference PB6, AD3 + SVXI, peak-hold
  on, OP1 chopped at 2 kHz; the A ranges use LPF OSR4 8192 instead of 16384.
  Hycon, Digilent and Temco: IG 0.5, reference REFO, chopper off.
- 121GW normal rows: AD1 on PB7 (OSR 1024, the AD8436 output), AD2 off.

## Ohms

Ratiometric: OP3 drives a reference resistor on a PA pin (DS + FS, so FB is the
top of Rref) in series with the unknown; AD1 reads the voltage across the
unknown (PB0 − PB3, or OP1O − PB3 with OP1 buffering PB0) against the voltage
across Rref (FB − RLU), so the OP3 voltage cancels. On the high ranges the
sources switch to a current source (SMODE 0110: SVDD + SVSO2 + SCP) and an
absolute reference (VDDA − VSSA with AD1RG 1/3, DM40 60 MΩ: REFO − AGND). SFUVR
sets the OP3 voltage and with it the test current and open-circuit voltage.
DM40 Rref: 1 k .. 10 M and 63 M on PA2..PA7 (*inferred* from the stock scaling,
[`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131)).

{{TABLE:ohm}}

- DM40 has two variants selected by the ohm key, different only in R31: SFUVR 2
  (VDS15, the default; listed) or SFUVR 5 / 4 (AGNDP7, AGNDP8 on 6 MΩ;
  "P8SET" rows in the appendix). Measured open-circuit voltage 2.12 V / 0.67 V
  ([`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131)).
- DM40 stays ratiometric (FB − RLU) up to 6 MΩ and uses REFO − AGND on 60 MΩ;
  121GW, Digilent, Temco and OWON switch to VDDA − VSSA with RG 1/3 from 5 MΩ
  (Digilent from 500 kΩ).
- DM40 buffers PB0 through OP1 on every range (OP1O − PB3, chop 1 kHz) and
  uses SFT1 = 0 below 6 MΩ, 100 k above; OWON reads PB0 − PB3 directly with
  OP1 enabled on PB8 (x10, purpose unknown) and SFT1 open; Hycon reads PB0 directly on the middle ranges.
- 121GW v2.05 cut the OSR on 50 Ω..500 kΩ by 4 against v1.02 (OSR 512..2048)
  and raised 5 MΩ to 32768, with a software moving average of 5..20 readings.
  It adds 1.5 nF from PB0 to AGND in Ω (U16). While EEPROM byte 0x26 ≠ 0xA5
  (the byte the 50 MΩ calibration step writes) its 50 MΩ row opens PA0's bus
  switch and the 5 / 50 MΩ software corrections are skipped.
- 4-wire: only the OWON (sense on PB2 − PB5, `FRES`); the others have no 4-wire
  rows.
- Metrix: ratiometric r0..r3 with Rref on PA5 / PA7 / PA8 / PA3, current
  source with RG 1/3 against REFO on r4 / r5. Its mode 6 (10 mA low-Ω,
  *inferred*) closes no PA switch. Its continuity row is Ω r0 with OP2 on and
  the comparator off.

## Continuity

An ohm configuration at the low range with the comparator watching the
voltage across the probe: the comparator output CMPO is enabled to the CMP pin
(ENPCMPO = 1), so the beeper is driven by the chip without the MCU.

{{TABLE:cont}}

- DM40 compares OP1O (OP1 on SENSE) against AGNDP0 / AGNDN2 and swaps the AD1
  reference (RLU − FB, read as a negative value); OP3 VREF is AGNDN7 (SFUVR
  13, below AGND). The others compare PB0 against AGNDP2 / AGNDP1 with OP3 at
  VDS15; OWON uses VDSC7 for both thresholds.
- DM40 runs AD1 at OSR 2048 with SFT1 = 0; ENCTR = 1.

## Diode

OP3 with the current-source switches (SMODE 0110) drives the diode; SDIO
connects PB0 to PB8 so the 900 k / 100 k divider brings the forward voltage
(up to ~3 V) into range; AGND is set to 0.1 · VDDA to leave room above it.

{{TABLE:diode}}

- All sources set SDIO = 1, SMODE 06 (121GW and Keysight 0x12: SVDD only) and
  AGND 0.1 · VDDA, except Keysight (SDIO = 0).
- DM40 buffers PB0 through OP1 (OP1O − PB3, IG 0.9); the others read PB0
  directly (IG 1.8; Temco 2.7, 121GW 0.9). The DM40 comparator is on RLD and
  the firmware sounds the beeper by toggling ENPCMPO (R20 0xB4 / 0xB0).

## Capacitance

Three methods appear:

1. RC relaxation (small C): SMODE 1010 lets CMPO switch the capacitor between
   SVDD and SVSS through a range resistor on a PA pin, OP3 VREF flips between
   AGNDP0 and PB7, the comparator on PB0 has thresholds VDSC11 / VDSC5, and the
   counter measures the oscillation; C = 1 / (2 ln 2 · R · f) (DM40 stock).
2. Constant current (SMODE 1110: SVDD + SVSO2 + SCP / SVSS + SVSO2 + SCN by
   CMPO): OP3 drives a current in alternate directions, the comparator period
   gives C (counter).
3. Large C with AD1: a constant-current ramp whose slope AD1 reads (DM40: AD1
   on OP2O, OP2 buffering PB0; comparator on OP1O reverses the ramp at ±AGNDP3),
   or charge / discharge phases read with AD1 against VDDA − VSSA (Hycon CAP-2..4,
   OWON 5 / 50 mF, Temco "Charge ADC get" / "disCharge").

{{TABLE:cap}}

- DM40: RC relaxation on r0..r4 (6 nF .. 60 µF), AD1 ramp on r5..r7 (600 µF ..
  60 mF) with a discharge row (PA4..PA7 on the bus) before each ramp. 121GW and
  Keysight use the constant-current method for the small ranges, OWON and
  Hycon CAP-0 the RC method.
- 121GW takes every range from the counter (C ∝ CTC / CTB); AD1 is configured
  on 10 / 100 µF but not read for the value. A board strap (PF8 low at boot)
  selects a second table with other comparator thresholds on 100 nF and
  100 µF; with no counter result before a timeout it jumps to 1 µF or 10 mF.
- The AD1 ramp rows: DM40 AD1 on OP2O with reference PB6, IG 0.9, RG 1,
  chopped; Hycon / Temco / OWON read PB0 − PB3 against VDDA − VSSA with RG 1/3,
  unchopped (Hycon and Temco with DCOS −0.25).
- Metrix: RC relaxation on r0..r3 (comparator OP1O against VDSC10 / VDSC8,
  AD1 off, INTE = CTIE), AD1 against VDDA − AGND with the current source on
  r4..r7.
- OWON `CAP 50E-9` AD1 fields are left from the previous mode (the RC rows do
  not call the AD1 composer); 121GW and Keysight use the band-gap REFO
  (SREFO = 0) in these rows.

## Frequency / counter

The counter measures either ACPO (the comparator on an analog node) or the CNT
pin (ENCNTI = 1). Gate time = (0x1000000 − CTA preset) / SYSCLK; f = CTB / T,
duty = CTC / gate counts. The V and A rows also run the counter for the
secondary frequency readout (ENCTR = 1 in their AC rows).

{{TABLE:freq}}

- DM40 compares PB4 (the AC V node) against AGNDP1 / AGNDN1 (±20.8 mV) and
  keeps AD2 + RMS running for the level; at run time it alternates the source
  between the comparator and the CNT pin and reports the steadier one.
  Its AD1 is enabled on the die temperature sensor (SAD1I = 3) in this row;
  what the stock reads from it is unknown.
- Hycon has three variants: CNT pin, DC V path (comparator on PB4) and AC V
  path (comparator on OP1O); Temco has the same three. 121GW and OWON use the
  CNT pin (ENCNTI = 1). Keysight's mode 0x0B uses the DC V row 1 (*inferred*).
- Metrix: five sensitivity rows (the AC V divider rows) in two tables, which
  differ only in the comparator input (PB3 with ENCNTI = 1, or OP1O).

## Temperature

A thermocouple is a DC mV measurement plus a cold-junction temperature. The
HY3131 has a die sensor read by AD1 with SAD1I = 10 (TS1P / TS2N) and 11
(TS1N / TS2P); the difference of the two codes cancels the sensor offset
(datasheet §12.4). An RTD is an ohm measurement.

{{TABLE:temp}}

- DM40 (K type): three AD1 conversions per result at OSR 16384, R33 SAD1I = 11,
  10 and 00 in turn; the thermocouple is read through OP1 x10 on PB0 (row 0).
  Row 1 differs in the PA switches and OPS1; whether the stock selects it is
  unknown.
- OWON runs the thermocouple as DC V 500 mV (PB0 − PB3, OSR 4096), reads the
  die sensor only at boot (R33 0xAA / 0xAB, SAD1I 2 / 3), and runs the RTD as
  ohm 500. 121GW uses its DC mV 50 mV row (K type, −200..1350 °C) with the
  cold junction from an NTC read by the MCU ADC (*inferred*), and feeds the input from VDD
  through 20 MΩ in this mode (open-lead detection, *inferred*). Temco's two rows are the die sensor
  only. Keysight row 6: OP1 on PB1, OP1O − AGND (*inferred* mapping).
- Metrix reuses rows by sensor: Ω r0 (Pt100), Ω r1 (Pt1000), V DC r0 (J / K);
  no row selects the die sensor.
- APD-DMM003 has no temperature chapter; the ENOB tool has no temperature row.

## Other rows

- Hycon ENOB `Sel_Cal` 0..9: OP3 drives one divider resistor (DF) and the
  neighbouring tap is on the bus and SENSE (PS), AD1 reads RLU − SENSE: a
  divider-ratio check of adjacent PA resistors (*inferred* from the decode).
- Keysight row 12 (flag 0xFE24.2): AD2 and AD1 on, OP1 on PB2; use unknown.
- Metrix mode 10: AD1 OP1O − PB5 against REFO, RG 1/3, OSR 32768, no PA
  switch; use unknown.

## Runtime behaviour

| | DM40 stock V2.6 | 121GW | Digilent | Temco | OWON XDM2041 | Keysight | Metrix MTX3293 |
|---|---|---|---|---|---|---|---|
| mode change | R26, R34, R36 = 0; relays; R20..R35 (22 bytes); INTE = 0; poll INTF | 20 bytes R20..R33 from the row table; R1E..R1F = 00, INTE (AD1IE; + CTIE in AC; RMSIE + CTIE in 1 ms peak; CTIE in cap / Hz); counter restart (ENCTR off, R1B..R1D = 00 + CTA preset, read back, ENCTR on); calibration; then the AC-path muxes and relays (GPIO); no read-back | R37 = 0x60 (reset) every scale change, then INTE + R20..R36 (24 bytes), read back and compared with a mask | 20 bytes from R20, then INTF = 0x02 and INTE (ESP32 driver) | R20 ENCMP off at once, then all 56 bytes R00..R37 in one burst; 74HC595 relay byte before every reading | buffer of 20 bytes from the table, per-range overlays, 22 bytes sent (R20..R34; R34 unset) | relays, then one 25-byte burst R1E..R36 (INTF = 0, INTE from the row); R37 = 0x60 only when the driver opens; no read-back |
| zero / offset | none in software; AD1CHOP = 10 and OP1 chopping | no zero row (AD1CHOP = 10 in the DC rows); calibrated offset subtracted, plus a stored zero on range 0 of each current mode | none (software averaging) | `Get_Volt6V_Zero`: the "6V ZERO" row (OP1+ on RLU) averaged 50 times at DC V entry, subtracted | none; AD1CHOP / OP1CHOP; calibration in software | unknown | 12-bit MCU DAC loaded from a calibration record on every V / A range change (offset trim, *inferred*); OP1 and AD1 chopping in the rows |
| speed / OSR | R22 0x15 (8192) while the reading moves, 0x17 (32768) when steady (DC V 0x206 codes, DC mV 3618 / 1218, DC A 0x168); ohm 0x13 on overload, 0x17 in range; continuity 0x14 / 0x17 | R22 only from the rows; no other R22 write. Software moving average instead: N 2..20 readings, restarted when a reading leaves a ±W window (DC V 2 / 2, DC mV 10 / 5, AC 10 / 20, Ω 5..20 / 10..100) | fixed R22 0x17 (OSR 32768) | unknown | speed key: R22 OSR {4096, 2048, 2048} by a single R22 write | a flag sets R22 = 0x11 (OSR 512) and changes LPFBW | R22 only from the rows (AD1 period 100 ms; 200 ms in mode 10); no other R22 write |
| autorange first conversion | settle deadlines after each range write (ohm 150 ms, DC V 250 ms, AC V 450 ms, …) | autorange held for N readings after a change (DC V 2, AC V 3, AC mV 4, DC mV 1, Ω 1..2, cap 1, Low-Z DC 5, 1 ms peak 5, else 3); up > 110 % of full scale, down < 10 %; Ω down-range blocked for 100 timer ticks after an up-range | — | unknown | after a range change OSR 1024 (Ω, DC A) / LPFBW 5 (AC A) for the first conversion | unknown | unknown |
| AC trim (ACC) | none, R30 = 0x80 | table values only: 0, except 25 / 48 in the 1 ms peak 5 V / 50 V rows; frequency response corrected in software (per-range tables × two calibrated coefficients) | 127 fixed | table values | per-unit calibration byte into R30 on 5 V / 50 V | calibrated value on mode 1 range 0 | 16-bit calibration word per range: ACC (R30), P0..P2 (the power-bus switches of PA0..PA2) and three MCU GPIOs; 0 in V DC |
| AC+DC | alternates the AC and DC rows of the same quantity (~3 readings each) | 5 readings on the AC V row, 3 on the DC V row of the same range, row and calibration reloaded at each switch; √(AC² + DC²) | — (no such mode) | unknown | — (no such mode in the captures) | unknown | coupling 2 sends the AC rows; how the DC part is added is unknown |
| capacitance | counter restart per reading (R20 0x50, CTA 0xFC0000, R20 0x52); r5..r7: discharge row until \|V\| < 30 mV, then the ramp row; R21 rewritten to 0xDC / 0xCC / 0xBB by slope class; comparator state (R14 bit 6) splits batches | all ranges from the counter: C ∝ CTC / CTB; no reading before a timeout → 1 µF or 10 mF range; second table for boards with PF8 low | — | counter preset CTA by frequency class; R33 0xA0 / 0xA8 switches SFT1 100 k / 0 for counting | counter restart R20 0x70 / CTA 0xBF0000 / R20 0x72; 5 / 50 mF: open R2A / R2C, read AD1, alternate two bulk rows (phases A / B) | — | ioctl 2 restarts the counter: ENCTR off, R1C..R1D = 00 F1, INTF read, ENCTR on |
| temperature | R33 0xAF → 0xAE → 0xAC: die sensor 11, 10, then the thermocouple; 700 / 800 / 700 ms | DC mV 50 mV row, K type table in flash; cold junction from the MCU side (*inferred*: NTC); 20 MΩ pull-up on the input in this mode | — | rows with SAD1I 3 / 2 | die sensor at boot (R33 0xAA, 0xAB, ~2 s each) | unknown | Pt100 / Pt1000 on Ω r0 / r1, J / K on V DC r0 with relays; die sensor unused |
| other | diode beeper by R20 0xB4 / 0xB0 (ENPCMPO); frequency source alternates ENCMP (0xF0) / CNT (0x08) | AC value from an external AD8436 on PB7; HY3131 RMS result unused; 1 ms peak: R29 ENPKH pulsed after every RMS interrupt; CTA preset alternates 0x8000 / 0xE000 every 100 ticks in AC (row re-sent); 5 / 50 MΩ corrections and the 50 MΩ PA0 switch depend on an EEPROM marker | — | — | ENCTR set in most rows; Phoenix clears it and manages the counter itself, hard-codes ACC E9 / FF, and has a 50 Ω range and its own 500 µF..10 mF rows | idle set: R26 0x19, R33 0x0C (*inferred*) | AD2 / RMS unused; Modbus 0xE016 / 0xE017 select and read / write any register R00..R37 |

Sources: DM40 — stock V2.6 disassembly, mirrored in [`src/dmm.rs`](../src/dmm.rs);
121GW — the v2.05 image and the circuit diagram
([notes](data/hy3131-comparison/eevblog-121gw.md));
Digilent — `dmm.cpp` `DMM_SetScale`; Temco — `DMM_DCV.c`, `main/hy3131_sw.c`
(not built in the STM32 project); OWON — captures and the stock image;
Keysight — the decoded image; Metrix — the v1.02 image
([notes](data/hy3131-comparison/metrix-mtx3293.md)).

## Not determined

- Board wiring of PB / PA pins for the Digilent, Temco, OWON and Keysight
  beyond what the row decode shows; for the 121GW beyond the nets in its
  [notes](data/hy3131-comparison/eevblog-121gw.md#board-wiring-circuit-diagram).
- Keysight: the mode-to-function mapping and R34..R36.
- Temco: which rows a built firmware writes.
- Metrix: the range values behind each index, the circuit on PB1 (AC) and
  PB8 (A), autorange thresholds and settle times, mode 10, and which
  calibration register holds which record.
- 121GW: what the PF8 board strap marks, why the 50 MΩ row and the Ω
  corrections depend on EEPROM byte 0x26, and the timer tick period.

## Appendix: raw rows

Every row in hex from R20 upwards (22 bytes R20..R35 for the DM40, 20 bytes
R20..R33 for the 121GW, Temco and Keysight, R20..R36 for Digilent and OWON,
R1E..R36 for Metrix, as present in the file for the Hycon tool), with the mode key used above and the
source location. 121GW rows are the v2.05 tables (flash address, and whether
v1.02 has the same row); the two rows marked "modified at run time" are the
table row as the firmware edits it before sending. Keysight "composed" rows are base row
+ overlay as read from the decoded code. Metrix origins are flash addresses.

{{APPENDIX}}
