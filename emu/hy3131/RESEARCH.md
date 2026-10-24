# HY3131 research notes

Sources found for the Hycon HY3131 (50,000-count DMM analog front end: AD1 24-bit
sigma-delta, AD2/AD3 19-bit fast sigma-delta, digital LPF / true-RMS / peak hold,
frequency counter, switch network, 2 op amps, OP3 source, comparators, temperature
sensor, SPI). Status 2026-10-14. Nothing from Hycon is redistributed here: the
datasheet and the application note carry "reproduction or copying without the
permission of HYCON Technology Corp. is strictly prohibited" (datasheet p.4 item 8),
so `ref/` holds only our own transcriptions.

## 1. Documents

| document | where | notes |
|---|---|---|
| DS-HY3131-V10_EN (HY3130/HY3131 datasheet, 42 pages, PDF dated 2014-11-12) | local copy outside the repo (`~/Downloads/DS-HY3131_EN.pdf`, md5 4f53dcb7...); public mirrors `https://shaddack.mauriceward.com/projects/reveng_multimeters/chips/HY3131_EN.pdf`, `http://essentialscrap.com/owon_fix/DS-HY3131_EN.pdf`; vendor link (unverified, hycontek.com unreachable from here) `https://www.hycontek.com/wp-content/uploads/DS-HY3131_EN.pdf` | no newer revision (V11+) found anywhere; revision record ends at V10 |
| APD-DMM003-V02_EN "HY313X Configurations" (application note, 51 pages, 2012) | `http://essentialscrap.com/owon_fix/HY313X_appnote.pdf` | annotated block diagrams per function (DCmV, ACmV, DCV, ACV, capacitor, resistor, diode, continuity, current, frequency); no temperature chapter, no register hex tables |
| "HY313x ENOB V1.4" ADC performance test tool (Hycon, 2014) | local zip outside the repo (`~/Telegram Desktop/HY3131-ENOB.zip`) | contains `Include/HY3131.MEM` (register/bit names + mux option lists, transcribed in section 4), `Config/*/HYCONFIG-n.txt` (per-function register images R14..R36, section 5), the user's manual of the tool, and the schematic of the T11001 V03 evaluation board (HY3131 + 4.9152 MHz crystal + USB HID bridge) |
| Digilent DMM Shield (HY3131) Arduino library | `https://github.com/Digilent/Arduino-DMMShield-Library` (`dmm.cpp`: 27 configuration rows R1F..R36 with scale factors, `calib.cpp`) | independent register images for V / A / ohm / diode / continuity (section 6) |
| Project Phoenix (OWON XDM2041 replacement firmware, STM32, CC0) | `https://github.com/aziemer/Project-Phoenix` | only open code using the HY3131 frequency counter; cap / temp not done |
| owon-xdm2041-info (CC0) | `https://github.com/PetteriAimonen/owon-xdm2041-info` | board photos, SPI captures of a commercial HY3131 meter (`spi_captures/`), hidden SCPI `INT:HYREG` |
| essentialscrap.com/owon_fix + Hackaday 2021-07-14 | `http://essentialscrap.com/owon_fix/` | "The ADC samplerate is determined by a constant 30x divider" and "a configurable oversampler with ratio of 256 ... 32768": OSR 16384 / 32768 at 4.9152 MHz -> 100 / 200 ms (no chopper); crystal swap changes the frequency readings (counter clocked by SYSCLK), not the capacitance |
| EEVblog threads (OWON XDM1041, 121GW RE wiki `tpwrules/121gw-re`) | various | 121GW, UT171A/B, Keysight U1282A, Brymen BM786 reported to use the HY3131; no register data |

Not found: HY3131-AK01 / evaluation board user guide, Hycon reference DMM firmware
(HY11P / HY16F sample code), other APD-DMM notes, any Chinese-language code or
register tables (amobbs / 21ic / CSDN / Gitee: no hits), any public statement on
AD1CHOP halving the rate, on the LPF / RMS output rate or on SPHACAL.

