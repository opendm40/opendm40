# fw-rs developer notes

Paths in this file are relative to `fw-rs/`, and the commands run there.

Replacement firmware for the Alientek DM40 (GD32F427, Cortex-M4F @ 200 MHz):
multimeter, scope and generator running at the same time, stock-like GUI with
the main colour red, menus, touch, Bluetooth app protocol, USBTMC/SCPI.
Calibration and the vendor boot data are read from their stock locations, never
rewritten (served read-only on the USB disk, plus one write-once safety copy
of the calibration on the SPI flash).

fw-rs is installed like a stock update: `dm40-fw-native.bin` is a vendor-format
update file that the vendor FS-loader flashes to `0x08004000`; a small shim in
front of it handles the power key, the watchdog and recovery
([native-boot.md](native-boot.md)). This is the only build.

## USB debug link

fw-rs enumerates as a composite USBTMC + mass storage device (`1209:0001`) on
USBFS. Debug commands are SCPI `DBG:...` over USBTMC ([`src/scpi.rs`](../src/scpi.rs)); the host
tool [`tools/dm40dbg.py`](../tools/dm40dbg.py) (PIL for screenshots) needs read/write access to
`/dev/usbtmc*` (udev rule). Mass storage is ONE removable disk "DM40"
generated on the fly ([`src/vdisk.rs`](../src/vdisk.rs)): `LOGS/*.CSV`, `SCOPE/*.CSV`,
`SHOTS/*.BMP`, `INFO/` (calibration, boot data, firmware image),
`DEBUG/SPIFLASH.BIN`, `README.TXT`. The SPI flash itself is internal storage.
Deleting a LOGS / SCOPE / SHOTS file on the PC deletes it on the meter, and
copying an update file (`dm40*.bin`) onto the disk offers to install it
([`src/vwrite.rs`](../src/vwrite.rs)); nothing else written is kept.

