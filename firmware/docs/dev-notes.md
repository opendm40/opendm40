# opendm40 developer notes

The USB debug link of opendm40 and the developer loop: build, reload over
USB, check, go back to stock. Paths are relative to `firmware/`, and the
commands run there.

Elsewhere: build and toolchain in [firmware/README.md](../README.md); first
install, backup and recovery for users in the [root README](../../README.md);
every SCPI command (also the `DBG:*` ones) in
[docs/manual/scpi-reference.md](../../docs/manual/scpi-reference.md); the
image, shim, recovery and `DBG:BOOT?` fields in [native-boot.md](native-boot.md);
open work in [TODO.md](../TODO.md).

## USB debug link

opendm40 enumerates as a composite USBTMC + mass-storage device (`1209:D040`; builds before 0.1.0: `1209:0001`)
on USBFS. Debug commands are SCPI `DBG:...` over USBTMC
([`src/scpi.rs`](../src/scpi.rs)), refused over Bluetooth. The host tool
[`tools/dm40dbg.py`](../tools/dm40dbg.py) (PIL for screenshots) needs
read/write access to `/dev/usbtmc*`: the udev rule in the
[user manual](../../docs/manual/README.md#access-without-root).
[`tools/dm40scpi.py`](../tools/dm40scpi.py) is the SCPI client for readings,
CSV logs and the fast logger (USB or Bluetooth).

The USB disk is a read-only volume generated on the fly; how it is built and
what a PC may write: [native-boot.md, Unit data](native-boot.md#unit-data-the-usb-volume-srcvdiskrs-the-spi-disk-srcfatdirrs-and-the-calibration-safety-copy-srccalbackuprs).

```
python3 tools/dm40dbg.py ping
python3 tools/dm40dbg.py shot out.png        # full screen in ~0.8 s
python3 tools/dm40dbg.py tap X Y             # touch tap; also left / right (view swipe)
python3 tools/dm40dbg.py drag X0 Y0 X1 Y1 [--ms N] [--hold MS]
python3 tools/dm40dbg.py key 0..3 [--hold MS]  # side key: 0 MENU, 1 RUN, 2 AUTO, 3 MF
python3 tools/dm40dbg.py diag                # DBG:DIAG? block (hex)
python3 tools/dm40dbg.py boot                # DBG:BOOT? decoded: reset cause, marker, crash counter...
python3 tools/dm40dbg.py scpi 'DBG:FLASH?'   # any SCPI; this one: SPI flash JEDEC id, blocks, write error
python3 tools/dm40dbg.py upload FILE         # update file -> DM40FW.BIN in the SPI root (DBG:UPLOAD)
python3 tools/dm40dbg.py update [--wait S] [--expect FILE]   # DBG:UPDATE: the loader installs it
python3 tools/dm40dbg.py image FILE          # offline: is FILE a valid dm40*.bin update file?
python3 tools/dm40dbg.py reset               # reboot opendm40
python3 tools/dm40dbg.py release             # stop feeding the watchdog (counts as a crash)
python3 tools/dm40dbg.py recover             # DESTRUCTIVE: into the vendor update screen (recover -h)
```

`python3 tools/dm40dbg.py -h` lists the rest (`hold`, `lcdread`, `genuine`).

**Watchdog.** The main loop feeds it while it runs. A hang, a panic (magenta
bar at the bottom) or `DBG:RELEASE` resets the meter after ~20 s and opendm40
reboots once; a second one within 60 s enters recovery (the loader update
screen). Details: [native-boot.md, Normal native mode](native-boot.md#normal-native-mode).

**Bring-up failures** show as red squares at the top left (`fail` in
[`src/main.rs`](../src/main.rs)): slot 0 = PLLSAI (the 48 MHz USB clock) not
locked, slot 1 = USBFS core not answering (GRSTCTL.AHBIDL; GSNPSID reads 0 on
this GD32).

**USB re-attach.** Resetting USBFS does not make the host see a disconnect
(the D+ pull-up stays), so `usb::init_hw` drives PA11 / PA12 low for 100 ms
first ([`src/usb.rs`](../src/usb.rs)).

**Screenshots** read the LCD's GRAM back (0x2E): one dummy word, then RGB666
as 3 bytes per pixel packed big-endian into the 16-bit words
([`src/lcd.rs`](../src/lcd.rs)).

## Developer workflow

**Reload while opendm40 runs** (the normal loop):

```
tools/reload.sh [cargo args]     # e.g. --features keep-screen
```

1. `./build.sh [cargo args]`; `dm40dbg.py image` checks the file (vendor
   header CRC, body CRC, size, reset vector) before anything is staged.
2. A running opendm40 that does not answer `DBG:BOOT?` is refused; so is the
   stock "USB Connect" disk or the loader update screen (`28e9:028f`) unless
   `--from-stock`.
3. `dm40dbg.py upload`: `DBG:UPLOAD` writes the file as `DM40FW.BIN` into the
   SPI flash's root (every other `dm40*.bin` there removed first), the meter
   re-reads it and checks the CRC-32 ([`src/upload.rs`](../src/upload.rs));
   then `DBG:FLASH?` must report no SPI write error.
4. `dm40dbg.py update --wait 90 --expect FILE`: `DBG:UPDATE` zeroes the loader
   marker and resets; the vendor loader installs the file and deletes it. If
   the device is still on USB after 5 s, `SYST:ERR?` is reported ("Flash
   error" = marker not zeroed, nothing changed). The first boot after an
   install is dark for several seconds: do not power-cycle. If `28e9:028f`
   stays up for 20 s instead, the install failed (loader screen). Then
   `DBG:BOOT?` is printed and the loader marker compared with the file's
   12-byte update header: "installed OK" (~1 min in all).

Options (`tools/reload.sh --help`): `--no-build`, `--image FILE`, `--wait S`,
`--stock FILE`, `--from-stock`.

**First install** (the meter runs stock): `tools/reload.sh --from-stock`
stages the file on the stock "USB Connect" disk and tells you to press
"update" or power-cycle, then waits up to 300 s for opendm40. By hand:
[README.md, Install](../../README.md#install).

**Back to stock** from a running opendm40: `tools/reload.sh --stock FILE`
with the official [`../stock/vendor/dm40_v_2_6.bin`](../../stock/vendor/dm40_v_2_6.bin)
or `../stock/patch/build-memrw/dm40_patched.bin` (patched stock with
Bluetooth memrw, built by [`../stock/patch/kit/mkkit.sh`](../../stock/patch/kit/mkkit.sh)).
Same flow, no wait (stock does not enumerate `1209:D040`).

**Recovery** (opendm40 hangs or does not start):
[README.md, Recovery](../../README.md#recovery). `INFO/FIRMWARE.BIN` on the
USB volume (the running image) or `opendm40.bin` both work as the image.

**Tests**: host suites `tools/*/run.sh`, the emulator
([emu/README.md](../../emu/README.md)), and the device log
[device-check-2026-10-21.md](device-check-2026-10-21.md).
