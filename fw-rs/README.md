# DM40 replacement firmware (Rust, RAM-loaded PoC)

Replacement firmware for the Alientek DM40 (GD32F427, Cortex-M4F @ 200 MHz).
Scope of this PoC: multimeter only, GUI matching the stock firmware but with the
main colour changed from orange/yellow to red. No settings screens, no
Bluetooth. Calibration and configuration are read from their existing locations;
nothing is written. USB host-side access uses USBTMC (added in a later step).

The image runs from SRAM, taking over from the stock firmware and
re-initialising the peripherals it needs. How the image is placed in RAM and
started is the loader's job (provided separately); this crate only produces the
image and defines the contract below.

## Load contract

- **Format:** bare binary, position-dependent, loaded at `0x20008000`.
- **Layout:** Cortex-M vector table at the start — word[0] = initial MSP
  (`0x10010000`), word[1] = reset entry (thumb bit set). A loader copies the
  image to `0x20008000` and branches to word[1].
- **Region:** the image occupies `0x20008000`..`0x2001C000` (80 KiB) of the
  SRAM0 the stock firmware leaves free in multimeter mode (free from
  `0x20005024`); the low 32 KiB of SRAM0 (stock scope buffer) is not touched.
  `.bss` and the stack are in TCM SRAM (`0x10000000`, 64 KiB), which the loader
  never writes; word[0] (initial MSP) is `0x10010000`.
- **On entry:** reset sets MSP from word[0], masks interrupts (`cpsid i`), points
  `VTOR` at the in-SRAM table, zeroes `.bss` and calls `main`. The loader does
  not need to keep its interrupt handlers alive after the branch.

## USB debug link

The image enumerates as a composite USBTMC + mass storage device (`1209:0001`)
on USBFS. Debug commands are SCPI `DBG:...` over USBTMC (`src/scpi.rs`); the
host tool `tools/dm40dbg.py` (PIL for screenshots) needs read/write access to
`/dev/usbtmc*` (udev rule). The disk is the stock SPI flash volume.

```
tools/reload.sh                       # build, reset to stock, BT load, wait for USB (~30 s)
/usr/bin/python3 tools/dm40dbg.py ping
/usr/bin/python3 tools/dm40dbg.py shot out.png     # full screen in ~0.8 s
/usr/bin/python3 tools/dm40dbg.py scpi 'DBG:FLASH?'  # SPI disk: EF4018,4096,0
/usr/bin/python3 tools/dm40dbg.py release|reset    # back to stock (~20 s | now)
```

Use `/usr/bin/python3`: the linuxbrew `python3` first on PATH has no Bluetooth
sockets, which `dm40_loader.py` needs.

Watchdog: fed for 60 s after start, then only while the host keeps talking (each
request extends by 15 s; `PING` with `a` = ms extends up to 10 min). After
`release`, a timeout or a panic it is not fed and the device returns to stock
in ~20 s (FWDGT PSC=/256, RLD=2500, measured 19 s).

Bring-up status squares (top left): white = running, then PLLSAI locked, USB
core answering, USB configured, host request seen (green = ok, red = failed,
grey = not yet, yellow = watchdog released). Magenta bar at the bottom = panic.

Hardware findings (measured on the device, not from the stock code):
- USB 48 MHz: the stock clock tree gives PLLQ = 400/9 = 44.4 MHz with IRC48M and
  PLLSAI off, so we start PLLSAI (N=288, P=6) and select it for CK48M.
- GSNPSID reads 0 on this GD32; use GRSTCTL.AHBIDL to see the core is clocked.
- Resetting USBFS does not make the host see a disconnect (the D+ pull-up stays),
  so `usb::init_hw` drives PA11/PA12 low for 100 ms first.
- ILI9488 GRAM read-back (0x2E) with COLMOD 16 bpp: one dummy word, then
  RGB666 as 3 bytes per pixel packed big-endian into the 16-bit words.
  ID (0xD3) = 00 94 88.

## Features (state)

All instruments run continuously; only the view changes (swipe left/right:
Multimeter -> DMM + Scope -> Scope -> Generator; MENU opens the main menu).

- **Multimeter**: all 16 HY3131 modes ported from the stock behaviour
  (V/mV DC+AC, A/mA/uA DC+AC with A->mA->uA autorange, ohm, continuity,
  diode with beeper patterns, capacitance RC + ramp, frequency/duty,
  K thermocouple), stock filters / settle times / autorange / formats.
  Page drawn with the stock fonts and layout read in place from the stock app
  in flash (pixel-identical except the red main value). HOLD (RUN), REL
  (touch button), MIN/MAX, 5 history slots, strip chart 160/320/640 ms/div,
  MF range page (manual ranges, AUTO, AUTO+, DC/AC), HV lightning >= 100 V.
