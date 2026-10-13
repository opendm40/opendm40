# fw-rs TODO

## Goal

> feature parity со стоковой прошивкой в режиме мультиметра, осциллографа, генератора, включая bluetooth. по usb - usbtmc. В отличие от стоковй прошивке не надо "переключать" режимы, рбаотать должно быть всё одновременно. Переключаться должен только режим отображения: каждый на своём экране (как в исходной) или какие-то комбинации одновременно (сам придумай как лучше). Меню тоже надо сделать.

Additional requirements:
- Main DMM value drawn in red.
- No Alientek logo in menus.
- Touch incl. side keys and gestures (swipes), with debounce and long-press filtering.
- DMM digits and chart redraw as fast as stock (~6 ms).
- Both analog rails on by default.
- USB composite = USBTMC + mass-storage disk, always present (like the stock disk mode).
- Debug commands live in USBTMC as `DBG:*` SCPI commands (no CDC).
- Data-logger mode (see FR-38 in `../docs/dm40-feature-request.md`).

## Remaining work

- [x] `tools/dm40dbg.py` ported from CDC/pyserial to `/dev/usbtmc` + `DBG:` commands (a3427d4).
  - Still verify that `tools/reload.sh` and `tools/legacy/flash.py` work with it (no serial/ttyACM references remain).
- [x] On-device test of USBTMC (`*IDN?`, `DBG:PING?`/`DIAG?`/`LCD?` screenshot, `DBG:TAP`/`SWIPE`) and the mass-storage disk.  (code done; verify on device)
  - Disk is backed by the W25Q128 SPI flash (1f2bf30); README records `DBG:FLASH?` -> EF4018, but a full test (mount, write/read) is not logged.
- [ ] Faster redraw of the main DMM value and trend chart (target ~6 ms like stock: render to RAM, single window flush, chart column ring).
  - 4d2dc40 sped up digits and chart, but the ~6 ms target is not confirmed.
- [x] Item-by-item parity check vs stock -> `docs/parity.md`. DMM/scope/gen/BT/GUI/touch/USB
  tabulated; punch-list extracted. Deliberate deviations recorded (red value, no logo,
  English-only, temp F 1.8 not the stock 1.82 bug, combined views, USBTMC, gestures).
  Remaining parity gaps (from the punch-list):
  - [x] BT cmd-09 flags byte: real battery (power.bars) / charging / key-lock; bits per bt_meas_v2_6.h.
  - [x] BT module startup AT init (AT+SBNDM40<model> -> if OK: AT+SLED0 -> AT+RESET), once per power-on.
  - [x] Generator defaults Square/2 MHz; snap square 50k-100k DOWN to 50k; non-square >50k = 0 Hz silent.
        (note: app.rs:533 reclamps a persisted 0 Hz to 1 Hz on config load - left as-is, minor.)
  - [x] Touch: Goodix GT1158 fallback (addr 0x14, probe ID "1158" first, coords GUIx=GoodixY;
        side keys normalised to the same 1<<code). 0x5A path byte-identical. Untestable on this
        0x5A unit.
  - [x] Scope: slow timebases to 50 s/div (TB_MAX 29, roll from 200 ms/div, YT ceiling 1 s/div);
        stop-and-zoom re-renders the stored 26 K record at the selected timebase. Not done: horizontal
        pan (lives in app.rs/ui.rs), full 65 K record (DMARAM budget). Untested on hardware.
  - [x] Scope hardware (comparator) trigger, src/hwtrig.rs: wired into scope.rs (H0..H8 of
        docs/hwtrig-integration.md), DBG:HWTRIG[?|:ENABLE|:CAL?|:STAT?|:STAT:RESET]. Off by default
        (hwtrig::DEFAULT_ENABLED = false).  (code done; verify on device, then flip the default)
  - [ ] Confirm simultaneous DMM+scope accuracy (both-rails crosstalk). [scope.rs]
  - [x] Device/About page: Menu->Settings->Device (Vendor/Web/Model/Bootloader/Software/Hardware);
        HW+bootloader ver read from the stock boot-data block 0x080A4000 (survives app updates).
        SN + BT name omitted (not sourceable without inventing a value / a blocking AT round-trip).