In this repo: `stock/re/analysis/hy3131_v2_6.h` (bus protocol, the stock
firmware's 60 configuration rows with a decoded reading of each, calibration
layout), `dmm_v2_6.h` (per-mode formulas, filters, settle deadlines),
`firmware/src/hy3131.rs` / `hy3131_tables.rs` (opendm40 driver, identical rows),
`docs/history/acdc-stock-vs-opendm40.md` (device measurements: AC+DC cadence, AC
coupling transient, AD2/AD3 enable-order beat).

## 2. Key facts from the datasheet (DS-HY3131-V10_EN)

Clocks (chapter 11): crystal 4.9152 MHz (ENOSC, R33.5; ENXI selects XIN as SYSCLK),
`AD1CLK = SYSCLK / 30` (fixed), `AD2CLK = SYSCLK / 2` (SAD2CLK = 0) or `/ 4`
(SAD2CLK = 1), AD3 shares AD2CLK. Reset: write 0x60 to R37; BOR below 1.9 V (BORF).

Conversion rates (13.1, 13.2):
* AD1 data rate = FAD1CLK / OSR1, OSR1 = 256 << AD1OSR (256..32768); with
  AD1CHOP = 1x the chip alternates the input polarity per conversion and "the ADC
  output rate will be one time slower" (halved).
* AD2 / AD3 rate = FAD2CLK / OSR2, OSR2 = 32, 64, 128, 256, 512, 1024, 1024, 1024
  (AD2OSR); ENCHOPAD2 / ENCHOPAD3 halve it.
* Low pass filter rate = AD2 rate / OSR4, OSR4 = 128 << LPFBW (128..16384); the RMS
  converter outputs at the LPF rate. RMS = sum(X^2)/N (SDO23 = 0, SVXI = 0),
  sum(X*Y)/N (SDO23 = 0, SVXI = 1), sum(Y^2)/N (SDO23 = 1), X = AD2 and Y = AD3
  after a high pass filter, N = OSR4. Square root in software.
* Peak hold input: AD2<18:0>, AD1<23:5>, LPF<18:0> (PKHSEL); ENPKH = 0 presets
  PKHMAX = 0x40000 (-262144) and PKHMIN = 0x3FFFF.

AD1 transfer (13.1): `AD1<23:0> = IG * VIN / (RG * VR) + DCOS / RG` in units of full
scale (2^23 codes), IG = 0.9 / 1.8 / 2.7 / 3.6 (AD1IG), RG = 1 / 0.333 (AD1RG),
DCOS = 0, +-0.25, +-0.5, +-0.75 (AD1OS). AD1CHOP = 00: VX + VOS, 01: VX - VOS,
1x: VX (offset removed). AD2 / AD3 gain 0.5 / 1.0 / 1.5 / 2.0 (AD2IG / AD3IG), RG
1 / 0.333. Setting a converter's enable bit to 0 clears its data register (AD1,
AD2, LPF, RMS). Note 7: do not switch AD2/AD3 while AD1 runs (AD1 offset changes).

Muxes (12.3, 12.4): tables transcribed in `ref/registers.md` (SAD1FP, SAD1FN,
SAD1I, SAD1RH, SAD1RL, SAD2IP/IN, SAD2RH/RL, SAD3IP/IN, SOP1P, SOP2P, SCMPI,
SCMPRH, SCMPRL, OP1CHOP, SFT1). SDIO connects PB<0> and PB<8> (diode divider).

Voltage references (12.1): REFO = internal band gap ~1.2 V relative to AGND or the
PB<6> pin (SREFO); AGND = K3 * VDDA with K3 = 0.5 / 0.3 / 0.1 / floating (SAGND);
ACM = 1.2 / 0.9 / 1.5 / 1.125 V (SACM). Divider nodes: `VDSC<n> = K1 * VDDA`
(K1 = 35/36 .. 1/36, table in ref/registers.md), `AGNDP<n> = AGND + K2 * REFO`,
`AGNDN<n> = AGND - K2 * REFO` (K2 = 100/120, 80, 60, 40, 20, 10, 6, 4, 2, 0 for n =
9..0), valid while REFO < 2 (AGND - VSSA); otherwise the K2B column applies.