```
python3 tools/dm40dbg.py ping
python3 tools/dm40dbg.py shot out.png        # full screen in ~0.8 s
python3 tools/dm40dbg.py boot                # DBG:BOOT? decoded: reset cause, marker, crash counter...
python3 tools/dm40dbg.py scpi 'DBG:FLASH?'   # SPI flash: EF4018,4096,0 (last = write error)
python3 tools/dm40dbg.py upload FILE         # update file -> DM40FW.BIN in the SPI root (DBG:UPLOAD)
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
  (tools/dm40dbg.py). Mass storage (native builds): ONE FAT16
  volume "DM40" (2 GiB, 32 KiB clusters) synthesized on the fly
  ([`src/vdisk.rs`](../src/vdisk.rs)): `LOGS/LOGnnn.CSV` and `SCOPE/SCOPnnn.CSV` (generated from
  the compact DLG logs), `SHOTS/Snnn.BMP`, `INFO/` (`CALIB.BIN`,
  `BOOTDATA.BIN`, `UID.BIN`, `OPTBYTES.BIN`, `OTP.BIN`, `ROMBOOT.BIN`,
  `LOADER.BIN` raw region copies, `BACKUP.TXT` with CRC-32s and restore notes,
  `FIRMWARE.BIN` = the running firmware as a vendor update file),
  `DEBUG/SPIFLASH.BIN` (raw 16 MiB SPI flash image) and `README.TXT`.
  Read-only by default, "Files live" ([decisions-2026-10-22.md](decisions-2026-10-22.md) #6): new
  recordings / screenshots appear while the volume is mounted (UNIT
  ATTENTION at once; existing files keep their place). Settings 3/3 "USB
  disk" = "PC can edit" (volatile, in force from the next USB plug;
  `DBG:VDISK:RO 0|1`, `RO?` = in force, asked for) makes it writable for two
  things only ([`src/vwrite.rs`](../src/vwrite.rs), [native-boot.md](native-boot.md) "Host writes"): a
  delete of a LOGS / SCOPE / SHOTS file (also through the trash: at the
  eject) deletes the SPI file; a `dm40*.bin` copied into the root is checked
  as the loader does and the meter asks "Install it now?" (Yes =
  DBG:UPDATE). Other writes fail (the hosts' own trash / indexer files are
  kept for the session only); new files then appear at the eject / re-plug. The W25Q128 SPI flash (stock FAT16,
  4096-byte sectors) is internal storage: our files in `LOGS/`, `SHOTS/`,
  `SCOPE/` ([`src/fatdir.rs`](../src/fatdir.rs); files of earlier builds stay in its root and are
  shown too), the vendor loader's `dm40*.bin` and a write-once safety copy of
  `CALIB.BIN` / `BOOTDATA.BIN` / `UID.BIN` in its root (made when missing,
  never overwritten: it survives an SWD mass erase of the MCU).
  `DBG:FLASH?` = JEDEC id, blocks, write error flag. A block write that needs
  an erase stalls the main loop for ~50 ms (up to 400 ms). Firmware files
  reach the SPI root over USBTMC (`DBG:UPLOAD`, `tools/dm40dbg.py upload`).
  [native-boot.md](native-boot.md) "Unit data".
- **Touch**: CST2xx controller, gestures (tap, long press, swipes, drag),
  side keys MENU / RUN / AUTO / MF. Power key: hold 1.5 s = off, double
  press = key/touch lock.
- **Settings** (stock main menu tiles + settings page): dim / shutdown,
  backlight, volume, Bluetooth, start page, analog power; kept in backup SRAM.
- **Analog power**: the stock powers the scope side (PC4) and the DMM side
  (PE2) alternately per page; "Analog power: Both (exp.)" keeps both on so the
  DMM and the scope measure at the same time (never done by the stock, off by
  default).

Not implemented: config.bin (settings are in backup SRAM instead).
Recordings (`LOGS/`), screenshots (3-finger swipe, stock `S%03u.BMP` files,
`SHOTS/`), scope records (`SCOPE/`) and AC+DC on all models (stock: DM40B/C
only) are implemented; their open items are in [TODO.md](../TODO.md).

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
WAV:POIN SCR|MAX|<n>  WAV:FORM BYTE|WORD  WAV:SOUR CHAN1  DIG  TRIG:STAT?
CHAN1:OFFS <V>  ACQ:SRAT?               (docs/bt-scpi.md "Scope waveform readout")
CALC:AHOL ON|OFF|?                      AutoHold
CALC:AVER?  CALC:AVER:MIN?|MAX?|AVER?|COUN?  CALC:AVER:CLE  CALC:AVER:STAT ON|OFF|?
CALC:LIM:LOW|UPP <v>|?  CALC:LIM:STAT ON|OFF|?  CALC:LIM:CLE  CALC:LIM:FAIL?
```

`CALC:` acts on the current DMM function, values in its SI unit (power meter: W). `CALC:AVER...?`
before the first sample and an unset `CALC:LIM:LOW|UPP?` answer `+9.9E37`
(the former also queues "Data stale"); setting a limit turns the check on;
`CALC:LIM:CLE` clears both limits; `CALC:AVER:STAT ON` starts a recording
session (statistics restarted, no idle power-off). Temperature, `SYST:RAIL`,
`LOG:` and `DBG:` commands: see the header of [`src/scpi.rs`](../src/scpi.rs).

Reply timing of `READ?` / `MEAS:<f>?` / `FETC?`:

- **A reading of the current function is already flowing** (`READ?`, or
  `MEAS:<f>?` naming the current function): the next fresh reading
  (0.1-0.4 s in DC V), or the latest one if none comes within 0.85 s. This
  keeps every reply inside libsigrok's hard 1000 ms USBTMC Bulk-IN timeout,
  also in slow capacitance / temperature ranges.