- [ ] **Native-format firmware (top priority, user 2026-10-13).** Drop the RAM/sector-8 relocation
  scheme: build a full firmware in the stock update-file format, flashed like the stock one at
  0x08004000. Our image starts with a tiny shim "bootloader": power key held ~2 s at start
  (optionally with USB cable) or a watchdog-reset cause -> start the native updater (the one
  that brings up mass storage); otherwise run fw-rs. Shim as small and simple as possible.
  Must be unbrickable and allow easy stock <-> fw-rs toggling.
  HARD CONSTRAINT (user): recovery must NOT depend on fw-rs running or on fw-rs's MSC. With the
  app dead there is otherwise no way to put a file on the SPI FAT. The shim itself must give the
  host a way to place a stock dm40*.bin: jump into the FS-loader's own host-facing disk mode if
  it has one, else carry a minimal self-contained USB-MSC + SPI/FatFs in the shim.
  - [x] Architecture doc `docs/native-boot.md`. Key finding: a permanent FS-loader at
        0x080a0000 (own USB MSC + FatFs) reflashes any dm40*.bin from the SPI FAT on boot
        and is never overwritten by an app update -> unbrickable recovery already exists.
        Shim stays tiny (RCU_RSTSCK FWDGTRSTF, PD6 power key, SRAM magic + boot counter).
        dm40fw.py `pack` already computes both header words. fw-rs must move to XIP from
        0x08004000 (clock.rs to set PLL+FMC waitstates+cache; link.x reserve 0x08008000 hole;
        drop the SRAM copy in startup.rs).
  - [x] Opus review (`docs/native-boot-review.md`). Verdict: Stage 0 SAFE; Stage 1 NOT until
        dead-app recovery is closed. App reachable ONLY via FS-loader (runs every boot); it
        installs ANY dm40*.bin on EVERY boot (must delete after good install). MANDATORY before
        Stage 1: shim carries its OWN USB-MSC (Option B; fw-rs MSC is dead when recovery needed);
        PD6 release-then-hold gesture; clock.rs own PLL+FMC-WS+cache + VTOR=0x08004000; boot magic
        in backup SRAM; 12-byte 0x08008000 hole truly 0xFF.
  - [x] Stage 0 PROVEN on device (2026-10-13): staged dm40_v_2_6.bin on the FAT via fw-rs MSC,
        then COLD power-on -> the loader installed it to 0x08004000 and deleted the file (confirmed
        gone). App-independent recovery works. KEY FINDING: a WARM reset (SYSRESETREQ / DBG:RECOVER)
        does NOT trigger the loader install; only a COLD power-on does. So the shim's recovery =
        host stages a valid dm40*.bin via the shim's own MSC (Option B) + power-cycle. See the
        FS-loader-trigger investigation (docs/fsloader-install-trigger.md) for whether a marker can
        make install happen on a warm reset.
  - [x] Own fonts (prereq for independence): tools/mkstockfont.py extracts the 19 used fonts
        (714 glyphs, ~83 KB) from the stock image into src/stockfont_data.rs; stockfont.rs reads
        embedded data. Public API unchanged, pixel-identical. Feature-gated `embedded-fonts`
        (default build keeps reading stock flash so the RAM loader still links; native build turns
        it on). The native image has room for the 83 KB; sector-8 RAM layout does not.
  - [x] clock.rs: own PLL bring-up (`init_core`, measured stock RCU_PLL 0x29406419, no FLASH_ACR
        write, bounded waits, IRC16M fallback). Built, not yet run (test plan T1/T2).
  - [x] Native build (`./build.sh native` -> dm40-fw-native.bin): shim (FWDGT, power hold, reset
        flags, PD6 hold 2.5 s, WDT only after our own normal boot), recovery = zero the loader
        marker + reset, board init audit, fatren (rename installed dm40*.bin to _DM40PRV.BIN,
        cold boot + valid marker only). See docs/native-boot.md "Implementation". Hardware tests T1/T2/T3 pending.
  - [x] VENDOR UPDATE MODE found and proven on device (user's idea, 2026-10-13). Any dm40*.bin on the
        FAT (even 0 bytes / bad header) makes the loader zero the 0x08008000 marker, print
        "update error" and expose a WORKING USB disk on every boot until a valid image is installed
        via its "update" button. Header is checked before any erase (app sectors untouched).
        Boot gate is PA2, not the reset cause: ROM polls PA2 x1000; high -> ROM verifies the marker +
        body CRC and launches the app itself; low or invalid app -> loader install state. That is why
        "cold installed / warm didn't". Full analysis: docs/fsloader-install-trigger.md.
  - [x] Recovery = program 12 zero bytes over the 0x08008000 marker + SYSRESETREQ (works on warm reset,
        no FAT/SPI/USB/LCD in the shim); DBG:UPDATE / DBG:RECOVER do the same (dev flow: copy a
        dm40*.bin onto fw-rs's disk, DBG:UPDATE). Built (recovery ~490 B, test-recovery image
        1712 B); DM40RCVR.BIN / own-USB variants kept behind recovery-file / recovery-usb.
        Not yet run on hardware.
  - [x] Test: power-on holding the power key ~5 s with no dm40*.bin -> stock booted. No vendor
        button entry; PA2 is not the power key (probably a power-on-delayed signal: low for a while
        after a cold power-on, high on warm resets). So our marker-zero recovery is required; per code
        an invalid marker sends the ROM to the loader regardless of PA2 (to be confirmed by T2).
  - [x] Full verified flash dump (1 MiB, OB, OTP) taken before Stage 1: firmware/re/dumps/full-20261013/.
        RDP is level 1 (SWD/DFU only via mass erase), so the software recovery path matters.
  - [x] Review of the native image + shim (docs/native-review-2026-10-13.md): T1/T2 GO, T3 NO-GO until
        B1 (native stack overflow: fatren buffers live during main) and B2 (shim reads PD6 without
        pull-down; stock = input + pull-down, pressed = 1) are fixed; H1 fatren renames only the file whose
        header == marker; H2 key-hold trap (light backlight at once, cue before zeroing); H3 stale flags;
        crash counter so one panic doesn't force a reinstall. [fixes in progress]
  - [x] T1 on device (test-initcore over stock): init_core HXTAL OK, LCD/USB/disk/DMM/BT OK, FSMC live
        BCR1 0x5091 BTR1 0x0F04 BWTR1 0x0FF00201 (board.rs aligned), panel 0x9488, marker VALID.
  - [x] T2 on device (test-recovery): marker zeroed + WARM reset -> loader 'open file error' + working
        USB disk. Restored dm40_patched.bin through it.
  - [ ] Hardware tests per docs/native-boot.md: T1 test-initcore (RAM), T2 test-recovery (RAM,
        destructive-but-recoverable), T3 flash native via the loader, toggle stock <-> fw-rs.
  - [x] T3 / Stage 1 on device (2026-10-13 17:28): native dm40_fwrs.bin (commit 70bf333) installed via
        the loader screen; DBG:BOOT? NORMAL/HXTAL/VALID marker, CALBK WRITTEN (8 backup files on the disk),
        STACK 30760/39000, PD6 pull-down, DMM reading OK. Way back proven: dm40_patched.bin on the fw-rs
        disk + DBG:UPDATE -> loader installed patched stock (marker = its header, memrw answers).
        The loader (not the stock app) appears to delete the installed dm40*.bin: fatren found nothing to
        rename (code check in progress) -> instead regenerate the running firmware's update file from
        flash (header = marker, body = 0x08004000..) as a backup file on the disk.
  - [ ] T3 remaining: recovery triggers on native (power-key 4 s hold + beep + 0.5 s confirm; DBG:RELEASE
        twice within 60 s -> recovery); scope view stack high-water; first-boot timing.
  - [x] Native bugs seen: boots into AC+DC mV (persisted-mode mapping after the persist v3 change?);  (code done; verify on device)
        trend chart plots the separate AC/DC phases; AC part 40 mV on a DC calibrator input (AC zero?).
  - [x] Stage 2: retire stub/, flash.py, BT loader path; update README/tools.  (code done; verify on device)
- [x] Data-logger mode (FR-38) implemented: src/logger.rs, main-loop polled constant interval,
      timestamps + sequence numbers + drop counter, HY3131 fast mode, LOG:* SCPI. Not tested on device.
- [x] Code review 2026-10-13 (docs/review-2026-10-13.md): H2/M1/M2/M3/M5 + lows fixed; H1/M4 call sites
      in main.rs/msc.rs being applied by the native agent.
- [x] UI: HOLD repaint fixed (label only drawn inside reading(), skipped while held), Settings back
      navigation (parent chain), Settings rows like vendor (8 rows, value next to label, "< Back"
      title bar), compact DMM+scope view (22 px bar, 30 px value, scope +56 px). Not verified on device.
- [ ] UI/UX review punch-list (docs/ui-review.md): drag-misread-as-swipe bug (gesture.rs:77), unified
      top bar with lock/BT/battery on all views, swallow the wake tap after dim, scope right-panel
      button labels, page dots, scope trigger handle, generator keypad prefill.
- [x] CSV-to-disk recording on the DMM screen, swipe-triggered (user). Needs a FAT writer in fw-rs
      (msc.rs/spiflash.rs only expose a block device today), the sampling path from FR-38, and a
      DMM-screen record indicator + swipe gesture. Relates to FR-04 / FR-35. Queued after logger + UI.
      (code done; verify on device) src/csvlog.rs: one-finger swipe down on the DMM page = start / stop,
      toasts, red REC in the status blocks, LOG:CSV ON|OFF / LOG:CSV?, power-off / reset commit the tail.
      Host writes are refused while recording; start refused ("Disk in use by PC - eject first") while
      a PC has the disk mounted.

## Feature requests to add (from ../docs/dm40-feature-request.md; user: add autohold + useful ones)
- [x] **FR-01 AutoHold (P0, critical).** Auto-freeze the reading when it stabilises, re-arm on a new  (code done; verify on device)
      signal, beep on capture (Fluke/Keysight style). Algorithm sketch in Appendix B of the FR doc.
      Touches meter.rs/app.rs/ui.rs. Queue after the UI agent (HOLD/layout) to avoid conflict; builds
      on fixing the plain HOLD first.
- [x] FR-02 MIN/MAX/AVG (P0): MIN/MAX exist; add AVG (running average, no auto-off while active).  (code done; verify on device)
- [x] FR-05 smoothing + selectable sample rate (P1): slow/fast conversion + display smoothing.  (code done; verify on device)
      Rate chip "MED 9.4/s" under the DMM chart timebase (tap = SLOW/MED/FAST, grey "n/a" where the
      rate does not apply), Settings 2/2 "DMM rate" / "Smoothing", SENS:RATE[:PER?], CALC:SMO[:STAT|:RESP],
      persisted in p[38]. CSV log and MIN/MAX/AVG stay unsmoothed.
- [x] FR-06 Hi/Lo limits (P1): pass/fail thresholds with a beep.  (code done; verify on device)
- [x] FR-16 screenshot + log gallery (P1): save screenshots/CSV to the SPI disk and browse them.  (code done; verify on device)
      Backend + SCPI (csvlog list/delete, MMEM:CAT? / MMEM:DEL, USB only for DEL); on-screen gallery
      src/gallery.rs, opened from the main menu's top row "Gallery: screenshots and logs".
- [x] FR-28 generator DC offset (P1): adjustable offset (note: real bipolar offset is HW-4).  (code done; verify on device)
      Offset box on the generator page (keypad; OK on an empty field = auto), SOUR:VOLT:OFFS <V>|AUTO, persisted p[39].
- [x] FR-35 CSV export from DMM (P1): = the CSV-to-disk item above, with timestamps.  (code done; verify on device)
- Likely already done (verify, then tick): FR-09 REL, FR-10 idle auto-off, FR-12 screen dim,
  FR-18 touch lock, FR-27 generator keeps running in DMM. FR-33 is USBTMC+MSC (we chose DBG over CDC).
- [ ] Repo tidy-up (user request): plan in ../TIDY-PLAN.md (46f9352); .gitignore for dumps/backups and README
      fixes done (3aedb6f, efb7e50). Moves/deletes wait for the user's decisions: vendor-derived material
      (app_decompile.c is tracked) and BT MACs in docs/bt-protocol.md / tools/dm40.py while origin is a public GitHub repo.

## From user community feedback (docs/user-feedback.md)
- [ ] UF-01 (P0) Verify continuity CMPO beeper + diode beep classes on device, latency <10 ms (most-praised stock feature). §5.2
- [x] UF-04 (P1) Scope AUTO keeps the user's AC/DC coupling (app.rs autoset_start forces DC = stock bug, 4 users). §6.1  (code done; verify on device)
      AC search first; nothing in AC -> the same search in DC, kept with the toast "AUTO: DC (no AC signal)".
- [x] UF-14 (P1) Generator: clamp to the wave's limit + visible note instead of a silent 0 Hz (revert the parity snap; owners call it a bug). §7  (code done; verify on device)
- [x] UF-15 (P1) Generator: show the actual quantised output frequency (and in SOUR:FREQ?); later a DDS for finer steps. §7  (code done; verify on device)
- [x] UF-03 (P1) AC+DC sub-mode on DM40A (owners proved the A hardware does it when the app commands it). §10  (code done; verify on device)
- [ ] UF-02 (P1) Cold-junction self-heating with both rails/concurrent instruments: measure vs stock, correct or power down. §3
- [ ] UF-19 (P1) Measure fw-rs battery draw vs stock (10-15 h baseline); decide on a scope-on-demand power policy. §8
- [x] UF-17 (P1) 3-finger swipe-down screenshot to the USB disk (stock gesture, missing in fw-rs; shares the FAT writer). §2, §10  (code done; verify on device)
- [x] UF-18 (P1) 2-finger swipe-down power-off (stock gesture; touch point count is available). §8  (code done; verify on device)
- [x] UF-20 (P1) On first boot write CALBAK.BIN (cal record + boot data + UID, CRC) to the USB disk, written when missing or changed (compare-first). §4  (code done; verify on device)
- [x] UF-21 (P1) Make AUTO+ (V-first, already ported: v_auto=1) the default V autorange; warn when mV-first AUTO is chosen. §5.1  (code done; verify on device: an upgraded unit boots in AUTO+; Range page warning)
- [ ] UF-05 (P1) Scope drag axis lock (dominant axis after ~12 px). §6.1
- [ ] UF-09 (P1) Scope horizontal trigger position (pre-trigger %) + double-tap to centre. §6.1
- [x] UF-32 (P1) CSV files: '.' decimals, ISO-8601 timestamps, header (model/fw/function/range). §10  (code done; verify on device. No RTC: time_s since the start + the start uptime, no ISO dates)
- [x] UF-37 (P1) Native shim keeps the short-press power-on (stock V2.1 behaviour). §8  (code done; verify on device)
- [x] UF-16 (P2) Generator ramp direction: gen.rs falls, preview rises; verify on scope and align. §7  (code done: ramp rises; verify on device)
- [ ] UF-11 (P2) Scope FFT with labelled axes + automatic peak (f, dBV) readout. §6.2
- [ ] UF-12 (P2) Full probe attenuation list x0.1..x1000 (stock scope_probe[13]). §6.2
- [ ] UF-07/08/10 (P2) Scope: corner-tap timebase + list picker, one-tap cursors, V/div +/- shown as zoom. §6.1
- [ ] UF-13 (P2) Save scope record as CSV to disk; software pulse-width/runt trigger. §6.2
- [x] UF-23 (P2) PT100/PT1000/NTC temperature via the ohm path. §3  (code done; verify on device)
- [ ] UF-25/26/27/28/35 (P2) DMM small fixes: extra-digit option, 4-20mA% label, store-while-HOLD, volume semantics, MIN/MAX reset. §4-§5
- [ ] UF-29 (P2) On-device Help page (gestures, resets, input limits). §9
- [ ] UF-24/30/31/36/38/41 (P2) Tempco comp, fast bargraph, stylus sensitivity, BT on all views at 50 ms, SCPI host script, sinc interp. §3-§10
- Community-confirmed existing items (priority signal only): native boot/recovery; FR-26 temperature offset (raise to P1);
  FR-36 user calibration; FR-13 sub-mode memory; FR-01/FR-02 (AVG on chart); FR-04/CSV record + FR-38 logger; FR-05;
  FR-09/FR-24 stray-C REL; FR-10; FR-14/FR-17; FR-19; FR-21; FR-28/29/32; FR-34; USBTMC; ui-review #1, #4, #5, #8, #10, #11.

## User decisions 2026-10-13 (on community feedback)
- [x] Temperature: user offset (cold-junction / reading), plus sensors PT100, PT1000 and common NTCs  (code done; verify on device)
      (10k B3950, 10k B3435, 100k, ...). [queued after the button/60000 agent: dmm.rs/hy3131.rs]
- [x] Gestures: 3-finger swipe = screenshot to the SPI disk, 2-finger swipe = power off (as stock).  (code done; verify on device)
      [queued: gesture.rs/touch.rs/app.rs]
- [x] AC+DC on ALL models (DM40A hardware supports it). [in progress with the 60000-count agent]  (code done; verify on device)
- [x] Always 60000 counts (DM40C behaviour) regardless of the model byte; IDN/Device keep real model.  (code done; verify on device)
      [in progress] + physical power-key short/long/double press as in stock; PD6 pressed level.
- [ ] Scope UI redesign: research + plan in docs/scope-ui-plan.md [agent], then step-by-step impl.
- [x] Calibration (0x080C0000) + boot data (0x080A4000) + UID copied to the SPI disk at every boot  (code done; verify on device)
      (only when missing/changed) so they're reachable over USB. [agent]
- [x] Analog rails: default OFF (stock-like alternation per page); both on automatically in the  (code done; verify on device)
      DMM+Scope combined view; SCPI command to force both on (e.g. SYST:RAIL BOTH|AUTO). Measure
      consumption/heating vs stock with the BT power meter. [queued: app.rs/power.rs/scpi.rs]
- [x] +1 display digit (at least DC voltage): show one more digit than the 60000-count format where the  (code done; verify on device)
      HY3131 resolution/noise allows (slow OSR mode, averaging). Always on, not a setting: draw the extra
      digit in a smaller font after the main digits (user). Check AD1 code-per-count headroom per range
      (dmm.rs, hy3131.rs) first.
- [x] Milliohm resolution on the lowest resistance range (units of mΩ): finer format on the 600 Ω range,  (code done; verify on device)
      using slow OSR + averaging + REL (lead-zero) — measure noise first; consider a dedicated low-Ω mode
      (higher test current if the HY3131 range tables allow).

## Backlog (user ideas)
- [ ] SCPI over Bluetooth serial (classic SPP / BLE UART): the same parser as USBTMC, so standard
      software (sigrok/PyVISA over a serial port, Keysight-style loggers) works wirelessly. Keep the
      stock BT protocol (cmd 0x09 etc.) working alongside: dispatch on the first byte (ASCII
      letter/'*' = SCPI line ending in \n, else the stock binary frame). Check the BT module's
      throughput for READ? polling and WAV:DATA?.

## Found in the screenshot session 2026-10-13 (native db679a3)
- [x] READ?/MEAS? take ~8 s (wait for a full averaging window that never fills -> 20 conversions);  (code done; verify on device)
      PyVISA/kernel usbtmc time out and later replies arrive shifted. [wiring agent]
- [x] USBTMC wedges: after a timed-out query (or SmuView/sigrok) every reply is empty for minutes;  (code done; verify on device)
      INITIATE_CLEAR does not help, only a USB bus reset. Pending waits must expire, new commands
      must cancel them, class CLEAR/ABORT requests must resync. [wiring agent]
- [x] SCPI function change (CONF/MEAS) does not redraw like a touch: function bar keeps the old  (code done; verify on device)
      button/sign, chart keeps the previous function's data and axis. [wiring agent]
- [x] AC+DC sub-readings line: AC tilde detached at the far left, DC sign drawn after the AC value  (code done; verify on device)
      (user: "значки разъехались"). [ui agent]
- [x] Temperature sensor label in the status bar: glyphs overlap for PT100/PT1000/NTC names. [ui agent]  (code done; verify on device)
- [ ] AC+DC current reads ~150 uA AC with nothing connected; AC V of a 0.5 V DC source reads
      23-56 mV and decays slowly (compare with stock before calling it a bug).
- [x] 3-finger screenshot blocks the main loop ~8 s (BMP write to SPI flash); USB and DMM stall.  (code done; verify on device)
      Now incremental (screenshot.rs state machine, <= ~3 ms per main-loop pass; frame captured
      run-length coded into 96 KiB SRAM, screen frozen only while captured); SPI1 /16 -> /4 (12.5 MHz).
      Verify: loop max in DBG:DIAG? during a shot, SCPI READ? keeps answering, S00x.BMP correct.
- [x] Data logger (LOG:STAT ON) has no on-screen indicator; the DMM reading freezes silently.  (code done; verify on device)
      "DATA LOGGING" panel (dialog::log_panel, rate + sample count, 1 s refresh), view repainted on stop.
- [ ] sigrok/SmuView: libsigrok scpi-dmm opens the device over USBTMC (*OPC?, *IDN? answered) but
      drops it because "Alientek,DM40A" is not in its model table. Add a DM40 profile to libsigrok
      src/hardware/scpi-dmm (command set like the 34401A: CONF:<f>, READ?, FUNC?), test with
      sigrok-cli -d scpi-dmm:conn=usbtmc/<bus>.<addr> and SmuView, then send upstream.
- [ ] Scope drag axis lock (user: horizontal drags knock the vertical): dominant axis decided at the
      slop and locked until release; trigger drag relative, not absolute; dashed trigger line while
      dragging; optional press-and-hold-to-grab flag to A/B [agent].
- [ ] Scope touch: pinch = scale (timebase / V/div, 1-2-5 snap), one-finger drag = offset, draggable
      trigger level drawn as a dashed line + marker; gesture map without clashes with view swipes.
      Rendering approach (own strip compositor vs embedded-graphics / Slint / LVGL) evaluated in
      docs/combined-view-design.md [design agent], then implement with the scope redesign.
- [x] Combined DMM+scope view design: meter strip inside the scope page, 3-size type hierarchy, gesture
      map, renderer choice (own layered compositor, no GUI library) -> docs/combined-view-design.md (8714bdd).
- [ ] Implement combined view C1-C5 + runtime drag/grab switch G1 [agent]; then G2 markers/bubbles,
      G3 pinch (measure 2-finger tracking first), G4 A/B on device, R1 redraw timing + compositor, U1-U3.
- [ ] AC+DC: the AC part reads ~5% low vs plain AC V on a 40 Hz 0..9.6 V square (4.578 vs 4.812 V);
      combined view FREQ reads 13.3 Hz instead of 40 Hz [AC agent].
- [ ] sigrok: scpi-dmm has no option to force a model (scan options are only conn/serialcomm), so
      either the libsigrok profile [agent] or an optional *IDN? compatibility mode in fw-rs.

## Device run 2026-10-13 19:10, native e07c11a (+docs) installed OK
- Verified: READ? ~0.5 s, MEAS:VOLT:AC? 0.5 s, MEAS:FREQ? 1.0 s; USBTMC survives a libusb timeout
  (sigrok) and answers afterwards; SCPI mode change highlights the right function button and clears
  the chart; AC+DC sub-reading signs next to their values; PT100/NTC labels legible; mOhm resolution
  (10.117 Ohm); MIN/AVG/MAX + session time on the chart.
- Calibrator (MR9270S+, resistance mode func 3 mode 1, i32 = 0.1 Ohm, accepted up to 4000 = 400 Ohm):
  with the DM40's test current the simulated resistance saturates at ~101 Ohm. In range: 10.03 @ 10,
  50.28 @ 50, 100.98 @ 100 Ohm (+1 % near the top = simulator compression); 0 Ohm reads 1.59 (sim
  floor). PT100 -100 C -> -99.76; -50 C -> -48.77; NTC10k3950 200 C -> 198.94; PT1000/NTC100k only
  below the simulator's ceiling -> OL (correct, out of table). Re-check with real resistors (PT1000,
  NTCs at room temperature need 1-100 kOhm).