Switch network (12.2): each PA<n> has four switches: PS (power bus: SVDD / SVSS /
SGND selected by SMODE<3:0>), DS (OP3 output), FS (FB node), SS (SENSE node);
resistances 80 ohm (PS9..2), 40 (PS1..0), 80 (DS), 300 (FS, SS). OP3 (+ input VREF
from the SFUVR / SMODE<3> / CMPO table) with the SCP / SCN MOS switches forms the
voltage / current source; CMPO (comparator output) can flip VREF, which is how the
capacitor relaxation oscillator and the charge / discharge ramp work. SMODE<4>
connects RLU, SMODE<5> the capacitor array (ACC<6:0> x 0.2 pF, up to 25.4 pF,
AC V bandwidth compensation).

Comparator (12.3): CMPH / CMPL with VRHCMP / VRLCMP thresholds and a latch -> ACPO
(readable in CTSTA.6, counter source), CMPO = ACPO buffered (drives the switch
network and, with ENPCMPO, the CMP pin). Input INCMP from SCMPI.

Frequency counter (14): source ACPO (ENCNTI = 0) or the CNT pin (ENCNTI = 1).
Procedure: ENCTR = 0 (clears CTA<7:0>, CTB, CTC, CTBOV; CTA<23:8> writable),
write the gate preset to CTA<23:8>, ENCTR = 1: CTA counts SYSCLK; when CTA
overflows it keeps counting until CTB has recorded a whole number of input
cycles, then the counter stops and sets CTF. `T = (0x1000000 - CTA_init +
CTA_final) / FSYSCLK`, `f = CTB / T`, `duty = CTC / (0x1000000 - CTA_init +
CTA_final)` (CTC = SYSCLK cycles with the input high). CTBOV = CTB overflow
(cleared by reading CTSTA or ENCTR = 0).

Temperature sensor (12.4): two differential outputs (TS1P, TS2N) = SAD1I 10 and
(TS1N, TS2P) = SAD1I 11 measured by AD1; `TCode = (TCode2 - TCode1) / 2`
removes the sensor offset; one-point calibration at 25 C: `G = TCode@25 / (25 +
273.15 + TOS)`, TOS ~ -3.15 K, `TX = TCode@TX / G - 273.15 - TOS`.

SPI (7): command byte = address<6:0> << 1 | R/W (1 = read), MSB first, CK idle low,
data changes on the CK falling edge (DO ready 60 ns), one blank "read period"
clock after a read command, address auto-increment in both directions until CS
goes high. CS high: DO = IRQ (INTF & INTE != 0). INTF is cleared when it is read
(and by writing 0s; 1s cannot be written); XXF bits are set regardless of INTE.
Register R35..R37: HY3131 only; R37 is "testing mode, write 0 only" except the
0x60 reset.

Electrical (6): VDD 3.6 V; AD1 zero reading +-1 count of 50,000 at VIN = 0, drift
0.03 counts/C; AD1 linearity +-(0.01 % + 2 counts) at 30,000 of 50,000 counts;
input leakage 10 pA; OP1/OP2 offset 2 mV without / 20 uV with chopper, BW 2 MHz;
OP3 source / sink 600..900 uA; comparator BW 6 MHz; NMR 120 dB at 5 sps, 75 dB at
10 sps (50 / 60 Hz +-1 Hz); CK high >= 20 ns, low >= 60 ns.

## 3. Conversion rate: resolving "~9.4/s vs OSR/SYSCLK"

Before f306cf5, `firmware/src/dmm.rs` assumed the stock's ~9.4 readings/s come from
OSR 32768 and `firmware/src/hy3131.rs::osr_period_us` modelled OSR / SYSCLK (OSR
2048 -> 417 us). Neither matched the datasheet. Fixed in f306cf5:
`osr_period_us` is gone, `hy3131::ad1_period_us` / `ad1_period_us_r22` use the
formula below and dmm.rs's FR-05 table (and the data logger's nominal period)
follow it:

    AD1 period = OSR1 * 30 / FSYSCLK * (AD1CHOP = 1x ? 2 : 1)

Every R22 value the stock writes has AD1CHOP = 10 (0x13, 0x14, 0x15, 0x16, 0x17),
so at 4.9152 MHz:

