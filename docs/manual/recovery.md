# Backup, recovery and back to stock

An update rewrites only the firmware part of the meter's memory. The
Alientek bootloader, the update loader and your meter's calibration stay as
they are. A bad or broken update file cannot brick the meter: the update
loader then starts instead, with its own USB disk, until a good file is
installed.

## Back up your meter

Do this once, right after you install opendm40.

1. Connect the meter to a computer with a USB cable. The opendm40 USB disk
   appears ([USB disk](usb-disk.md)).
2. Copy the whole `INFO/` folder to the computer.
3. Optionally, also copy `DEBUG/SPIFLASH.BIN`: a raw copy of the meter's
   16 MB internal disk.

Keep the copy off the meter, in a private place: it contains the
calibration and the chip's unique ID of your unit.

| File in `INFO/` | What it is |
|---|---|
| `CALIB.BIN` | The settings and calibration record of your unit. |
| `BOOTDATA.BIN` | The update loader's data, tied to your unit's chip. |
| `UID.BIN` | The chip's unique ID: says which unit the two files above belong to. |
| `ROMBOOT.BIN` | The Alientek bootloader. |
| `LOADER.BIN` | The update loader, including its data. |
| `OPTBYTES.BIN`, `OTP.BIN` | Chip configuration areas. |
| `FIRMWARE.BIN` | The running firmware, as an update file. Keep a copy: you can reinstall it. |
| `BACKUP.TXT` | Addresses, sizes, checksums and restore notes for the files above. |

`CALIB.BIN`, `BOOTDATA.BIN` and `UID.BIN` cannot be recreated: no update
file contains them. opendm40 also keeps one copy of these three files in the
top folder of its internal disk, written once and never overwritten.

Restoring these files needs a debug probe on the board; `BACKUP.TXT` lists
the steps.

## Reach the update loader

The update loader shows an update screen and its own USB disk. Any of these
gets you there:

- **From the power key.** With the meter off, press and hold the power key.
  The screen lights up. About 4 s later the backlight blinks and the meter
  beeps. Keep holding at least 0.5 s more, then let go. Letting go during
  that half second cancels: the meter starts normally.
- **From a computer.** While opendm40 runs and is connected over USB:
  `python3 firmware/tools/dm40dbg.py update`
  (the SCPI command `DBG:UPDATE`, see [SCPI reference](scpi-reference.md)).
- **By itself after repeated hangs.** If the firmware hangs, it restarts
  after about 20 s. If it hangs again within its first 60 s of running, the
  next start opens the update loader.
- **Through the board.** If nothing else works: open the case and connect
  the debug header's TX pad to ground while switching on. See
  [hw/README.md, Hardware recovery](../../hw/README.md#hardware-recovery).

Once there, the loader shows its update screen on every start until a valid
update file has been installed.

## Install a file from the update loader

1. Connect the meter to the computer. The loader's USB disk appears.
2. Copy the update file onto it, as the only file named `dm40*.bin`:
   - opendm40: `opendm40.bin`, copied as `dm40_fwrs.bin`, or the
     `FIRMWARE.BIN` from your backup, copied as `dm40_fwrs.bin`;
   - stock firmware: `dm40_v_2_6.bin`.
3. Press **update** on the meter's screen.

The loader deletes the file after a successful install and starts the new
firmware.

**Keep only one `dm40*.bin` on the disk.** The loader installs the first
match it finds, not the newest one. The stock firmware also installs any
`dm40*.bin` it finds on its own disk, so do not leave such files there by
accident.

## Back to the stock firmware

1. Get the official stock update file
   [`dm40_v_2_6.bin`](../../stock/vendor/dm40_v_2_6.bin).
2. Either:
   - reach the update loader (above) and install the file from there; or
   - while opendm40 runs: set the USB disk to **PC can edit**, re-plug, copy
     the file onto the disk and answer **Yes** to **Install it now?**
     ([Getting started, Update](getting-started.md#update)).

Your calibration was never changed, so the stock firmware measures as
before.

## Known limitations

Not yet tried on a real meter: entering the update loader with the power
key, the automatic entry after repeated hangs, installing opendm40 over the
stock firmware's "USB Connect" disk, and the install from the "PC can edit"
disk. Installing from the update loader's screen and from a running
opendm40 over USB has been done on a DM40A.
