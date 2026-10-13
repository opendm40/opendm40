# DM40 replacement firmware (Rust)

Replacement firmware for the Alientek DM40 (GD32F427, Cortex-M4F @ 200 MHz):
multimeter, scope and generator running at the same time, stock-like GUI with
the main colour red, menus, touch, Bluetooth app protocol, USBTMC/SCPI.
Calibration and the vendor boot data are read from their stock locations, never
rewritten (a backup copy goes to the SPI disk).

fw-rs is installed like a stock update: `dm40-fw-native.bin` is a vendor-format
update file that the vendor FS-loader flashes to `0x08004000`; a small shim in
front of it handles the power key, the watchdog and recovery
(docs/native-boot.md). The older RAM/sector-8 image loaded over Bluetooth is
kept only for the T1/T2 test images (see "Legacy RAM loader").

## USB debug link

fw-rs enumerates as a composite USBTMC + mass storage device (`1209:0001`) on
USBFS. Debug commands are SCPI `DBG:...` over USBTMC (`src/scpi.rs`); the host
tool `tools/dm40dbg.py` (PIL for screenshots) needs read/write access to
`/dev/usbtmc*` (udev rule). The disk is the stock SPI flash FAT volume.

```
python3 tools/dm40dbg.py ping
python3 tools/dm40dbg.py shot out.png        # full screen in ~0.8 s
python3 tools/dm40dbg.py boot                # DBG:BOOT? decoded: reset cause, marker, crash counter...
python3 tools/dm40dbg.py scpi 'DBG:FLASH?'   # SPI disk: EF4018,4096,0 (last = write error)
python3 tools/dm40dbg.py reset               # reboot fw-rs
python3 tools/dm40dbg.py image FILE          # offline: is FILE a valid dm40*.bin update file?
```

Watchdog (native): fed while the main loop runs. A hang, a panic (magenta bar
at the bottom) or `DBG:RELEASE` resets it after ~20 s and fw-rs reboots once;
a second one within 60 s enters recovery (the loader update screen, below).

Bring-up status squares (top left): white = running, then PLLSAI locked, USB
core answering, USB configured, host request seen (green = ok, red = failed,
grey = not yet, yellow = watchdog released).

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