| R22 | OSR1 | period, chopped | rate | where the stock uses it |
|---|---|---|---|---|
| 0x13 | 2048 | 25.0 ms | 40 /s | ohm over range, opendm40 data logger (measured 38 Hz on the device: the chip, not the SPI link, is the limit) |
| 0x14 | 4096 | 50 ms | 20 /s | continuity while the reading moves |
| 0x15 | 8192 | 100 ms | 10 /s | every DC row as written, AC+DC DC phase; "3 AD1 results in 123..222 ms" (acdc doc 3) and the users' "~10 readings/s" |
| 0x16 | 16384 | 200 ms | 5 /s | temperature rows |
| 0x17 | 32768 | 400 ms | 2.5 /s | "stable" DC reading, ohm in range |

The 5 sps "50,000 counts @ 5 Hz" of the feature list is OSR 32768 without the
chopper. The unchopped OSR / FAD1CLK rates (5, 10, 20, 40 sps at OSR 32768 ..
4096) are what essentialscrap.com observed on an OWON XDM2041 (different crystal,
R22 unknown), consistent with the same /30 divider.

AD2 / AD3 in every AC row: R26 = 0x80 (SAD2CLK = 0, OSR2 = 32): 2.4576 MHz / 32 =
76.8 kHz, one sample per 13.02 us (the period of the "bad window" in the enable
order sweep, acdc doc 10.2). LPF / RMS: R29 LPFBW = 111 -> OSR4 = 16384 -> 213 ms
by the datasheet formula, but the device delivers an RMS / LPF result every
105..110 ms (acdc doc 3 and 10.1, "0.2175 cycles per result = 106.7 ms"), i.e.
exactly half: 76.8 kHz / 8192. Open point (section 8); the model uses the measured
period through the constant `LpfRateDivisor` (= 0.5 of the datasheet OSR4).

## 4. HY3131.MEM (Hycon ENOB tool): register and mux names

Transcribed in `ref/registers.md`. It agrees with the datasheet and adds the
option lists of every mux field (used in the model), bit 6 of CTSTA = ACPO,
"ADSIP 2 0 ADSFP FB TS1P TS1N / ADSFN RLU TS2N TS2P SAD1I" (SAD1I 00 = filter,
01 = FB/RLU, 10 = TS1P/TS2N, 11 = TS1N/TS2P), and the SFUVR option list
`VDS17 VDS16 VDS15 AGNDP9 AGNDP8 AGNDP7 AGNDP6 AGNDP0 VDS1 VDS2 VDS3 AGNDN9
AGNDN8 AGNDN7 AGNDN6 RB7` (SMODE<3> = 0 column of the datasheet VREF table).

## 5. HYCONFIG rows of the ENOB tool (R20..R36)

Hycon's own per-function register images for the T11001 evaluation board (the
datasheet typical circuit: 10M / 1M / 100k / 10k / 1k divider on PA9..PA5, 900k /
100k on PB0 / PB8, 0.01 / 0.99 / 99 ohm shunts on PB1 / PB5). They confirm the
meaning of the fields independently of the DM40 rows:

