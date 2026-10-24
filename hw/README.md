# DM40 hardware

What is inside the Alientek DM40 (DM40A / DM40B / DM40C), compiled from the
code and docs of this repo, the teardown photos in [photos/](photos/README.md)
and the mysku.club review they come from (CTAHOK, 2025-04-25,
https://mysku.club/blog/aliexpress/103047.html). Datasheets are in
[datasheets/](datasheets/README.md).

This page is about hardware only: chips, pins, circuits. Flash layout and boot
chain: see [fw-rs/docs/native-boot.md](../fw-rs/docs/native-boot.md) (and
[`fsloader-install-trigger.md`](../fw-rs/docs/fsloader-install-trigger.md), [`firmware/re/ANALYSIS.md`](../firmware/re/ANALYSIS.md)).

No schematic of the DM40 is public. Every claim below is marked:

- **verified**: seen in the stock firmware's code (RE headers in
  [`firmware/re/analysis/`](../firmware/re/analysis)), driven by fw-rs and checked on the device, or read
  off a photo. The pointer says where.
- **inferred**: deduced from register settings, scaling constants, a community
  BOM or the photos, not confirmed on the board.
- **unknown**: listed in [Unknowns](#unknowns).

Name clash: the HY3131 has its own analog pins called PA0..PA9 and PB0..PB8.
On this page they are always written "HY3131 PBn"; a bare "PBn" is an MCU pin.

Sources used most: [`fw-rs/src/board.rs`](../fw-rs/src/board.rs) and the other drivers in [`fw-rs/src/`](../fw-rs/src),
`firmware/re/analysis/*_v2_6.h`, [`fw-rs/docs/dac-adc-paths.md`](../fw-rs/docs/dac-adc-paths.md),
[`fw-rs/docs/power-meter-and-misconnect.md`](../fw-rs/docs/power-meter-and-misconnect.md), [`emu/hy3131/RESEARCH.md`](../emu/hy3131/RESEARCH.md),
[`docs/dm40-feature-request.md`](../docs/dm40-feature-request.md) 1.1 (community BOM of board V1.1, after
jebem in the [EEVblog DM40C thread](https://www.eevblog.com/forum/testgear/alientek-dm40c-multimeter-%28dm40a-dm40b-dm40c%29/), from the [mydigit.cn teardown](https://www.mydigit.cn/thread-486604-1-1.html), registration needed), [`fw-rs/docs/user-feedback.md`](../fw-rs/docs/user-feedback.md) (owner reports, cited by
forum post number).

## Contents

- [Block diagram](#block-diagram)
- [Main parts](#main-parts)
- [MCU and clocks](#mcu-and-clocks)
- [SPI flash (USB disk)](#spi-flash-usb-disk)
- [LCD](#lcd)
- [Touch panel and touch keys](#touch-panel-and-touch-keys)
- [Power key, PA2, buzzer](#power-key-pa2-buzzer)
- [Debug header and hardware recovery](#debug-header-and-hardware-recovery)
- [USB](#usb)
- [Bluetooth](#bluetooth)
- [DMM front end (HY3131)](#dmm-front-end-hy3131)
- [Oscilloscope](#oscilloscope)
- [Generator](#generator)
- [Power and battery](#power-and-battery)
- [Pin table](#pin-table)
- [Models DM40A / B / C](#models-dm40a--b--c)
- [Unknowns](#unknowns)

## Block diagram

The isolation boundary is **inferred** (BOM + photos: the CA-IS3741 isolator
and two VPS8504B / LT61 transformer supplies sit between the MCU area and the
shielded DMM area; see [docs/dm40-feature-request.md](../docs/dm40-feature-request.md) 1.2).

```
 USB-C ──TVS──┬── TP4056 ── Li-po 1S 2000 mAh (LN425272, protection PCB)
  D+/D- │     │                │
        │     └────────────────┴── system supply (regulators not identified)
        │                                   │
        │   ┌───────────────── MCU domain (battery / USB ground) ──────────────────┐
        └──►│ GD32F427VGT6  200 MHz ── 25 MHz crystal                              │
            │   FSMC 16 bit ──► ILI9488 3.5" IPS 480x320 (backlight PWM PA6)      │
            │   I2C2 ◄──────── capacitive touch (Hynitron 0x5A) + 4 touch keys    │
            │   SPI1 ◄───────► W25Q128JV 16 MiB (USB disk)                        │
            │   UART4 ◄──────► Jieli BT (BP2A838) ── 24 MHz, PCB antenna          │
            │   TIM14 PA7 ───► buzzer                                             │
            │   PB0..PB9 ◄──── 3PA1030 10 bit ADC ◄─ gain (4051 + op amp) ◄─┐     │
            │   PC6 50 MHz ──► ADC clock                 /50 relay PC13 ◄───┤ CHA │
            │   DAC0 PA4 ───┐                            AC/DC PE3, offset   │ MCX │
            │   TIM12 PB14 ─┴─ SGM3157 (PB12) ─ output stage ─────────────► GEN MCX│
            │   PE2 / PC4 rails;  PA9/PA10, PE0/PE1 drive; SPI2 PC10..12 + PD2 │   │
            └─────────────┬──────────────────────────────────────────────────────┘
                          │ CA-IS3741 digital isolator (SPI), optocouplers
                          │ (EL3H7, "174 F163H"), VPS8504B + LT61-117 / LT61-113
            ┌─────────────┴────────── isolated DMM domain (shielded) ──────────────┐
            │ HY3131 ── 4.9152 MHz crystal ── 1.25 V reference (MAX6190, BOM)     │
            │   divider 10 M + taps (TC1746B6 on B/C, discrete on A)              │
            │   HFD4/3-LS latching relay ── V/ohm input network (PTC, MOVs)       │
            │   shunt chain 10 mohm + 0.5 ohm + 47.5 ohm, MOSFETs RUH30J51M       │
            │   fuse ── jacks  V/ohm   COM   A/mA/uA                              │
            └──────────────────────────────────────────────────────────────────────┘
```

## Main parts

| Function | Part | Status / source |
|---|---|---|
| MCU | GigaDevice GD32F427VGT6, Cortex-M4F, LQFP100 | verified: marking `GD32F427 VGT6` in photo `main-board-mcu-gd32f427.jpg`; review |
| MCU crystal | 25.000 MHz | verified: photo (`main-board-mcu-gd32f427.jpg`, crystal next to the SPI flash) and PLL read back on the device ([`fw-rs/src/clock.rs`](../fw-rs/src/clock.rs)). The community BOM says 26 MHz (contradiction, see [Unknowns](#unknowns)) |
| SPI flash | Winbond W25Q128JV, 16 MiB | verified in code ([`fw-rs/src/spiflash.rs`](../fw-rs/src/spiflash.rs), JEDEC check at boot); BOM |
| LCD controller | ILI9488 (ID read 0x9488) | verified on the device (ID 0x9488 read by `board::init`, [`fw-rs/TODO.md`](../fw-rs/TODO.md) T1); stock init sequence `lcd_buzzer_v2_6.h` |
| LCD panel | 3.5" IPS, 480 x 320, active area 74 x 50 mm | review |
| Touch | Hynitron CST-type controller at I2C 0x5A (chip type byte 0xA8) on this unit; the stock also supports a Goodix GT1158 at 0x14 | verified on the device ([`fw-rs/src/touch.rs`](../fw-rs/src/touch.rs)); GT1158 path from the stock code only |
| Bluetooth | Jieli BP2A838-20A2, 24 MHz crystal, PCB meander antenna | BOM; antenna and 24.000 MHz crystal next to a chip with the Jieli logo in `main-board-mcu-gd32f427.jpg` (marking not readable) |
| DMM front end | Hycon HY3131 | verified in code (register protocol, `hy3131_v2_6.h`); the chip itself is under the shield can (4.9152 MHz crystal and VDD/VSS silkscreen next to it in `dmm-isolation-hy3131-shield-area.jpg`) |
| HY3131 crystal | 4.9152 MHz | verified: photo and stock constant 4915.2 (`hy3131_v2_6.h`) |
| Voltage reference | MAX6190 (1.25 V) on HY3131 PB6 | BOM; 1.25 V reference voltage verified by the stock scaling ([`emu/hy3131/RESEARCH.md`](../emu/hy3131/RESEARCH.md) 7) |
| Divider network | TC1746B6 (B / C); discrete resistors on the A | BOM; owner reports #25087, #51830 ([`fw-rs/docs/user-feedback.md`](../fw-rs/docs/user-feedback.md) 12) |
| Digital isolator | Chipanalog CA-IS3741HW | verified marking: photos `dmm-isolation-hy3131-shield-area.jpg`, `main-board-mcu-gd32f427.jpg` |
| Isolated supplies | 2 x VPS8504B push-pull driver + transformers LT61-117 and LT61-113 | verified markings: photos `isolated-supply-vps8504b-lt61.jpg`, `dmm-isolation-hy3131-shield-area.jpg` |
| Optocouplers | 2 x EL3H7, 2 x "174 F163H" | BOM; the "174 F163H" (4-pin) next to the relay and the EL3H7 (marked "D424 EL3H7") are in `dmm-isolation-hy3131-shield-area.jpg` |
| DMM relay | Hongfa HFD4/3-LS (3 V, latching per BOM) | verified marking: photos `input-jacks-shunts-fuse-relay.jpg`, `relay-hfd4-and-varistor-closeup.jpg`; review |
| Input protection | varistors 07D681K (680 V), blue disc parts, 2 W wirewound/film resistors; FMMT619 pair (owner report #41820) | photos (`relay-hfd4-and-varistor-closeup.jpg`, `input-jacks-shunts-fuse-relay.jpg`); which disc is a PTC and which a MOV is inferred from the markings only |
| Current shunts | 10 mohm (wire shunt), 0.5 ohm (SMD "R500"), 47.5 ohm | 0.5 ohm verified marking in `input-jacks-shunts-fuse-relay.jpg`; values from the stock scaling and fw-rs checks (below) |
| Current range MOSFETs | RUH30J51M dual N-MOSFET, 30 V | BOM |
| Fuse | ceramic cartridge in clips, next to the A jack | photo `input-jacks-shunts-fuse-relay.jpg` and `opened-case-overview.jpg`; rating not readable, unknown |
| Scope ADC | 3PEAK 3PA1030, 10 bit, 50 MSPS, parallel | BOM; 10-bit parallel bus on PB0..PB9 verified in code ([`fw-rs/src/scope.rs`](../fw-rs/src/scope.rs)) |
| Scope gain mux | HC4051 (header says "HCF4051") | BOM, `scope_gen_v2_6.h` |
| Scope op amp | "8091" (Gainsil GS8091?) | BOM, unconfirmed |
| Generator path switch | SGM3157 SPDT | owner report (#24902: killed by 12 V); [`fw-rs/docs/dac-adc-paths.md`](../fw-rs/docs/dac-adc-paths.md) 1 |
| Charger | TP4056 (marked "TP4056 2413"), linear, 1 cell | verified marking: photo `charger-tp4056-and-power.jpg`; review |
| Battery | LN425272 Li-ion polymer, 3.7 V, 2000 mAh, 7.4 Wh, 4.2 V charge limit, made 2025-02-10 by 东莞市立能电子科技有限公司, with protection PCB | verified: label in `battery-ln425272-label.jpg` |
| Connectors | USB-C; two coaxial (MCX per BOM) jacks silkscreened CHA (scope) and GEN (generator); banana V/ohm, COM, A/mA/uA | photos; review ("coaxial, not BNC") |
| Debug header | two rows of 4 holes: "VCC RX TX GND" (TX = PA2, the boot strap) and "V D C G" (SWD, inferred) | see [Debug header and hardware recovery](#debug-header-and-hardware-recovery). SWD read-out is locked (read protection set, [`firmware/re/ANALYSIS.md`](../firmware/re/ANALYSIS.md) 1) |

Small SOT-23 / SOT-89 / MSOP parts in `charger-tp4056-and-power.jpg`
("A2SHB", "93N44", "XFHVB", "K14", "S1K", "NE.", inductor "2R2", tantalum
"J227") are not identified; they make up the power path and regulators.

## MCU and clocks

- GD32F427VGT6: Cortex-M4F, 1 MB internal flash, 256 KB SRAM as 64 KB TCM
  (0x10000000) + 128 KB SRAM0/1 (0x20000000) + 64 KB ADDSRAM (0x20020000).
  verified ([`firmware/re/ANALYSIS.md`](../firmware/re/ANALYSIS.md) 4: dump sizes and stack top).
- HXTAL 25 MHz, PLL M=25 N=400 P=2 -> 200 MHz SYSCLK / HCLK; APB1 50 MHz,
  timer kernel clocks 200 MHz (TIMPRE). verified: RCU_PLL read back on the
  device ([`fw-rs/src/clock.rs`](../fw-rs/src/clock.rs), [`fw-rs/src/genplan.rs`](../fw-rs/src/genplan.rs)).
- USB 48 MHz: the stock PLLQ gives 44.4 MHz; fw-rs runs PLLSAI (P=6 -> 48 MHz)
  for USBFS. verified ([`fw-rs/src/usb.rs`](../fw-rs/src/usb.rs)).
- RTC / LSE: not traced here.

## SPI flash (USB disk)

W25Q128JV, 16 MiB, on SPI1 (GD32 naming; base 0x40003800): SCK PD3, MISO PC2,
MOSI PC3 (AF5), CS PE4 (GPIO, active low), mode 3. The stock runs it at
3.125 MHz, fw-rs at 12.5 MHz after a boot self-check. The whole chip is one
FAT16 volume with 4096-byte sectors: the USB disk of the meter. verified
([`fw-rs/src/spiflash.rs`](../fw-rs/src/spiflash.rs), `spiflash_v2_6.h`). The FS-loader also uses it for
updates.

## LCD

- ILI9488, 16-bit 8080 bus on FSMC bank 1 NE1: D0..D15 on PD0 PD1 PD8 PD9
  PD10 PD14 PD15 PE7..PE15, NOE PD4, NWE PD5, NE1 PD7, RS = A16 on PD11
  (command 0x6001FFFE, data 0x60020000). No reset GPIO, no DMA. verified
  ([`fw-rs/src/board.rs`](../fw-rs/src/board.rs), [`fw-rs/src/lcd.rs`](../fw-rs/src/lcd.rs), `lcd_buzzer_v2_6.h`).
- FSMC timing as the stock: BCR1 0x5091, BTR1 0x0F04, BWTR1 0x0FF00201
  (device read-back, [`fw-rs/TODO.md`](../fw-rs/TODO.md) T1).
- Pixel format RGB565 on writes; reads return RGB666 (`lcd.rs`).
- Backlight: PA6 AF9 = TIM13_CH1, 20 kHz PWM. verified (`board.rs`).
- Panel: 3.5" IPS 480 x 320 (review). The panel's FPC pinout is unknown.

## Touch panel and touch keys

- I2C2 (GD32 naming; STM32 "I2C3", base 0x40005C00): SCL PA8, SDA PC9 (AF4),
  INT PE5 (falling edge), reset PE6. verified ([`fw-rs/src/touch.rs`](../fw-rs/src/touch.rs),
  `touch_v2_6.h`).
- This unit: Hynitron CST-type at 7-bit 0x5A, chip type byte 0xA8. The stock
  probes a Goodix GT1158 at 0x14 first (absent here), so other units may carry
  it. verified on the device / stock code.
- The four touch keys below the screen (review: Menu, Start/Stop, Auto,
  manual range) are reported by the touch controller itself as key codes 0..3,
  not by GPIOs. verified (`touch.rs`, measured X 372/628/884/1140).
- Pull-up strength unknown; fw-rs measures the rise time before going to
  400 kHz.

## Power key, PA2, buzzer

- **Power key PD6**: the one physical button (side). Input with pull-down,
  pressed = 1. verified ([`fw-rs/src/boot.rs`](../fw-rs/src/boot.rs) `PD6_PRESSED_HIGH`, stock
  FUN_0800b7e0). How the key also switches the unit on from off (it must reach
  the power path) is unknown.
- **PA2**: the "TX" pad of the debug header (owner, verified on the device
  2026-10-24). The Alientek primary bootloader and the FS-loader enable a
  pull-up on PA2 and poll it ~1000 times each at boot; low for the whole poll
  sends the boot to the FS-loader's install state (verified in their code,
  [`fw-rs/docs/fsloader-install-trigger.md`](../fw-rs/docs/fsloader-install-trigger.md) 1). PA2 is not the power key.
  See [Debug header and hardware recovery](#debug-header-and-hardware-recovery).
- **Buzzer**: PA7 AF9 = TIM14_CH1 PWM,
  200 MHz / 5 / 11200 = 3.57 kHz, loudness = duty. verified
  (`lcd_buzzer_v2_6.h`, fw-rs `app.rs`). The continuity / diode beep is not
  made by the MCU: the stock only toggles the HY3131 CMP pin buffer (ENPCMPO),
  so the HY3131 comparator output drives the buzzer stage (inferred; the
  path is not traced; [`fw-rs/docs/parity.md`](../fw-rs/docs/parity.md)). The only buzzer-like part in
  the photos is a magnetic buzzer in the DMM area next to the HY3131 can
  (`dmm-isolation-hy3131-shield-area.jpg`); how PA7 reaches it across the
  isolation is unknown.

## Debug header and hardware recovery

**Where.** At the end of the main board opposite the banana jacks, next to
the Bluetooth antenna and the 24 MHz crystal: two rows of four through-holes,
unpopulated, silkscreened. Seen with the board as in
`photos/opened-case-overview.jpg` (jacks at the top), left to right:

| Group | Pads, left to right | Square pad |
|---|---|---|
| left | G, C, D, V | V |
| right | GND, TX, RX, VCC | VCC |

![Debug header: TX (PA2) and GND marked](photos/debug-header-annotated.jpg)

*A marked-up crop of [main-board-mcu-gd32f427.jpg](photos/main-board-mcu-gd32f427.jpg)
(photo © CTAHOK, mysku.club, see [photos/README.md](photos/README.md); the
original is kept unchanged). The silkscreen is readable in both review photos.*

| Pad | Signal | Status |
|---|---|---|
| TX | **PA2** (pull-up enabled by the bootloader and the FS-loader) | verified by the owner on the device, 2026-10-24 |
| GND | ground, next to TX | silkscreen; used for the short, so verified in practice |
| RX | probably PA3 (USART1_RX on the GD32F4; PA3 is unused by the firmware) | unverified |
| VCC | supply (3.3 V?) | unverified |
| V, D, C, G | SWD: VCC, SWDIO (PA13), SWCLK (PA14), GND | inferred from the labels, unverified |

PA2's alternate function on the GD32F4 is USART1_TX, so the pad is labelled
as a UART TX. The stock firmware never enables USART1 (as far as the RE notes
show), and neither does fw-rs: in practice the pad is only the boot strap.
PA2 reads low only when it is shorted to GND, or possibly when something
connected to the header loads it. Earlier observations that some cold
power-ons went to the loader without a short (PA2 low for a while after a
cold power-on) remain unexplained ([`fw-rs/TODO.md`](../fw-rs/TODO.md),
[`fw-rs/docs/fsloader-install-trigger.md`](../fw-rs/docs/fsloader-install-trigger.md) 6).

### Hardware recovery

This is the canonical procedure; the other docs link here. It works whatever
the application does, because the decision is taken by the Alientek
bootloader and the FS-loader before the application runs.

1. Switch the meter off. Open the case (back cover) to reach the header.
2. Short the **TX** pad (PA2) to the neighbouring **GND** pad (a tweezers or a
   wire), then switch on with the power key.
3. Keep the short until the update screen appears ("open file error" and the
   FS-loader's own USB disk on the PC).
4. Copy an update file onto that disk under a `dm40*.bin` name: the official
   `dm40_v_2_6.bin` from [`firmware/vendor/`](../firmware/vendor) in this repo (back to stock) or a fw-rs image (e.g. `dm40_fwrs.bin`),
   then press "update" on the screen.
5. Remove the short.

Verified by the owner: the short enters the update mode. Not verified yet:
the exact timing, i.e. whether the short must be in place at power-on
(as written above) or also works when applied during a reset, and how long
it must be held. Mind the isolation: the header is on the MCU / USB ground,
not on the DMM side; keep the probe leads disconnected while the case is open.

Software ways to the same screen while fw-rs runs (`DBG:UPDATE`, the key-hold
recovery, the double-hang rule) are in [fw-rs/README.md](../fw-rs/README.md)
"Recovery".

## USB

USB-C, USBFS only: DM PA11, DP PA12 (AF10). No VBUS sensing (PA9/PA10, the
usual VBUS/ID pins, drive the relay). verified ([`fw-rs/src/usb.rs`](../fw-rs/src/usb.rs)). The USB
port also charges the battery (TP4056); a TVS on VBUS failed at 21 V in one
owner report (#36290). USB-PD trigger chargers work (owner report). Data and
the DMM are galvanically separate only if the inferred isolation holds.

## Bluetooth

- Module / chip: Jieli BP2A838-20A2 (BOM; marking not readable in the review
  photo), own 24 MHz crystal, PCB meander antenna at the board edge
  (`main-board-mcu-gd32f427.jpg`). Classic SPP (RFCOMM channel 1,
  [`docs/bt-protocol.md`](../docs/bt-protocol.md)) and BLE ([`fw-rs/src/bt.rs`](../fw-rs/src/bt.rs)).
- UART4: TX PA0, RX PA1 (AF8), 115200 8N1, no RTS/CTS. verified (`bt.rs`).
- PC0 = module power, **low = on**, high = off. verified (`bt.rs`,
  `bt_meas_v2_6.h`; `board.rs` sets it high at boot = off, as the stock).
- PC14 = link status input, high = connected. verified (`power.rs`, `bt.rs`).
- AT commands the firmware sends: `AT+SBN<name>`, `AT+SLED0`, `AT+RESET`,
  `AT+CBN` (`bt.rs`, [`docs/bt-protocol.md`](../docs/bt-protocol.md)).

## DMM front end (HY3131)

Overview of the chip: [emu/hy3131/RESEARCH.md](../emu/hy3131/RESEARCH.md) 2
and the datasheet in [datasheets/](datasheets/README.md). The 60 stock
configuration rows (R20..R35 per mode and range) and their decoding are in
[`firmware/re/analysis/hy3131_v2_6.h`](../firmware/re/analysis/hy3131_v2_6.h); the terminal-level model is
[emu/hy3131/README.md](../emu/hy3131/README.md).

**Bus.** SCK PC10, MISO PC11, MOSI PC12, CS PD2 (active low), mode 0. The stock
bit-bangs it; fw-rs uses SPI2 (AF6) at 3.125 MHz. verified on the device
([`fw-rs/src/hy3131.rs`](../fw-rs/src/hy3131.rs), [`fw-rs/docs/dmm-input-scope-feasibility.md`](../fw-rs/docs/dmm-input-scope-feasibility.md) 8). The
signals cross the CA-IS3741 isolator (inferred: it has 3 forward and 1
reverse channel, which fits SCK, MOSI, CS out and MISO back; datasheet
`chipanalog-ca-is374x-v1.08.pdf`). The HY3131 IRQ output is not used.

**Converters** (datasheet facts, `RESEARCH.md` 2; SYSCLK = 4.9152 MHz):

| Block | What | Rates on the DM40 |
|---|---|---|
| AD1 | 24-bit sigma-delta, IG 0.9..3.6, reference PB6 (1.25 V) or internal | AD1CLK = SYSCLK / 30; OSR 256..32768. Stock steady 32768, moving 8192 ([`fw-rs/src/dmm.rs`](../fw-rs/src/dmm.rs)) |
| AD2 / AD3 | 19-bit fast converters, shared clock and OSR | 76.8 kS/s in the stock AC rows (SYSCLK / 2 / 32); ladder down to 600 S/s (`dmm-input-scope-feasibility.md` 1.1) |
| RMS / LPF | digital true RMS (sum X^2 or X*Y) and low-pass of AD2 | ~106.7 ms per result measured with LPFBW 111 (half the datasheet formula, `RESEARCH.md` 8) |
| Peak hold | on AD2, AD1<23:5> or LPF | used by fw-rs's A-jack probe ([`fw-rs/src/sentinel.rs`](../fw-rs/src/sentinel.rs)) |
| Comparator | window comparator, thresholds from R21; output CMPO / CMP pin | continuity beeper, capacitance ramp, frequency, fw-rs sentinels |
| Counter | CTA / CTB / CTC, gate preset 0xFC0000 = 0x40000 SYSCLK = 53.3 ms | frequency (reciprocal), capacitance (RC oscillator) |
| Op amps | OP1, OP2 (2 MHz), OP3 (ohm / diode source) | x1 / x10 gain stages on the shunt and mV paths |

AD1 on a periodic input aliases (the "AD1 beat"): see `fw-rs/docs/dc-aliasing.md` (branch `dcalias`, not merged yet)
(on branch `dcalias`, not on `rust-fw-poc` yet). AC bandwidth is about 20 kHz (-3 dB, owner reports #24765,
#16732); the AC V path has an external coupling capacitor with a time constant
of ~0.3 s ([`fw-rs/docs/acdc-approaches.md`](../fw-rs/docs/acdc-approaches.md), `acdc-stock-vs-fwrs.md`).

**MCU GPIOs on the DMM side** (the complete list the stock drives, verified,
`hy3131_v2_6.h` lines 14-18, [`fw-rs/src/dmm.rs`](../fw-rs/src/dmm.rs) `relay_pa` / `relay_pe`):

| MCU pin | Function | Notes |
|---|---|---|
| PA9 / PA10 | HFD4/3-LS latching relay, 10 ms pulse. Arg 0: PA10 = 1, PA9 = 0; arg 1: PA10 = 0, PA9 = 1 | V/A path vs the ohm / mV network. One latching relay with a single coil driven in either polarity is inferred from the drive pattern (one relay in the photos; the BOM says DPDT, 3 V, latching); the drive crosses the isolation (optocouplers, inferred) |
| PE0 / PE1 | shunt MOSFETs: PE0 = 1 -> A (10 mohm), PE1 = 1 -> mA (0.5 ohm), both 0 -> uA (47.5 ohm chain) | the resting state in every non-current mode is both 0 (`power-meter-and-misconnect.md` 4.1) |
| PC7 | 1 only in the frequency mode (mode 14) | what it switches is unknown |

**Input network (HY3131 pins, inferred from the stock rows and scaling):**

- V/ohm input through a 10 M divider with taps: /10 on HY3131 PA7, /100 PA6,
  /1000 PA5, /10000 PA4 + PA5; the buffered input appears as SENSE
  (`power-meter-and-misconnect.md` 3.1, `RESEARCH.md` 7). The taps follow the
  datasheet's 10 M / 1.111 M / 101.01 k / 10.01 k / 1 k network.
- Ohm: OP3 drives reference resistors on HY3131 PA2..PA7 (Rref 1 k .. 63 M per
  range); ratiometric AD1 (`hy3131_v2_6.h` mode 10). Ohm source current about
  0.85 mA short circuit, diode test 0.75 mA (owner reports, [`emu/hy3131/README.md`](../emu/hy3131/README.md)).
- Diode: OP3 source with the 900 k / 100 k divider on HY3131 PB0 / PB8.
- Capacitance: RC relaxation (R = 1020 k .. 1.5 k) or constant-current ramp
  switched on HY3131 PA4..PA7.
- mV / thermocouple: HY3131 PB0 through OP1 x10. Cold junction = the HY3131
  die sensor.
- AC V: OP1 output -> external coupling capacitor -> HY3131 PB4 -> AD2.

**Current path.** One A jack for A / mA / uA (silkscreen "A/mA/uA"); the
MOSFETs switch the range, not the user (owner report #26977). Series chain
from the A jack: fuse -> 10 mohm -> 0.5 ohm -> 47.5 ohm -> COM, with PE0 / PE1
MOSFETs shorting the lower parts (inferred from the mode table;
`sentinel.rs` fuse check, `power-meter-and-misconnect.md` 4.1 / 4.8).
Measurement nodes: HY3131 PB2 = top of the 10 mohm element (every A-jack
current passes it), PB1 = top of the 47.5 ohm resistor, PB3 = the low side
(inferred). Shunt values: 0.5 ohm verified by the "R500" marking; 10 mohm and
47.5 ohm from the stock scaling and the fw-rs fuse check, which reads ~48 ohm
A-COM through the whole chain on the device (`sentinel.rs`, verified
behaviour, values inferred). The 10 mohm element is very likely the bent
copper-coloured wire bridge between the jacks in
`input-jacks-shunts-fuse-relay.jpg` (inferred).

| Range | Shunt | Burden at full scale (inferred) |
|---|---|---|
| 10 A | 10 mohm | 0.2..0.5 V incl. fuse and MOSFET |
| mA | 0.5 ohm | 0.24..0.6 V |
| uA | 47.5 ohm | ~0.19 V |

The 47.5 ohm resistor is assumed a 2512 / ~1 W part (owner, not measured).

**No jack sensing.** The DM40 has no jack sense switches or pins: the stock
DMM GPIO list is the one above. fw-rs detects leads in the wrong jack only
through the shunt node (HY3131 PB1 comparator) and the divider tap
(`sentinel.rs`, `power-meter-and-misconnect.md` 4.1). A blown fuse cannot be
seen by the meter (the A side of the fuse has no path into the chip); fw-rs's
"Fuse check" measures A-COM in ohm mode instead.

## Oscilloscope

- ADC: 3PA1030, 10 bit, data on PB0..PB9 read as GPIOB IDR by DMA1, clocked by
  TIM8_CH1 on PC6 (AF3) at 50 MHz. verified ([`fw-rs/src/scope.rs`](../fw-rs/src/scope.rs)).
- Sample rates: 50 MS/s down to 50 S/s (timer-paced DMA from the 50 MHz
  stream). The ADC never runs faster than 50 MS/s; at 100 and 200 ns/div the
  display upsamples. Record 26 K samples. verified (`scope.rs` TB table). Spec 10 MHz bandwidth,
  10 mV..10 V/div (x1), 100 ns..50 s/div (review); owners measure flat to
  ~3 MHz and -3 dB at 11.5..17 MHz, input capacitance ~30..35 pF
  (`user-feedback.md` 6.3).
- Front end (pin use verified, meaning inferred from the gain steps,
  `scope_gen_v2_6.h`, `dac-adc-paths.md` 3):
  - PC13: input attenuator relay, 1 = direct, 0 = /50; 200 ms settle.
  - PB15 / PD12 / PD13: 4051 select code, gain x0.5 .. x20 in the amplifier
    feedback (which pin is A / B / C is unknown).
  - PE3: coupling, 1 = DC, 0 = AC.
  - PC8 = TIM3_CH3 (AF2) PWM ~10 kHz: vertical offset.
  - Trigger (stock only): DAC channel 2 on PA5 sets the level of an external
    comparator (chip not identified) whose output goes to PA15 = TIM2_CH1
    (AF1); the output is inverted and ~2 us late ([`fw-rs/src/hwtrig.rs`](../fw-rs/src/hwtrig.rs)).
- Supply: PC4 rail (below). Ground: shared with USB and the MCU (inferred).
- Connector: coaxial "CHA" (MCX per BOM); probe x1/x10 with compensation
  trimmer.

## Generator

- DAC path: DAC channel 1 (`DAC_OUT0`) on PA4, paced by TIM6 TRGO,
  DMA0 stream 5. Up to 10 MS/s (verified rate). Output 0..3 V unipolar (DAC
  reference 3.0 V, assumed in code); the stock turns the DAC output buffer
  off for sine / triangle / ramp, so an external buffer stage follows
  (inferred). verified ([`fw-rs/src/gen.rs`](../fw-rs/src/gen.rs), `scope_gen_v2_6.h`).
- PWM path (square above 50 kHz): TIM12_CH1 on PB14 (AF9), 200 MHz / D.
- PB12 selects the path: 1 = PWM, 0 = DAC; the switch is very likely the
  SGM3157 (owner report). No attenuator or relay after it.
- PA4 can be read back by ADC0 channel 4 (`DIAG:DAC?`, verified on the device).
- Connector: coaxial "GEN". No protection: 12 V on the output killed the
  switch in one owner report (#24902).
- Spec (review): sine / triangle / sawtooth 1 Hz..50 kHz, square up to 10 MHz,
  0.5..3 V.

## Power and battery

- Battery: 1-cell Li-ion polymer LN425272, 2000 mAh, with protection PCB,
  2-pin connector "VBAT" (photo). Runtime 10..15 h (owner reports; review:
  12 h DMM / 10 h scope).
- Charger: TP4056, linear, from USB VBUS. Charging takes ~2 h 35 min (owner
  report #16625). Status: PB10 / PB11 inputs (meaning guessed; the two RE
  headers disagree, see [Unknowns](#unknowns)).
- Battery voltage: PC5 = ADC0 channel 15, 1:2 divider, 3.0 V reference
  assumed: Vbat = raw * 3.0 / 4096 * 2. Firmware powers off below ~3.31 V.
  verified as code ([`fw-rs/src/power.rs`](../fw-rs/src/power.rs), `dmm_v2_6.h`); the divider ratio is
  inferred from the scaling.
- Rails switched by the MCU (verified pin use, `board.rs`, `scope.rs` `rails`,
  `boot.rs`):
  - **PE2** = DMM analog supply (the isolated side, via a VPS8504B, inferred)
    and part of the power hold.
  - **PC4** = scope / generator analog supply.
  - The Alientek primary bootloader raises PE2 and PC4 first thing on every
    reset; the stock power-off drops both and the unit switches off once the
    key is released (on battery). Which of the two, or both, keeps the system
    supply on is unknown.
  - PC0 = Bluetooth module power (low = on).
- MCU die temperature: ADC0 channel 16 (internal sensor), typical constants.

## Pin table

GD32 peripheral names (UART4, SPI1, SPI2, I2C2, DMA0 / DMA1) follow the
GD32F4xx user manual as the fw-rs sources do. "verified" = driven by fw-rs or
the stock code as stated; the board-level meaning may still be inferred.

| Pin | Function | Dir | Status / source |
|---|---|---|---|
| PA0 | UART4_TX -> Bluetooth (AF8) | out | verified, `bt.rs` |
| PA1 | UART4_RX <- Bluetooth (AF8) | in | verified, `bt.rs` |
| PA2 | debug header "TX" pad: boot strap of the bootloader / FS-loader (pull-up, low = update mode); AF USART1_TX unused | in | verified (owner, device 2026-10-24; loader code) |
| PA4 | DAC channel 1: generator; ADC0 ch4 read-back | analog | verified, `gen.rs`, `power.rs` |
| PA5 | DAC channel 2: scope trigger level (stock) | analog | verified (stock), `hwtrig.rs` |
| PA6 | TIM13_CH1 (AF9) backlight PWM 20 kHz | out | verified, `board.rs` |
| PA7 | TIM14_CH1 (AF9) buzzer | out | verified, `lcd_buzzer_v2_6.h` |
| PA8 | I2C2 SCL, touch (AF4) | od | verified, `touch.rs` |
| PA9 | DMM relay drive (pulse) | out | verified, `dmm.rs` |
| PA10 | DMM relay drive (pulse) | out | verified, `dmm.rs` |
| PA11 | USB DM (AF10) | io | verified, `usb.rs` |
| PA12 | USB DP (AF10) | io | verified, `usb.rs` |
| PA13 / PA14 | SWD (pads "D" / "C") | io | inferred (standard pins; pads in photo) |
| PA15 | TIM2_CH1 (AF1): scope trigger comparator (stock) | in | verified (stock), `hwtrig.rs` |
| PB0..PB9 | 3PA1030 data D0..D9 | in | verified, `scope.rs` |
| PB10 | charge status / control (meaning guessed) | in / od | stock code, `dmm_v2_6.h`, `power.rs` |
| PB11 | charge status (meaning guessed) | in, pull-up | stock code, `dmm_v2_6.h`, `power.rs` |
| PB12 | generator path select, 1 = PWM, 0 = DAC | out | verified, `gen.rs` |
| PB14 | TIM12_CH1 (AF9) generator square | out | verified, `gen.rs` |
| PB15 | scope 4051 select bit | out | verified, `scope.rs` |
| PC0 | Bluetooth module power, low = on | out | verified, `bt.rs` |
| PC2 | SPI1 MISO, flash (AF5) | in | verified, `spiflash.rs` |
| PC3 | SPI1 MOSI, flash (AF5) | out | verified, `spiflash.rs` |
| PC4 | scope / generator analog rail | out | verified, `scope.rs` |
| PC5 | ADC0 ch15, battery / 2 | analog | verified, `power.rs` |
| PC6 | TIM8_CH1 (AF3) 50 MHz scope ADC clock | out | verified, `scope.rs` |
| PC7 | DMM frequency mode (1 only in mode 14) | out | verified pin use; function unknown |
| PC8 | TIM3_CH3 (AF2) scope offset PWM | out | verified, `scope.rs` |
| PC9 | I2C2 SDA, touch (AF4) | od | verified, `touch.rs` |
| PC10 | SPI2 SCK -> HY3131 (AF6) | out | verified, `hy3131.rs` |
| PC11 | SPI2 MISO <- HY3131 (AF6) | in | verified, `hy3131.rs` |
| PC12 | SPI2 MOSI -> HY3131 (AF6) | out | verified, `hy3131.rs` |
| PC13 | scope attenuator relay, 1 = direct, 0 = /50 | out | verified, `scope.rs` |
| PC14 | Bluetooth link up (high = connected) | in | verified, `power.rs` |
| PD0, PD1, PD8, PD9, PD10, PD14, PD15 | FSMC data (AF12) | io | verified, `board.rs` |
| PD2 | HY3131 CS (active low) | out | verified, `hy3131.rs` |
| PD3 | SPI1 SCK, flash (AF5) | out | verified, `spiflash.rs` |
| PD4 / PD5 | FSMC NOE / NWE | out | verified, `board.rs` |
| PD6 | power key, pressed = 1 (pull-down) | in | verified, `boot.rs` |
| PD7 | FSMC NE1 (LCD CS) | out | verified, `board.rs` |
| PD11 | FSMC A16 = LCD RS | out | verified, `board.rs` |
| PD12 / PD13 | scope 4051 select bits | out | verified, `scope.rs` |
| PE0 | A shunt MOSFET (10 mohm) | out | verified, `dmm.rs` |
| PE1 | mA shunt MOSFET (0.5 ohm) | out | verified, `dmm.rs` |
| PE2 | DMM analog rail / power hold | out | verified pin use, `boot.rs` |
| PE3 | scope coupling, 1 = DC, 0 = AC | out | verified, `scope_gen_ui_v2_6.h` |
| PE4 | SPI1 CS, flash | out | verified, `spiflash.rs` |
| PE5 | touch INT (falling edge) | in | verified, `touch.rs` |
| PE6 | touch reset | out | verified, `touch.rs` |
| PE7..PE15 | FSMC data (AF12) | io | verified, `board.rs` |
| PH0 / PH1 | 25 MHz HXTAL | - | inferred (standard pins) |

Not used by the stock firmware or fw-rs as far as traced: PA3 (maybe the
header's RX pad), PB13, PC1, PC15. Whether they are connected is unknown.

## Models DM40A / B / C

- The firmware tells the models apart only by one byte in the calibration
  record; the stock HY3131 rows, relays and scope / generator code are the
  same for all three (verified, [fw-rs/docs/model-variants.md](../fw-rs/docs/model-variants.md)).
- Hardware difference reported by owners: the A has a discrete divider instead
  of the TC1746B6 network (#25087, #34902, #51830). Whether anything else
  differs (reference, shunts) is unknown.
- The A measures AC+DC as well; the stock hides it by model (owner reports,
  `user-feedback.md` UF-03).
- The review says only the top model (DM40C) has Bluetooth. This repo's unit
  is a DM40A and its Bluetooth works ([`docs/bt-protocol.md`](../docs/bt-protocol.md), `bt.rs`); the
  firmware has no model gate for Bluetooth. The review's statement is about
  the feature list, not confirmed for the hardware.
- Units before 2025 are reported unshielded; the reviewed unit (2025, battery
  dated 2025-02-10) has shield cans and a foil-lined back cover (photos,
  `user-feedback.md` 12).

## Unknowns

1. No schematic. The isolation topology (what sits on which side) is inferred
   from the BOM and the photos.
2. Which rail(s) hold the power on: PE2, PC4 or both; how the power key turns
   the unit on from off. [`fw-rs/src/boot.rs`](../fw-rs/src/boot.rs) and `native-boot.md` (shim
   steps) treat PE2 as the power hold; `native-boot.md` section 1 (older text)
   says no self-hold GPIO is evident and calls it unverified.
3. Why some cold power-ons without a short reached the loader (PA2 low for a
   while); whether anything else on the board drives PA2.
4. The MCU crystal: 25 MHz by the PLL read-back and the photo, 26 MHz in the
   community BOM ([`docs/dm40-feature-request.md`](../docs/dm40-feature-request.md) 1.1). The 25 MHz value is the
   one the running firmware depends on.
5. Charge status pins: `dmm_v2_6.h` says the stock shows the charging icon
   when PB10 = 1 and PB11 = 0; `bt_meas_v2_6.h` and [`fw-rs/src/power.rs`](../fw-rs/src/power.rs) use
   PB10 = 1 and PB11 = 1 for "charging". Both are guesses about the meaning.
6. Fuse rating and type (A input), and whether there is a separate mA fuse.
   The 47.5 ohm resistor's power rating.
7. PC7's function in the frequency mode; the HY3131 CNT pin source; how the
   buzzer is driven (PA7 and the HY3131 CMP pin) across the isolation.
8. The scope trigger comparator chip; which of PB15 / PD12 / PD13 is the 4051
   A / B / C; the exact gain resistors.
9. The scope op amp ("8091") and the generator output stage after the SGM3157.
10. The regulators and the power path parts in `charger-tp4056-and-power.jpg`.
11. The debug header's RX, VCC and V / D / C / G pads (only TX = PA2 is
    verified).
12. LCD panel FPC pinout; touch panel vendor beyond the controller ID.
13. Exact HY3131 wiring (ohm reference resistors, divider taps, shunt nodes):
    inferred from the stock scaling only ([`emu/hy3131/RESEARCH.md`](../emu/hy3131/RESEARCH.md) 8.5).