Not implemented: CSV recording to the SPI disk, config.bin (settings are in
backup SRAM instead). Screenshots to the SPI disk (3-finger swipe, stock
`S%03u.BMP` files) and AC+DC on all models (stock: DM40B/C only) are
implemented; their open items are in TODO.md.

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
CALC:AHOL ON|OFF|?                      AutoHold
CALC:AVER?  CALC:AVER:MIN?|MAX?|AVER?|COUN?  CALC:AVER:CLE  CALC:AVER:STAT ON|OFF|?
CALC:LIM:LOW|UPP <v>|?  CALC:LIM:STAT ON|OFF|?  CALC:LIM:CLE  CALC:LIM:FAIL?
```

`CALC:` acts on the current DMM function, values in its SI unit. `CALC:AVER...?`
before the first sample and an unset `CALC:LIM:LOW|UPP?` answer `+9.9E37`
(the former also queues "Data stale"); setting a limit turns the check on;
`CALC:LIM:CLE` clears both limits; `CALC:AVER:STAT ON` starts a recording
session (statistics restarted, no idle power-off). Temperature, `SYST:RAIL`,
`LOG:` and `DBG:` commands: see the header of `src/scpi.rs`.

Reply timing of `READ?` / `MEAS:<f>?` / `FETC?`:

- **A reading of the current function is already flowing** (`READ?`, or
  `MEAS:<f>?` naming the current function): the next fresh reading
  (0.1-0.4 s in DC V), or the latest one if none comes within 0.85 s. This
  keeps every reply inside libsigrok's hard 1000 ms USBTMC Bulk-IN timeout,
  also in slow capacitance / temperature ranges.
- **Just switched** (`MEAS:<f>?` changing the function, or `CONF:<f>` then
  `READ?`): the first reading after the converter's settle (0.45 s for DC V,
  2.5 s for resistance, then the autorange); `MEAS:<f>?` prefers a
  full-resolution average but waits for it at most 1.5 s after the command.
  The reply can take
  about 3 s for resistance, so give the host a timeout of 5 s or more
  (PyVISA defaults to 2 s; libsigrok's DM40 profile waits 3 s after switching
  to RES / CAP / TEMP / FREQ / CONT / DIOD).
- `+9.9E37` with "Data stale" in `SYST:ERR?` only when the current function
  has no reading at all within 1.9 s after the settle. A query the host gave
  up on (USBTMC abort / clear, or a new command: "Query INTERRUPTED") gets no
  late reply.

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

## Build and install

Needs the pinned toolchain in `../toolchain` (rustup + stable +
`thumbv7em-none-eabihf`); crates come from crates.io into `../toolchain/cargo`.

```
./build.sh native [cargo args]   # -> dm40-fw-native.bin (vendor update file, 0x08004000)
tools/fatren-test/run.sh         # host tests (also tools/genuine-test/run.sh)
```

**Reload while fw-rs runs** (the normal developer loop):

```
tools/reload.sh [cargo args]     # e.g. --features keep-screen
```

builds, stages the image as `dm40_fwrs.bin` on fw-rs's own USB disk (other
`dm40*.bin` there are removed), syncs, verifies, ejects, sends `DBG:UPDATE`
(zero the loader marker + reset; the vendor loader installs the file and
deletes it), waits for fw-rs to come back (the first boot after an install is
dark for several seconds: do not power-cycle) and checks `DBG:BOOT?`: the
loader marker must equal the header of the new file ("installed OK", ~1 min).
`--no-build`, `--image FILE`, `--wait S`: see `tools/reload.sh --help`.

**First install** (the meter runs stock): either reach the vendor loader's
update screen (its USB disk is `28e9:028f`; the proven way from stock is the
LEGACY `tools/reload.sh test-recovery`, which zeroes the marker), copy
`dm40-fw-native.bin` onto that disk as `dm40_fwrs.bin` (the only `dm40*.bin`),
eject and press "update"; or, on the stock "USB Connect" disk,
`tools/reload.sh --from-stock` stages the file and tells you to press
"update" / power-cycle, then waits for fw-rs.

**Recovery** (fw-rs hangs or does not start): the loader update screen and its
USB disk appear by themselves after two watchdog resets in a row. By hand:
from off, hold the power key ~4 s until the beep, keep holding 0.5 s more (let
go within 0.5 s to abort: normal boot); or `python3 tools/dm40dbg.py update` while
fw-rs still answers. Then copy an image (e.g. `dm40-fw-native.bin` or the
`_DM40PRV.BIN` backup of the previous one, renamed `dm40_fwrs.bin`) onto the
loader's disk and press "update".

**Back to stock:** `tools/reload.sh --stock ../firmware/patch/build/dm40_patched.bin`
(patched stock with Bluetooth memrw) or `--stock ../dm40_v_2_6.bin` (official).
From the loader screen: copy the stock file onto its disk, press "update".
Back to fw-rs from stock: `tools/reload.sh --from-stock`.

Native image, shim, recovery and boot info fields: docs/native-boot.md.

### Legacy RAM loader

`./build.sh` (RAM image `dm40-fw.bin`, flash sector 8, run from SRAM at
`0x20008000`), `./build.sh test-initcore` (T1) and `./build.sh test-recovery`
(T2) are loaded over Bluetooth by the RAM stub (`stub/`) and `tools/flash.py`:
`tools/reload.sh --ram | test-initcore | test-recovery [cargo args]`. This
needs STOCK firmware on the meter (go back to stock first), and
`/usr/bin/python3` (the linuxbrew `python3` has no Bluetooth sockets). In the
RAM build the watchdog is fed for 60 s, then only while the host talks
(`dm40dbg.py hold`); `release`/`reset` return to stock.
