# HY3131 configuration in seven firmwares: a per-mode comparison

How the Alientek DM40 stock firmware and six other sources set up the Hycon
[HY3131](../../hw/datasheets/hycon-hy3131-datasheet-v10.pdf) in each
measuring mode, field by field (R20..R36), and what each mode does on the chip.
Field names and option tables follow
[`emu/hy3131/ref/registers.md`](../../emu/hy3131/ref/registers.md).

## Sources

| meter / source | MCU | how the rows were obtained | where |
|---|---|---|---|
| Hycon reference | — | (a) application note APD-DMM003 "HY313X Configurations": block diagrams with the active switches and mux inputs drawn in red, read by eye (lines starting "APD p.N" in the Hycon column; where the drawing is ambiguous, e.g. whether the AD1 reference buffers are on, nothing is listed); (b) register images of Hycon's "HY313x ENOB V1.4" evaluation tool (`Config/*/HYCONFIG-n.txt`, rows R20..R33 or R20..R36) | [`hycon-hy313x-configurations-apd-dmm003.pdf`](../../hw/datasheets/hycon-hy313x-configurations-apd-dmm003.pdf); ENOB rows: [`RESEARCH.md`](../../emu/hy3131/RESEARCH.md) §5 (the tool archive is not in the repository) |
| Alientek DM40 stock V2.6 | GD32F427VGT6 | tables in the stock image (22 bytes, R20..R35); the open firmware carries them unchanged | [`src/hy3131_tables.rs`](../src/hy3131_tables.rs), two capacitance rows in [`src/dmm.rs`](../src/dmm.rs) (`CFG_F13_R7`, `CFG_F13_DISCHARGE`) |
| EEVblog 121GW | STM32L152 (the [88mph](https://github.com/tpwrules/121gw-88mph) project's linker script) | IDA tables of firmware v1.02 (20 bytes, R20..R33), each row searched in the nine release images v1.00..v2.05 | [tpwrules/121gw-re](https://github.com/tpwrules/121gw-re) `tables/hy_regtbl.h` |
| Digilent DMM Shield | Arduino Uno / Mega 2560 / Due | source table (INTE + R20..R36) | [Digilent/Arduino-DMMShield-Library](https://github.com/Digilent/Arduino-DMMShield-Library) `dmm.cpp` |
| Temco T3 | STM32F103 (tables), ESP32 (driver) | source tables (20 bytes, R20..R33). The STM32 project's `main.c` excludes the HY3131 task; which rows a shipped firmware writes is unknown | [temcocontrols/T3-programmable-controller-on-ESP32](https://github.com/temcocontrols/T3-programmable-controller-on-ESP32) `STM32/Multimeter/USER/HY3131_FunTable.h`, `main/hy3131_sw.c` |
| OWON XDM2041 V1.7.2 | GD32F103CBT6 | SPI captures (bulk write R00..R37 per mode change) plus a static decompilation of the stock image ([notes](data/hy3131-comparison/owon-xdm2041.md)) | [PetteriAimonen/owon-xdm2041-info](https://github.com/PetteriAimonen/owon-xdm2041-info) `spi_captures/all_modes.txt` |
| Keysight U1281A / U1282A v1.05 | NEC 78K0 (*inferred*) | static decode of the update image (nibble-swapped, byte-reversed): a 13-row table (R20..R33) at 0x18EA plus per-range overlays; the mode-to-function mapping is *inferred* ([notes](data/hy3131-comparison/keysight-u128x.md)) | "Keysight U1281A U1282A Firmware Update Software (Version 1.05)", file `u1281a_82a_v105.ag` |

[Project Phoenix](https://github.com/aziemer/Project-Phoenix) (open firmware
for the XDM2041) copies the OWON stock rows; its differences are listed under
the OWON runtime notes.

**Method.** One decoder turns every row (R20..R36, missing registers marked
"not written") into named fields; every table below is generated from it, 308
rows in all. The decoder output agrees with the hand decodes of DM40
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
Keysight base rows 1 / 4 / 7 (replaced by base row + per-range overlay). All
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

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | PB0 - PB3 (mV-0)<br>OP1O - PB3 (mV-1)<br>APD p7 50 mV OP1O − PB3; p8 500 mV PB0 − PB3 | OP1O - AGND | OP1O - AGND (50.000mV)<br>OP1O - PB3 (500.00mV) | PB0 - PB3 (500 mV)<br>OP1O - PB3 (50 mV) | OP1O - PB3 | OP1O - PB3 (DC 0.05)<br>PB0 - PB3 (DC 0.5) | OP1O - AGND |
| AD1 reference (+ / −) | PB6 - AGND<br>APD p7–8 PB6 − AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - PB3 |
| AD1IG | 1.8<br>APD p7–8 1.8 | 0.9 | 1.8 (50.000mV)<br>0.9 (500.00mV) | 1.8 | 1.8 | 1.8 | 1.8 |
| AD1RG | 1<br>APD p7–8 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) (mV-0)<br>OSR 16384, CHOP 10, DCOS 0 (200.0 ms) (mV-1)<br>APD p7–8 ADSCHP 11, OSR not marked | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) | OSR 4096, CHOP 11, DCOS 0 (50.0 ms) (50.000mV)<br>OSR 256, CHOP 10, DCOS 0 (3.1 ms) (500.00mV) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | HPN | PN | HPN (50.000mV)<br>HP (500.00mV) | HPN | HLPN | HPN | none |
| SFT1 pre-filter R | 0 (mV-0)<br>100k (mV-1)<br>APD p7–8 100k (FTMOD 00) | open | 100k (50.000mV)<br>open (500.00mV) | 0 (500 mV)<br>100k (50 mV) | 100k | 0 | open |
| OP1 (+ input, OPS1, chop) | off (mV-0)<br>on +PB0 OPS1=1 chop 1 kHz (mV-1)<br>APD p7 on +PB0 OPS1 (x10) chop 2 kHz; p8 off | on +RLU OPS1=1 chop 2 kHz (60 mV)<br>on +RLU OPS1=0 chop 2 kHz (600 mV) | on +PB0 OPS1=1 chop 1 kHz (50.000mV)<br>on +PB0 OPS1=0 chop off (500.00mV) | off (500 mV)<br>on +PB0 OPS1=1 chop 1 kHz (50 mV) | on +PB0 OPS1=1 chop 1 kHz (60mV)<br>on +PB0 OPS1=0 chop 1 kHz (600mV) | on +PB0 OPS1=1 chop 2 kHz (DC 0.05)<br>off (DC 0.5) | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | off |
| PA switches | none<br>APD p6 PS8 (10 M from the input to AGND) | P8 | P8 | none | P7 | P8 | P9 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) | ENVS=1 SMODE=0x11: bus SGND, SMODE4 (RLU) | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x21: bus SGND, SMODE5 (ACC array) |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | 0 | 0 | 1 (50.000mV)<br>0 (500.00mV) | 0 | 0 | 0 | 1 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V<br>APD p7 SREFO PB6, VAGND 00 (0.5) | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off | off | off | off | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | off |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | off | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | off | off | off | off | on, SCMPI=PB0; H=AGNDP3 L=AGNDP1 |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40 is the only source with AD1IG 0.9 on the 60 mV range; every other source
  uses 1.8 there (121GW drops to 0.9 only on 500 mV). The DM40 window at the
  input is ±139 mV on 60 mV ([frontend doc](hy3131-frontend-2026-10-26.md) §4).
- DM40 reads the 600 mV range through OP1 at x1 (OPS1 = 0), as 121GW and Temco
  do; Hycon, Digilent and OWON read PB0 directly.
- DM40 leaves SFT1 open on both ranges; Hycon and Digilent use 100 k on the x10
  range, OWON 0. DM40 AGND is 0.3 · VDDA here (as on DC V 6 V, ohm and
  temperature; 0.5 on DC V ≥60 V, AC and DC A), and its
  comparator stays enabled on PB4 in every DC row.
- 121GW sets AD1CHOP = 11 on 50 mV and OSR 256 on 500 mV in the table; whether
  the firmware changes R22 at run time is unknown.

## DC V

A 10 M input resistor and tap resistors on PA4..PA7 form the divider; the tap of
the range is connected to the power bus and to SENSE (PS + SS), so SENSE carries
the divided input and AD1 reads it against RLU or another low node. The Hycon
network (APD-DMM003 §3) is 10 M / 1.111 M / 101.01 k / 10.01 k / 1 k for /10
.. /10000; the DM40 taps follow the same ratios (*inferred*,
[`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131)). FS9 / FS0 / FS1
connect the input and divider foot to FB.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | SENSE - RLU<br>APD p16 SENSE − PB3 | SENSE - PB5<br>OP1O - RLU (6 V) | SENSE - PB3 | SENSE - RLU | OP1O - RLU | SENSE - RLU | OP1O - RLU |
| AD1 reference (+ / −) | PB6 - AGND<br>APD p16 PB6 − AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - PB3 |
| AD1IG | 1.8<br>APD p16 1.8 | 0.9 | 0.9 | 1.8 | 1.8 | 1.8<br>0.9 (DC 1000) | 1.8<br>0.9 (m1 r3) |
| AD1RG | 1<br>APD p16 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms)<br>APD p16 ADSCHP 11 | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | HPN | PN | HP | HPN | HLPN | HPN | none |
| SFT1 pre-filter R | 100k<br>APD p16 100k | open | open<br>100k (5.0000V) | 100k | 100k | 0 | open |
| OP1 (+ input, OPS1, chop) | off<br>APD p16 off | off<br>on +SENSE OPS1=0 chop 2 kHz (6 V) | on +SENSE OPS1=0 chop off<br>off (5.0000V) | off | on +SENSE OPS1=0 chop off<br>on +RLU OPS1=0 chop off (6V ZERO) | off | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | off |
| PA switches | F9 P8 PS7 F1 F0 (DCV-0)<br>F9 P8 PS6 F1 F0 (DCV-1)<br>F9 P8 PS5 F1 F0 (DCV-2)<br>F9 P8 PS4 F1 F0 (DCV-3)<br>APD p12–15 PS8 + PS7/6/5/4 (tap), SS of the tap, FS9 FS1 | F9 P8 PS7 DF0 (6 V)<br>DF9 P8 PS6 DF0 (60 V)<br>DF9 P8 PS5 DF0 (600 V)<br>DF9 P8 PS5 PS4 DF0 (1000 V) | F9 P8 PS7 F1 F0 (5.0000V)<br>F9 P8 PS6 F1 F0 (50.000V)<br>F9 P8 PS5 F1 F0 (500.00V)<br>F9 P8 PS4 F1 F0 (1000.0V) | F9 P8 PS6 F1 F0 (50 V)<br>F9 P8 PS7 F1 F0 (5 V) | P7 PS3<br>P7 PS2 (60V)<br>P7 PS1 (600V)<br>P7 PS0 (1000V) | F9 P8 PS5 F1<br>F9 P8 PS7 F1 (DC 5)<br>F9 P8 PS6 F1 (DC 50) | S7 P5 PDS1<br>S7 P5 PDS3 (m1 r0)<br>S7 D6 P5 PDS2 (m1 r1) |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array)<br>APD p12 SMODE5 | ENVS=1 SMODE=0x24: bus SVSO1, SMODE5 (ACC array)<br>ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) (6 V) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x00: bus none |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 0<br>1 (DC 1000) | 1 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V<br>ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V (6 V) | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF)<br>127 (25.4 pF) (m1 r1) |
| AD2 in / ref / gain | off | off | off | off | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | off |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | off | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | off | off | off | off | on, SCMPI=PB0; H=AGNDP3 L=AGNDP1 |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 |

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

## AC mV

The low-level input is amplified by OP1 (and on the DM40 also OP2) and digitised
by AD2; the LPF gives the DC part and the RMS block the mean square. AD1 is off
in every source except the 121GW's normal AC rows. The comparator watches the
same signal for the frequency readout (ENCTR = 1).

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | AD1 off | AD1 off | PB7 - PB3 (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 reference (+ / −) | AD1 off | AD1 off | PB6 - AGND (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1IG | AD1 off | AD1 off | 0.9 (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1RG | AD1 off | AD1 off | 1 (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| R22: OSR1, AD1CHOP, DCOS (period) | AD1 off | AD1 off | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 buffers (H L P N) | AD1 off | AD1 off | HP (50.000mV, 500.00mV)<br>AD1 off (50.000mV 1ms-pk, 500.00mV 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| SFT1 pre-filter R | 0 | open | open (50.000mV, 500.00mV)<br>0 (50.000mV 1ms-pk, 500.00mV 1ms-pk) | 0 | 0 | 100k | open |
| OP1 (+ input, OPS1, chop) | on +RLU OPS1=0 chop off (mV-2)<br>on +RLU OPS1=1 chop off (mV-3)<br>APD p10 +RLU OPS1 (x10) chop 2 kHz; p11 x1 chop 2 kHz | on +RLU OPS1=1 chop 2 kHz (60 mV)<br>on +RLU OPS1=0 chop 2 kHz (600 mV) | on +RLU OPS1=1 chop off (50.000mV, 50.000mV 1ms-pk)<br>on +RLU OPS1=0 chop off (500.00mV, 500.00mV 1ms-pk) | on +RLU OPS1=0 chop off (500 mV)<br>on +RLU OPS1=1 chop off (50 mV) | on +PB0 OPS1=1 chop off (60mV)<br>on +PB0 OPS1=0 chop off (600mV) | on +PB0 OPS1=0 chop 2 kHz | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | on +RLU OPS2=1 (60 mV)<br>on +RLU OPS2=0 (600 mV) | off | off | off | off | on +AGND OPS2=1 |
| PA switches | none<br>APD p9 PS8 | P8 | P8 (50.000mV, 500.00mV)<br>none (50.000mV 1ms-pk, 500.00mV 1ms-pk) | none | P7 | P8 | P9 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU)<br>APD p9 SMODE4 (RLD–RLU) | ENVS=1 SMODE=0x11: bus SGND, SMODE4 (RLU) | ENVS=1 SMODE=0x11: bus SGND, SMODE4 (RLU) (50.000mV, 500.00mV)<br>ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) (50.000mV 1ms-pk, 500.00mV 1ms-pk) | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x11: bus SGND, SMODE4 (RLU) | ENVS=1 SMODE=0x21: bus SGND, SMODE5 (ACC array) |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 (50.000mV, 500.00mV)<br>SFUVR=7: AGNDP0 (50.000mV 1ms-pk, 500.00mV 1ms-pk) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1<br>APD p10–11 OP1O − AGND; ref REFO − AGND; IG 0.5, RG 1 | OP2O - AGND; ref PB6 - AGND; IG 1.0, RG 1 | off (50.000mV, 500.00mV)<br>OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 (50.000mV 1ms-pk, 500.00mV 1ms-pk) | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - AGND; ref REFO - AGND; IG 0.5, RG 1 | PB7 - AGND; ref PB6 - PB3; IG 1.5, RG 1/3 |
| R26 AD2: OSR2, clock, chop | OSR2 32, CLK SYSCLK/2, CHOP 0<br>APD p10 ADFOSR 000 (32) | OSR2 32, CLK SYSCLK/2, CHOP 0 | off (50.000mV, 500.00mV)<br>OSR2 64, CLK SYSCLK/2, CHOP 0 (50.000mV 1ms-pk, 500.00mV 1ms-pk) | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/4, CHOP 0 | OSR2 64, CLK SYSCLK/4, CHOP 0 |
| R29 LPF / RMS / peak | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2<br>APD p10 LPFBW 111, ENLPF, ENRMS | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | off (50.000mV, 500.00mV)<br>ENRMS=1 ENLPF=1 OSR4 4096 ENPKH=1 PKHSEL=AD2 (50.000mV 1ms-pk, 500.00mV 1ms-pk) | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=0 ENLPF=1 OSR4 2048 ENPKH=0 PKHSEL=AD2 |
| AD3, SVXI, SDO23 (R34, R35) | not written | ENAD3=1 CHOP=0 RG=1 SVXI=1 SDO23=0; in OP2O - AGND, IG 1.0 | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | on, SCMPI=PB1; H=AGNDP1 L=AGNDN1 (mV-2)<br>on, SCMPI=OP1O; H=AGNDP1 L=AGNDN1 (mV-3) | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB1; H=AGNDP1 L=AGNDN1 (500 mV)<br>on, SCMPI=OP1O; H=AGNDP1 L=AGNDN1 (50 mV) | off | off | on, SCMPI=PB0; H=AGNDP2 L=AGNDN2 |
| Counter, CNT, CMP pin | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=1 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40 enables OP1 and OP2 both on RLU (OPS1 = OPS2 = 1 on 60 mV) and reads
  AD2 (and AD3) on OP2O − AGND with IG 1.0 and reference PB6; what OP1O drives
  on the board is unknown. Hycon, Digilent and Temco use OP1 only, AD2 on OP1O − PB3 with
  IG 0.5 and reference REFO.
- DM40 enables AD3 on the same node with SVXI = 1, so its RMS is ΣXY/N of AD2
  and AD3 rather than ΣX²/N; no other source enables AD3.
- DM40 keeps OP1 chopped at 2 kHz in AC (OWON also on 500 mV); the others turn
  the chopper off.
- 121GW: the normal AC mV rows read AD1 (unchopped, OSR 2048) on PB7 with AD2,
  LPF and RMS off; AD2 + RMS + peak are used only in the "1 ms peak" table.
  Keysight runs AD2 on PB7 with the LPF on and ENRMS = 0 in every row.

## AC V

The divider of DC V is used with the tap on the power bus and SENSE; OP1
buffers SENSE, and the signal reaches AD2 either straight from OP1O or through
an external coupling network (DM40: OP1O → capacitor → PB4, τ = 0.34 s, the
capacitor *inferred*, [frontend doc](hy3131-frontend-2026-10-26.md) §2). SMODE5
connects the ACC capacitor array (0.2 pF steps up to 25.4 pF) for the divider's
high-frequency compensation. The RMS block gives the AC value, the LPF the DC
part.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | AD1 off | AD1 off | PB7 - PB3 (5.0000V, 50.000V, 500.00V, 1000.0V)<br>AD1 off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 reference (+ / −) | AD1 off | AD1 off | PB6 - AGND (5.0000V, 50.000V, 500.00V, 1000.0V)<br>AD1 off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1IG | AD1 off | AD1 off | 0.9 (5.0000V, 50.000V, 500.00V, 1000.0V)<br>AD1 off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1RG | AD1 off | AD1 off | 1 (5.0000V, 50.000V, 500.00V, 1000.0V)<br>AD1 off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| R22: OSR1, AD1CHOP, DCOS (period) | AD1 off | AD1 off | AD1 off<br>OSR 1024, CHOP 00, DCOS 0 (6.2 ms) (5.0000V, 50.000V, 500.00V)<br>OSR 256, CHOP 00, DCOS 0 (1.6 ms) (1000.0V) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 buffers (H L P N) | AD1 off | AD1 off | PN (5.0000V, 50.000V, 500.00V, 1000.0V)<br>AD1 off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | AD1 off | AD1 off | AD1 off | AD1 off |
| SFT1 pre-filter R | 100k | open | open (5.0000V, 50.000V, 500.00V, 1000.0V)<br>100k (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | 100k | 100k | 100k | open |
| OP1 (+ input, OPS1, chop) | on +SENSE OPS1=0 chop off<br>APD p22 +SENSE x1, chop 00 | on +SENSE OPS1=0 chop 2 kHz<br>on +SENSE OPS1=1 chop 2 kHz (6 V) | on +SENSE OPS1=1 chop off (5.0000V, 50.000V, 500.00V, 1000.0V)<br>on +SENSE OPS1=0 chop off (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | on +SENSE OPS1=0 chop off | on +SENSE OPS1=0 chop off<br>on +PB1 OPS1=0 chop off (6V ZERO) | on +SENSE OPS1=0 chop off | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | on +AGND OPS2=1 |
| PA switches | P9 P8 PDS7 F1 F0 (ACV-0)<br>F9 P8 PDS6 F1 F0 (ACV-1)<br>F9 P8 PS5 F1 F0 (ACV-2)<br>F9 P8 PS4 F1 F0 (ACV-3)<br>APD p18 5 V: PS9 PS8 PS7 DS7 SS7; p19 50 V: PS8 PS6 DS6 SS6 FS9 FS1; p20–21: PS8 PS5/PS4 SS FS9 FS1 | F9 P8 PS6 F1<br>F9 P8 PS5 F1 (600 V)<br>F9 P8 PS5 PS4 F1 (1000 V) | P9 P8 PS4<br>P9 P8 PS6 (5.0000V, 50.000V 1ms-pk)<br>P9 P8 PS5 (50.000V, 500.00V 1ms-pk)<br>P9 P8 PS7 (5.0000V 1ms-pk) | F9 P8 PDS6 F1 F0 (30 V)<br>P9 P8 PDS7 F1 F0 (5 V) | P7 PS3<br>P7 PS2 (60V)<br>P7 PS1 (600V)<br>P7 PS0 (1000V) | F9 P8 PS5 F1 F0<br>P9 P8 PDS7 F1 F0 (AC 5)<br>F9 P8 PS6 F1 F0 (AC 50) | S7 P5 PDS3 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x30: bus none, SMODE4 (RLU), SMODE5 (ACC array) (5.0000V, 50.000V, 500.00V, 1000.0V)<br>ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x00: bus none |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 (5.0000V, 50.000V, 500.00V, 1000.0V)<br>SFUVR=7: AGNDP0 (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 1 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF)<br>25 (5.0 pF) (ACV-0)<br>48 (9.6 pF) (ACV-1)<br>APD p18–19 capacitor array on DS bus (value not given) | 0 (0.0 pF) | 0 (0.0 pF)<br>25 (5.0 pF) (5.0000V 1ms-pk)<br>48 (9.6 pF) (50.000V 1ms-pk) | 127 (25.4 pF) | 25 (5.0 pF) (6V, 6V ZERO)<br>0 (0.0 pF) (600V, 1000V)<br>48 (9.6 pF) (60V) | 0 (0.0 pF)<br>105 (21.0 pF) (AC 5)<br>127 (25.4 pF) (AC 50) | 64 (12.8 pF) |
| AD2 in / ref / gain | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1<br>APD p22 OP1O − AGND; ref REFO − AGND; IG 0.5 | PB4 - AGND; ref PB6 - AGND; IG 1.0, RG 1 | off (5.0000V, 50.000V, 500.00V, 1000.0V)<br>OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - AGND; ref REFO - AGND; IG 0.5, RG 1 | PB7 - AGND; ref PB6 - PB3; IG 1.5, RG 1/3 |
| R26 AD2: OSR2, clock, chop | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/2, CHOP 0 | off (5.0000V, 50.000V, 500.00V, 1000.0V)<br>OSR2 1024, CLK SYSCLK/2, CHOP 0 (5.0000V 1ms-pk, 50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk) | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/4, CHOP 0 | OSR2 64, CLK SYSCLK/4, CHOP 0 |
| R29 LPF / RMS / peak | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2<br>APD p22 LPFBW 111, ENLPF, ENRMS | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=1 PKHSEL=AD2 | off<br>ENRMS=1 ENLPF=1 OSR4 4096 ENPKH=1 PKHSEL=AD2 (50.000V 1ms-pk, 500.00V 1ms-pk, 1000.0V 1ms-pk)<br>ENRMS=1 ENLPF=1 OSR4 128 ENPKH=1 PKHSEL=AD2 (5.0000V 1ms-pk) | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=0 ENLPF=1 OSR4 2048 ENPKH=0 PKHSEL=AD2 |
| AD3, SVXI, SDO23 (R34, R35) | AD3 off (R34=0x02) (ACV-0, ACV-3)<br>not written (ACV-1, ACV-2) | ENAD3=1 CHOP=0 RG=1 SVXI=1 SDO23=0; in PB4 - AGND, IG 1.0 | not written | off (0x00) (30 V)<br>AD3 off (R34=0x02) (5 V) | not written | off (0x00) | not written |
| SPHACAL (R36) | 0x0c (ACV-0, ACV-3)<br>not written (ACV-1, ACV-2) | not written | not written | 0x00 (30 V)<br>0x0c (5 V) | not written | 0x00 | not written |
| Comparator (R20, R21) | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | off | on, SCMPI=PB0; H=AGNDP2 L=AGNDN2 |
| Counter, CNT, CMP pin | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=1 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40 feeds AD2 from PB4 (the external coupling network) with IG 1.0 and
  reference PB6; Hycon, Digilent and Temco use OP1O − PB3, IG 0.5, reference
  REFO; OWON OP1O − AGND.
- ACC: DM40 0 on every range (measured: no effect on the DM40, [frontend
  doc](hy3131-frontend-2026-10-26.md) §3); Hycon 25 / 48 (5.0 / 9.6 pF) on the
  two low ranges, Digilent 127, Keysight 64 (127 on one range, and a calibrated
  value on another); OWON loads it per unit from calibration (E9 / FF in the
  capture are one unit's values).
- DM40 compensates with FS9 + FS1 (no FS0) and the tap on the bus (PS6 on 6 V
  and 60 V, OP1 x10 on 6 V); Hycon and OWON close FS0 / FS1 / FS9 and drive
  the tap with DS as well on the low range (PDS7).
- DM40 enables AD3 + SVXI (ΣXY) and the peak-hold on AD2 (ENPKH = 1); 121GW
  normal rows use AD1 on PB7 with AD2 off, as in AC mV.

## DC A

The current flows through a shunt chain; the shunt voltage is read on PB1 (or
PB2) against PB3 or AGND, either directly or through OP1 at x10. The DM40 shunts
are 10 mΩ (A, PB2), 0.5 Ω (mA) and 47.5 Ω (µA) on PB1, switched by MOSFETs from
MCU pins PE0 / PE1 ([`hw/README.md`](../../hw/README.md#dmm-front-end-hy3131));
the mA and µA ranges share one row. The Hycon board has 0.01 / 0.99 / 99 Ω
(APD-DMM003 §9).

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | PB1 - PB3 (Current-0, Current-2)<br>OP1O - PB3 (Current-1, Current-3)<br>APD p45 50 mA OP1O − PB3; p46 500 mA PB1 − PB3 | OP1O - PB3 | PB1 - AGND | PB1 - PB3<br>OP1O - PB3 (50 mA, 500 uA) | OP1O - PB3 (60mA, 6A)<br>PB1 - PB3 (600mA, 10A) | OP1O - PB3<br>PB1 - PB3 (DC 0.005, DC 0.5) | OP1O - AGND |
| AD1 reference (+ / −) | PB6 - AGND<br>APD p45–46 PB6 − AGND (REFO block drawn black) | PB6 - AGND | PB6 - AGND | PB6 - AGND | PB6 - AGND | REFO - AGND | PB6 - PB3 |
| AD1IG | 1.8<br>APD p45–46 1.8 | 0.9 | 1.8<br>3.6 (500.00mA) | 1.8 | 1.8 (60mA, 600mA)<br>0.9 (6A, 10A) | 1.8<br>0.9 (DC 10) | 0.9 |
| AD1RG | 1 | 1 | 1<br>1/3 (500.00mA) | 1 | 1 | 1 | 1/3 |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) (Current-0, Current-1)<br>OSR 16384, CHOP 10, DCOS 0 (200.0 ms) (Current-2, Current-3) | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) | OSR 1024, CHOP 10, DCOS 0 (12.5 ms) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | HPN | PN<br>none (6 A) | HPN | HPN | HLPN | HPN | none |
| SFT1 pre-filter R | 100k<br>APD p45–46 100k | open<br>10k (mA,µA r0) | open | 100k | 100k | open | open |
| OP1 (+ input, OPS1, chop) | off (Current-0, Current-2)<br>on +PB1 OPS1=1 chop 2 kHz (Current-1, Current-3)<br>APD p45 +PB1 x10 chop 2 kHz; p46 off | on +PB2 OPS1=1 chop 2 kHz (6 A)<br>on +PB2 OPS1=0 chop 2 kHz (10 A)<br>on +PB1 OPS1=1 chop 2 kHz (mA,µA r0)<br>on +PB1 OPS1=0 chop 2 kHz (mA,µA r1) | off | off<br>on +PB1 OPS1=1 chop 2 kHz (50 mA, 500 uA) | off<br>on +PB1 OPS1=1 chop 2 kHz (60mA)<br>on +PB1 OPS1=0 chop 2 kHz (6A) | on +PB1 OPS1=1 chop 2 kHz<br>off (DC 0.005, DC 0.5) | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | off |
| PA switches | none | P8 | none | none | none | none | P5 PFS1 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x21: bus SGND, SMODE5 (ACC array) |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off | off | off | off | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | off |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | off | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | off | off | off | off | on, SCMPI=PB0; H=AGNDP1 L=AGNDN1 |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40 routes every range through OP1 (x10 on the low range of each shunt, x1 on
  the high one) and reads OP1O − PB3; Hycon, Digilent, Temco and OWON alternate
  OP1 x10 and the bare shunt pin; 121GW reads PB1 directly and changes AD1IG
  (1.8 / 3.6) and AD1RG (1 / 1/3) instead.
- DM40 AD1IG is 0.9 on every range (others 1.8, 0.9 only on 10 A for OWON /
  Temco). DM40 sets SFT1 = 10 k on the low mA / µA range (60 mA, 600 µA) and
  open elsewhere.
- OWON references AD1 to REFO − AGND here, the others to PB6 − AGND
  (Keysight PB6 − PB3 with RG 1/3).

## AC A

The shunt voltage of DC A goes to AD2 through OP1 (x10 or x1); RMS and LPF as in
AC V.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | AD1 off | AD1 off | PB7 - AGND | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 reference (+ / −) | AD1 off | AD1 off | PB6 - AGND | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1IG | AD1 off | AD1 off | 1.8 | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1RG | AD1 off | AD1 off | 1 | AD1 off | AD1 off | AD1 off | AD1 off |
| R22: OSR1, AD1CHOP, DCOS (period) | AD1 off | AD1 off | OSR 1024, CHOP 10, DCOS 0 (12.5 ms) | AD1 off | AD1 off | AD1 off | AD1 off |
| AD1 buffers (H L P N) | AD1 off | AD1 off | HPN | AD1 off | AD1 off | AD1 off | AD1 off |
| SFT1 pre-filter R | 0 | open | open | 0 | 0 | 100k | open |
| OP1 (+ input, OPS1, chop) | on +PB1 OPS1=0 chop off<br>on +PB1 OPS1=1 chop off (Current-5)<br>APD p47 +PB1 x10 chop 00; p48 x1 chop 2 kHz | on +PB2 OPS1=1 chop 2 kHz (6 A)<br>on +PB2 OPS1=0 chop 2 kHz (10 A)<br>on +PB1 OPS1=1 chop 2 kHz (mA,µA r0)<br>on +PB1 OPS1=0 chop 2 kHz (mA,µA r1) | on +PB1 OPS1=1 chop off<br>on +PB1 OPS1=0 chop off (10.000A) | on +PB1 OPS1=1 chop off<br>on +PB1 OPS1=0 chop off (500 mA, 5 mA) | on +PB1 OPS1=0 chop off<br>on +PB1 OPS1=1 chop off (60mA) | on +PB1 OPS1=1 chop off (AC 0.0005, AC 0.05, AC 5)<br>on +PB1 OPS1=0 chop 2 kHz (AC 0.005, AC 0.5, AC 10) | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | on +AGND OPS2=1 |
| PA switches | none | P8 | none | none | none | none | P5 PFS1 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x10: bus none, SMODE4 (RLU) | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | ENVS=1 SMODE=0x21: bus SGND, SMODE5 (ACC array) |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1<br>APD p47–48 OP1O − AGND; ref REFO − AGND; IG 0.5 | OP1O - PB3; ref PB6 - AGND; IG 1.0, RG 1 | off | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 | OP1O - AGND; ref REFO - AGND; IG 0.5, RG 1 | PB7 - AGND; ref PB6 - PB3; IG 1.0, RG 1/3 |
| R26 AD2: OSR2, clock, chop | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/2, CHOP 0 | off | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/2, CHOP 0 | OSR2 32, CLK SYSCLK/4, CHOP 0 | OSR2 64, CLK SYSCLK/4, CHOP 0 |
| R29 LPF / RMS / peak | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2<br>ENRMS=1 ENLPF=1 OSR4 2048 ENPKH=0 PKHSEL=AD2 (Current-6)<br>APD p47 LPFBW 111, ENLPF, ENRMS | ENRMS=1 ENLPF=1 OSR4 8192 ENPKH=1 PKHSEL=AD2 (6 A, 10 A)<br>ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=1 PKHSEL=AD2 (mA,µA r0, mA,µA r1) | off | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | ENRMS=0 ENLPF=1 OSR4 2048 ENPKH=0 PKHSEL=AD2 |
| AD3, SVXI, SDO23 (R34, R35) | not written | ENAD3=1 CHOP=0 RG=1 SVXI=1 SDO23=0; in OP1O - PB3, IG 1.0 | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | on, SCMPI=PB1; H=AGNDP1 L=AGNDN1<br>on, SCMPI=OP1O; H=AGNDP1 L=AGNDN1 (Current-5) | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=OP1O; H=AGNDP1 L=AGNDN1<br>on, SCMPI=PB1; H=AGNDP1 L=AGNDN1 (500 mA, 5 mA) | off | off | on, SCMPI=PB0; H=AGNDP1 L=AGNDN1 |
| Counter, CNT, CMP pin | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=1 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40: AD2 on OP1O − PB3 with IG 1.0, reference PB6, AD3 + SVXI, peak-hold
  on, OP1 chopped at 2 kHz; the A ranges use LPF OSR4 8192 instead of 16384.
  Hycon, Digilent and Temco: IG 0.5, reference REFO, chopper off.
- 121GW normal rows: AD1 on PB7 (OSR 1024), AD2 off.

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

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | PB0 - PB3<br>OP1O - PB3 (OHM-0, OHM-1)<br>FB - RLU (SAD1I=1; filter mux PB0 - PB3) (OHM-8)<br>APD p35 50 Ω OP1O − PB3 (OP1 x10); p36, p41 PB0 − PB3 | OP1O - PB3 | PB0 - PB3 | PB0 - PB3<br>OP1O - PB3 (500 Ohm, 50 Ohm) | PB0 - PB3<br>OP1O - PB3 (60, 600, 6K, 60K) | PB0 - PB3<br>PB2 - PB5 (FRES 500, FRES 5E3, FRES 50E3) | OP1O - AGND |
| AD1 reference (+ / −) | FB - RLU<br>VDDA - VSSA (OHM-4, OHM-5, OHM-6)<br>FB - AGND (OHM-7, OHM-8)<br>APD p35–36 FB − RLU; p41 (500 k..50 M) VDDA − VSSA | FB - RLU<br>REFO - AGND (60 M) | FB - RLU<br>VDDA - VSSA (5.0000M Ω, 50.000M Ω) | FB - RLU<br>VDDA - VSSA (50M Ohm, 5M Ohm, 500k Ohm) | FB - RLU<br>PB6 - AGND (CC0, CC1, CC2, CC3, CC4)<br>VDDA - VSSA (600K, 6M, 60M) | FB - RLU<br>VDDA - VSSA (RES 5E6, RES 50E6) | FB - RLU |
| AD1IG | 0.9 | 0.9 | 0.9 | 0.9 | 0.9 | 0.9 | 0.9 |
| AD1RG | 1<br>1/3 (OHM-4, OHM-5, OHM-6)<br>APD p35–36 1; p41 1/3 | 1 | 1<br>1/3 (5.0000M Ω, 50.000M Ω) | 1<br>1/3 (50M Ohm, 5M Ohm, 500k Ohm) | 1<br>1/3 (600K, 6M, 60M) | 1<br>1/3 (RES 5E6, RES 50E6) | 1 |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) (50.000 Ω, 500.00 Ω, 50.000M Ω)<br>OSR 4096, CHOP 10, DCOS 0 (50.0 ms) (50.000k Ω, 500.00k Ω, 5.0000M Ω)<br>OSR 2048, CHOP 10, DCOS 0 (25.0 ms) (5.0000k Ω) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | PN | PN<br>HLPN (600 k, 6 M) | PN<br>HLPN (50.000 Ω, 500.00 Ω) | PN | PN<br>HLPN (CV0, CV1, CV2, CV3, CV4) | PN | HLPN<br>none (m4 r0, m4 r1, m4 r2, m4 r3) |
| SFT1 pre-filter R | 100k<br>APD p35–41 100k | 0<br>100k (6 M, 60 M) | open<br>100k (500.00k Ω) | 100k | 100k | open | 100k |
| OP1 (+ input, OPS1, chop) | off<br>on +PB0 OPS1=0 chop 2 kHz (OHM-1, OHM-5, OHM-6)<br>on +PB0 OPS1=1 chop 2 kHz (OHM-0) | on +PB0 OPS1=0 chop 1 kHz | on +PB0 OPS1=0 chop off<br>off (5.0000M Ω, 50.000M Ω) | on +PB0 OPS1=0 chop 2 kHz (50M Ohm, 5M Ohm, 500 Ohm)<br>off (500k Ohm, 50k Ohm, 5k Ohm)<br>on +PB0 OPS1=1 chop 2 kHz (50 Ohm) | on +PB0 OPS1=0 chop off<br>on +PB0 OPS1=0 chop 1 kHz (CC0, CC1, CC2, CC3, CC4)<br>on +PB0 OPS1=0 chop 2 kHz (600, 6M, 60M)<br>on +PB0 OPS1=1 chop 2 kHz (60)<br>off (600K) | on +PB8 OPS1=1 chop 2 kHz | on +PB1 OPS1=0 chop 2 kHz<br>on +PB1 OPS1=1 chop 2 kHz (m4 r0) |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | off |
| PA switches | D9 D8 DF4 D3 (OHM-0, OHM-1)<br>D9 DF8 D3 (OHM-7, OHM-8)<br>D9 D8 DF5 D3 (OHM-2)<br>D9 D8 DF6 D3 (OHM-3)<br>P9 P6 P2 (OHM-4)<br>P9 P7 P2 (OHM-5)<br>P9 P2 (OHM-6)<br>APD p33 DS4 FS4 DS3 (10 nF); p34 DS5 FS5; p37 DS6 FS6; p38 PS6 PS2; p39 PS7 PS2; p40 PS2 | DF4 D3 (600 ohm)<br>DF5 D3 (6 k)<br>DF6 D3 (60 k)<br>DF7 DF2 (600 k)<br>DF2 (6 M)<br>P2 (60 M) | D8 DF4 D3<br>DF5 D3 (5.0000k Ω)<br>DF6 D3 (50.000k Ω)<br>DF7 D3 (500.00k Ω)<br>P9 P7 P2 (5.0000M Ω)<br>P2 P0 (50.000M Ω) | D9 D8 DF4 D3<br>P9 P2 (50M Ohm)<br>P9 P7 P2 (5M Ohm)<br>P9 P6 P2 (500k Ohm)<br>D9 D8 DF6 D3 (50k Ohm)<br>D9 D8 DF5 D3 (5k Ohm) | D4 DF0 (60, 600)<br>P5 P3 (600K, 6M)<br>D8 DF4 D3 (CV0)<br>D8 DF5 D3 (CV1)<br>D8 DF6 D3 (CV2)<br>D8 DF7 D3 (CV3)<br>DF8 DF5 D3 (CV4)<br>D4 DF1 (6K)<br>D4 DF2 (60K)<br>P5 (60M)<br>P4 D3 (CC0)<br>P5 D3 (CC1)<br>P6 D3 P2 (CC2)<br>P7 D3 P2 (CC3)<br>D3 P2 (CC4) | DF4 (RES 500, FRES 500)<br>DF5 (RES 5E3, FRES 5E3)<br>DF6 (RES 50E3, FRES 50E3)<br>DF7 (RES 500E3)<br>P7 (RES 5E6)<br>P2 (RES 50E6) | DF0<br>D8 DF1 (m4 r0)<br>DF1 (m4 r1)<br>DF2 (m4 r2)<br>DF3 (m4 r3)<br>DF4 (m4 r4) |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (OHM-4, OHM-5, OHM-6)<br>APD p33 SMODE 0100 (SVSO1) + SMODE4; p38–40 0110 (SVDD SVSO2 SCP) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (60 M) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (5.0000M Ω, 50.000M Ω) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (50M Ohm, 5M Ohm, 500k Ohm) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (600K, 6M, 60M, CC0, CC1, CC2, CC3, CC4) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (RES 5E6, RES 50E6) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU) |
| OP3 VREF (SFUVR) | SFUVR=3: AGNDP9<br>SFUVR=1: VDS16 (OHM-4, OHM-5, OHM-6)<br>SFUVR=2: VDS15 (OHM-0, OHM-1)<br>APD p33 VDS15; p34, p37 AGNDP9; p38–40 VDS16 | SFUVR=2: VDS15<br>SFUVR=1: VDS16 (60 M) | SFUVR=2: VDS15<br>SFUVR=3: AGNDP9 (5.0000k Ω, 50.000k Ω)<br>SFUVR=1: VDS16 (5.0000M Ω, 50.000M Ω) | SFUVR=1: VDS16<br>SFUVR=3: AGNDP9 (50k Ohm, 5k Ohm)<br>SFUVR=2: VDS15 (500 Ohm, 50 Ohm) | SFUVR=1: VDS16<br>SFUVR=2: VDS15 (CV0, CV1, CV2, CV3, CV4, 60, 600)<br>SFUVR=3: AGNDP9 (6K, 60K) | SFUVR=3: AGNDP9<br>SFUVR=2: VDS15 (RES 500, FRES 500)<br>SFUVR=1: VDS16 (RES 5E6, RES 50E6) | SFUVR=2: VDS15 |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0<br>1 (600K) | 0 | 1 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V<br>APD p35–41 VAGND 01 (0.3) | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V<br>ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.5 V (600 k, 6 M) | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off | off | off | off | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | off |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written<br>off (0x00) (OHM-7, OHM-8) | off (0x00) | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written<br>0x00 (OHM-7, OHM-8) | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | off<br>on, SCMPI=PB0; H=AGNDP2 L=AGNDP1 (OHM-7, OHM-8) | off | off<br>on, SCMPI=PB0; H=AGNDP2 L=AGNDP1 (50.000 Ω, 500.00 Ω) | off | off | off | on, SCMPI=OP1O; H=AGNDP2 L=AGNDP1 |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0<br>ENCTR=0 ENCNTI=0 ENPCMPO=1 (50.000 Ω, 500.00 Ω) | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

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
- 4-wire: only the OWON (sense on PB2 − PB5, `FRES`); the others have no 4-wire
  rows.

## Continuity

An ohm configuration at the low range with the comparator watching the
voltage across the probe: the comparator output CMPO is enabled to the CMP pin
(ENPCMPO = 1), so the beeper is driven by the chip without the MCU.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | OP1O - PB3<br>APD p44 no ADC block | OP1O - PB3 | PB0 - PB3 | OP1O - PB3 | PB0 - PB3 | PB0 - PB3 | — |
| AD1 reference (+ / −) | FB - RLU | RLU - FB | FB - RLU | FB - RLU | VDDA - VSSA | FB - RLU | — |
| AD1IG | 1.8 | 0.9 | 0.9 | 1.8 | 0.9 | 0.9 | — |
| AD1RG | 1 | 1 | 1 | 1 | 1 | 1 | — |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) | OSR 256, CHOP 10, DCOS 0 (3.1 ms) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) | — |
| AD1 buffers (H L P N) | PN | PN | HLPN | PN | PN | PN | — |
| SFT1 pre-filter R | 100k | 0 | open | 100k | 100k | open | — |
| OP1 (+ input, OPS1, chop) | on +PB0 OPS1=0 chop 2 kHz | on +SENSE OPS1=0 chop 1 kHz | on +PB0 OPS1=0 chop off | on +PB0 OPS1=0 chop 2 kHz | off | on +PB8 OPS1=1 chop 2 kHz | — |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | — |
| PA switches | D9 PF4<br>APD p44 PS1 (1 k) | S8 DF4 D3 | D8 DF4 D3 | D9 PF4 | P0 | DF4 D3 | — |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP<br>APD p44 0110 (SVDD SVSO2 SCP) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU) | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU) | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x14: bus SVSO1, SMODE4 (RLU) | — |
| OP3 VREF (SFUVR) | SFUVR=2: VDS15<br>APD p44 VDS15 | SFUVR=13: AGNDN7 | SFUVR=2: VDS15 | SFUVR=2: VDS15 | SFUVR=2: VDS15 | SFUVR=2: VDS15 | — |
| SDIO (PB0–PB8) | 0 | 0 | 0 | 0 | 0 | 0 | — |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V (DiodeBeeperHz-1)<br>ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V (DiodeBeeperHz-2) | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | — |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | — |
| AD2 in / ref / gain | off | off | off | off | off | off | — |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | — |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | — |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | off (0x00) | not written | off (0x00) | — |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | — |
| Comparator (R20, R21) | on, SCMPI=PB0; H=AGNDP2 L=AGNDP1<br>APD p44 on, PB0; H=AGNDP2 L=AGNDN1 | on, SCMPI=OP1O; H=AGNDP0 L=AGNDN2 | on, SCMPI=PB0; H=AGNDP2 L=AGNDP1 | on, SCMPI=PB0; H=AGNDP2 L=AGNDP1 | on, SCMPI=PB0; H=AGNDP2 L=AGNDP1 | on, SCMPI=PB0; H=VDSC7 L=VDSC7 | — |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=1 (DiodeBeeperHz-1)<br>ENCTR=0 ENCNTI=1 ENPCMPO=1 (DiodeBeeperHz-2)<br>APD p44 ACPO → CMPO; CMP pin drawn black | ENCTR=1 ENCNTI=0 ENPCMPO=1 | ENCTR=0 ENCNTI=0 ENPCMPO=1 | ENCTR=0 ENCNTI=0 ENPCMPO=1 | ENCTR=0 ENCNTI=0 ENPCMPO=1 | ENCTR=1 ENCNTI=0 ENPCMPO=1 | — |

- DM40 compares OP1O (OP1 on SENSE) against AGNDP0 / AGNDN2 and swaps the AD1
  reference (RLU − FB, read as a negative value); OP3 VREF is AGNDN7 (SFUVR
  13, below AGND). The others compare PB0 against AGNDP2 / AGNDP1 with OP3 at
  VDS15; OWON uses VDSC7 for both thresholds.
- DM40 runs AD1 at OSR 2048 with SFT1 = 0; ENCTR = 1.

## Diode

OP3 with the current-source switches (SMODE 0110) drives the diode; SDIO
connects PB0 to PB8 so the 900 k / 100 k divider brings the forward voltage
(up to ~3 V) into range; AGND is set to 0.1 · VDDA to leave room above it.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | PB0 - PB3<br>APD p43 PB0 − PB3 | OP1O - PB3 | PB0 - PB3 | PB0 - PB3 | PB0 - PB3 | PB0 - PB3 | OP1O - AGND |
| AD1 reference (+ / −) | PB6 - AGND<br>APD p43 PB6 − AGND | PB6 - AGND | REFO - AGND | PB6 - AGND | PB6 - AGND | REFO - AGND | PB6 - PB3 |
| AD1IG | 1.8<br>APD p43 1.8 | 0.9 | 0.9 | 1.8 | 2.7 | 1.8 | 0.9 |
| AD1RG | 1 | 1 | 1 | 1 | 1 | 1 | 1/3 |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) | OSR 1024, CHOP 10, DCOS 0 (12.5 ms) | OSR 32768, CHOP 10, DCOS 0 (400.0 ms) | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 2048, CHOP 10, DCOS 0 (25.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | HPN | PN | PN | HPN | HLPN | HPN | none |
| SFT1 pre-filter R | 100k | 0 | open | 100k | 100k | open | 100k |
| OP1 (+ input, OPS1, chop) | off | on +PB0 OPS1=0 chop 1 kHz | off | off | off | off | on +SENSE OPS1=0 chop off |
| OP2 (+ input, OPS2) | off | off | off | off | off | off | off |
| PA switches | D9 P4<br>APD p42 PS6 (101 k) | P5 P4 P1 | D9 P4 | D9 P4 | P2 | P4 | S9 P1 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP<br>APD p42 0110 | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x12: bus SVDD, SMODE4 (RLU) | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP | ENVS=1 SMODE=0x12: bus SVDD, SMODE4 (RLU) |
| OP3 VREF (SFUVR) | SFUVR=2: VDS15<br>APD p42 VDS15 | SFUVR=2: VDS15 | SFUVR=1: VDS16 | SFUVR=2: VDS15 | SFUVR=2: VDS15 | SFUVR=2: VDS15 | SFUVR=2: VDS15 |
| SDIO (PB0–PB8) | 1<br>APD p42 1 | 1 | 1 | 1 | 1 | 1 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V<br>APD p43 VAGND 10 (0.1) | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.1 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off | off | off | off | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | off | off | off | off |
| R29 LPF / RMS / peak | off | off | off | off | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | off (0x00) | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | 0x00 | not written | 0x00 | not written |
| Comparator (R20, R21) | off | on, SCMPI=RLD; H=AGNDN1 L=AGNDP1 | off | off | off | on, SCMPI=SENSE; H=VDSC7 L=VDSC7 | on, SCMPI=PB0; H=AGNDP1 L=AGNDN1 |
| Counter, CNT, CMP pin | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

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

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | PB0 - PB3<br>AD1 off (CAP-0, CAP-1)<br>APD p28, p30 PB0 − PB3 | AD1 off<br>OP2O - AGND (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | AD1 off<br>PB2 - PB3 (10.00uF, 100.0uF) | — | AD1 off<br>PB0 - PB3 (Charge ADC get, disCharge) | PB0 - PB3<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>OP1O - AGND (m7 r8) |
| AD1 reference (+ / −) | VDDA - VSSA<br>AD1 off (CAP-0, CAP-1)<br>APD p28, p30 VDDA − VSSA | AD1 off<br>PB6 - AGND (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | AD1 off<br>VDDA - VSSA (10.00uF, 100.0uF) | — | AD1 off<br>VDDA - VSSA (Charge ADC get, disCharge) | VDDA - VSSA<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>PB6 - PB3 (m7 r8) |
| AD1IG | 1.8<br>AD1 off (CAP-0, CAP-1)<br>APD p28 1.8 | AD1 off<br>0.9 (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | AD1 off<br>0.9 (10.00uF, 100.0uF) | — | AD1 off<br>1.8 (Charge ADC get, disCharge) | 1.8<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>0.9 (m7 r8) |
| AD1RG | 1/3<br>AD1 off (CAP-0, CAP-1)<br>APD p28 1/3 | AD1 off<br>1 (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | AD1 off<br>1 (10.00uF, 100.0uF) | — | AD1 off<br>1/3 (Charge ADC get, disCharge) | 1/3<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>1/3 (m7 r8) |
| R22: OSR1, AD1CHOP, DCOS (period) | OSR 4096, CHOP 00, DCOS -0.25 (25.0 ms)<br>AD1 off (CAP-0, CAP-1)<br>APD p28 OSR 4096, CHOP 00, DCOS 101 (−0.25) | AD1 off<br>OSR 4096, CHOP 10, DCOS 0 (50.0 ms) (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp))<br>OSR 2048, CHOP 10, DCOS 0 (25.0 ms) (discharge (row 8)) | AD1 off<br>OSR 4096, CHOP 00, DCOS 0 (25.0 ms) (10.00uF, 100.0uF) | — | AD1 off<br>OSR 16384, CHOP 00, DCOS -0.25 (100.0 ms) (Charge ADC get, disCharge) | OSR 32768, CHOP 00, DCOS 0 (200.0 ms)<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>OSR 4096, CHOP 10, DCOS 0 (50.0 ms) (m7 r8) |
| AD1 buffers (H L P N) | PN<br>AD1 off (CAP-0, CAP-1) | AD1 off<br>PN (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | AD1 off<br>HP (10.00uF, 100.0uF) | — | AD1 off<br>PN (Charge ADC get, disCharge) | PN<br>previous-mode leftover (CAP 50E-9) | AD1 off<br>none (m7 r8) |
| SFT1 pre-filter R | open<br>100k (CAP-0, CAP-1)<br>APD p28 open (11) | open<br>0 (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | 100k | — | open<br>100k (60nF~600nF ->0, 600nF~60uF ->1, 60uF~600uF ->2) | open<br>0 (CAP 50E-9) | open |
| OP1 (+ input, OPS1, chop) | off<br>on +PB0 OPS1=1 chop off (CAP-4) | on +PB0 OPS1=0 chop 2 kHz | on +PB2 OPS1=0 chop off<br>on +PB0 OPS1=1 chop off (10.00mF) | — | off<br>on +PB2 OPS1=0 chop off (CV0, CV1, CV2, CV3)<br>on +PB2 OPS1=1 chop off (CC0, CC1, CC2, CC3) | off | on +PB1 OPS1=0 chop off<br>on +PB1 OPS1=1 chop off (m7 r6, m7 r7, m7 r8, m7 r9) |
| OP2 (+ input, OPS2) | off | off<br>on +PB0 OPS2=0 (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | off | — | off | off | off |
| PA switches | P6<br>P5 (CAP-1)<br>P1 (CAP-3)<br>P4 P1 (CAP-4)<br>APD p24–26 PS6 (101 k); p27, p29 PS1 (1 k) | P9 P7 P2 (r0 6 nF (RC), r1 60 nF (RC))<br>P6 P5 (r3 6 uF (RC), r6 6 mF (ramp))<br>P6 (r2 600 nF (RC))<br>P5 P1 (r4 60 uF (RC))<br>P7 P6 (r5 600 uF (ramp))<br>P6 P5 P4 (r7 60 mF (ramp))<br>P7 P6 P5 P4 (discharge (row 8)) | P7 P2 (10.00nF, 100.0nF)<br>P4 P2 (10.00uF, 100.0uF)<br>P5 P2 (1.000uF)<br>P4 (10.00mF) | — | P0<br>P7 P2 (CV0, CC0)<br>P6 P2 (CV1, CC1)<br>P5 P2 (CV2, CC2)<br>P4 P2 (CV3, CC3)<br>P2 (60nF~600nF ->0)<br>P1 (600nF~60uF ->1) | P4 P1<br>P6 (CAP 50E-9) | P3 (m7 r0, m7 r1, m7 r2, m7 r3)<br>P1 (m7 r5, m7 r6, m7 r7, m7 r8)<br>P2 (m7 r4)<br>FS3 F2 (m7 r9) |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x0e: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN (CAP-1, CAP-4)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (CAP-2, CAP-3)<br>ENVS=1 SMODE=0x1a: bus CMPO 0/1: SVDD / SVSS, SMODE4 (RLU) (CAP-0)<br>APD p24 CV 1010 (SVDD/SVSS by CMPO) + SMODE4; p25 CC 1110; p26–27 0110 charge; p29 1110 discharge | ENVS=1 SMODE=0x1a: bus CMPO 0/1: SVDD / SVSS, SMODE4 (RLU)<br>ENVS=1 SMODE=0x0e: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | ENVS=1 SMODE=0x0e: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN<br>ENVS=1 SMODE=0x1a: bus CMPO 0/1: SVDD / SVSS, SMODE4 (RLU) (10.00mF) | — | ENVS=1 SMODE=0x1a: bus CMPO 0/1: SVDD / SVSS, SMODE4 (RLU)<br>ENVS=1 SMODE=0x0f: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN (CC0, CC1, CC2, CC3)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (Charge ADC get)<br>ENVS=1 SMODE=0x0e: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN (disCharge) | ENVS=1 SMODE=0x1a: bus CMPO 0/1: SVDD / SVSS, SMODE4 (RLU) (CAP 50E-9)<br>ENVS=1 SMODE=0x06: bus SVDD+SVSO2+SCP (CAP 5E-3)<br>ENVS=1 SMODE=0x0f: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN (CAP 50E-3) | ENVS=1 SMODE=0x0e: bus CMPO 0/1: SVDD+SVSO2+SCP / SVSS+SVSO2+SCN |
| OP3 VREF (SFUVR) | SFUVR=2: VDS15 / VDS3 (CMPO 0/1) (CAP-1, CAP-4)<br>SFUVR=1: VDS16 (CAP-2, CAP-3)<br>SFUVR=7: AGNDP0 / PB7 (CMPO 0/1) (CAP-0)<br>APD p25 VDS16 / VDS2; p26–27 VDS16; p29 VDS3 | SFUVR=7: AGNDP0 / PB7 (CMPO 0/1)<br>SFUVR=2: VDS15 / VDS3 (CMPO 0/1) (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp), discharge (row 8)) | SFUVR=2: VDS15 / VDS3 (CMPO 0/1) | — | SFUVR=2: VDS15 / VDS3 (CMPO 0/1)<br>SFUVR=1: VDS16 / VDS2 (CMPO 0/1) (CV0, CV1, CV2, CV3)<br>SFUVR=7: AGNDP0 / PB7 (CMPO 0/1) (60nF~600nF ->0, 600nF~60uF ->1, 60uF~600uF ->2)<br>SFUVR=1: VDS16 (Charge ADC get) | SFUVR=7: AGNDP0 / PB7 (CMPO 0/1) (CAP 50E-9)<br>SFUVR=1: VDS16 (CAP 5E-3)<br>SFUVR=10: VDS15 / VDS3 (CMPO 0/1) (CAP 50E-3) | SFUVR=2: VDS15 / VDS3 (CMPO 0/1) |
| SDIO (PB0–PB8) | 0 | 0 | 0 | — | 0 | 0 | 0 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V<br>APD p28 VAGND 01 (0.3) | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=0 (bandgap) AGND=0.5 VDDA ACM=1.2 V | — | ENREFO=1 SREFO=0 (bandgap) AGND=0.5 VDDA ACM=1.2 V<br>ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V (60nF~600nF ->0, 600nF~60uF ->1, 60uF~600uF ->2, Charge ADC get, disCharge) | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=0 (bandgap) AGND=0.3 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | — | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off | off | off | — | off | off | off |
| R26 AD2: OSR2, clock, chop | off | off | off | — | off | off | off |
| R29 LPF / RMS / peak | off | off | off | — | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | off (0x00)<br>not written (CAP-1) | off (0x00) | not written | — | not written | off (0x00) | not written |
| SPHACAL (R36) | 0x00<br>not written (CAP-1) | not written | not written | — | not written | 0x00 | not written |
| Comparator (R20, R21) | on, SCMPI=PB0; H=AGNDP1 L=AGNDP0<br>on, SCMPI=PB0; H=VDSC11 L=VDSC5 (CAP-0)<br>on, SCMPI=PB0; H=VDSC9 L=VDSC7 (CAP-1)<br>APD p24–25 PB0, H=VDSC11 L=VDSC5; p29 PB0, AGNDP0 / AGNDP0; p26–27 off | on, SCMPI=PB0; H=VDSC11 L=VDSC5<br>on, SCMPI=OP1O; H=AGNDP3 L=AGNDN3 (r5 600 uF (ramp), r6 6 mF (ramp), r7 60 mF (ramp))<br>on, SCMPI=PB0; H=VDSC11 L=VDSC7 (r4 60 uF (RC))<br>on, SCMPI=OP1O; H=AGNDP1 L=AGNDN1 (discharge (row 8)) | on, SCMPI=PB0; H=VDSC11 L=VDSC5<br>on, SCMPI=OP1O; H=VDSC11 L=VDSC5 (10.00mF) | — | on, SCMPI=PB0; H=VDSC13 L=VDSC5 (CV0, CV1, CV2, CV3)<br>on, SCMPI=PB0; H=VDSC13 L=VDSC11 (CC0, CC1, CC2, CC3)<br>on, SCMPI=PB0; H=VDSC11 L=VDSC5 (60nF~600nF ->0, 600nF~60uF ->1, 60uF~600uF ->2)<br>off (Charge ADC get)<br>on, SCMPI=PB0; H=AGNDP1 L=AGNDP0 (disCharge) | on, SCMPI=PB0; H=VDSC11 L=VDSC5 (CAP 50E-9)<br>off (CAP 5E-3)<br>on, SCMPI=PB0; H=AGNDP1 L=AGNDP0 (CAP 50E-3) | on, SCMPI=OP1O; H=VDSC11 L=VDSC7 |
| Counter, CNT, CMP pin | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=1<br>ENCTR=1 ENCNTI=0 ENPCMPO=0 (10.00mF) | — | ENCTR=1 ENCNTI=0 ENPCMPO=1<br>ENCTR=1 ENCNTI=0 ENPCMPO=0 (60nF~600nF ->0, 600nF~60uF ->1, 60uF~600uF ->2)<br>ENCTR=0 ENCNTI=0 ENPCMPO=0 (Charge ADC get, disCharge) | ENCTR=0 ENCNTI=0 ENPCMPO=0<br>ENCTR=1 ENCNTI=0 ENPCMPO=0 (CAP 50E-9) | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40: RC relaxation on r0..r4 (6 nF .. 60 µF), AD1 ramp on r5..r7 (600 µF ..
  60 mF) with a discharge row (PA4..PA7 on the bus) before each ramp. 121GW and
  Keysight use the constant-current method for the small ranges, OWON and
  Hycon CAP-0 the RC method.
- The AD1 ramp rows: DM40 AD1 on OP2O with reference PB6, IG 0.9, RG 1,
  chopped; Hycon / Temco / OWON read PB0 − PB3 against VDDA − VSSA with RG 1/3,
  unchopped (Hycon and Temco with DCOS −0.25).
- OWON `CAP 50E-9` AD1 fields are left from the previous mode (the RC rows do
  not call the AD1 composer); 121GW and Keysight use the band-gap REFO
  (SREFO = 0) in these rows.

## Frequency / counter

The counter measures either ACPO (the comparator on an analog node) or the CNT
pin (ENCNTI = 1). Gate time = (0x1000000 − CTA preset) / SYSCLK; f = CTB / T,
duty = CTC / gate counts. The V and A rows also run the counter for the
secondary frequency readout (ENCTR = 1 in their AC rows).

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | AD1 off | TS1N - TS2P (SAD1I=3; filter mux PB5 - PB5) | AD1 off | — | AD1 off | previous-mode leftover | OP1O - RLU |
| AD1 reference (+ / −) | AD1 off | REFO - AGND | AD1 off | — | AD1 off | previous-mode leftover | PB6 - PB3 |
| AD1IG | AD1 off | 0.9 | AD1 off | — | AD1 off | previous-mode leftover | 1.8 |
| AD1RG | AD1 off | 1 | AD1 off | — | AD1 off | previous-mode leftover | 1 |
| R22: OSR1, AD1CHOP, DCOS (period) | AD1 off | OSR 8192, CHOP 10, DCOS 0 (100.0 ms) | AD1 off | — | AD1 off | previous-mode leftover | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | AD1 off | PN | AD1 off | — | AD1 off | previous-mode leftover | none |
| SFT1 pre-filter R | 100k | open | open | — | 100k | open | open |
| OP1 (+ input, OPS1, chop) | off (Freq-0)<br>on +SENSE OPS1=1 chop off (Freq-1)<br>on +SENSE OPS1=0 chop off (Freq-2)<br>APD p51 +SENSE x1; p53 +PB1 x1 | on +RLU OPS1=0 chop off | on +SENSE OPS1=1 chop off | — | on +SENSE OPS1=0 chop off<br>off (CNT) | off | on +PB1 OPS1=1 chop off |
| OP2 (+ input, OPS2) | off | off | off | — | off | off | off |
| PA switches | none (Freq-0)<br>F9 P8 PS7 F0 (Freq-1)<br>P9 P8 PDS7 F0 (Freq-2) | P8 | F9 P8 PS7 F1 F0 | — | P7 PS3<br>none (CNT) | none | S7 P5 PDS3 |
| R2F ENVS, SMODE | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array)<br>ENVS=1 SMODE=0x00: bus none (Freq-0) | ENVS=1 SMODE=0x11: bus SGND, SMODE4 (RLU) | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array) | — | ENVS=1 SMODE=0x20: bus none, SMODE5 (ACC array)<br>ENVS=1 SMODE=0x00: bus none (CNT) | ENVS=1 SMODE=0x00: bus none | ENVS=1 SMODE=0x00: bus none |
| OP3 VREF (SFUVR) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 | — | SFUVR=7: AGNDP0<br>SFUVR=2: VDS15 (CNT) | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 |
| SDIO (PB0–PB8) | 0 | 1 | 0 | — | 0 | 0 | 1 |
| REFO, AGND, ACM | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=0 (bandgap) AGND=0.5 VDDA ACM=1.2 V | — | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.5 V |
| ACC (R30) | 0 (0.0 pF) | 0 (0.0 pF) | 0 (0.0 pF) | — | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | off<br>OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 (Freq-2) | OP1O - AGND; ref REFO - AGND; IG 0.5, RG 1 | off | — | off<br>OP1O - PB3; ref REFO - AGND; IG 0.5, RG 1 (ACPO-6V) | off | off |
| R26 AD2: OSR2, clock, chop | off<br>OSR2 32, CLK SYSCLK/2, CHOP 0 (Freq-2) | OSR2 32, CLK SYSCLK/2, CHOP 0 | off | — | off<br>OSR2 32, CLK SYSCLK/2, CHOP 0 (ACPO-6V) | off | off |
| R29 LPF / RMS / peak | off<br>ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 (Freq-2) | ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 | off | — | off<br>ENRMS=1 ENLPF=1 OSR4 16384 ENPKH=0 PKHSEL=AD2 (ACPO-6V) | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | not written | off (0x00) | not written | — | not written | off (0x00) | not written |
| SPHACAL (R36) | not written | not written | not written | — | not written | 0x00 | not written |
| Comparator (R20, R21) | off (Freq-0)<br>on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 (Freq-1)<br>on, SCMPI=OP1O; H=AGNDP2 L=AGNDP1 (Freq-2)<br>APD p51, p53 PB4 (OP1O → 1 µF → 1 M → PB4); H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | — | off (CNT)<br>on, SCMPI=OP1O; H=AGNDP2 L=AGNDP1 (ACPO-6V)<br>on, SCMPI=PB4; H=AGNDP2 L=AGNDP1 (ACPO) | off | on, SCMPI=PB0; H=AGNDP3 L=AGNDP1 |
| Counter, CNT, CMP pin | ENCTR=1 ENCNTI=0 ENPCMPO=0<br>ENCTR=1 ENCNTI=1 ENPCMPO=0 (Freq-0)<br>APD p51 ACPO → counter (ENCTR); p54 CNT pin (ENCNTI) | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=1 ENPCMPO=1 | — | ENCTR=1 ENCNTI=1 ENPCMPO=0 (CNT)<br>ENCTR=1 ENCNTI=0 ENPCMPO=0 (ACPO-6V)<br>ENCTR=1 ENCNTI=0 ENPCMPO=1 (ACPO) | ENCTR=1 ENCNTI=1 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 |
| Clock | ENOSC=1 ENXI=1<br>ENOSC=1 ENXI=0 (Freq-2) | ENOSC=1 ENXI=0 | ENOSC=1 ENXI=0 | — | ENOSC=1 ENXI=0<br>ENOSC=1 ENXI=1 (ACPO) | ENOSC=1 ENXI=0 | ENOSC=1 ENXI=0 |

- DM40 compares PB4 (the AC V node) against AGNDP1 / AGNDN1 (±20.8 mV) and
  keeps AD2 + RMS running for the level; at run time it alternates the source
  between the comparator and the CNT pin and reports the steadier one.
  Its AD1 is enabled on the die temperature sensor (SAD1I = 3) in this row;
  what the stock reads from it is unknown.
- Hycon has three variants: CNT pin, DC V path (comparator on PB4) and AC V
  path (comparator on OP1O); Temco has the same three. 121GW and OWON use the
  CNT pin (ENCNTI = 1). Keysight's mode 0x0B uses the DC V row 1 (*inferred*).

## Temperature

A thermocouple is a DC mV measurement plus a cold-junction temperature. The
HY3131 has a die sensor read by AD1 with SAD1I = 10 (TS1P / TS2N) and 11
(TS1N / TS2P); the difference of the two codes cancels the sensor offset
(datasheet §12.4). An RTD is an ohm measurement.

| field | Hycon ENOB tool rows | DM40 stock | 121GW | Digilent | Temco | OWON XDM2041 | Keysight U128xA (mode mapping inferred) |
|---|---|---|---|---|---|---|---|
| AD1 input (+ / −) | — | TS1N - TS2P (SAD1I=3; filter mux OP1O - AGND) | OP1O - AGND | — | TS1N - TS2P (SAD1I=3; filter mux OP1O - PB3) (row 0 (SAD1I=3))<br>TS1P - TS2N (SAD1I=2; filter mux OP1O - PB3) (row 1 (SAD1I=2)) | PB0 - PB3 | OP1O - AGND |
| AD1 reference (+ / −) | — | PB6 - AGND | PB6 - AGND | — | PB6 - AGND | PB6 - AGND | PB6 - PB3 |
| AD1IG | — | 0.9 | 1.8 | — | 0.9 | 1.8 | 0.9 |
| AD1RG | — | 1 | 1 | — | 1 | 1 | 1 |
| R22: OSR1, AD1CHOP, DCOS (period) | — | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 4096, CHOP 11, DCOS 0 (50.0 ms) | — | OSR 16384, CHOP 10, DCOS 0 (200.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) | OSR 4096, CHOP 10, DCOS 0 (50.0 ms) |
| AD1 buffers (H L P N) | — | PN | HPN | — | HP | HPN | none |
| SFT1 pre-filter R | — | open | 100k | — | 100k | 0 | open |
| OP1 (+ input, OPS1, chop) | — | on +PB0 OPS1=1 chop 2 kHz (row 0)<br>on +PB0 OPS1=0 chop 2 kHz (row 1) | on +PB0 OPS1=1 chop 1 kHz | — | on +PB0 OPS1=0 chop off | off | on +PB1 OPS1=0 chop off |
| OP2 (+ input, OPS2) | — | off | off | — | off | off | off |
| PA switches | — | P8 DF7 DF2 (row 0)<br>P8 DF6 (row 1) | P8 | — | P8 | P8 | none |
| R2F ENVS, SMODE | — | ENVS=1 SMODE=0x15: bus SGND+SVSO1, SMODE4 (RLU) | ENVS=1 SMODE=0x01: bus SGND | — | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x01: bus SGND | ENVS=1 SMODE=0x21: bus SGND, SMODE5 (ACC array) |
| OP3 VREF (SFUVR) | — | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 | — | SFUVR=7: AGNDP0 | SFUVR=7: AGNDP0 | SFUVR=0: VDS17 |
| SDIO (PB0–PB8) | — | 0 | 1 | — | 0 | 0 | 1 |
| REFO, AGND, ACM | — | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | — | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.5 VDDA ACM=1.2 V | ENREFO=1 SREFO=1 (PB6) AGND=0.3 VDDA ACM=1.5 V |
| ACC (R30) | — | 0 (0.0 pF) | 0 (0.0 pF) | — | 0 (0.0 pF) | 0 (0.0 pF) | 64 (12.8 pF) |
| AD2 in / ref / gain | — | off | off | — | off | off | off |
| R26 AD2: OSR2, clock, chop | — | off | off | — | off | off | off |
| R29 LPF / RMS / peak | — | off | off | — | off | inert, AD2 off (R29=0xf8) | off |
| AD3, SVXI, SDO23 (R34, R35) | — | off (0x00) | not written | — | not written | off (0x00) | not written |
| SPHACAL (R36) | — | not written | not written | — | not written | 0x00 | not written |
| Comparator (R20, R21) | — | on, SCMPI=PB4; H=AGNDP1 L=AGNDN1 | off | — | off | off | on, SCMPI=PB0; H=AGNDP3 L=AGNDP1 |
| Counter, CNT, CMP pin | — | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=0 | — | ENCTR=0 ENCNTI=0 ENPCMPO=0 | ENCTR=1 ENCNTI=0 ENPCMPO=0 | ENCTR=0 ENCNTI=0 ENPCMPO=1 |

- DM40 (K type): three AD1 conversions per result at OSR 16384, R33 SAD1I = 11,
  10 and 00 in turn; the thermocouple is read through OP1 x10 on PB0 (row 0).
  Row 1 differs in the PA switches and OPS1; whether the stock selects it is
  unknown.
- OWON runs the thermocouple as DC V 500 mV (PB0 − PB3, OSR 4096), reads the
  die sensor only at boot (R33 0xAA / 0xAB, SAD1I 2 / 3), and runs the RTD as
  ohm 500. 121GW uses its DC mV 50 mV row. Temco's two rows are the die sensor
  only. Keysight row 6: OP1 on PB1, OP1O − AGND (*inferred* mapping).
- APD-DMM003 has no temperature chapter; the ENOB tool has no temperature row.

## Other rows

- Hycon ENOB `Sel_Cal` 0..9: OP3 drives one divider resistor (DF) and the
  neighbouring tap is on the bus and SENSE (PS), AD1 reads RLU − SENSE: a
  divider-ratio check of adjacent PA resistors (*inferred* from the decode).
- Keysight row 12 (flag 0xFE24.2): AD2 and AD1 on, OP1 on PB2; use unknown.

## Runtime behaviour

| | DM40 stock V2.6 | 121GW | Digilent | Temco | OWON XDM2041 | Keysight |
|---|---|---|---|---|---|---|
| mode change | R26, R34, R36 = 0; relays; R20..R35 (22 bytes); INTE = 0; poll INTF | 20 bytes from R20; INTE per mode (AD1IE; + CTIE in AC; RMSIE + CTIE in 1 ms peak; CTIE in cap / Hz) | R37 = 0x60 (reset) every scale change, then INTE + R20..R36 (24 bytes), read back and compared with a mask | 20 bytes from R20, then INTF = 0x02 and INTE (ESP32 driver) | R20 ENCMP off at once, then all 56 bytes R00..R37 in one burst; 74HC595 relay byte before every reading | buffer of 20 bytes from the table, per-range overlays, 22 bytes sent (R20..R34; R34 unset) |
| zero / offset | none in software; AD1CHOP = 10 and OP1 chopping | unknown | none (software averaging) | `Get_Volt6V_Zero`: the "6V ZERO" row (OP1+ on RLU) averaged 50 times at DC V entry, subtracted | none; AD1CHOP / OP1CHOP; calibration in software | unknown |
| speed / OSR | R22 0x15 (8192) while the reading moves, 0x17 (32768) when steady (DC V 0x206 codes, DC mV 3618 / 1218, DC A 0x168); ohm 0x13 on overload, 0x17 in range; continuity 0x14 / 0x17 | unknown | fixed R22 0x17 (OSR 32768) | unknown | speed key: R22 OSR {4096, 2048, 2048} by a single R22 write | a flag sets R22 = 0x11 (OSR 512) and changes LPFBW |
| autorange first conversion | settle deadlines after each range write (ohm 150 ms, DC V 250 ms, AC V 450 ms, …) | unknown | — | unknown | after a range change OSR 1024 (Ω, DC A) / LPFBW 5 (AC A) for the first conversion | unknown |
| AC trim (ACC) | none, R30 = 0x80 | table values | 127 fixed | table values | per-unit calibration byte into R30 on 5 V / 50 V | calibrated value on mode 1 range 0 |
| AC+DC | alternates the AC and DC rows of the same quantity (~3 readings each) | unknown | — (no such mode) | unknown | — (no such mode in the captures) | unknown |
| capacitance | counter restart per reading (R20 0x50, CTA 0xFC0000, R20 0x52); r5..r7: discharge row until \|V\| < 30 mV, then the ramp row; R21 rewritten to 0xDC / 0xCC / 0xBB by slope class; comparator state (R14 bit 6) splits batches | unknown | — | counter preset CTA by frequency class; R33 0xA0 / 0xA8 switches SFT1 100 k / 0 for counting | counter restart R20 0x70 / CTA 0xBF0000 / R20 0x72; 5 / 50 mF: open R2A / R2C, read AD1, alternate two bulk rows (phases A / B) | — |
| temperature | R33 0xAF → 0xAE → 0xAC: die sensor 11, 10, then the thermocouple; 700 / 800 / 700 ms | DC mV 50 mV row | — | rows with SAD1I 3 / 2 | die sensor at boot (R33 0xAA, 0xAB, ~2 s each) | unknown |
| other | diode beeper by R20 0xB4 / 0xB0 (ENPCMPO); frequency source alternates ENCMP (0xF0) / CNT (0x08) | software AC frequency-response correction tables; 50 MΩ offset correction | — | — | ENCTR set in most rows; Phoenix clears it and manages the counter itself, hard-codes ACC E9 / FF, and has a 50 Ω range and its own 500 µF..10 mF rows | idle set: R26 0x19, R33 0x0C (*inferred*) |

Sources: DM40 — stock V2.6 disassembly, mirrored in [`src/dmm.rs`](../src/dmm.rs);
121GW — the comments of `hy_regtbl.h` and the routines in `121gw-re/routines`;
Digilent — `dmm.cpp` `DMM_SetScale`; Temco — `DMM_DCV.c`, `main/hy3131_sw.c`
(not built in the STM32 project); OWON — captures and the stock image;
Keysight — the decoded image.

## Not determined

- Board wiring of PB / PA pins for the 121GW, Digilent, Temco, OWON and
  Keysight beyond what the row decode shows.
- Keysight: the mode-to-function mapping and R34..R36.
- Temco: which rows a built firmware writes.
- 121GW: run-time R22 / R29 changes, and the circuit on PB7 that its normal AC
  rows read with AD1.

## Appendix: raw rows

Every row in hex from R20 upwards (22 bytes R20..R35 for the DM40, 20 bytes
R20..R33 for the 121GW, Temco and Keysight, R20..R36 for Digilent and OWON, as
present in the file for the Hycon tool), with the mode key used above and the
source location. 121GW rows give the v1.02 file offset of the first match and
the release images that contain the row. Keysight "composed" rows are base row
+ overlay as read from the decoded code.

### Hycon ENOB tool (HY313x ENOB V1.4, Config/*/HYCONFIG-n.txt)

| mode | label | R20 … | origin |
|---|---|---|---|
| acv | ACV/HYCONFIG-0 | `F2 DD 04 03 52 10 80 25 31 F8 22 00 00 D0 88 A0 99 C7 38 20 02 50 0C` | ENOB V1.4 Config/ACV/HYCONFIG-0.txt |
| acv | ACV/HYCONFIG-1 | `F2 DD 04 03 52 10 80 25 31 F8 22 00 00 0D 28 A0 B0 C7 38 20` | ENOB V1.4 Config/ACV/HYCONFIG-1.txt |
| acv | ACV/HYCONFIG-2 | `F2 DD 04 03 52 10 80 25 31 F8 22 00 90 00 28 A0 80 C7 38 20` | ENOB V1.4 Config/ACV/HYCONFIG-2.txt |
| acv | ACV/HYCONFIG-3 | `F2 DD 04 03 52 10 80 25 31 F8 22 00 09 00 28 A0 80 C7 38 20 02 50 0C` | ENOB V1.4 Config/ACV/HYCONFIG-3.txt |
| cap | CAP/HYCONFIG-0 | `72 21 14 0B 52 11 00 55 31 00 00 00 00 08 00 9A 80 C7 33 20 00 00 00` | ENOB V1.4 Config/CAP/HYCONFIG-0.txt |
| cap | CAP/HYCONFIG-1 | `72 42 14 0B 52 11 00 55 31 00 00 00 80 00 00 8E 80 C2 33 20` | ENOB V1.4 Config/CAP/HYCONFIG-1.txt |
| cap | CAP/HYCONFIG-2 | `72 DE A4 93 85 11 00 55 55 00 00 00 00 08 00 86 80 C1 33 2C 00 00 00` | ENOB V1.4 Config/CAP/HYCONFIG-2.txt |
| cap | CAP/HYCONFIG-3 | `72 DE A4 93 85 11 00 55 55 00 80 00 00 00 00 86 80 C1 33 2C 00 00 00` | ENOB V1.4 Config/CAP/HYCONFIG-3.txt |
| cap | CAP/HYCONFIG-4 | `72 DE A4 93 85 11 00 55 55 00 80 00 08 00 00 8E 80 C2 3C 2C 00 00 00` | ENOB V1.4 Config/CAP/HYCONFIG-4.txt |
| dca | Current/HYCONFIG-0 | `00 00 14 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20` | ENOB V1.4 Config/Current/HYCONFIG-0.txt |
| dca | Current/HYCONFIG-1 | `00 00 14 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | ENOB V1.4 Config/Current/HYCONFIG-1.txt |
| dca | Current/HYCONFIG-2 | `00 00 16 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20` | ENOB V1.4 Config/Current/HYCONFIG-2.txt |
| dca | Current/HYCONFIG-3 | `00 00 16 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | ENOB V1.4 Config/Current/HYCONFIG-3.txt |
| aca | Current/HYCONFIG-4 | `92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28` | ENOB V1.4 Config/Current/HYCONFIG-4.txt |
| aca | Current/HYCONFIG-5 | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28` | ENOB V1.4 Config/Current/HYCONFIG-5.txt |
| aca | Current/HYCONFIG-6 | `92 DD 07 03 52 10 80 25 11 E0 00 00 00 00 00 90 80 C7 3D 28` | ENOB V1.4 Config/Current/HYCONFIG-6.txt |
| dcv | DCV/HYCONFIG-0 | `60 00 14 8B 01 11 00 55 31 00 22 00 00 90 28 A0 80 C7 33 20` | ENOB V1.4 Config/DCV/HYCONFIG-0.txt |
| dcv | DCV/HYCONFIG-1 | `60 00 14 8B 01 11 00 55 31 00 22 00 00 09 28 A0 80 C7 33 20` | ENOB V1.4 Config/DCV/HYCONFIG-1.txt |
| dcv | DCV/HYCONFIG-2 | `60 00 14 8B 01 11 00 55 31 00 22 00 90 00 28 A0 80 C7 33 20` | ENOB V1.4 Config/DCV/HYCONFIG-2.txt |
| dcv | DCV/HYCONFIG-3 | `60 00 14 8B 01 11 00 55 31 00 22 00 09 00 28 A0 80 C7 33 20` | ENOB V1.4 Config/DCV/HYCONFIG-3.txt |
| diode | DiodeBeeperHz/HYCONFIG-0 | `C0 CF 16 8B 8D 10 00 55 31 00 00 00 08 00 40 86 80 E2 33 A0` | ENOB V1.4 Config/DiodeBeeperHz/HYCONFIG-0.txt |
| cont | DiodeBeeperHz/HYCONFIG-1 | `74 CF 16 83 35 10 00 55 00 00 00 00 0A 00 40 86 80 D2 3C A0` | ENOB V1.4 Config/DiodeBeeperHz/HYCONFIG-1.txt |
| cont | DiodeBeeperHz/HYCONFIG-2 | `7C CF 16 83 35 10 00 55 00 00 00 00 0A 00 40 86 80 E2 3C A0` | ENOB V1.4 Config/DiodeBeeperHz/HYCONFIG-2.txt |
| freq | Freq/HYCONFIG-0 | `6A 00 00 00 00 01 00 00 00 00 00 00 00 00 00 80 80 C7 33 30` | ENOB V1.4 Config/Freq/HYCONFIG-0.txt |
| freq | Freq/HYCONFIG-1 | `F2 DD 00 0B 53 11 00 55 31 00 02 00 00 90 28 A0 80 C7 38 30` | ENOB V1.4 Config/Freq/HYCONFIG-1.txt |
| freq | Freq/HYCONFIG-2 | `52 CF 04 03 52 10 80 25 31 F8 02 00 00 D0 88 A0 80 C7 38 20` | ENOB V1.4 Config/Freq/HYCONFIG-2.txt |
| ohm | OHM/HYCONFIG-0 | `C0 CF 16 83 35 01 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0` | ENOB V1.4 Config/OHM/HYCONFIG-0.txt |
| ohm | OHM/HYCONFIG-1 | `C0 CF 16 83 35 00 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0` | ENOB V1.4 Config/OHM/HYCONFIG-1.txt |
| ohm | OHM/HYCONFIG-2 | `C0 CF 16 83 85 00 00 55 00 00 00 40 60 00 44 94 80 D3 33 20` | ENOB V1.4 Config/OHM/HYCONFIG-2.txt |
| ohm | OHM/HYCONFIG-3 | `C0 CF 16 83 85 00 00 55 00 00 00 40 00 06 44 94 80 D3 33 20` | ENOB V1.4 Config/OHM/HYCONFIG-3.txt |
| ohm | OHM/HYCONFIG-4 | `C0 CF 16 93 85 00 00 55 55 00 00 08 00 08 80 86 80 D1 33 20` | ENOB V1.4 Config/OHM/HYCONFIG-4.txt |
| ohm | OHM/HYCONFIG-5 | `C0 CF 16 93 85 00 00 55 55 00 00 08 00 80 80 86 80 D1 3C A0` | ENOB V1.4 Config/OHM/HYCONFIG-5.txt |
| ohm | OHM/HYCONFIG-6 | `C0 CF 16 93 85 00 00 55 55 00 00 08 00 00 80 86 80 D1 3C A0` | ENOB V1.4 Config/OHM/HYCONFIG-6.txt |
| ohm | OHM/HYCONFIG-7 | `70 CF 16 83 85 00 00 55 01 00 00 40 00 00 46 94 80 D3 33 A0 00 00 00` | ENOB V1.4 Config/OHM/HYCONFIG-7.txt |
| ohm | OHM/HYCONFIG-8 | `70 CF 16 83 85 00 00 55 01 00 00 40 00 00 46 94 80 D3 33 A1 00 00 00` | ENOB V1.4 Config/OHM/HYCONFIG-8.txt |
| other | Sel_Cal/HYCONFIG-0 | `C0 CF 17 8B 20 01 00 55 31 00 00 00 00 90 06 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-0.txt |
| other | Sel_Cal/HYCONFIG-1 | `C0 CF 17 8B 80 01 00 55 31 00 00 00 00 60 09 95 80 C3 33 20` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-1.txt |
| other | Sel_Cal/HYCONFIG-2 | `70 CF 17 8B 20 01 00 55 31 00 00 00 00 69 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-2.txt |
| other | Sel_Cal/HYCONFIG-3 | `70 CF 17 8B 20 01 00 55 31 00 00 00 00 96 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-3.txt |
| other | Sel_Cal/HYCONFIG-4 | `70 CF 17 8B 20 01 00 55 31 00 00 00 90 06 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-4.txt |
| other | Sel_Cal/HYCONFIG-5 | `70 CF 17 8B 20 01 00 55 31 00 00 00 60 09 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-5.txt |
| other | Sel_Cal/HYCONFIG-6 | `70 CF 17 8B 20 01 00 55 31 00 00 00 69 00 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-6.txt |
| other | Sel_Cal/HYCONFIG-7 | `70 CF 17 8B 20 01 00 55 31 00 00 00 96 00 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-7.txt |
| other | Sel_Cal/HYCONFIG-8 | `70 CF 17 8B 20 01 00 55 31 00 00 06 00 90 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-8.txt |
| other | Sel_Cal/HYCONFIG-9 | `70 CF 17 8B 20 01 00 55 31 00 00 09 00 60 00 95 80 C3 33 20 00 00 00` | ENOB V1.4 Config/Sel_Cal/HYCONFIG-9.txt |
| dcmv | mV/HYCONFIG-0 | `C0 00 14 8B 85 11 00 55 31 00 00 00 00 00 00 90 80 C7 33 28` | ENOB V1.4 Config/mV/HYCONFIG-0.txt |
| dcmv | mV/HYCONFIG-1 | `00 00 16 8B 35 11 00 55 31 00 00 00 00 00 00 90 80 C7 3C 60` | ENOB V1.4 Config/mV/HYCONFIG-1.txt |
| acmv | mV/HYCONFIG-2 | `92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28` | ENOB V1.4 Config/mV/HYCONFIG-2.txt |
| acmv | mV/HYCONFIG-3 | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28` | ENOB V1.4 Config/mV/HYCONFIG-3.txt |

### Alientek DM40 stock V2.6

| mode | label | R20 … | origin |
|---|---|---|---|
| acv | 6 V | `F2 DD 16 03 33 43 80 9D 31 FC 20 00 00 09 28 A0 80 C7 38 AC 88 94` | firmware/src/hy3131_tables.rs:8 (CFG_F0[0]) |
| acv | 60 V | `F2 DD 16 03 33 40 80 9D 31 FC 20 00 00 09 28 A0 80 C7 38 AC 88 94` | firmware/src/hy3131_tables.rs:9 (CFG_F0[1]) |
| acv | 600 V | `F2 DD 16 03 33 40 80 9D 31 FC 20 00 90 00 28 A0 80 C7 38 AC 88 94` | firmware/src/hy3131_tables.rs:10 (CFG_F0[2]) |
| acv | 1000 V | `F2 DD 16 03 33 40 80 9D 31 FC 20 00 99 00 28 A0 80 C7 38 AC 88 94` | firmware/src/hy3131_tables.rs:11 (CFG_F0[3]) |
| dcv | 6 V | `F0 DD 15 83 31 40 00 55 31 00 06 00 00 90 28 A0 80 D7 38 AC 00 54` | firmware/src/hy3131_tables.rs:16 (CFG_F1[0]) |
| dcv | 60 V | `F0 DD 15 83 07 40 00 55 31 00 06 00 00 09 68 A4 80 C7 33 AC 00 54` | firmware/src/hy3131_tables.rs:17 (CFG_F1[1]) |
| dcv | 600 V | `F0 DD 15 83 07 40 00 55 31 00 06 00 90 00 68 A4 80 C7 33 AC 00 54` | firmware/src/hy3131_tables.rs:18 (CFG_F1[2]) |
| dcv | 1000 V | `F0 DD 15 83 07 40 00 55 31 00 06 00 99 00 68 A4 80 C7 33 AC 00 54` | firmware/src/hy3131_tables.rs:19 (CFG_F1[3]) |
| acmv | 60 mV | `F2 DD 16 03 33 43 80 5D 31 F8 00 00 00 00 08 91 80 C7 AA AC 88 54` | firmware/src/hy3131_tables.rs:24 (CFG_F2[0]) |
| acmv | 600 mV | `F2 DD 16 03 33 40 80 5D 31 F8 00 00 00 00 08 91 80 C7 AA AC 88 54` | firmware/src/hy3131_tables.rs:25 (CFG_F2[1]) |
| dcmv | 60 mV | `F0 DD 15 83 33 01 00 55 31 00 00 00 00 00 08 91 80 D7 3A AC 00 50` | firmware/src/hy3131_tables.rs:30 (CFG_F3[0]) |
| dcmv | 600 mV | `F0 DD 15 83 33 00 00 55 31 00 00 00 00 00 08 91 80 D7 3A AC 00 50` | firmware/src/hy3131_tables.rs:31 (CFG_F3[1]) |
| aca | A 6 A | `F2 DD 15 03 52 43 80 2D 31 F4 00 00 00 00 08 81 80 C7 3E AC 88 24` | firmware/src/hy3131_tables.rs:36 (CFG_F4[0]) |
| aca | A 10 A | `F2 DD 15 03 52 40 80 2D 31 F4 00 00 00 00 08 81 80 C7 3E AC 88 24` | firmware/src/hy3131_tables.rs:37 (CFG_F4[1]) |
| aca | mA/uA r0 (60 mA, 600 uA) | `F2 DD 14 03 35 43 80 2D 31 FC 00 00 00 00 08 81 80 C7 3D AC 88 24` | firmware/src/hy3131_tables.rs:42 (CFG_F5_6[0]) |
| aca | mA/uA r1 (600 mA, 6000 uA) | `F2 DD 14 03 35 40 80 2D 31 FC 00 00 00 00 08 81 80 C7 3D AC 88 24` | firmware/src/hy3131_tables.rs:43 (CFG_F5_6[1]) |
| dca | A 6 A | `F0 DD 15 80 35 01 00 55 31 00 00 00 00 00 08 81 80 C7 3E AC 00 50` | firmware/src/hy3131_tables.rs:48 (CFG_F7[0]) |
| dca | A 10 A | `F0 DD 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 C7 3E AC 00 50` | firmware/src/hy3131_tables.rs:49 (CFG_F7[1]) |
| dca | mA/uA r0 (60 mA, 600 uA) | `F0 DD 15 83 35 01 00 55 31 00 00 00 00 00 08 81 80 C7 3D A4 00 50` | firmware/src/hy3131_tables.rs:54 (CFG_F8_9[0]) |
| dca | mA/uA r1 (600 mA, 6000 uA) | `F0 DD 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 C7 3D AC 00 50` | firmware/src/hy3131_tables.rs:55 (CFG_F8_9[1]) |
| ohm | P8SET r0 60 ohm (never selected) | `E0 DD 14 83 35 01 00 55 00 00 00 46 06 00 00 94 80 D5 3C 68 00 50` | firmware/src/hy3131_tables.rs:60 (CFG_F10_P8SET[0]) |
| ohm | P8SET 600 ohm | `E0 DD 14 83 35 00 00 55 00 00 00 40 06 00 00 94 80 D5 3C 68 00 50` | firmware/src/hy3131_tables.rs:61 (CFG_F10_P8SET[1]) |
| ohm | P8SET 6 k | `E0 DD 14 83 35 00 00 55 00 00 00 40 60 00 00 94 80 D5 3C 68 00 50` | firmware/src/hy3131_tables.rs:62 (CFG_F10_P8SET[2]) |
| ohm | P8SET 60 k | `E0 DD 14 83 35 00 00 55 00 00 00 40 00 06 00 94 80 D5 3C 68 00 50` | firmware/src/hy3131_tables.rs:63 (CFG_F10_P8SET[3]) |
| ohm | P8SET 600 k | `E0 DD 14 8F 35 08 00 55 00 00 00 06 00 60 00 94 80 D5 3C 68 00 50` | firmware/src/hy3131_tables.rs:64 (CFG_F10_P8SET[4]) |
| ohm | P8SET 6 M | `E0 DD 14 8F 35 08 00 55 00 00 00 06 00 00 00 94 80 D4 3C 60 00 50` | firmware/src/hy3131_tables.rs:65 (CFG_F10_P8SET[5]) |
| ohm | P8SET 60 M | `E0 DD 14 83 35 00 00 55 11 00 00 08 00 00 00 86 80 D1 3C 60 00 50` | firmware/src/hy3131_tables.rs:66 (CFG_F10_P8SET[6]) |
| ohm | P8CLR r0 60 ohm (never selected) | `E0 DD 14 83 35 01 00 55 00 00 00 46 06 00 00 94 80 D2 3C 68 00 50` | firmware/src/hy3131_tables.rs:71 (CFG_F10_P8CLR[0]) |
| ohm | P8CLR 600 ohm | `E0 DD 14 83 35 00 00 55 00 00 00 40 06 00 00 94 80 D2 3C 68 00 50` | firmware/src/hy3131_tables.rs:72 (CFG_F10_P8CLR[1]) |
| ohm | P8CLR 6 k | `E0 DD 14 83 35 00 00 55 00 00 00 40 60 00 00 94 80 D2 3C 68 00 50` | firmware/src/hy3131_tables.rs:73 (CFG_F10_P8CLR[2]) |
| ohm | P8CLR 60 k | `E0 DD 14 83 35 00 00 55 00 00 00 40 00 06 00 94 80 D2 3C 68 00 50` | firmware/src/hy3131_tables.rs:74 (CFG_F10_P8CLR[3]) |
| ohm | P8CLR 600 k | `E0 DD 14 8F 35 08 00 55 00 00 00 06 00 60 00 94 80 D2 3C 68 00 50` | firmware/src/hy3131_tables.rs:75 (CFG_F10_P8CLR[4]) |
| ohm | P8CLR 6 M | `E0 DD 14 8F 35 08 00 55 00 00 00 06 00 00 00 94 80 D2 3C 60 00 50` | firmware/src/hy3131_tables.rs:76 (CFG_F10_P8CLR[5]) |
| ohm | P8CLR 60 M | `E0 DD 14 83 35 00 00 55 11 00 00 08 00 00 00 86 80 D1 3C 60 00 50` | firmware/src/hy3131_tables.rs:77 (CFG_F10_P8CLR[6]) |
| ohm | P8CLR row 7 (never selected) | `C0 CF 11 83 35 08 00 55 51 00 00 A8 00 00 80 86 80 D0 3C AC 00 50` | firmware/src/hy3131_tables.rs:78 (CFG_F10_P8CLR[7]) |
| cont | continuity | `56 EC 13 83 35 08 00 55 44 00 00 40 06 00 01 94 80 CD 38 68 00 50` | firmware/src/hy3131_tables.rs:83 (CFG_F11[0]) |
| diode | diode | `B0 FF 13 83 3D 00 00 55 31 00 80 00 88 00 00 86 80 E2 3C 68 00 50` | firmware/src/hy3131_tables.rs:88 (CFG_F12[0]) |
| cap | r0 6 nF (RC) | `72 21 16 03 B5 00 00 55 31 00 00 08 00 80 80 9A 80 C7 3C AC 00 50` | firmware/src/hy3131_tables.rs:93 (CFG_F13[0]) |
| cap | r1 60 nF (RC) | `72 21 16 03 B5 00 00 55 31 00 00 08 00 80 80 9A 80 C7 3C AC 00 50` | firmware/src/hy3131_tables.rs:94 (CFG_F13[1]) |
| cap | r2 600 nF (RC) | `72 21 16 03 B5 00 00 55 11 00 00 00 00 08 00 9A 80 C7 3C AC 00 50` | firmware/src/hy3131_tables.rs:95 (CFG_F13[2]) |
| cap | r3 6 uF (RC) | `72 21 16 03 B5 00 00 55 11 00 00 00 80 08 00 9A 80 C7 3C AC 00 50` | firmware/src/hy3131_tables.rs:96 (CFG_F13[3]) |
| cap | r4 60 uF (RC) | `72 22 16 03 B5 00 00 55 11 00 80 00 80 00 00 9A 80 C7 3C AC 00 50` | firmware/src/hy3131_tables.rs:97 (CFG_F13[4]) |
| cap | r5 600 uF (ramp) | `52 BB 14 83 43 40 00 5D 31 00 00 00 00 88 00 8E 80 C2 CC A8 00 54` | firmware/src/hy3131_tables.rs:98 (CFG_F13[5]) |
| cap | r6 6 mF (ramp) | `52 BB 14 83 43 40 00 5D 31 00 00 00 80 08 00 8E 80 C2 CC A8 00 54` | firmware/src/hy3131_tables.rs:99 (CFG_F13[6]) |
| freq | frequency | `F2 DD 15 83 DF 00 80 15 11 F8 00 00 00 00 08 91 80 C7 3A 2F 00 50` | firmware/src/hy3131_tables.rs:104 (CFG_F14[0]) |
| temp | row 0 (thermocouple, used) | `F0 DD 16 83 33 01 00 55 31 00 00 06 00 60 08 95 80 D7 3C AF 00 50` | firmware/src/hy3131_tables.rs:109 (CFG_F15[0]) |
| temp | row 1 (use unknown) | `F0 DD 16 83 33 00 00 55 31 00 00 00 00 06 08 95 80 D7 3C AF 00 50` | firmware/src/hy3131_tables.rs:110 (CFG_F15[1]) |
| cap | r7 60 mF (ramp) | `52 BB 14 83 43 40 00 5D 31 00 00 00 88 08 00 8E 80 C2 CC A8 00 54` | firmware/src/dmm.rs:158 (CFG_F13_R7; stock flash 0x08049ff2) |
| cap | discharge (row 8) | `52 DD 13 83 43 40 00 5D 31 00 00 00 88 88 00 8E 80 C2 CC A8 00 54` | firmware/src/dmm.rs:162 (CFG_F13_DISCHARGE; stock flash 0x0804a008) |

### EEVblog 121GW (121gw-re, v1.02 IDA tables)

| mode | label | R20 … | origin |
|---|---|---|---|
| dcv | DCV 5.0000V, DC power 5.0000V, Burden Voltage | `00 00 13 8A 05 40 00 4D 31 01 22 00 00 90 28 A0 80 C7 00 20` | 121gw-re/tables/hy_regtbl.h:16 (hy_regtbl_dcv); v1.02 file offset (first match) 0x1c17c; in all 9 images v1.00..v2.05 |
| dcv | DCV 50.000V, DC power 50.000V | `00 00 13 8A 05 40 00 4D 31 01 22 00 00 09 28 A0 80 C7 08 2C` | 121gw-re/tables/hy_regtbl.h:19 (hy_regtbl_dcv); v1.02 file offset (first match) 0x1c190; in all 9 images v1.00..v2.05 |
| dcv | DCV 500.00V | `00 00 13 8A 05 40 00 4D 31 01 22 00 90 00 28 A0 80 C7 08 2C` | 121gw-re/tables/hy_regtbl.h:22 (hy_regtbl_dcv); v1.02 file offset (first match) 0x1c1a4; in all 9 images v1.00..v2.05 |
| dcv | DCV 1000.0V (600.0V), LowZ in DC | `00 00 13 8A 05 40 00 4D 31 01 22 00 09 00 28 A0 80 C7 08 2C` | 121gw-re/tables/hy_regtbl.h:25 (hy_regtbl_dcv); v1.02 file offset (first match) 0x1c1b8; in all 9 images v1.00..v2.05 |
| dcmv | DCmV 50.000mV, Temperature | `60 00 1C 8B 3B 51 00 6D 31 01 00 00 00 00 08 81 80 C0 0C 60` | 121gw-re/tables/hy_regtbl.h:35 (hy_regtbl_dcmv); v1.02 file offset (first match) 0x1c648; in all 9 images v1.00..v2.05 |
| dcmv | DCmV 500.00mV | `60 00 10 8A 35 40 00 6D 31 01 00 00 00 00 08 81 80 C0 0C 2C` | 121gw-re/tables/hy_regtbl.h:38 (hy_regtbl_dcmv); v1.02 file offset (first match) 0x1c65c; in all 9 images v1.00..v2.05 |
| dca | DCA 500.00mA, DCmA 5.0000mA, DCuA 50.000uA / DCVA 500.00mA, DCmVA 5.0000mA, DCuVA 50.000uA | `00 00 12 9B 93 31 00 55 31 00 00 00 00 00 00 80 80 C7 33 2C` | 121gw-re/tables/hy_regtbl.h:47 (hy_regtbl_dc_amps); v1.02 file offset (first match) 0x1c4c0; in all 9 images v1.00..v2.05 |
| dca | DCA 5.0000A, DCmA 50.000mA, DCuA 500.00uA / DCmVA 50.000mA, DCuVA 500.00uA | `00 00 12 8B 93 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 2C` | 121gw-re/tables/hy_regtbl.h:51 (hy_regtbl_dc_amps); v1.02 file offset (first match) 0x1c4d4; in all 9 images v1.00..v2.05 |
| dca | DCA 10.000A, DCVA 10.000A | `00 00 12 8B 93 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 2C` | 121gw-re/tables/hy_regtbl.h:54 (hy_regtbl_dc_amps); v1.02 file offset (first match) 0x1c4d4; in all 9 images v1.00..v2.05 |
| acv | ACV 5.0000V, AC power 5.0000V, Burden Voltage | `F6 DD 02 83 F5 41 02 4D 31 01 00 00 00 09 88 B0 80 C0 08 2C` | 121gw-re/tables/hy_regtbl.h:65 (hy_regtbl_acv); v1.02 file offset (first match) 0x1c244; only in v1_00, v1_02, v1_04, v1_05, v1_07, v1_10 |
| acv | ACV 50.000V, AC power 50.000V | `F6 DD 02 83 F5 41 02 4D 31 01 00 00 90 00 88 B0 80 C0 38 2C` | 121gw-re/tables/hy_regtbl.h:68 (hy_regtbl_acv); v1.02 file offset (first match) 0x1c258; in all 9 images v1.00..v2.05 |
| acv | ACV 500.00V | `F6 DD 02 83 F5 41 02 4D 31 01 00 00 09 00 88 B0 80 C0 38 2C` | 121gw-re/tables/hy_regtbl.h:71 (hy_regtbl_acv); v1.02 file offset (first match) 0x1c26c; in all 9 images v1.00..v2.05 |
| acv | ACV 1000.0V (600.0V), LowZ in AC | `F6 DD 00 83 F5 41 02 4D 31 01 00 00 09 00 88 B0 80 C0 38 2C` | 121gw-re/tables/hy_regtbl.h:74 (hy_regtbl_acv); v1.02 file offset (first match) 0x1c280; in all 9 images v1.00..v2.05 |
| acv | ACV 5.0000V (1ms peak) [1 ms peak table] | `F6 DD 04 03 52 10 87 25 31 C4 00 00 00 90 88 A0 99 C7 38 20` | 121gw-re/tables/hy_regtbl.h:81 (hy_regtbl_acv_1ms_peak); v1.02 file offset (first match) 0x1c1e0; in all 9 images v1.00..v2.05 |
| acv | ACV 50.000V (1ms peak) [1 ms peak table] | `F6 DD 04 03 52 10 87 25 31 EC 00 00 00 09 88 A0 B0 C7 38 20` | 121gw-re/tables/hy_regtbl.h:84 (hy_regtbl_acv_1ms_peak); v1.02 file offset (first match) 0x1c1f4; in all 9 images v1.00..v2.05 |
| acv | ACV 500.00V (1ms peak) [1 ms peak table] | `F6 DD 04 03 52 10 87 25 31 EC 00 00 90 00 88 A0 80 C7 38 20` | 121gw-re/tables/hy_regtbl.h:87 (hy_regtbl_acv_1ms_peak); v1.02 file offset (first match) 0x1c208; in all 9 images v1.00..v2.05 |
| acv | ACV 1000.0V (600.0V) (1ms peak) [1 ms peak table] | `F6 DD 04 03 52 10 87 25 31 EC 00 00 09 00 88 A0 80 C7 38 20` | 121gw-re/tables/hy_regtbl.h:90 (hy_regtbl_acv_1ms_peak); v1.02 file offset (first match) 0x1c21c; in all 9 images v1.00..v2.05 |
| acmv | ACmV 50.000mV | `F6 DD 13 8A F5 43 06 6D 31 01 00 00 00 00 08 91 80 C0 3A 2C` | 121gw-re/tables/hy_regtbl.h:100 (hy_regtbl_acmv); v1.02 file offset (first match) 0x1c670; in all 9 images v1.00..v2.05 |
| acmv | ACmV 500.00mV | `F6 DD 13 8A F5 40 00 6D 31 01 00 00 00 00 08 91 80 C0 3A 2C` | 121gw-re/tables/hy_regtbl.h:103 (hy_regtbl_acmv); v1.02 file offset (first match) 0x1c684; in all 9 images v1.00..v2.05 |
| acmv | ACmV 50.000mV (1ms peak) [1 ms peak table] | `F6 DD 07 03 00 13 81 25 11 EC 00 00 00 00 00 90 80 C7 3A 28` | 121gw-re/tables/hy_regtbl.h:110 (hy_regtbl_acmv_1ms_peak); v1.02 file offset (first match) 0x1c698; in all 9 images v1.00..v2.05 |
| acmv | ACmV 500.00mV (1ms peak) [1 ms peak table] | `F6 DD 07 03 52 10 81 25 11 EC 00 00 00 00 00 90 80 C7 3A 28` | 121gw-re/tables/hy_regtbl.h:113 (hy_regtbl_acmv_1ms_peak); v1.02 file offset (first match) 0x1c6ac; in all 9 images v1.00..v2.05 |
| aca | ACA 500.00mA, ACmA 5.0000mA, ACuA 50.000uA / ACVA 500.00mA, ACmVA 5.0000mA, ACuVA 50.000uA | `F6 DD 12 8B F3 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D 2C` | 121gw-re/tables/hy_regtbl.h:122 (hy_regtbl_ac_amps); v1.02 file offset (first match) 0x1c484; in all 9 images v1.00..v2.05 |
| aca | ACA 5.0000A, ACmA 50.000mA, ACuA 500.00uA / ACmVA 50.000mA, ACuVA 500.00uA | `F6 DD 12 8B F3 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D 2C` | 121gw-re/tables/hy_regtbl.h:126 (hy_regtbl_ac_amps); v1.02 file offset (first match) 0x1c484; in all 9 images v1.00..v2.05 |
| aca | ACA 10.000A, ACVA 10.000A | `F6 DD 12 8B F3 10 00 55 31 00 00 00 00 00 00 80 80 C7 3D 2C` | 121gw-re/tables/hy_regtbl.h:129 (hy_regtbl_ac_amps); v1.02 file offset (first match) 0x1c4ac; in all 9 images v1.00..v2.05 |
| freq | freq | `FE DD 00 00 00 01 00 00 00 00 22 00 00 90 28 A0 00 C0 08 2C` | 121gw-re/tables/hy_regtbl.h:139 (hy_regtbl_freq); v1.02 file offset (first match) 0x1c6e8; in all 9 images v1.00..v2.05 |
| ohm | 50.000Ω | `74 CF 15 8F 85 00 00 00 00 00 00 40 06 00 04 94 80 D2 0C 2C` | 121gw-re/tables/hy_regtbl.h:146 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c010; only in v1_00, v1_02, v1_04 |
| ohm | 500.00Ω | `74 CF 15 8F 85 00 00 00 00 00 00 40 06 00 04 94 80 D2 0C 2C` | 121gw-re/tables/hy_regtbl.h:149 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c010; only in v1_00, v1_02, v1_04 |
| ohm | 5.0000kΩ | `00 00 13 83 85 00 00 00 00 00 00 40 60 00 00 94 80 D3 0C 2C` | 121gw-re/tables/hy_regtbl.h:152 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c038; only in v1_00, v1_02, v1_04 |
| ohm | 50.000kΩ | `00 00 14 83 85 00 00 00 00 00 00 40 00 06 00 94 80 D3 0C 2C` | 121gw-re/tables/hy_regtbl.h:155 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c04c; only in v1_00, v1_02, v1_04 |
| ohm | 500.00kΩ | `00 00 14 83 85 00 00 00 00 00 00 40 00 60 00 94 80 D2 0C 20` | 121gw-re/tables/hy_regtbl.h:158 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c060; only in v1_00, v1_02, v1_04 |
| ohm | 5.0000MΩ | `00 00 14 93 85 00 00 00 55 00 00 08 00 80 80 86 80 D1 00 AC` | 121gw-re/tables/hy_regtbl.h:161 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c074; only in v1_00, v1_02, v1_04 |
| ohm | 50.000MΩ | `00 00 15 93 85 00 00 00 55 00 08 08 00 00 00 86 80 D1 00 AC` | 121gw-re/tables/hy_regtbl.h:164 (hy_regtbl_ohms); v1.02 file offset (first match) 0x1c088; only in v1_00, v1_02, v1_04, v1_05 |
| cont | continuity | `74 CF 10 8F 85 00 00 00 00 00 00 40 06 00 04 94 80 D2 0C 2C` | 121gw-re/tables/hy_regtbl.h:170 (hy_regtbl_continuity); v1.02 file offset (first match) 0x1c85c; in all 9 images v1.00..v2.05 |
| diode | diode | `00 00 12 83 8D 00 00 00 11 00 00 00 08 00 40 92 80 E1 03 AC` | 121gw-re/tables/hy_regtbl.h:176 (hy_regtbl_diode); v1.02 file offset (first match) 0x1c6c0; in all 9 images v1.00..v2.05 |
| cap | 10.00nF | `76 21 07 00 00 00 00 00 00 00 00 08 00 80 00 8E 00 C2 0E 20` | 121gw-re/tables/hy_regtbl.h:183 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c09c; in all 9 images v1.00..v2.05 |
| cap | 100.0nF | `76 21 07 00 00 00 00 00 00 00 00 08 00 80 00 8E 00 C2 0E 20` | 121gw-re/tables/hy_regtbl.h:186 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c09c; in all 9 images v1.00..v2.05 |
| cap | 1.000uF | `76 21 07 00 00 00 00 00 00 00 00 08 80 00 00 8E 00 C2 0E 20` | 121gw-re/tables/hy_regtbl.h:189 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c0c4; in all 9 images v1.00..v2.05 |
| cap | 10.00uF | `76 21 04 8A A5 00 00 00 55 00 00 08 08 00 00 8E 00 C2 0E 20` | 121gw-re/tables/hy_regtbl.h:192 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c0d8; in all 9 images v1.00..v2.05 |
| cap | 100.0uF | `76 21 04 8A A5 00 00 00 55 00 00 08 08 00 00 8E 00 C2 0E 20` | 121gw-re/tables/hy_regtbl.h:195 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c0d8; in all 9 images v1.00..v2.05 |
| cap | 10.00mF | `52 21 00 00 00 11 00 50 11 00 00 00 08 00 00 9A 00 C2 3C 20` | 121gw-re/tables/hy_regtbl.h:198 (hy_regtbl_cap); v1.02 file offset (first match) 0x1c100; in all 9 images v1.00..v2.05 |

### Digilent DMM Shield (dmm.cpp)

| mode | label | R20 … | origin |
|---|---|---|---|
| ohm | 50M Ohm (INTE 0x00) | `C0 CF 17 93 85 00 00 55 55 00 00 08 00 00 80 86 80 D1 3C A0 00 00 00` | digilent-arduino/dmm.cpp:89 (dmmcfg[0]) |
| ohm | 5M Ohm (INTE 0x00) | `C0 CF 17 93 85 00 00 55 55 00 00 08 00 80 80 86 80 D1 3C A0 00 00 00` | digilent-arduino/dmm.cpp:90 (dmmcfg[1]) |
| ohm | 500k Ohm (INTE 0x00) | `C0 CF 17 93 85 00 00 55 55 00 00 08 00 08 80 86 80 D1 33 20 00 00 00` | digilent-arduino/dmm.cpp:91 (dmmcfg[2]) |
| ohm | 50k Ohm (INTE 0x00) | `C0 CF 17 83 85 00 00 55 00 00 00 40 00 06 44 94 80 D3 33 20 00 00 00` | digilent-arduino/dmm.cpp:92 (dmmcfg[3]) |
| ohm | 5k Ohm (INTE 0x00) | `C0 CF 17 83 85 00 00 55 00 00 00 40 60 00 44 94 80 D3 33 20 00 00 00` | digilent-arduino/dmm.cpp:93 (dmmcfg[4]) |
| ohm | 500 Ohm (INTE 0x00) | `C0 CF 17 83 35 00 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0 00 00 00` | digilent-arduino/dmm.cpp:94 (dmmcfg[5]) |
| ohm | 50 Ohm (INTE 0x00) | `C0 CF 17 83 35 01 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0 00 00 00` | digilent-arduino/dmm.cpp:95 (dmmcfg[6]) |
| dcv | 50 V DC (INTE 0x00) | `60 00 17 8B 01 11 00 55 31 00 22 00 00 09 28 A0 80 C7 33 20 00 00 00` | digilent-arduino/dmm.cpp:96 (dmmcfg[7]) |
| dcv | 5 V DC (INTE 0x00) | `60 00 17 8B 01 11 00 55 31 00 22 00 00 90 28 A0 80 C7 33 20 00 00 00` | digilent-arduino/dmm.cpp:97 (dmmcfg[8]) |
| dcmv | 500 mV DC (INTE 0x00) | `C0 00 17 8B 85 11 00 55 31 00 00 00 00 00 00 90 80 C7 33 28 00 00 00` | digilent-arduino/dmm.cpp:98 (dmmcfg[9]) |
| dcmv | 50 mV DC (INTE 0x00) | `00 00 17 8B 35 11 00 55 31 00 00 00 00 00 00 90 80 C7 3C 60 00 00 00` | digilent-arduino/dmm.cpp:99 (dmmcfg[10]) |
| acv | 30 V AC (INTE 0x00) | `F2 DD 07 03 52 10 80 25 31 F8 22 00 00 0D 28 A0 FF C7 38 20 00 00 00` | digilent-arduino/dmm.cpp:100 (dmmcfg[11]) |
| acv | 5 V AC (INTE 0x00) | `F2 DD 07 03 52 10 80 25 31 F8 22 00 00 D0 88 A0 FF C7 38 20 02 50 0C` | digilent-arduino/dmm.cpp:101 (dmmcfg[12]) |
| acmv | 500 mV AC (INTE 0x00) | `92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28 00 00 00` | digilent-arduino/dmm.cpp:102 (dmmcfg[13]) |
| acmv | 50 mV AC (INTE 0x00) | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28 00 00 00` | digilent-arduino/dmm.cpp:103 (dmmcfg[14]) |
| dca | 5 A DC (INTE 0x00) | `00 00 17 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20 00 00 00` | digilent-arduino/dmm.cpp:104 (dmmcfg[15]) |
| aca | 5 A AC (INTE 0x00) | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28 00 00 00` | digilent-arduino/dmm.cpp:105 (dmmcfg[16]) |
| cont | Continuity (INTE 0x00) | `74 CF 17 83 35 10 00 55 00 00 00 00 0A 00 40 86 80 D2 3C A0 00 00 00` | digilent-arduino/dmm.cpp:106 (dmmcfg[17]) |
| diode | Diode (INTE 0x00) | `C0 CF 17 8B 8D 10 00 55 31 00 00 00 08 00 40 86 80 E2 33 A0 00 00 00` | digilent-arduino/dmm.cpp:107 (dmmcfg[18]) |
| dca | 500 mA DC (INTE 0x00) | `00 00 17 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20 00 00 00` | digilent-arduino/dmm.cpp:108 (dmmcfg[19]) |
| dca | 50 mA DC (INTE 0x00) | `00 00 17 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0 00 00 00` | digilent-arduino/dmm.cpp:109 (dmmcfg[20]) |
| dca | 5 mA DC (INTE 0x00) | `00 00 17 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20 00 00 00` | digilent-arduino/dmm.cpp:110 (dmmcfg[21]) |
| dca | 500 uA DC (INTE 0x00) | `00 00 17 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0 00 00 00` | digilent-arduino/dmm.cpp:111 (dmmcfg[22]) |
| aca | 500 mA AC (INTE 0x00) | `92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28 00 00 00` | digilent-arduino/dmm.cpp:112 (dmmcfg[23]) |
| aca | 50 mA AC (INTE 0x00) | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28 00 00 00` | digilent-arduino/dmm.cpp:113 (dmmcfg[24]) |
| aca | 5 mA AC (INTE 0x00) | `92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28 00 00 00` | digilent-arduino/dmm.cpp:114 (dmmcfg[25]) |
| aca | 500 uA AC (INTE 0x00) | `52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28 00 00 00` | digilent-arduino/dmm.cpp:115 (dmmcfg[26]) |

### Temco T3 (HY3131_FunTable.h)

| mode | label | R20 … | origin |
|---|---|---|---|
| dcv | DCVolt 6V Auto | `60 00 15 8F 31 10 00 55 31 00 00 90 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:19 (DCVolt_Setting[0]) |
| dcv | DCVolt 6V Manual | `60 00 15 8F 31 10 00 55 31 00 00 90 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:20 (DCVolt_Setting[1]) |
| dcv | DCVolt 60V Manual | `60 00 15 8F 31 10 00 55 31 00 00 09 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:21 (DCVolt_Setting[2]) |
| dcv | DCVolt 600V Manual | `60 00 15 8F 31 10 00 55 31 00 90 00 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:22 (DCVolt_Setting[3]) |
| dcv | DCVolt 1000V Manual | `60 00 15 8F 31 10 00 55 31 00 09 00 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:23 (DCVolt_Setting[4]) |
| dcv | DCVolt 6V ZERO | `60 00 15 8F 31 10 00 55 31 00 00 90 00 80 00 A0 80 C7 3A 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:24 (DCVolt_Setting[5]) |
| acv | ACVolt 6V Auto | `F2 DD 04 03 52 10 80 25 31 F8 00 90 00 80 00 A0 99 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:41 (ACVolt_Setting[0]) |
| acv | ACVolt 6V Manual | `F2 DD 04 03 52 10 80 25 31 F8 00 90 00 80 00 A0 99 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:42 (ACVolt_Setting[1]) |
| acv | ACVolt 60V Manual | `F2 DD 04 03 52 10 80 25 31 F8 00 09 00 80 00 A0 B0 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:43 (ACVolt_Setting[2]) |
| acv | ACVolt 600V Manual | `F2 DD 04 03 52 10 80 25 31 F8 90 00 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:44 (ACVolt_Setting[3]) |
| acv | ACVolt 1000V Manual | `F2 DD 04 03 52 10 80 25 31 F8 09 00 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:45 (ACVolt_Setting[4]) |
| acv | ACVolt 6V ZERO | `F2 DD 04 03 52 10 80 25 31 F8 00 90 00 80 00 A0 99 C7 3D 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:46 (ACVolt_Setting[5]) |
| dcmv | DCmVolt 60mV Auto | `00 00 16 8F 35 11 00 55 31 00 00 00 00 80 00 81 80 C7 3C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:57 (DCmVolt_Setting[0]) |
| dcmv | DCmVolt 60mV Manual | `00 00 16 8F 35 11 00 55 31 00 00 00 00 80 00 81 80 C7 3C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:58 (DCmVolt_Setting[1]) |
| dcmv | DCmVolt 600mV Manual | `00 00 16 8F 35 10 00 55 31 00 00 00 00 80 00 81 80 C7 3C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:59 (DCmVolt_Setting[2]) |
| dca | DCmA 60mA Auto | `00 00 14 8F 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:65 (DCmA_Setting[0]) |
| dca | DCmA 60mA Manual | `00 00 14 8F 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:66 (DCmA_Setting[1]) |
| dca | DCmA 600mA Manual | `00 00 14 8F 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:67 (DCmA_Setting[2]) |
| dca | DCA 6A Auto | `00 00 14 8F 35 00 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:72 (DCA_Setting[0]) |
| dca | DCA 6A Manual | `00 00 14 8F 35 00 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:73 (DCA_Setting[1]) |
| dca | DCA 10A Manual | `00 00 14 8F 95 00 00 55 31 00 00 00 00 00 00 80 80 C7 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:74 (DCA_Setting[2]) |
| aca | ACmA 60mA Auto | `00 00 00 00 00 01 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:80 (ACmA_Setting[0]) |
| aca | ACmA 60mA Manual | `00 00 00 00 00 01 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:81 (ACmA_Setting[1]) |
| aca | ACmA 600mA Manual | `00 00 00 00 00 00 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:82 (ACmA_Setting[2]) |
| aca | ACA 6A Auto | `00 00 00 00 00 00 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:87 (ACA_Setting[0]) |
| aca | ACA 6A Manual | `00 00 00 00 00 00 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:88 (ACA_Setting[1]) |
| aca | ACA 10A Manual | `00 00 00 00 00 00 80 25 11 F8 00 00 00 00 00 80 80 C7 3D 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:89 (ACA_Setting[2]) |
| acmv | ACmVolt 60mV Auto | `82 DD 07 03 00 11 80 25 11 F8 00 00 00 80 00 81 80 C7 3C 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:102 (ACmVolt_Setting[0]) |
| acmv | ACmVolt 60mV Manual | `82 DD 07 03 00 11 80 25 11 F8 00 00 00 80 00 81 80 C7 3C 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:103 (ACmVolt_Setting[1]) |
| acmv | ACmVolt 600mV Manual | `82 DD 07 03 00 10 80 25 11 F8 00 00 00 80 00 81 80 C7 3C 28` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:104 (ACmVolt_Setting[2]) |
| diode | Diode row 0 | `C0 CF 16 8F 8D 20 00 55 31 00 00 08 00 00 00 86 80 E2 33 A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:115 (Diode_Setting[0]) |
| cont | Continuity row 0 | `74 CF 16 83 85 00 00 55 55 00 08 00 00 00 00 86 80 C2 33 A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:120 (Continuity_Setting[0]) |
| temp | Temp row 0 (SAD1I=3) | `60 00 16 8A 35 00 00 00 31 00 00 00 00 00 08 81 80 C7 0C 23` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:131 (Temp_Setting[0]) |
| temp | Temp row 1 (SAD1I=2) | `60 00 16 8A 35 00 00 00 31 00 00 00 00 00 08 81 80 C7 0C 22` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:132 (Temp_Setting[1]) |
| ohm | OHMCV row 0 | `60 00 16 8F 85 00 00 00 00 00 00 40 06 00 04 94 80 D2 0C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:149 (OHMCV_Setting[0]) |
| ohm | OHMCV row 1 | `60 00 16 8F 85 00 00 00 00 00 00 40 60 00 04 94 80 D2 0C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:150 (OHMCV_Setting[1]) |
| ohm | OHMCV row 2 | `60 00 16 8F 85 00 00 00 00 00 00 40 00 06 04 94 80 D2 0C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:151 (OHMCV_Setting[2]) |
| ohm | OHMCV row 3 | `60 00 16 8F 85 00 00 00 00 00 00 40 00 60 04 94 80 D2 0C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:152 (OHMCV_Setting[3]) |
| ohm | OHMCV row 4 | `60 00 16 8F 85 00 00 00 00 00 00 40 60 00 06 94 80 D2 0C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:153 (OHMCV_Setting[4]) |
| ohm | OHM 60 Auto | `C0 CF 16 83 35 01 00 55 00 00 06 00 04 00 00 94 80 D2 3C A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:158 (OHM_Setting[0]) |
| ohm | OHM 60 Manual | `C0 CF 16 83 35 01 00 55 00 00 06 00 04 00 00 94 80 D2 3C A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:159 (OHM_Setting[1]) |
| ohm | OHM 600 Manual | `C0 CF 16 83 35 00 00 55 00 00 06 00 04 00 00 94 80 D2 3C A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:160 (OHM_Setting[2]) |
| ohm | OHM 6K Manual | `C0 CF 16 83 35 00 00 55 00 00 60 00 04 00 00 94 80 D3 3C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:161 (OHM_Setting[3]) |
| ohm | OHM 60K Manual | `C0 CF 16 83 35 00 00 55 00 00 00 06 04 00 00 94 80 D3 3C 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:162 (OHM_Setting[4]) |
| ohm | OHM 600K Manual | `C0 CF 16 93 8D 00 00 55 55 00 00 80 80 00 00 86 80 D1 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:163 (OHM_Setting[5]) |
| ohm | OHM 6M Manual | `C0 CF 16 93 85 00 00 55 55 00 00 80 80 00 00 86 80 D1 3C A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:164 (OHM_Setting[6]) |
| ohm | OHM 60M Manual | `C0 CF 16 93 85 00 00 55 55 00 00 00 80 00 00 86 80 D1 3C A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:165 (OHM_Setting[7]) |
| ohm | OHMCC row 0 | `60 00 16 83 85 00 00 00 31 00 00 40 08 00 00 86 80 D1 0C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:183 (OHMCC_Setting[0]) |
| ohm | OHMCC row 1 | `60 00 16 83 85 00 00 00 31 00 00 40 80 00 00 86 80 D1 0C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:184 (OHMCC_Setting[1]) |
| ohm | OHMCC row 2 | `60 00 16 83 85 00 00 00 31 00 00 48 00 08 00 86 80 D1 0C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:185 (OHMCC_Setting[2]) |
| ohm | OHMCC row 3 | `60 00 16 83 85 00 00 00 31 00 00 48 00 80 00 86 80 D1 0C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:186 (OHMCC_Setting[3]) |
| ohm | OHMCC row 4 | `60 00 16 83 85 00 00 00 31 00 00 48 00 00 00 86 80 D1 0C 60` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:187 (OHMCC_Setting[4]) |
| cap | CapCV row 0 | `76 11 00 00 00 00 00 00 00 00 00 08 00 80 00 9A 00 C1 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:202 (CapCV_Setting[0]) |
| cap | CapCV row 1 | `76 11 00 00 00 00 00 00 00 00 00 08 00 08 00 9A 00 C1 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:203 (CapCV_Setting[1]) |
| cap | CapCV row 2 | `76 11 00 00 00 00 00 00 00 00 00 08 80 00 00 9A 00 C1 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:204 (CapCV_Setting[2]) |
| cap | CapCV row 3 | `76 11 00 00 00 00 00 00 00 00 00 08 08 00 00 9A 00 C1 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:205 (CapCV_Setting[3]) |
| cap | CapCC row 0 | `76 16 00 00 00 01 00 00 00 00 00 08 00 80 00 8F 00 C2 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:221 (CapCC_Setting[0]) |
| cap | CapCC row 1 | `76 16 00 00 00 01 00 00 00 00 00 08 00 08 00 8F 00 C2 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:222 (CapCC_Setting[1]) |
| cap | CapCC row 2 | `76 16 00 00 00 01 00 00 00 00 00 08 80 00 00 8F 00 C2 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:223 (CapCC_Setting[2]) |
| cap | CapCC row 3 | `76 16 00 00 00 01 00 00 00 00 00 08 08 00 00 8F 00 C2 0E 2C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:224 (CapCC_Setting[3]) |
| freq | Freq CNT | `0A CF 16 03 35 10 00 55 00 00 00 00 00 00 00 80 80 C2 30 A0` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:242 (Freq_Setting[0]) |
| freq | Freq ACPO-6V | `52 CF 04 03 52 10 80 25 31 F8 00 90 00 80 00 A0 80 C7 38 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:243 (Freq_Setting[1]) |
| freq | Freq ACPO | `F6 CF 04 03 32 10 00 25 31 00 00 90 00 80 00 A0 80 C7 38 30` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:244 (Freq_Setting[2]) |
| cap | Cap 60nF~600nF ->0 | `72 21 14 0F 52 11 00 55 31 00 00 08 00 00 00 9A 80 C7 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:262 (Cap_Setting[0]) |
| cap | Cap 600nF~60uF ->1 | `72 21 14 0F 52 11 00 55 31 00 80 00 00 00 00 9A 80 C7 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:263 (Cap_Setting[1]) |
| cap | Cap 60uF~600uF ->2 | `72 21 14 0F 52 11 00 55 31 00 08 00 00 00 00 9A 80 C7 33 20` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:264 (Cap_Setting[2]) |
| cap | Cap Charge ADC get | `00 E7 A6 93 85 10 00 55 55 00 08 00 00 00 00 86 80 C1 30 6C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:265 (Cap_Setting[3]) |
| cap | Cap disCharge | `70 DE A6 93 85 10 00 55 55 00 08 00 00 00 00 8E 80 C2 30 6C` | temco-t3/STM32/Multimeter/USER/HY3131_FunTable.h:266 (Cap_Setting[4]) |

### OWON XDM2041 V1.7.2 (SPI capture, bulk writes)

| mode | label | R20 … | origin |
|---|---|---|---|
| dcmv | CONF:VOLT:DC 0.05 | `C2 21 14 8B 35 11 08 15 31 F8 00 00 00 00 08 81 80 C7 3C A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:82 |
| dcmv | CONF:VOLT:DC 0.5 | `C2 21 14 8B 85 11 08 15 31 F8 00 00 00 00 08 81 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:184 |
| dcv | CONF:VOLT:DC 5 | `C2 21 14 8B 01 11 08 15 31 F8 20 00 00 90 28 A0 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:278 |
| dcv | CONF:VOLT:DC 50 | `C2 21 14 8B 01 11 08 15 31 F8 20 00 00 09 28 A0 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:378 |
| dcv | CONF:VOLT:DC 500 | `C2 21 14 8B 01 11 08 15 31 F8 20 00 90 00 28 A0 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:481 |
| dcv | CONF:VOLT:DC 1000 | `C2 21 14 8B 09 01 08 15 31 F8 20 00 90 00 28 A0 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:579 |
| acmv | CONF:VOLT:AC 0.5 | `C2 21 00 00 45 00 88 15 31 F8 00 00 00 00 08 91 80 C7 3C A0 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:682 |
| acv | CONF:VOLT:AC 5 | `C2 21 00 00 4D 00 88 15 31 F8 22 00 00 D0 88 A0 E9 C7 38 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:859 |
| acv | CONF:VOLT:AC 50 | `C2 21 00 00 4D 00 88 15 31 F8 22 00 00 09 28 A0 FF C7 38 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1051 |
| acv | CONF:VOLT:AC 500 | `C2 21 00 00 4D 00 88 15 31 F8 22 00 90 00 28 A0 80 C7 38 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1238 |
| acv | CONF:VOLT:AC 750 | `C2 21 00 00 4D 00 88 15 31 F8 22 00 90 00 28 A0 80 C7 38 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1430 |
| dca | CONF:CURR:DC 0.0005 | `C2 21 14 8B 35 11 08 15 11 F8 00 00 00 00 00 A0 80 C7 3D AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1617 |
| dca | CONF:CURR:DC 0.005 | `C2 21 14 8B 95 11 08 15 11 F8 00 00 00 00 00 A0 80 C7 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1717 |
| dca | CONF:CURR:DC 0.05 | `C2 21 14 8B 35 11 08 15 11 F8 00 00 00 00 00 A0 80 C7 3D AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1817 |
| dca | CONF:CURR:DC 0.5 | `C2 21 14 8B 95 11 08 15 11 F8 00 00 00 00 00 A0 80 C7 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:1917 |
| dca | CONF:CURR:DC 5 | `C2 21 14 8B 35 11 08 15 11 F8 00 00 00 00 00 A0 80 C7 3D AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2011 |
| dca | CONF:CURR:DC 10 | `C2 21 14 8B 35 01 08 15 11 F8 00 00 00 00 00 A0 80 C7 3D AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2109 |
| aca | CONF:CURR:AC 0.0005 | `C2 21 12 00 45 01 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2212 |
| aca | CONF:CURR:AC 0.005 | `C2 21 12 00 45 00 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D A0 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2324 |
| aca | CONF:CURR:AC 0.05 | `C2 21 12 00 45 01 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2441 |
| aca | CONF:CURR:AC 0.5 | `C2 21 12 00 45 00 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D A0 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2553 |
| aca | CONF:CURR:AC 5 | `C2 21 12 00 45 01 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D 20 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2665 |
| aca | CONF:CURR:AC 10 | `C2 21 12 00 45 00 88 15 31 F8 00 00 00 00 00 A0 80 C7 3D A0 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2777 |
| ohm | CONF:RES 500 | `C2 21 14 83 85 01 08 15 00 F8 00 00 06 00 00 94 80 D2 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2894 |
| ohm | CONF:RES 5E3 | `C2 21 14 83 85 01 08 15 00 F8 00 00 60 00 00 94 80 D3 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:2991 |
| ohm | CONF:RES 50E3 | `C2 21 14 83 85 01 08 15 00 F8 00 00 00 06 00 94 80 D3 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3096 |
| ohm | CONF:RES 500E3 | `C2 21 14 83 85 01 08 15 00 F8 00 00 00 60 00 94 80 D3 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3198 |
| ohm | CONF:RES 5E6 | `C2 21 14 93 85 01 08 15 55 F8 00 00 00 80 00 86 80 D1 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3298 |
| ohm | CONF:RES 50E6 | `C2 21 14 93 85 01 08 15 55 F8 00 08 00 00 00 86 80 D1 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3397 |
| ohm | CONF:FRES (after RES 500) | `C2 21 14 83 A7 01 08 15 00 F8 00 00 06 00 00 94 80 D2 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3591 |
| ohm | CONF:FRES (after RES 5E3) | `C2 21 14 83 A7 01 08 15 00 F8 00 00 60 00 00 94 80 D3 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3788 |
| ohm | CONF:FRES (after RES 50E3) | `C2 21 14 83 A7 01 08 15 00 F8 00 00 00 06 00 94 80 D3 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:3990 |
| cont | CONF:CONT | `76 62 13 83 85 01 08 15 00 F8 00 40 06 00 00 94 80 D2 3F AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4091 |
| diode | CONF:DIOD | `12 62 13 8B 8D 11 08 15 11 F8 00 00 08 00 00 86 80 E2 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4224 |
| cap | CONF:CAP 50E-9 [AD1 fields left from the previous mode] | `72 21 14 8B 01 11 08 15 31 F8 00 00 00 08 00 9A 80 D7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4362 |
| cap | CONF:CAP 5E-3 | `C0 21 07 93 85 11 08 15 55 F8 80 00 08 00 00 86 80 D1 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4663 |
| cap | CONF:CAP 50E-3 | `70 DE 07 93 85 11 08 15 55 F8 80 00 08 00 00 8F 80 DA 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4688 |
| freq | CONF:FREQ [AD1 fields left from the previous mode] | `CA DE 07 93 85 11 08 15 55 F8 00 00 00 00 00 80 80 D7 33 AC 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4704 |
| temp | CONF:TEMP:RTD KITS90 | `C2 DE 14 8B 85 11 08 15 31 F8 00 00 00 00 08 81 80 C7 33 A8 00 00 00` | owon-xdm2041-info/spi_captures/all_modes.txt:4729 |

### Keysight U1281A/U1282A v1.05 (decoded table 0x18EA and overlays)

| mode | label | R20 … | origin |
|---|---|---|---|
| dcmv | row 0: mode 0 DC (mV, inferred) | `74 BF 14 80 3B 9B 19 DE 32 20 00 00 00 00 80 A1 C0 C0 3D 2C` | decoded image offset 0x18EA (table 0x18EA row 0) |
| dcv | row 1: mode 1 DC (V, inferred); also mode 0x0B | `70 BF 14 80 39 9B 19 CE 32 20 00 D0 80 10 00 80 C0 C7 3D 2C` | decoded image offset 0x18FE (table 0x18EA row 1) |
| dca | row 2: mode 2 current DC (inferred) | `74 DD 14 90 33 4B 19 DE 32 20 B0 00 80 00 00 A1 C0 D0 3D 2C` | decoded image offset 0x1912 (table 0x18EA row 2) |
| dca | row 3: mode 3 current DC (inferred) | `74 DD 14 90 33 4B 19 DE 32 20 B0 00 80 00 00 A1 C0 D0 3D 2C` | decoded image offset 0x1926 (table 0x18EA row 3) |
| ohm | row 4: mode 4 ohm / continuity | `54 CF 14 80 3B 4B 19 DE 00 20 60 00 00 00 04 94 C0 D2 3D A0` | decoded image offset 0x193A (table 0x18EA row 4) |
| diode | row 5: mode 5 diode | `74 DD 14 90 33 4A 19 DE 32 20 80 00 00 00 10 92 C0 E2 38 20` | decoded image offset 0x194E (table 0x18EA row 5) |
| temp | row 6: mode 6 temperature | `74 BF 14 80 3B 4A 19 DE 32 20 00 00 00 00 00 A1 C0 D0 3D 2C` | decoded image offset 0x1962 (table 0x18EA row 6) |
| cap | row 7: mode 7 capacitance | `54 22 14 10 33 4A 19 DE 32 20 00 80 00 00 00 8E 40 D2 3D 2C` | decoded image offset 0x1976 (table 0x18EA row 7) |
| acmv | row 8: mode 0 AC (mV, inferred) | `74 CC 14 00 33 9B 99 DE 32 60 00 00 00 00 80 A1 C0 C0 BD 2C` | decoded image offset 0x198A (table 0x18EA row 8) |
| acv | row 9: mode 1 AC (V, inferred) | `74 CC 14 00 31 9B 99 DE 32 60 00 D0 80 10 00 80 C0 C0 BD 2C` | decoded image offset 0x199E (table 0x18EA row 9) |
| aca | row 10: mode 2 AC (inferred) | `74 DD 14 10 33 4B 99 DE 32 60 B0 00 80 00 00 A1 C0 D0 BD 2C` | decoded image offset 0x19B2 (table 0x18EA row 10) |
| aca | row 11: mode 3 AC (inferred) | `74 DD 14 10 33 4B 99 DE 32 60 B0 00 80 00 00 A1 C0 D0 BD 2C` | decoded image offset 0x19C6 (table 0x18EA row 11) |
| other | row 12: row 12, flag 0xFE24.2 (use unknown) | `74 CC 14 80 31 4B 99 DE 32 60 90 00 80 10 00 80 C0 C0 BE 2C` | decoded image offset 0x19DA (table 0x18EA row 12) |
| dcv | mode 1 r0 = row 1 + overlay 0x19EE (composed, inferred) | `70 BF 14 80 39 9B 19 CE 32 20 00 D0 80 10 00 80 C0 C7 3D 2C` | decoded image, overlay table 0x19EE |
| dcv | mode 1 r1 = row 1 + overlay 0x19EE (composed, inferred) | `70 BF 14 80 39 9B 19 CE 32 20 00 0D 80 14 00 80 FF C7 3D 2C` | decoded image, overlay table 0x19EE |
| dcv | mode 1 r2 = row 1 + overlay 0x19EE (composed, inferred) | `70 BF 14 80 39 9B 19 CE 32 20 D0 00 80 10 00 80 C0 C7 3D 2C` | decoded image, overlay table 0x19EE |
| dcv | mode 1 r3 = row 1 + overlay 0x19EE (composed, inferred) | `70 BF 14 80 39 4B 19 CE 32 20 D0 00 80 10 00 80 C0 C7 3D 2C` | decoded image, overlay table 0x19EE |
| ohm | mode 4 r0 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 80 3B 4B 19 DE 00 20 60 00 00 00 04 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r1 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 80 3B 4A 19 DE 00 20 60 00 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r2 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 80 3B 4A 19 DE 00 20 00 06 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r3 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 80 3B 4A 19 DE 00 20 00 60 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r4 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 8F 3B 4A 19 DE 00 20 00 00 06 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r5 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 8F 3B 4A 19 DE 00 20 06 00 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r6 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 8F 3B 4A 19 DE 00 20 06 00 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r7 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 8F 3B 4A 19 DE 00 20 06 00 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| ohm | mode 4 r8 = row 4 + overlay 0x19FA (composed, inferred) | `54 CF 14 8F 3B 4A 19 DE 00 20 06 00 00 00 00 94 C0 D2 3D A0` | decoded image, overlay table 0x19FA |
| cap | mode 7 r0 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 00 80 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r1 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 00 80 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r2 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 00 80 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r3 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 00 80 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r4 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 00 08 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r5 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4A 19 DE 32 20 80 00 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r6 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4B 19 DE 32 20 80 00 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r7 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4B 19 DE 32 20 80 00 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r8 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 90 33 4B 19 DE 32 20 80 00 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |
| cap | mode 7 r9 = row 7 + overlay 0x1A27 (composed, inferred) | `54 22 14 10 33 4B 19 DE 32 20 00 32 00 00 00 8E 40 D2 3D 2C` | decoded image, overlay table 0x1A27 |

