# HY3131 register map and mux tables (transcribed)

Transcribed from DS-HY3131-V10_EN chapters 8..14 and the field lists of Hycon's
`HY3131.MEM`; our own wording, no figures. Bit 7 first.

## Data registers (read only)

| addr | name | content |
|---|---|---|
| 00..02 | AD1 | AD1<23:0>, signed, LSB first (7FFFFFh .. 800000h) |
| 03..05 | AD2 | AD2<23:0>: 19-bit signed in <18:0>, <23:19> = sign copy; AD2 or AD3 data by SDO23 |
| 06..08 | LPF | LPF<23:0>, 19-bit signed as AD2 |
| 09..0D | RMS | RMS<39:0>, 38-bit signed in <37:0> |
| 0E..10 | PKHMIN | 19-bit signed, preset 3FFFFh when ENPKH = 0 |
| 11..13 | PKHMAX | 19-bit signed, preset 40000h when ENPKH = 0 |
| 14 | CTSTA | PCNTI.7 ACPO.6 CMPHO.5 CMPLO.4 -.3..1 CTBOV.0 (reading clears CTBOV) |
| 15..17 | CTC | SYSCLK count with the input high (duty) |
| 18..1A | CTB | input cycles |
| 1B..1D | CTA | gate counter; <23:8> writable while ENCTR = 0 |
| 1E | INTF | BORF.7 -.6 -.5 RMSF.4 LPFF.3 AD1F.2 AD2F.1 CTF.0; cleared by reading, 0s writable |
| 1F | INTE | -.7..5 RMSIE.4 LPFIE.3 AD1IE.2 AD2IE.1 CTIE.0 |

## Configuration registers

| addr | bit 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |
|---|---|---|---|---|---|---|---|---|
| 20 | SCMPI2 | SCMPI1 | SCMPI0 | ENCMP | ENCNTI | ENPCMPO | ENCTR | 0 |
| 21 | SCMPRH3 | SCMPRH2 | SCMPRH1 | SCMPRH0 | SCMPRL3 | SCMPRL2 | SCMPRL1 | SCMPRL0 |
| 22 | AD1OS2 | AD1OS1 | AD1OS0 | AD1CHOP1 | AD1CHOP0 | AD1OSR2 | AD1OSR1 | AD1OSR0 |
| 23 | ENAD1 | - | 0 | AD1RG | AD1RHBUF | AD1RLBUF | AD1IPBUF | AD1INBUF |
| 24 | SAD1FP3 | SAD1FP2 | SAD1FP1 | SAD1FP0 | SDIO | SAD1FN2 | SAD1FN1 | SAD1FN0 |
| 25 | AD2IG1 | AD2IG0 | AD1IG1 | AD1IG0 | SACM1 | SACM0 | OPS2 | OPS1 |
| 26 | ENAD2 | 0 | ENCHOPAD2 | AD2RG | SAD2CLK | AD2OSR2 | AD2OSR1 | AD2OSR0 |
| 27 | SAD2IP1 | SAD2IP0 | SAD2IN1 | SAD2IN0 | SAD2RH1 | SAD2RH0 | SAD2RL1 | SAD2RL0 |
| 28 | - | SAD1RH2 | SAD1RH1 | SAD1RH0 | - | SAD1RL2 | SAD1RL1 | SAD1RL0 |
| 29 | ENRMS | ENLPF | LPFBW2 | LPFBW1 | LPFBW0 | ENPKH | PKHSEL1 | PKHSEL0 |
| 2A | PS1 | DS1 | FS1 | SS1 | PS0 | DS0 | FS0 | SS0 |
| 2B | PS3 | DS3 | FS3 | SS3 | PS2 | DS2 | FS2 | SS2 |
| 2C | PS5 | DS5 | FS5 | SS5 | PS4 | DS4 | FS4 | SS4 |
| 2D | PS7 | DS7 | FS7 | SS7 | PS6 | DS6 | FS6 | SS6 |
| 2E | PS9 | DS9 | FS9 | SS9 | PS8 | DS8 | FS8 | SS8 |
| 2F | ENVS | SMODE6 | SMODE5 | SMODE4 | SMODE3 | SMODE2 | SMODE1 | SMODE0 |
| 30 | SREFO | ACC6 | ACC5 | ACC4 | ACC3 | ACC2 | ACC1 | ACC0 |
| 31 | ENREFO | ENBIAS | SAGND1 | SAGND0 | SFUVR3 | SFUVR2 | SFUVR1 | SFUVR0 |
| 32 | ENOP2 | SOP2P2 | SOP2P1 | SOP2P0 | ENOP1 | SOP1P2 | SOP1P1 | SOP1P0 |
| 33 | OP1CHOP1 | OP1CHOP0 | ENOSC | ENXI | SFT11 | SFT10 | SAD1I1 | SAD1I0 |
| 34 | ENAD3 | 0 | ENCHOPAD3 | AD3RG | SVXI | SDO23 | 0 | 0 |
| 35 | SAD3IP1 | SAD3IP0 | SAD3IN1 | SAD3IN0 | AD3IG1 | AD3IG0 | 0 | 0 |
| 36 | SPHACAL<7:0> (bit 7: AD2 leads AD3; <6:0> = delay in AD2CLK periods) |
| 37 | test mode: write 0x60 = chip reset, otherwise write 0 only |

## Mux option tables