- **Scope**: stock timebases (100 ns .. 500 ms/div, roll from 200 ms/div),
  10 mV .. 10 V/div with the stock gain/offset calibration, probe x1/x10/x100,
  DC/AC, Auto/Normal/Single with software trigger, AUTO-set, 20 stock
  measurement items (4 shown), X/Y cursors, persistence, settings window (MF).
- **Generator**: sine / square / triangle / ramp via DAC + DMA, square up to
  10 MHz via PWM, stock page with keypads; RUN toggles the output. Keeps
  running on every page (state shown on the scope bar).
- **Bluetooth**: stock phone-app protocol (commands 00..09) on UART4.
- **USB**: USBTMC/USB488 interface + SCPI, debug commands as `DBG:` SCPI
  (tools/dm40dbg.py). Mass storage: the stock FAT16 volume on the W25Q128 SPI
  flash (4096-byte sectors), always attached; `DBG:FLASH?` = JEDEC id, blocks,
  write error flag. A block write that needs an erase stalls the main loop for
  ~50 ms (up to 400 ms).
- **Touch**: CST2xx controller, gestures (tap, long press, swipes, drag),
  side keys MENU / RUN / AUTO / MF. Power key: hold 1.5 s = off, double
  press = key/touch lock.
- **Settings** (stock main menu tiles + settings page): dim / shutdown,
  backlight, volume, Bluetooth, start page, analog power; kept in backup SRAM.
- **Analog power**: the stock powers the scope side (PC4) and the DMM side
  (PE2) alternately per page; "Analog power: Both (exp.)" keeps both on so the
  DMM and the scope measure at the same time (never done by the stock, off by
  default).

Not implemented: screenshots / CSV to the SPI disk,
config.bin (settings are in backup SRAM instead), AC+DC (DM40B/C only).

## SCPI

USBTMC (`/dev/usbtmc*`, needs a udev rule for non-root):

```
*IDN? *RST *CLS *OPC? SYST:ERR?
MEAS:<f>?  CONF:<f>  READ?  FETC?  FUNC?   <f> = VOLT[:DC] VOLT:AC CURR[:DC]
           CURR:AC RES CONT DIOD CAP FREQ TEMP
SOUR:FUNC SIN|SQU|TRI|RAMP  SOUR:FREQ <Hz>  SOUR:VOLT <Vpp>
SOUR:FUNC:SQU:DCYC <%>  OUTP ON|OFF
TIM:SCAL <s/div>  CHAN1:SCAL <V/div>  CHAN1:COUP AC|DC
TRIG:LEV <V>  TRIG:SLOP POS|NEG  TRIG:SWE AUTO|NORM|SING  RUN STOP SING
SCOP:MEAS:VPP|VAVG|VRMS|FREQ?  WAV:PRE?  WAV:DATA? (#3400 + 400 bytes)
```

## Milestones

Goal and remaining work: see [TODO.md](TODO.md).

1. **(done)** Prove load + run + display.
1b. **(done)** Debug link: keepalive-gated watchdog, screenshots, LCD register
   access, reset/release to stock (first USB CDC, now `DBG:` SCPI over USBTMC).
2. **(done)** Re-init clocks, FSMC and the ILI9488 from scratch; text/number
   rendering with the stock fonts.
3. **(done)** HY3131 driver and measurement loop, calibration read from the
   stock settings record at 0x080C0000.
4. **(done)** Multimeter GUI matching `firmware/re/analysis/` notes, main value
   in red; scope and generator pages, menus, touch.
5. **(done)** USBTMC + SCPI over USBFS, composite with a mass-storage disk on
   the SPI flash.

## Build

```
./build.sh                 # -> dm40-fw.bin (RAM image, sector 8)
./build.sh native          # -> dm40-fw-native.bin (update file for the loader, 0x08004000)
./build.sh test-initcore   # -> dm40-fw.bin (T1 RAM test of the native init)
./build.sh test-recovery   # -> dm40-fw.bin (T2 RAM test of native recovery)
tools/fatren-test/run.sh   # host test of the FAT rename / recovery trigger
```

Native image, shim, recovery and test plan: docs/native-boot.md "Implementation".

Needs the pinned toolchain in `../toolchain` (rustup + stable +
`thumbv7em-none-eabihf`). `build.sh` sources `../toolchain/env.sh`. Crates:
`usb-device`, `synopsys-usb-otg` (fs), `usbd-serial`, `cortex-m`
(critical-section only); fetched from crates.io into `../toolchain/cargo`.