- [ ] H1: MEAS:RES? (and CONF:RES + READ?) answers 9.9E37 "Data stale": the ohm settle is 2.5 s, the
      wait expires at 1.9 s. Start the wait at max(t0, dmm.deadline()); for sigrok (libusb timeout is a
      hard 1 s) cap a single READ? at ~900 ms and give libsigrok a longer delay after a RES/CAP/TEMP
      function change (profile in ../libsigrok-dm40).
- [ ] Review M1 (CHECK_CLEAR_STATUS pending after the last packet left), L1 (ABORT_BULK_IN should set
      cleared), L2 (don't read Bulk-OUT while a command is pending), L3 (mask EPENA/EPDIS in the
      toggle-reset RMW) from docs/native-review-2026-10-13.md "Pre-flash review e07c11a".
- [ ] AC agent (no fw bug proven; same path as stock): decide on device -
      (a) 40 Hz PWM: plain AC V on manual 60 V vs auto vs AC+DC (5 % low AC part = r1 range or RMS
          settling after each phase switch);
      (b) 1 V 50 Hz sine: AC+DC AC part must equal plain AC V;
      (c) A input open/shorted: plain CURR:AC uA vs AC+DC (150 uA);
      (d) FREQ in the combined view with/without the scope probe on the node (13.3 Hz = 40/3).
      e07c11a run: DC 4.8162 V and AC 4.8137 V steady, but the AC+DC AC part wanders 4.5..5.3 V phase
      to phase (total 6.4..7.4 V, expected 6.81) -> RMS not settled after the phase switch [agent].