| field | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|---|---|
| SAD1FP<3:0> (0..7) | SENSE | FB | RLU | OP1O | OP2O | VDDA | REFO | VREF |
| SAD1FP<3:0> (8..15) | PB0 | PB1 | PB2 | PB3 | PB4 | PB5 | PB6 | PB7 |
| SAD1FN<2:0> | SENSE | RLU | VSSA | AGND | PB2 | PB3 | PB4 | PB5 |
| SAD1I<1:0>: AD1IP | AD1FP (filter) | FB | TS1P | TS1N |
| SAD1I<1:0>: AD1IN | AD1FN (filter) | RLU | TS2N | TS2P |
| SAD1RH<2:0> | FB | REFO | VREF | PB6 | RLU | VDDA | AGND | - |
| SAD1RL<2:0> | RLU | AGND | PB3 | PB5 | FB | VSSA | VREF | - |
| SAD2IP<1:0>, SAD3IP | OP1O | OP2O | PB4 | PB7 |
| SAD2IN<1:0>, SAD3IN | RLU | AGND | PB3 | PB5 |
| SAD2RH<1:0> | FB | REFO | VREF | PB6 |
| SAD2RL<1:0> | RLU | AGND | PB3 | PB5 |
| SOP1P<2:0> | SENSE | FB | RLU | AGND | PB0 | PB1 | PB2 | PB8 |
| SOP2P<2:0> | SENSE | FB | RLU | AGND | PB0 | PB1 | PB2 | PB3 |
| SCMPI<2:0> | SENSE | FB | OP1O | PB0 | PB1 | RLD | PB3 | PB4 |
| SCMPRH<3:0> (0..7) | VDSC16 | VDSC13 | VDSC11 | VDSC10 | VDSC9 | VDSC8 | VDSC7 | PB7 |
| SCMPRH<3:0> (8..15) | AGNDP6 | AGNDP5 | AGNDP4 | AGNDP3 | AGNDP2 | AGNDP1 | AGNDP0 | AGNDN1 |
| SCMPRL<3:0> (0..7) | VDSC2 | VDSC5 | VDSC7 | VDSC8 | VDSC9 | VDSC10 | VDSC11 | PB6 |
| SCMPRL<3:0> (8..15) | AGNDN6 | AGNDN5 | AGNDN4 | AGNDN3 | AGNDN2 | AGNDN1 | AGNDP0 | AGNDP1 |
| SFUVR<3:0>, SMODE3 = 0 (VREF of OP3) | VDS17 | VDS16 | VDS15 | AGNDP9 | AGNDP8 | AGNDP7 | AGNDP6 | AGNDP0 |
| SFUVR<3:0> (8..15), SMODE3 = 0 | VDS1 | VDS2 | VDS3 | AGNDN9 | AGNDN8 | AGNDN7 | AGNDN6 | PB7 |
| SFUVR<3:0>, SMODE3 = 1, CMPO = 0 / 1 | 0: VDS17 / VDS1 | 1: VDS16 / VDS2 | 2: VDS15 / VDS3 | 3: AGNDP9 / AGNDN9 | 4: AGNDP8 / AGNDN8 | 5: AGNDP7 / AGNDN7 | 6: AGNDP6 / AGNDN6 | 7: AGNDP0 / PB7 |
| OP1CHOP<1:0> | off | 1 kHz | 2 kHz | on (DC) |
| SFT1<1:0> (pre-filter R) | 100 k | 10 k | 0 | open |
| AD1IG | 0.9 | 1.8 | 2.7 | 3.6 | | AD1RG 0 / 1 = 1.0 / 0.333 |
| AD2IG, AD3IG | 0.5 | 1.0 | 1.5 | 2.0 | | AD2RG, AD3RG 0 / 1 = 1.0 / 0.333 |
| AD1OS (DCOS) | 0 | +0.25 | +0.5 | +0.75 | 0 | -0.25 | -0.5 | -0.75 |
| AD1OSR (OSR1) | 256 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768 |
| AD2OSR (OSR2) | 32 | 64 | 128 | 256 | 512 | 1024 | 1024 | 1024 |
| LPFBW (OSR4) | 128 | 256 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 |
| PKHSEL | AD2 | AD1<23:5> | LPF | LPF |
| SAGND (K3 of VDDA) | 0.5 | 0.3 | 0.1 | floating |
| SACM (ACM - VSSA) | 1.2 V | 0.9 V | 1.5 V | 1.125 V |

SMODE<3:0> with CMPO -> SGND SVDD SVSS SVSO1 SVSO2 SCP SCN (power bus and OP3
switches): 0000: none; 0001: SGND; 0010: SVDD; 0011: SVSS; 0100: SVSO1; 0101:
SGND+SVSO1; 0110: SVDD+SCP+SCN (CMPO = x); 0111: SVSS+SVSO2+SCN; 1000 / CMPO 0:
none, CMPO 1: SGND; 1010 / 0: SVDD, 1: SVSS; 1100 / 0: SVSO1, 1: SGND+SVSO1;
1110 / 0: SVDD+SCP+SCN, 1: SVSS+SVSO2+SCN (from the switch-network figure; the
CMPO-dependent rows are the capacitor charge / discharge modes).

Divider nodes: VDSC<n> = VDS<n> = K1 * VDDA, K1 = 35/36 (17), 34/36 (16),
32/36 (15), 28/36 (14), 26/36 (13), 24/36 (12), 22/36 (11), 20/36 (10),
18/36 (9), 16/36 (8), 14/36 (7), 12/36 (6), 10/36 (5), 8/36 (4), 4/36 (3),
2/36 (2), 1/36 (1). AGNDP<n> = AGND + K2 * (REFO - AGND), AGNDN<n> = AGND - K2 *
(REFO - AGND), K2 = 100/120 (9), 80/120 (8), 60/120 (7), 40/120 (6), 20/120
(5), 10/120 (4), 6/120 (3), 4/120 (2), 2/120 (1), 0 (AGNDP<0>).