```
                       R20 21 22 23 24 25 26 27 28 29 2A 2B 2C 2D 2E 2F 30 31 32 33 34 35 36
ACV 0 (6 V)            F2 DD 04 03 52 10 80 25 31 F8 22 00 00 D0 88 A0 99 C7 38 20 02 50 0C
ACV 1                  F2 DD 04 03 52 10 80 25 31 F8 22 00 00 0D 28 A0 B0 C7 38 20
ACV 2                  F2 DD 04 03 52 10 80 25 31 F8 22 00 90 00 28 A0 80 C7 38 20
ACV 3                  F2 DD 04 03 52 10 80 25 31 F8 22 00 09 00 28 A0 80 C7 38 20 02 50 0C
CAP 0 (60-600 nF, RC)  72 21 14 0B 52 11 00 55 31 00 00 00 00 08 00 9A 80 C7 33 20 00 00 00
CAP 1                  72 42 14 0B 52 11 00 55 31 00 00 00 80 00 00 8E 80 C2 33 20
CAP 2 (ramp)           72 DE A4 93 85 11 00 55 55 00 00 00 00 08 00 86 80 C1 33 2C 00 00 00
CAP 3                  72 DE A4 93 85 11 00 55 55 00 80 00 00 00 00 86 80 C1 33 2C 00 00 00
CAP 4                  72 DE A4 93 85 11 00 55 55 00 80 00 08 00 00 8E 80 C2 3C 2C 00 00 00
Current 0 (DC)         00 00 14 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20
Current 1              00 00 14 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0
Current 2              00 00 16 8B 95 11 00 55 31 00 00 00 00 00 00 80 80 C7 33 20
Current 3              00 00 16 8B 35 11 00 55 31 00 00 00 00 00 00 80 80 C7 3D A0
Current 4 (AC)         92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28
Current 5              52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3D 28
Current 6              92 DD 07 03 52 10 80 25 11 E0 00 00 00 00 00 90 80 C7 3D 28
DCV 0 (6 V)            60 00 14 8B 01 11 00 55 31 00 22 00 00 90 28 A0 80 C7 33 20
DCV 1                  60 00 14 8B 01 11 00 55 31 00 22 00 00 09 28 A0 80 C7 33 20
DCV 2                  60 00 14 8B 01 11 00 55 31 00 22 00 90 00 28 A0 80 C7 33 20
DCV 3                  60 00 14 8B 01 11 00 55 31 00 22 00 09 00 28 A0 80 C7 33 20
Diode                  C0 CF 16 8B 8D 10 00 55 31 00 00 00 08 00 40 86 80 E2 33 A0
Beeper (continuity)    74 CF 16 83 35 10 00 55 00 00 00 00 0A 00 40 86 80 D2 3C A0
Beeper 2               7C CF 16 83 35 10 00 55 00 00 00 00 0A 00 40 86 80 E2 3C A0
Freq 0 (CNT pin)       6A 00 00 00 00 01 00 00 00 00 00 00 00 00 00 80 80 C7 33 30
Freq 1 (DCV path)      F2 DD 00 0B 53 11 00 55 31 00 02 00 00 90 28 A0 80 C7 38 30
Freq 2 (ACV path)      52 CF 04 03 52 10 80 25 31 F8 02 00 00 D0 88 A0 80 C7 38 20
mV 0 (DC 60 mV, x10)   C0 00 14 8B 85 11 00 55 31 00 00 00 00 00 00 90 80 C7 33 28
mV 1 (DC 600 mV)       00 00 16 8B 35 11 00 55 31 00 00 00 00 00 00 90 80 C7 3C 60
mV 2 (AC)              92 DD 07 03 52 10 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28
mV 3 (AC)              52 DD 07 03 00 13 80 25 11 F8 00 00 00 00 00 90 80 C7 3A 28
OHM 0 (ratio, Rn 1k)   C0 CF 16 83 35 01 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0
OHM 1                  C0 CF 16 83 35 00 00 55 00 00 00 40 06 00 44 94 80 D2 3C A0
OHM 2                  C0 CF 16 83 85 00 00 55 00 00 00 40 60 00 44 94 80 D3 33 20
OHM 3                  C0 CF 16 83 85 00 00 55 00 00 00 40 00 06 44 94 80 D3 33 20
OHM 4 (const. current) C0 CF 16 93 85 00 00 55 55 00 00 08 00 08 80 86 80 D1 33 20
OHM 5                  C0 CF 16 93 85 00 00 55 55 00 00 08 00 80 80 86 80 D1 3C A0
OHM 6                  C0 CF 16 93 85 00 00 55 55 00 00 08 00 00 80 86 80 D1 3C A0
OHM 7                  70 CF 16 83 85 00 00 55 01 00 00 40 00 00 46 94 80 D3 33 A0 00 00 00
OHM 8                  70 CF 16 83 85 00 00 55 01 00 00 40 00 00 46 94 80 D3 33 A1 00 00 00
Sel_Cal 0..9           C0/70 CF 17 8B 20/80 01 00 55 31 00 00 00 ... 95 80 C3 33 20  (internal
                       reference checks: AD1 on VDS / AGNDP taps through the PA switches)
```