- [ ] libsigrok scpi-dmm DM40 profile (../libsigrok-dm40, branch alientek-dm40, 8c292c31): DC V works
      on the device via usbtmc/1209.0001; RES times out (H1). Then send upstream; consider an
      Agilent-style CONF? reply so the profile gets range/resolution.
- [x] firmware/dm40fw.py is untracked but build.sh needs it (packs the native image): commit it. (165b37a)

## UI toolkit decision (user, 2026-10-13)
- Keep our own renderer + layer compositor (docs/combined-view-design.md §7) for now. Considered:
  Slint (native Rust, MCU line renderer, real widgets/swipes; GPLv3 or commercial for embedded),
  LVGL via lv_binding_rust (C, MIT, experimental bindings), egui (needs std + framebuffer/GPU-style
  rasteriser: does not fit 125 KB SRAM). Revisit with a one-screen Slint/LVGL prototype if the
  hand-made widgets stay clumsy. Meanwhile: layout rules + automated overlap check for every screen.
- [ ] Measure the real DMM conversion period per rate (SENS:RATE:PER?, after the integration build): code
      assumes ~9.4/s at OSR 32768 (stock-like), the datasheet formula OSR/SYSCLK gives ~150/s — settle it.
- [ ] Data logger runs at 38 Hz at OSR 2048 although AD1 allows up to ~2400 Hz by the formula: the SPI
      read loop limits it — profile (bit-banged SPI on PC10-12?) and speed up (hardware SPI / DMA / read
      only the AD1 data bytes).
      Device test: LOG:OSR 0..7 sweep with LOG:RATE? and the noise (counts) per code on a steady 0.5 V,
      to fix the real rate/noise table (OSR 256 ~1 kHz by the formula, unverified).