- **Just switched** (`MEAS:<f>?` changing the function, or `CONF:<f>` then
  `READ?`): the first reading after the converter's settle (0.3 s for DC V,
  0.15 s for resistance, then the autorange; the AUTO coupling: its first
  DC / AC decision, at most 3.9 s after the entry, [bt-scpi.md](bt-scpi.md));
  `MEAS:<f>?` prefers a full-resolution average but waits for it at most
  1.5 s after the command. The reply can take about 1.5 s for resistance
  (the range hunt from 60 Mohm) and ~4 s in AUTO, so give the host a timeout
  of 5 s or more
  (PyVISA defaults to 2 s; libsigrok's DM40 profile waits 3 s after switching
  to RES / CAP / TEMP / FREQ / CONT / DIOD / AC+DC). With `SYST:IDN:EMUL
  34401A` (for sigrok / SmuView builds without the DM40 profile: `*IDN?`
  then reads as an HP 34401A) this wait is capped at 0.85 s, `+9.9E37` until
  the first reading.
- `+9.9E37` with "Data stale" in `SYST:ERR?` only when the current function
  has no reading at all within 1.9 s after the settle. A query the host gave
  up on (USBTMC abort / clear, or a new command: "Query INTERRUPTED") gets no
  late reply.

## Milestones

Goal and remaining work: see [TODO.md](../TODO.md).

1. **(done)** Prove load + run + display.
1b. **(done)** Debug link: keepalive-gated watchdog, screenshots, LCD register
   access, reset/release to stock (`DBG:` SCPI over USBTMC).
2. **(done)** Re-init clocks, FSMC and the ILI9488 from scratch; text/number
   rendering with the stock fonts.
3. **(done)** HY3131 driver and measurement loop, calibration read from the
   stock settings record at 0x080C0000.
4. **(done)** Multimeter GUI matching `../firmware/re/analysis/` notes, main value
   in red; scope and generator pages, menus, touch.
5. **(done)** USBTMC + SCPI over USBFS, composite with a mass-storage disk on
   the SPI flash.

## Build and install

Needs the pinned toolchain in `../toolchain` (rustup + stable +
`thumbv7em-none-eabihf`); crates come from crates.io into `../toolchain/cargo`.

```
./build.sh [cargo args]          # -> dm40-fw-native.bin (vendor update file, 0x08004000)
tools/fatren-test/run.sh         # host tests (also tools/genuine-test/run.sh)
```

**Reload while fw-rs runs** (the normal developer loop):

```
tools/reload.sh [cargo args]     # e.g. --features keep-screen
```

builds, uploads the image over USBTMC as `DM40FW.BIN` into the SPI flash's
root (`dm40dbg.py upload` = `DBG:UPLOAD`; other `dm40*.bin` there are
removed, the meter re-reads and checks the CRC-32), sends `DBG:UPDATE`
(zero the loader marker + reset; the vendor loader installs the file and
deletes it), waits for fw-rs to come back (the first boot after an install is
dark for several seconds: do not power-cycle) and checks `DBG:BOOT?`: the
loader marker must equal the header of the new file ("installed OK", ~1 min).
`--no-build`, `--image FILE`, `--wait S`: see `tools/reload.sh --help`.

**First install** (the meter runs stock): `tools/reload.sh --from-stock`
stages the file on the stock "USB Connect" disk (`28e9:028f`) and tells you to
power-cycle (the stock update procedure), then waits for fw-rs. The by-hand
steps and what is verified: [README.md, Install](../README.md#install).

**Recovery** (fw-rs hangs or does not start) and the update-file rules:
[README.md, Recovery](../README.md#recovery). `INFO/FIRMWARE.BIN` on the USB
volume (the running image) or `dm40-fw-native.bin` both work as the image.

**Back to stock** from a running fw-rs: `tools/reload.sh --stock FILE` with
`../firmware/patch/build-memrw/dm40_patched.bin` (patched stock with Bluetooth
memrw, built by `../firmware/patch/kit/mkkit.sh`) or the official
[`../firmware/vendor/dm40_v_2_6.bin`](../../firmware/vendor/dm40_v_2_6.bin).
Back to fw-rs from stock: `tools/reload.sh --from-stock`.

Native image, shim, recovery and boot info fields: [native-boot.md](native-boot.md).