Observations: the ENOB tool's DC rows use R22 = 0x14 / 0x16 (OSR 4096 / 16384,
chopped), the AC rows R22 = 0x04 / 0x07 (AD1 unchopped) and R26 = 0x80 / R29 =
0xF8 (OSR2 32, LPFBW 111) like the DM40; the 60 mV / 50 mA rows set OPS1 (R25
bit 0) and SOP1P = PB<x> for the x10 op-amp stage; the capacitor RC rows use
R20 = 0x72 (SCMPI = PB<0>, ENCMP, ENCTR) with R21 = 0x21 / 0x42 (VDSC<11> /
VDSC<5> and VDSC<9> / VDSC<7> thresholds) and R2F = 0x9A / 0x8E (ENVS, SMODE =
0x1A / 0x0E, the "constant current" / "constant voltage" test modes named in
APD-DMM003 chapter 5); the ramp rows use R33 = 0x2C (SFT1 = 11) and R22 = 0xA4
(AD1OS = 101, DCOS = -0.25: the ramp is one-sided).

## 6. Digilent DMM Shield rows (dmm.cpp)

Same chip on a different board (ranges 50 mV .. 1000 V, 5 A, 50 ohm .. 50 M, no cap
/ Hz / temperature): R1F..R36 images and scale factors, e.g.
`R 50M: C0 CF 17 93 85 00 00 55 55 00 00 08 00 00 80 86 80 D1 3C A0 (6e7/0.9/2^23)`,
`DCV 50: 60 00 17 8B 01 11 00 55 31 00 22 00 00 09 28 A0 80 C7 33 20 (125/1.8/2^23)`,
`ACV 5: F2 DD 07 03 52 10 80 25 31 F8 22 00 00 D0 88 A0 FF C7 38 20 02 50 0C (1e-4
per sqrt(RMS))`, `Cont: 74 CF 17 83 35 10 ...`, `Diode: C0 CF 17 8B 8D 10 ...`.
They are the ENOB rows with R22 = 0x17 (OSR 32768, ~2.5 readings/s) and confirm
the DC scale `IG * VIN / VR * 2^23` with VR = 1.25 V (the "1.8" in Digilent's
constants is their AD1IG = 01 setting) and the AC scale `sqrt(RMS) * VR / 2^18 /
IG` with the ACV gain 0.5. Reset: 0x60 to R37; data window 0x00..0x1F read in one
transfer; INTF 0x04 = AD1 done, 0x10 = RMS done.

## 7. What the DM40 rows say about the board (for the model)

The DM40 netlist is not known; the stock formulas (dmm_v2_6.h) give the nominal
end-to-end scale of every row, and that is what the model reproduces per row
(see README.md, "Analog path per configuration row"). The facts used:

* DC V: AD1 on OP1O - RLU (r0, SENSE buffered by OP1) or SENSE - PB<5> (r1..r3),
  reference PB<6> - AGND = 1.25 V (REFO buffers PB<6>, SREFO = 1), `V = AD1 *
  k[r] * 1.25 / 0.9 / 2^23`, k = 10, 100, 1000, 10000. The divider taps follow
  the datasheet's 10M + 1.111M / 101.01k / 10.01k / 1k network (APD-DMM003 ch. 3).
* AC V: AD2 on PB<4> - AGND (AC coupled externally, time constant ~0.3 s, acdc doc
  6a), AD2IG = 0.5, RMS + LPF with LPFBW = 111, AD3 on the same node with SVXI =
  1 (RMS = sum(X*Y)/N), `Vrms = sqrt(RMS) * 1.25 / 2^18 * k[r]`.
* mV / A: OP1 from PB<0> (mV) / PB<1>, PB<2> (shunts) with the x10 stage (OPS1 =
  1, 10k / 90k) in the low range, x1 in the high range; DC on AD1 (OP1O - PB<3> /
  AGND), AC on AD2 (OP1O - PB<3>, AD2IG = 1.0, DC coupled: the LPF sees the full
  DC, acdc doc 6a).
