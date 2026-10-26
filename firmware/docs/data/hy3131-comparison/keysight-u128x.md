# Keysight U1281A/U1282A firmware v1.05: decode and HY3131 configuration

Source: `bin/keysight-u128x/cab/u1281a_82a_v105.ag` (61,440 B), updater `cab/u128xa_firmware_update_softw`.
Static analysis only. The decoded image and the k0dasm listing are not in the repository (Keysight's code); the decode below reproduces the image from the official package.

## .ag format and decode

From the updater (VB6 native code, form method at 0x40FA80 = vtable +0x738, upload handler around 0x409453):

1. `Get #1` reads 0xF000 bytes into `buf(0xF001 - i)`, so the file is stored **byte-reversed**.
2. Each byte goes through `Val("&H" & Mid(h,2,1) & Mid(h,1,1))`: a **nibble swap**.
3. Before sending, a byte sum is taken (displayed as hex) and each byte is XORed with **0x9A**; that
   XORed stream goes over the serial link (9600 8N1, U1173 cable; `*IDN?`, `UPGR 1` ... `ACK`, `UPGR 0`).
   So the 0x9A XOR is transport only; the device undoes it.

Decode: `plain = bytes(((b<<4)|(b>>4)) & 0xFF for b in ag[::-1])`. 0x00/0xFF are fixed points of both
steps, which is why they looked "unchanged". The result has readable SCPI strings (`*IDN?`, `CONF?`,
`Keysight Technologies,`, `,V1.05`, `+0,No error`, `UACK 0..3`).

## MCU

NEC/Renesas **78K0** (8-bit), flash 0x0000..0xEFFF (60 KB; 0xF000+ is RAM/SFR on 78K0):
vector table at 0x0000 (reset 0x05CE), CALLT table 0x0040..0x007F, option byte 0x0080 = 0x7E,
on-chip-debug byte 0x0084 = 0x02; reset code starts `MOVW SP,#0FE20h`. Exact part (78K0/Lx2/Lx3 with
LCD, inferred) not identified. `k0dasm` (PyPI) disassembles it.

## AFE: HY3131 (evidence from code)

The SPI routines build the HY3131 command byte `addr<<1 | R/W` (write = bit0 clear):
- `sub_8ad8` (0x8AD8): single-register write, `cmd = (reg*2) & 0xFE`, then value; CS = port bit `0xFF01.7`
  (low = active), data register 0xFF84, busy flag `0xFF80.0` (chip-specific SFR names unknown).
- `sub_8d73` (0x8D73): full configuration burst. Buffer 0xF660 = `0x40` (write from R20), then 0xF661..0xF674
  = R20..R33 copied from a 20-byte row of the table at **0x18EA**, then mode/range overlays, then **22 bytes**
  sent (cmd + R20..R34; R34 = 0xF675, not set by this routine — inferred left as-is/0).
- Register-only writes seen elsewhere: R1F INTE = 0x0C/0x0D, R1B..R1D (CTA), R2D = 0x14, R32 = 0x3B/0x3D/0x05,
  R2F = 0x80/0x81/0x87, R26 = 0x9A/0x19, R29 = 0x60/0x64, R23 = 0x05, R33 = 0x0C (at 0xD493, with R26 = 0x19:
  looks like the power-down/idle set — inferred).

### Row selection (sub_8d73)

`h = mode (0xFE55)`; `h += 8` if `0xFE3D.1` (AC flag) and mode < 4; `h = 1` if mode = 0x0B; `h = 12` if `0xFE24.2`.
Range index = `0xFE85`.

Mode numbers vs function (inferred from the SCPI `CONF?` code at 0xCB27, which maps mode to the string table
at 0x1F2D, and from the rotary switch dispatch at 0x31E6, positions 0..9 in `0xFE80`):

| mode | SCPI class | dial pos. | notes |
|---|---|---|---|
| 0 | VOLT | 1, 3 | range overlay only for range 1 (R25 = 0x4A); likely mV (inferred) |
| 1 | VOLT | 0, 2 | 4 ranges, per-range divider switches from 0x19EE; likely V (inferred) |
| 2 | CURR | 7 | starts in range 1; `CPER` (0-20/4-20 mA) variant; ranges 2,4: R25 = 0x4A |
| 3 | CURR | 8 | |
| 4 | RES (CONT if 0xFE2E.7) | 4 | 9 ranges, R2A..R2E from 0x19FA; range >= 4: R23 = 0x8F |
| 5 | DIOD | 5 | |
| 6 | TEMP | (shift) | |
| 7 | CAP | 6 | 10 ranges, R2A/R2B from 0x1A27; range >= 6: R25 = 0x4B; range 8: R23 = 0x90 |
| 0x0B | (FREQ/square-wave position, inferred) | 9 | uses row 1 |
| 0x0C | invalid dial position | - | row 12 |

Dial positions 0..3 set the AC flag `0xFE3D.1` only for positions 0 and 1 (0xFE3D = 0x22 vs 0x21) — which
of these is AC is inferred from rows 8..11 enabling AD2+LPF.

### Table 0x18EA: 13 rows x 20 bytes (R20..R33)

| row | use | 20 21 22 23 24 25 26 27 28 29 2A 2B 2C 2D 2E 2F 30 31 32 33 |
|---|---|---|
| 0 | mode 0 DC | 74 bf 14 80 3b 9b 19 de 32 20 00 00 00 00 80 a1 c0 c0 3d 2c |
| 1 | mode 1 DC / 0x0B | 70 bf 14 80 39 9b 19 ce 32 20 00 d0 80 10 00 80 c0 c7 3d 2c |
| 2 | mode 2 | 74 dd 14 90 33 4b 19 de 32 20 b0 00 80 00 00 a1 c0 d0 3d 2c |
| 3 | mode 3 | 74 dd 14 90 33 4b 19 de 32 20 b0 00 80 00 00 a1 c0 d0 3d 2c |
| 4 | mode 4 Ω | 54 cf 14 80 3b 4b 19 de 00 20 60 00 00 00 04 94 c0 d2 3d a0 |
| 5 | mode 5 diode | 74 dd 14 90 33 4a 19 de 32 20 80 00 00 00 10 92 c0 e2 38 20 |
| 6 | mode 6 temp | 74 bf 14 80 3b 4a 19 de 32 20 00 00 00 00 00 a1 c0 d0 3d 2c |
| 7 | mode 7 cap | 54 22 14 10 33 4a 19 de 32 20 00 80 00 00 00 8e 40 d2 3d 2c |
| 8 | mode 0 AC | 74 cc 14 00 33 9b 99 de 32 60 00 00 00 00 80 a1 c0 c0 bd 2c |
| 9 | mode 1 AC | 74 cc 14 00 31 9b 99 de 32 60 00 d0 80 10 00 80 c0 c0 bd 2c |
| 10 | mode 2 AC | 74 dd 14 10 33 4b 99 de 32 60 b0 00 80 00 00 a1 c0 d0 bd 2c |
| 11 | mode 3 AC | 74 dd 14 10 33 4b 99 de 32 60 b0 00 80 00 00 a1 c0 d0 bd 2c |
| 12 | 0xFE24.2 | 74 cc 14 80 31 4b 99 de 32 60 90 00 80 10 00 80 c0 c0 be 2c |

Key fields:
- **R33**: `0x2C` in every row except Ω and diode: OP1CHOP = 00 (off), ENOSC = 1, **SFT1 = 11 (open)**, SAD1I = 0.
  Ω row `0xA0`: **OP1CHOP = 10 (2 kHz)**, SFT1 = 00 (100 k). Diode row `0x20`: OP1CHOP off, SFT1 = 00 (100 k).
- **R25 AD1IG**: 01 (x1.8) in mode 0/1 rows (0x9B), 00 (x0.9) elsewhere (0x4A/0x4B); overlays switch to 0x4A/0x4B per range.
- R22 = 0x14 everywhere: OSR1 = 4096, AD1CHOP = 2, DCOS = 0. R24 = FP OP1O, FN AGND (0x3x/0x33) or RLU (0x31/0x39).
- R26: DC rows 0x19 (AD2 off), AC rows 0x99 (ENAD2 = 1, AD2RG = 1, OSR2 = 64). R29: DC 0x20, AC 0x60 (ENLPF = 1,
  LPFBW 2048). ENRMS (bit 7) is 0 in every row and in every R29 write found (0x20/0x60/0x64, and `& 0xC7 | 0x08` in the burst): RMS is not taken from the HY3131 RMS block in the paths seen (inferred: computed in software from LPF/AD2).
- All four AC rows (8..11) have ENAD1 = 0 (R23 = 0x00 / 0x10).
- R31 = 0xC0..0xE2 (ENREFO, ENBIAS, SAGND 0.5/0.3/0.1). R2F = SMODE/ENVS per mode. R32: OP1 on, input PB1;
  AC rows also enable OP2 (0xBD/0xBE).

### Overlays (written into the buffer after the row copy)

- 0x19EE, 3 bytes per range (mode 1), into R2A R2B R2C: r0 `00 d0 80`, r1 `00 0d 80`, r2 `d0 00 80`, r3 `d0 00 80`;
  mode 1 range 3: R25 = 0x4B; range 1: R30 = 0xFF, R2D = 0x14.
- 0x19FA, 5 bytes per range (mode 4), into R2A..R2E: r0 `60 00 00 00 04`, r1 `60 00 00 00 00`, r2 `00 06 00 00 00`,
  r3 `00 60 00 00 00`, r4 `00 00 06 00 00`, r5..r8 `06 00 00 00 00`; ranges >= 1: R25 = 0x4A.
- 0x1A27, 2 bytes per range (mode 7), into R2A R2B: r0..3 `00 80`, r4 `00 08`, r5..8 `80 00`, r9 `00 32` (>= 9 entries; end of table not established).
- Flag-driven: R31 = 0xD0 if 0xFE56 = 0x0B; if 0xFE2A.0: R2F = 0x82, R31 = 0xD2, R32 = 0x3D (+ R20 = 0x5C if 0xFE2A.1);
  if 0xF434 = 1 and none of the hold flags: R29 = (R29 & 0xC7) | 0x08, R22 = 0x11 (OSR1 = 512); mode 1 range 0:
  R30 = 0x80 | calibrated value from 0x095D lookup; 0xFE39.0+.1: R21 = 0xBF/0xDD, R24 = 0x39/0x3B/0x33.

Full per-row field decode (generated with the [registers.md](../../../../emu/hy3131/ref/registers.md) tables; switch bits listed as `P/D/F/S<n>`):

```
### row 0 @0x18EA: 74 bf 14 80 3b 9b 19 de 32 20 00 00 00 00 80 a1 c0 c0 3d 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=bf
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=1
  R25 AD2IG=1.5 AD1IG=1.8 SACM=1.5 OPS=3
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: P9
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.5 SFUVR=0
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 1 @0x18FE: 70 bf 14 80 39 9b 19 ce 32 20 00 d0 80 10 00 80 c0 c7 3d 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=0 ENCTR=0 | R21=bf
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=RLU SDIO=1
  R25 AD2IG=1.5 AD1IG=1.8 SACM=1.5 OPS=3
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=RLU RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PDS3 P5 S7
  R2F ENVS=1 SMODE=0x00
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.5 SFUVR=7
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 2 @0x1912: 74 dd 14 90 33 4b 19 de 32 20 b0 00 80 00 00 a1 c0 d0 3d 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=dd
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PFS1 P5
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=0
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 3 @0x1926: 74 dd 14 90 33 4b 19 de 32 20 b0 00 80 00 00 a1 c0 d0 3d 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=dd
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PFS1 P5
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=0
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 4 @0x193A: 54 cf 14 80 3b 4b 19 de 00 20 60 00 00 00 04 94 c0 d2 3d a0
  R20 SCMPI=2 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=cf
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=1
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=FB RL=RLU
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: DF1 D8
  R2F ENVS=1 SMODE=0x14
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=2
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=2k ENOSC=1 ENXI=0 SFT1=100k SAD1I=0
### row 5 @0x194E: 74 dd 14 90 33 4a 19 de 32 20 80 00 00 00 10 92 c0 e2 38 20
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=dd
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=2
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: P1 S9
  R2F ENVS=1 SMODE=0x12
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.1 SFUVR=2
  R32 OP2=0:AGND OP1=1:SENSE
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=100k SAD1I=0
### row 6 @0x1962: 74 bf 14 80 3b 4a 19 de 32 20 00 00 00 00 00 a1 c0 d0 3d 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=bf
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=1
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=2
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: -
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=0
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 7 @0x1976: 54 22 14 10 33 4a 19 de 32 20 00 80 00 00 00 8e 40 d2 3d 2c
  R20 SCMPI=2 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=22
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=0 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=2
  R26 ENAD2=0 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=0 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: P3
  R2F ENVS=1 SMODE=0x0E
  R30 SREFO=0 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=2
  R32 OP2=0:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 8 @0x198A: 74 cc 14 00 33 9b 99 de 32 60 00 00 00 00 80 a1 c0 c0 bd 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=cc
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=0 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.5 AD1IG=1.8 SACM=1.5 OPS=3
  R26 ENAD2=1 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=1 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: P9
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.5 SFUVR=0
  R32 OP2=1:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 9 @0x199E: 74 cc 14 00 31 9b 99 de 32 60 00 d0 80 10 00 80 c0 c0 bd 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=cc
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=0 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=RLU SDIO=0
  R25 AD2IG=1.5 AD1IG=1.8 SACM=1.5 OPS=3
  R26 ENAD2=1 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=1 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PDS3 P5 S7
  R2F ENVS=1 SMODE=0x00
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.5 SFUVR=0
  R32 OP2=1:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 10 @0x19B2: 74 dd 14 10 33 4b 99 de 32 60 b0 00 80 00 00 a1 c0 d0 bd 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=dd
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=0 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=1 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=1 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PFS1 P5
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=0
  R32 OP2=1:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 11 @0x19C6: 74 dd 14 10 33 4b 99 de 32 60 b0 00 80 00 00 a1 c0 d0 bd 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=dd
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=0 AD1RG=1 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=AGND SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=1 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=1 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PFS1 P5
  R2F ENVS=1 SMODE=0x21
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.3 SFUVR=0
  R32 OP2=1:AGND OP1=1:PB1
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
### row 12 @0x19DA: 74 cc 14 80 31 4b 99 de 32 60 90 00 80 10 00 80 c0 c0 be 2c
  R20 SCMPI=3 ENCMP=1 ENCNTI=0 ENPCMPO=1 ENCTR=0 | R21=cc
  R22 OSR1=4096 CHOP=2 DCOS=0
  R23 ENAD1=1 AD1RG=0 buf[RH RL IP IN]=0000
  R24 FP=OP1O FN=RLU SDIO=0
  R25 AD2IG=1.0 AD1IG=0.9 SACM=1.5 OPS=3
  R26 ENAD2=1 CHOP2=0 AD2RG=1 SAD2CLK=1 OSR2=64
  R27 AD2 IP=PB7 IN=AGND RH=PB6 RL=PB3
  R28 AD1 RH=PB6 RL=PB3
  R29 ENRMS=0 ENLPF=1 LPFBW=2048 ENPKH=0 PKHSEL=0
  R2A-2E sw: PS1 P5 S7
  R2F ENVS=1 SMODE=0x00
  R30 SREFO=1 ACC=0x40
  R31 ENREFO=1 ENBIAS=1 SAGND=0.5 SFUVR=0
  R32 OP2=1:AGND OP1=1:PB2
  R33 OP1CHOP=off ENOSC=1 ENXI=0 SFT1=open SAD1I=0
```

## Inferred / not established

- Mode-to-function mapping and the AC/DC row split are inferred (CONF? string mapping + dial dispatch), not traced
  through the display code. mode 0 vs 1 = mV vs V is a guess (mode 1 has 4 ranges with divider switching).
- Exact 78K0 part and SPI peripheral names; R34..R36 values (R34 sent from 0xF675, not set in sub_8d73).
- HY3131 identity rests on the command-byte format and the register layout fitting R20..R34; the EEVblog #832
  thread mentions HY3131 in a forum post.
