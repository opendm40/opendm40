# Native-boot Stage 0 — prove the recovery path

> **Historical (2026-10-13).** Run with the RAM-loaded fw-rs build, which was
> removed on 2026-10-22; `DBG:RECOVER` is now the destructive marker-zeroing
> recovery (`tools/dm40dbg.py recover -h`), not a plain reset.

Goal: prove end-to-end that the stock **FS-loader** will re-install a firmware
image from the SPI-flash FAT on reset, **before** we ever replace the stock app.
This is the first, zero-risk step of the plan in
[`native-boot.md`](native-boot.md) (reviewed in
[`native-boot-review.md`](../../docs/history/native-boot-review.md)).

**This test reflashes STOCK over the (still intact) stock app.** The current
fw-rs is only RAM-loaded over Bluetooth, so nothing we rely on is at risk:
a battery pull alone already returns the device to stock. Honest framing (per
the review): Stage 0 proves **auto-install of a file already on the FAT**, not
dead-app recovery — that harder case is a Stage 1 concern.

## Verified mechanism (do not re-derive — see `firmware/re/ANALYSIS.md` §1, §3)

- On every reset the ROM bootloader at `0x08000000` launches the FS-loader at
  `0x080a0000` (it has **no** reference to the app at `0x08004000` — it can only
  ever reach the app through the FS-loader). The FS-loader is in sector 9 and is
  **never** overwritten by an app update.
- The FS-loader calls `f_findfirst("/","dm40*.bin")` (`0x080a26c4`) and installs
  the **first** matching file (FAT directory order — not newest, one per boot,
  no version compare). It CRC-checks the header and body, writes the body to
  `0x08004000` (writer bounded to `0x08004000`..`0x080a0000`), then launches the
  app.
- **Filename pattern: `dm40*.bin`.** The official stock file `dm40_v_2_6.bin`
  already matches, so it needs **no rename**.
- **Auto-delete:** the FS-loader does **not** delete the file. The **stock app**
  deletes it after its first successful boot. So in this test, once stock comes
  back up it removes `dm40_v_2_6.bin` itself and no reinstall loop forms. Still,
  verify it is gone (step 6) and delete it manually if it is not.
- **fw-rs cannot write or delete FAT files.** `src/msc.rs` + `src/spiflash.rs`
  only expose the W25Q128 as a USB **block device**; the host OS owns the FAT
  filesystem (there is no FAT driver in the firmware). So the **host** stages
  and removes the file; the firmware only triggers the reset.

## What `DBG:RECOVER` does

`DBG:RECOVER` (SCPI, `src/scpi.rs`) performs a plain **`SYSRESETREQ`** via
`AIRCR` (`0xE000ED0C` ← `0x05FA_0004`) — the *same* reset path as `DBG:RESET`
(`dm40dbg reset`), deferred ~50 ms in the main loop so the USB transaction
finishes first. In Stage 0 that is all recovery needs: the ROM + FS-loader do
the reflash on the way back up, independent of whatever is at `0x08004000`.

It is kept as a distinct SCPI verb because once the native shim exists (Stage 1)
`RECOVER` must additionally set the shim's recovery magic before resetting, while
`RESET` becomes an ordinary fw-rs reboot. No presence check is built in: reading
the FAT directory would need a BPB/root-dir parser the firmware does not have,
and the host already *shows* the staged file in its file manager — a better
confirmation than anything the firmware could report.

## Test procedure

1. With fw-rs running (RAM-loaded via `fw-rs/tools/reload.sh`), it enumerates a
   USB mass-storage disk. Mount it on the host.
2. Copy the official stock update file to the disk root. It lives at the repo
   root:

       cp ~/work/board/alientek-dm40/dm40_v_2_6.bin  /run/media/<you>/<DM40 disk>/

   - Use the **obfuscated official** file `dm40_v_2_6.bin`, **not**
     `dm40_v_2_6.dec.bin` (the decoded body). The FS-loader expects the packaged
     update file and verifies its header + body CRC.
   - Do **not** rename it — `dm40_v_2_6.bin` already matches `dm40*.bin`.
   - Keep **exactly one** `dm40*.bin` on the disk (two give an order-dependent,
     possibly wrong install).
3. `ls` the mounted disk to confirm the file is there, then `sync` / unmount so
   the write actually reaches the SPI flash.
4. Trigger recovery:

       fw-rs/tools/dm40dbg.py recover

   (`dm40dbg.py recover -h` prints this same procedure.)
5. The device reboots. **Expect the stock splash / stock UI** to come up — the
   FS-loader reflashed stock to `0x08004000`. The FS-loader shows `update ok`
   vs `update error` based on the CRC check; a corrupt file is rejected and the
   previous app is kept (never half-flashed).
6. Re-mount the disk and confirm `dm40_v_2_6.bin` is **gone** (the stock app
   deletes it after a good boot). If it is still present, delete it so the next
   boot does not reinstall it.
7. Return to fw-rs: run

       fw-rs/tools/reload.sh

   which rebuilds and RAM-loads fw-rs over Bluetooth. It does not touch the FAT.

## Pass criterion

The device boots the **stock** firmware after step 4 with no cable/SWD/battery
intervention — proving the ROM + FS-loader reflash a staged `dm40*.bin` entirely
on their own. Log the result in `README.md` before proceeding to Stage 1
(per `native-boot.md` §8).