* ohm: ratiometric, AD1 input OP1O - PB<3> (voltage across Rx buffered from PB<0>),
  reference FB - RLU (voltage across the reference resistor driven by OP3, VREF
  from SFUVR), `R = |AD1| * Rref[r] / 0.9 / 2^23`, Rref = 0.1 k (r0, unused by the
  stock), 1 k, 10 k, 100 k, 1 M, 10 M, 63 M. Continuity: same at 1 k with the
  reference order swapped (RLU / FB, hence the stock's minus sign), AD1 OSR 4096,
  comparator on OP1O against AGNDP<0> / AGNDN<2> (-41.7 mV) driving the CMP pin
  (ENPCMPO = 1): the beeper is in hardware.
* diode: OP3 source, SDIO = 1 (PB<0> / PB<8> 900k / 100k divider, x1/10),
  `V = AD1 * 10 * 1.25 / 0.9 / 2^23`, comparator thresholds R21 = 0xFF (always
  high), the stock beeps by toggling ENPCMPO (R20 0xB4 / 0xB0).
* capacitance r0..r4: RC relaxation oscillator (SMODE = 0x1A, R20 = 0x72: comparator
  on PB<0>, VDSC<11> / VDSC<5> thresholds, CMPO flips VREF), counted by the frequency
  counter with the 0x40000-cycle gate: `C = 0.72135 / (R[r] * f)`, R = 1020 k,
  1003 k, 102.7 k, 10.68 k, 1.5 k (0.72135 = 1 / (2 ln 2)); r5..r7: constant
  current ramp (SMODE = 0x0E, PA4..PA7 switches) read by AD1 through OP2 from
  PB<0>, slope `dV/dt = I[r] / C`, I = 4.06, 41, 400 uA referred to the AD1 node
  (the /10 divider is folded into the constant), comparator ACPO (CTSTA.6)
  reverses the ramp at the R21 thresholds (0xDC / 0xCC / 0xBB: AGNDP<1..3> =
  +-20.8 / 41.7 / 62.5 mV), the "discharge" row (0x0804a008) grounds the capacitor.
* frequency: comparator on PB<4> (the AC node) with AGNDP<1> / AGNDN<1> = +-20.8
  mV thresholds (R21 = 0xDD) or the CNT pin (R20 0x08, source unknown on the DM40),
  gate preset CTA = 0xFC0000 (0x40000 SYSCLK = 53.3 ms, reciprocal count), 350 ms
  restart period; AD2 RMS of the same node for the level.
* temperature: three AD1 readings per result with R33 SAD1I = 11, 10, 00 (OSR 16384,
  AD1IG = 3.6): `Tcj = (codeA - codeB) / 640000 * 314.15 - 289.15 + cal` (codeA =
  SAD1I 11, codeB = SAD1I 10), then the thermocouple mV on the DC mV x10 path
  (`Vtc = AD1 * 1.25 / 0.9 * 100 / 2^23 mV`) and the type K table. With the dump's
  cal 0.53 C this means `TCode = (codeA - codeB) / 2 = 1018.6 * (T + 273.15 + 15.5)`
  codes per kelvin at that gain: the stock's implied sensor offset (+15.5 K) is not
  the datasheet's TOS (-3.15 K); the model uses the stock's constants so that the
  emulated meter shows the scripted cold junction temperature.

## 8. Open points

1. LPF / RMS output period: measured 106.7 ms with LPFBW = 111 and OSR2 = 32,
   datasheet formula 213 ms. Either the RMS/LPF block runs at 2x the AD2 data
   rate or the OSR4 table is off by one for this setting. Modelled with the
   measured value.
2. AD2/AD3 enable order beat (acdc doc 10): the datasheet's SPHACAL note supports
   a phase mechanism (X and Y decimated with different phases so that sum(X*Y)
   deviates from sum(X^2)); the 2.04 Hz beat and the 13 us window structure are
   empirical. Modelled as an option (`beat`), off by default.
3. Temperature sensor constants (section 7): stock constants vs datasheet TOS.
4. The CNT pin source on the DM40 board (mode 14 alternates it with the comparator).
5. The exact DM40 PA / PB wiring (ohm reference resistors on PA2..PA7, divider
   taps, shunts): inferred from the stock scaling only.
6. Whether reserved register bits read back as written (the model keeps them,
   except the bits the datasheet marks "0").
