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

## Next up (groomed 2026-10-14)

Device needed (in this order):
1. UF-01 continuity CMPO beeper + diode beep, latency < 10 ms.
2. Both-rails crosstalk: DMM accuracy with the scope running; combined-view FREQ (13.3 Hz on 40 Hz);
   AC V of a 0.5 V DC source decaying.
3. T3 recovery triggers on the native image (power-key hold, DBG:RELEASE x2), scope stack high-water.
4. DBG:PROF? numbers: scope drag freeze (touch -> frame), DMM redraw time.
5. Pinch on the device, two-finger report rate / stability first (DBG:TOUCH?).
6. UF-02 cold-junction self-heating vs stock; UF-19 battery draw vs stock.
7. The "verify on device" ticks below (sweep them during the runs above).

Code work (in this order):
1. Faster DMM value + chart redraw (~6 ms target).
2. (done 2026-10-21: UF-11 FFT, UF-13 scope CSV + pulse / runt trigger, UF-08/09/10, UF-41 wired.)
3. Wire src/bargraph.rs into DmmView (UF-30).
6. libsigrok DM40 profile: device retest over USBTMC (all functions passed in the emulator), send upstream.

## Remaining work

- [x] `tools/dm40dbg.py` ported from CDC/pyserial to `/dev/usbtmc` + `DBG:` commands (a3427d4).
      `tools/reload.sh` is the native reload flow (fa29a80); no serial/ttyACM references remain.
- [x] On-device test of USBTMC (`*IDN?`, `DBG:PING?`/`DIAG?`/`LCD?` screenshot, `DBG:TAP`/`SWIPE`) and
      the mass-storage disk (W25Q128, 1f2bf30).  (device-verified: used in every device run since;
      CALBK files written, dm40_patched.bin copied onto the disk for DBG:UPDATE)
- [ ] Faster redraw of the main DMM value and trend chart (target ~6 ms like stock: render to RAM,
      single window flush, chart column ring). 4d2dc40 sped it up; measure with DBG:PROF? (95287a8).
- [x] Item-by-item parity check vs stock -> `docs/parity.md`. DMM/scope/gen/BT/GUI/touch/USB
  tabulated; punch-list extracted. Deliberate deviations recorded (red value, no logo,
  English-only, temp F 1.8 not the stock 1.82 bug, combined views, USBTMC, gestures).
  Remaining parity gaps (from the punch-list):
  - [x] BT cmd-09 flags byte: real battery (power.bars) / charging / key-lock; bits per bt_meas_v2_6.h.
  - [x] BT module startup AT init (AT+SBNDM40<model> -> if OK: AT+SLED0 -> AT+RESET), once per power-on.
  - [x] Generator defaults Square/2 MHz; snap square 50k-100k DOWN to 50k; non-square >50k = 0 Hz silent.
        (superseded by UF-14 clamp + note.)
  - [x] Touch: Goodix GT1158 fallback (addr 0x14, probe ID "1158" first, coords GUIx=GoodixY;
        side keys normalised to the same 1<<code). 0x5A path byte-identical. Untestable on this
        0x5A unit.
  - [x] Scope: slow timebases to 50 s/div (TB_MAX 29, roll from 200 ms/div, YT ceiling 1 s/div);
        stop-and-zoom re-renders the stored 26 K record at the selected timebase. Horizontal
        position = the stock T marker drag (stock scope port). Not done: full 65 K record (DMARAM budget).
  - [x] Scope hardware (comparator) trigger, src/hwtrig.rs (H0..H8 of docs/hwtrig-integration.md),
        DBG:HWTRIG[?|:ENABLE|:CAL?|:STAT?|:STAT:RESET]. On by default since 49ec62e (enabled in
        Scope::new; hwtrig::DEFAULT_ENABLED stays the module default). (device-verified: Auto/Normal/
        Single, 100 us..100 ms/div all T'D, 0 misses)
  - [ ] Confirm simultaneous DMM+scope accuracy (both-rails crosstalk). Fold in: combined-view FREQ
        read 13.3 Hz on a 40 Hz input (= 40/3; with/without the scope probe on the node), and AC V
        of a 0.5 V DC source reading 23-56 mV and decaying slowly (compare with stock first).
  - [x] Device/About page: Menu->Settings->Device (Vendor/Web/Model/Bootloader/Software/Hardware);
        HW+bootloader ver read from the stock boot-data block 0x080A4000 (survives app updates).
        SN + BT name omitted (not sourceable without inventing a value / a blocking AT round-trip).
- [x] **Native-format firmware (top priority, user 2026-10-13).** (Stage 1 on device 2026-10-13;
  T3 recovery triggers still open below.) Drop the RAM/sector-8 relocation
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
        write, bounded waits, IRC16M fallback). (device-verified by T1)
  - [x] Native build (`./build.sh native` -> dm40-fw-native.bin): shim (FWDGT, power hold, reset
        flags, PD6 hold 2.5 s, WDT only after our own normal boot), recovery = zero the loader
        marker + reset, board init audit, fatren (rename installed dm40*.bin to _DM40PRV.BIN,
        cold boot + valid marker only). See docs/native-boot.md "Implementation".
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
        (device-verified by T2 and the T3 way back)
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
        crash counter so one panic doesn't force a reinstall. (fixed in 70bf333 before T3)
  - [x] T1 on device (test-initcore over stock): init_core HXTAL OK, LCD/USB/disk/DMM/BT OK, FSMC live
        BCR1 0x5091 BTR1 0x0F04 BWTR1 0x0FF00201 (board.rs aligned), panel 0x9488, marker VALID.
  - [x] T2 on device (test-recovery): marker zeroed + WARM reset -> loader 'open file error' + working
        USB disk. Restored dm40_patched.bin through it.
  - [x] T3 / Stage 1 on device (2026-10-13 17:28): native dm40_fwrs.bin (commit 70bf333) installed via
        the loader screen; DBG:BOOT? NORMAL/HXTAL/VALID marker, CALBK WRITTEN (8 backup files on the disk),
        STACK 30760/39000, PD6 pull-down, DMM reading OK. Way back proven: dm40_patched.bin on the fw-rs
        disk + DBG:UPDATE -> loader installed patched stock (marker = its header, memrw answers).
        The loader (not the stock app) deletes the installed dm40*.bin (340faca) -> the running
        firmware's update file is regenerated from flash as a backup on the disk (db679a3).
  - [ ] T3 remaining: recovery triggers on native (power-key 4 s hold + beep + 0.5 s confirm; DBG:RELEASE
        twice within 60 s -> recovery); scope view stack high-water; first-boot timing.
  - [x] Native bugs seen: boots into AC+DC mV (persisted-mode mapping after the persist v3 change?);
        trend chart plots the separate AC/DC phases; AC part 40 mV on a DC calibrator input (AC zero?).
        (f4dc59d; verify on device)
  - [x] Stage 2: retire stub/, flash.py, BT loader path; update README/tools. (fa29a80; verify on device)
- [x] Data-logger mode (FR-38) implemented: src/logger.rs, main-loop polled constant interval,
      timestamps + sequence numbers + drop counter, HY3131 fast mode, LOG:* SCPI. Not tested on device.
- [x] Code review 2026-10-13 (docs/review-2026-10-13.md): H2/M1/M2/M3/M5 + lows fixed; H1/M4 call sites
      in main.rs/msc.rs applied by the native agent.
- [x] UI: HOLD repaint fixed (label only drawn inside reading(), skipped while held), Settings back
      navigation (parent chain), Settings rows like vendor (8 rows, value next to label, "< Back"
      title bar), compact DMM+scope view (22 px bar, 30 px value, scope +56 px). Not verified on device.
- [ ] UI/UX review punch-list (docs/ui-review.md §4) remainder. Done: #1 drag-misread-as-swipe
      (gesture.rs: a drag never swipes), #2 unified status block with lock/BT/battery on every bar,
      #5 page dots, #4/#8 superseded by the stock scope port (stock right panel, markers, T/trigger
      handles). Done 2026-10-21 (verify on device):
      - #3 wake gate (70bed89): after an idle dim the first touch / side key only wakes; input is
        ignored for 500 ms (stock FUN_08020248) and the rest of the waking touch is dropped.
      - #10 generator keypad prefill (579f951): current value + unit, drawn selected; the first
        digit / '.' / backspace replaces it; OK on an untouched prefill keeps the setting.
      - #9 generator bar status + tappable RUN/STOP (22e7dd6).
      - #11 pressed-key feedback on the keypads (7dad1b9); menu tiles / settings rows / gen boxes
        still have none (the scope page has its own press_fx).
      - #12 backlight / volume -/+ steppers, clamped (1f19f05 drawing, 7e2abce logic).
      - #13 already in: DMM lightning >= 100 V (stock), combined strip HV badge >= 30 V and red
        ring >= 100 V. DMM page >= 30 V: done 2026-10-21 as the stock cue itself (value 0xC9A0 red in V
        functions from 30 V, gui_v2_6.h:175; the value is orange otherwise).
      - #14 won't do: the preview is the stock geometry (4,85,239,231; stock UI is the baseline);
        the bar now carries the status (#9).
      - #15 the duplicate Keypad/ScopeMenu arms are gone (ScopeMenu removed by the scope port).
        Dead code removed (2026-10-21): ui::bar / button_row / DMM_BUTTONS / sub_label / val_line,
        src/fonts.rs + src/text.rs (only those used them), tools/mkfont.py + fonts/Roboto-*.
      Open:
      - [x] UF-08 cursor tab "CUR" at the graph bottom (108,280): off -> X -> Y -> off, as the
        stock cursor measurements (X2-1 + Xf / Y2-1) in the measurement row; "off" restores the
        selection. (2c47fa5; verify on device)
      - [x] UF-10 CH1 V/div buttons drawn as magnifiers: top = zoom in (smaller V/div), bottom =
        zoom out (2c47fa5; stock-diff: btn_vdiv_up/down ~9 % by design; verify on device)
      - [x] UF-09 double tap = CH1 position 0 + trigger at 50 % (stopped: centred on the trigger
        point); only where a single tap does nothing (Corners: the graph middle; Off: anywhere;
        never in Halves, every tap steps there). (2c47fa5; verify on device)
- [x] CSV-to-disk recording on the DMM screen, swipe-triggered (user). (a1a7e74; verify on device)
      src/csvlog.rs: one-finger swipe down on the DMM page = start / stop,
      toasts, red REC in the status blocks, LOG:CSV ON|OFF / LOG:CSV?, power-off / reset commit the tail.
      Host writes are refused while recording; start refused ("Disk in use by PC - eject first") while
      a PC has the disk mounted.
- [x] Compact binary logs, CSV only on read (owner 2026-10-21: "not CSV on the disk, RLE or a
      standard compressed format; CSV only for export"). docs/compact-log-format.md: own DLG format
      (adaptive Rice of delta-of-delta time + 6-digit value deltas, CRC chunk per commit) chosen over
      gzip / sigrok / FLAC on measured data: 0.9..1.9 B per DMM reading vs ~21 B CSV (11..26x; 13 MB of
      disk = 325 h at 10/s instead of 17 h); scope record 53 KB vs 692 KB.
      - [x] src/dlog.rs codec + CSV generator; tools/dlog-test: decoded CSV byte-identical to the old
            writer for 9 data sets, random access, truncation at every byte keeps every whole chunk, bit
            flips contained. tools/dlog.py: standalone decoder (same bytes).
      - [x] csvlog.rs records LOGnnn.DLG (same swipe / LOG:CSV UI); numbers shared with legacy LOGnnn.CSV;
            fatren-test: md5 of the decoded CSV == the old writer's LOG000.CSV, cut sweeps, disk full.
      - [x] scope "Save CSV" writes SCOPnnn.DLG (codes + map), CSV md5 == the old SCOPnnn.CSV.
      - [x] Export: virtual LOGnnn.CSV / SCOPnnn.CSV on the read-only LUN 1 (dlog::refresh ->
            vdisk::set_extra, refreshed when a recording ends / a delete / a host write ends);
            SCPI MMEM:DATA? "<name>"[,off[,len]] (LOGnnn.CSV generated, any file raw);
            tools/dm40scpi.py get LOG000.CSV. Gallery reads .DLG (exact counts).
      - [x] Emulator: LOG:CSV ON 60 s (154 readings at 2.5/s): LOG000.DLG 455 B, CSV 3108 B; MMEM:DATA?
            CSV == dlog.py of the DLG == LUN-1 LOG000.CSV read over USB/IP MSC.
      - [ ] Verify on device: swipe REC 10 min, LUN 1 shows LOG000.CSV on Windows / Linux after the
            recording (re-plug or eject if it does not appear: UNIT ATTENTION is held while mounted),
            open in a spreadsheet; scope Save CSV -> SCOP000.CSV on LUN 1; gallery tile of a .DLG;
            DBG:PROF? csv lap during a LUN-1 refresh with a large log (~0.1 s per 100 KB expected);
            DBG:DIAG? STACK high-water after a LUN-1 refresh with several logs (~2 KiB frame);
            copy a file to the DM40 disk: LUN 1 re-reads once, 2 s after the copy, not per write.
      - [ ] Owner 2026-10-21: drop legacy text LOG*.CSV / SCOP*.CSV support on the SPI disk (gallery parser,
            numbering, list/delete): only .DLG is written and read; CSV exists only as the generated export.
      - [ ] FR-38 logger to disk (DLG kind 3 reserved): needs pre-allocated clusters (commit stalls
            lose ~290 samples at 640/s); see the doc section 3.

## Feature requests to add (from ../docs/dm40-feature-request.md; user: add autohold + useful ones)
- [x] **FR-01 AutoHold (P0, critical).** Auto-freeze the reading when it stabilises, re-arm on a new
      signal, beep on capture (Fluke/Keysight style). (adcfa23; verify on device)
- [x] FR-02 MIN/MAX/AVG (P0): MIN/MAX exist; add AVG (running average, no auto-off while active).  (adcfa23; verify on device)
- [x] FR-05 smoothing + selectable sample rate (P1): slow/fast conversion + display smoothing.  (7399d86, 0cb58e8; verify on device)
      Rate chip under the DMM chart timebase (tap = SLOW/MED/FAST, grey "n/a" where the
      rate does not apply), Settings 2/2 "DMM rate" / "Smoothing", SENS:RATE[:PER?], CALC:SMO[:STAT|:RESP],
      persisted in p[38]. CSV log and MIN/MAX/AVG stay unsmoothed. Rates: see "Measure the real DMM
      conversion period" below.
- [x] FR-06 Hi/Lo limits (P1): pass/fail thresholds with a beep.  (adcfa23; verify on device)
- [x] FR-16 screenshot + log gallery (P1): save screenshots/CSV to the SPI disk and browse them.  (d74b221, 2fcf080; verify on device)
      Backend + SCPI (csvlog list/delete, MMEM:CAT? / MMEM:DEL, USB only for DEL); on-screen gallery
      src/gallery.rs, opened from the main menu's top row "Gallery: screenshots and logs".
- [x] FR-28 generator DC offset (P1): adjustable offset (note: real bipolar offset is HW-4).  (300e530, 8cf32aa; verify on device)
      Offset box on the generator page (keypad; OK on an empty field = auto), SOUR:VOLT:OFFS <V>|AUTO, persisted p[39].
- [x] FR-35 CSV export from DMM (P1): = the CSV-to-disk item above, with timestamps.  (verify on device)
- [x] FR-09 REL, FR-10 idle auto-off (+ countdown warning), FR-12 screen dim, FR-18 touch/key lock,
      FR-27 generator keeps running in DMM: present in code (2da9c98, 2170418, concurrent instruments).
      FR-33 is USBTMC+MSC (we chose DBG over CDC). (verify on device)
- [x] Repo tidy-up (user request): ../TIDY-PLAN.md executed 2026-10-13 (26ba0e5..5d3d005); the repo is
      now private (wirenboard/opendm40). Left to the user: whether vendor-derived material
      (firmware/re/analysis/app_decompile.c) and BT MACs stay tracked if it ever goes public again.

## From user community feedback (docs/user-feedback.md)
- [ ] UF-01 (P0) Verify continuity CMPO beeper + diode beep classes on device, latency <10 ms (most-praised stock feature). §5.2
- [x] UF-04 (P1) Scope AUTO keeps the user's AC/DC coupling (app.rs autoset_start forces DC = stock bug, 4 users). §6.1  (bdec75d; verify on device)
      AC search first; nothing in AC -> the same search in DC, kept with the toast "AUTO: DC (no AC signal)".
- [x] UF-14 (P1) Generator: clamp to the wave's limit + visible note instead of a silent 0 Hz (revert the parity snap; owners call it a bug). §7  (300e530; verify on device)
- [x] UF-15 (P1) Generator: show the actual quantised output frequency (and in SOUR:FREQ?); later a DDS for finer steps. §7  (300e530; verify on device)
- [x] UF-03 (P1) AC+DC sub-mode on DM40A (owners proved the A hardware does it when the app commands it). §10  (verify on device)
- [ ] UF-02 (P1) Cold-junction self-heating with both rails/concurrent instruments: measure vs stock, correct or power down. §3
- [ ] UF-19 (P1) Measure fw-rs battery draw vs stock (10-15 h baseline); decide on a scope-on-demand power policy. §8
- [x] UF-17 (P1) 3-finger swipe-down screenshot to the USB disk (stock gesture, missing in fw-rs; shares the FAT writer). §2, §10  (a20a52d; verify on device)
- [x] UF-18 (P1) 2-finger swipe-down power-off (stock gesture; touch point count is available). §8  (a20a52d; verify on device)
- [x] UF-20 (P1) On first boot write CALBAK.BIN (cal record + boot data + UID, CRC) to the USB disk, written when missing or changed (compare-first). §4  (device-verified: CALBK WRITTEN in T3)
- [x] UF-21 (P1) Make AUTO+ (V-first, already ported: v_auto=1) the default V autorange; warn when mV-first AUTO is chosen. §5.1  (7399d86; verify on device: an upgraded unit boots in AUTO+; Range page warning)
- [x] UF-05 (P1) Scope drag axis lock (user: horizontal drags knock the vertical): dominant axis decided at
      the slop and locked until release; relative trigger drag; drag mode Stock / Axis lock with the
      hold-to-grab time in Settings. §6.1  (5fd7871, stock port 419fd96, 65a82fe; verify on device)
- [x] UF-09 (P1) Scope horizontal trigger position: the stock T marker drag (stock port 419fd96). §6.1
      Double-tap to centre: see the UI punch-list remainder.  (verify on device)
- [x] UF-32 (P1) CSV files: '.' decimals, ISO-8601 timestamps, header (model/fw/function/range). §10  (verify on device. No RTC: time_s since the start + the start uptime, no ISO dates)
- [x] UF-37 (P1) Native shim keeps the short-press power-on (stock V2.1 behaviour). §8  (verify on device)
- [x] UF-16 (P2) Generator ramp direction: gen.rs falls, preview rises; verify on scope and align. §7  (300e530: ramp rises; verify on device)
- [x] UF-11 (P2) Scope FFT with labelled axes + automatic peak (f, dBV) readout. §6.2  (45885cb, bde5bff; verify on device)
      src/fft.rs: 2048-pt real FFT (1024-pt complex radix-2, f32), 6 windows (stock 5 + flat-top),
      dBV / Vrms, sliced: load on the frame's pass, 3 butterfly stages per pass, split + dB in one
      pass. Source: the record around the frame (decimated by the mean of k samples when the screen
      holds >= 2048, else 2048 record samples centred on it). Stock math pill / popup (Mode FFT/ABS,
      Color, Hz/Div = 4 1-2-5 values, Window, Unit) live; the M strip marker (0 dBV row) and the M
      top marker (FFT centre, drag pans) as the stock; fw-rs: "10dB/div", "50kHz/div" boxes, peak
      readout "f dBV" + white peak marker. ABS math too. Emulator (1 IPC, so ~1.5x on the device):
      FFT section avg 0.54 ms, max 1.3 ms per pass; frame 6 ms avg. Persist v5 (48 words).
      Not in roll (no linear record).
- [x] UF-12 (P2) Full probe attenuation list x0.1..x1000 (stock scope_probe[13]). §6.2  (bc2c478, 3845408; verify on device)
      Scope MF -> Settings tab, 13 buttons; persisted. No SCPI probe command exists (none added); TRIG:LEV <V>
      now takes probe-tip volts like TRIG:LEV? answers.
- [x] UF-07 (P2) Scope corner-tap timebase + list picker: Corners (default) / Halves / Off, preview tabs,
      timebase list popup. §6.1  (e4bae33, docs/scope-scale-ux.md; verify on device)
      UF-08 (one-tap cursors) and UF-10 (V/div +/- as zoom): open, in the UI punch-list remainder.
- [x] UF-13 (P2) Save scope record as CSV to disk; software pulse-width/runt trigger. §6.2  (c8db910, 6b392af; verify on device)
      MF Settings "Save CSV": stopped scope only (stock "Please stop sample"); the whole stored record
      (<= 26624 rows, ~690 KB, ~10 s) as SCOPnnn.CSV through the screenshot writer (fixed 26-byte rows,
      header timebase / V/div / probe / coupling / trigger / sample rate); the scope is held stopped
      while it writes; refused while a PC has the disk mounted. Trigger tab Type Edge / Pulse (> < =,
      1-2-5 width 20 ns..10 s, = within 5 %) / Runt (second level = orange T marker, drag).
      Limits (src/trigsoft.rs): searched after each complete linear record (no comparator), dead time
      between records misses events, width resolution 1 sample, slow timebases wait for a full
      26624-sample record (10.6 s at 1 s/div).
- [x] UF-23 (P2) PT100/PT1000/NTC temperature via the ohm path. §3  (device-verified on the calibrator, 19:10 run)
- [x] UF-25/26/27/28/35 (P2) DMM small fixes: extra-digit option, 4-20mA% label, store-while-HOLD, volume semantics, MIN/MAX reset. §4-§5  (59614fa; verify on device)
      UF-25: no option (owner: the extra digit is always on). UF-26: DC mA loop % captioned "4-20mA", hidden
      outside -25..125 % (OL). UF-27: a history tap stores the held value while HOLD / AutoHold. UF-28: row
      renamed "Key/alert volume"; idle power-off announced 30/20/10 s before (toast + beep at >= 50 % even at
      volume 0). The continuity / diode beeper is the HY3131 CMPO pin (not TIM14): volume does not apply (check).
      UF-35: long-press on the MIN/MAX/AVG column = statistics reset, distinct from the history-row clear (existed).
- [x] UF-29 (P2) On-device Help page (gestures, resets, input limits). §9  (a98ef21; verify on device)
      Settings 2/2 -> Help: 4 list pages (gestures, side keys, multimeter taps, limits; ui::HELP); tap = next page,
      title bar / MENU = back. No input voltage ratings (none sourced in the repo).
- [ ] UF-24/30/31/36/38/41 (P2) §3-§10, worked 2026-10-21 (decisions in docs/user-feedback.md):
  - [ ] UF-24 tempco compensation: parked, documented (needs FR-36 + a per-unit two-temperature
        characterisation; the only always-on sensor is the MCU die, biased by the rails, UF-02).
  - [x] UF-30 fast bargraph: widget src/bargraph.rs + tools/bargraph-test (bf17876); wired 2026-10-21
        (see the DMM batch below). Rate = the AD1 conversion rate (10/s FAST /
        moving, 2.5/s steady); >= 40/s needs OSR 0x13 + a firmware average for the readout.
  - [x] UF-31 stylus: no sensitivity register (stock writes only D1 01 / D2 04 / D1 09); documented.
        Possible firmware cause found: touch.rs Debounce drops a touch whose only pressed frame is
        followed by a release frame. Device check first: DBG:TOUCH? log of stylus taps.
  - [x] UF-36 BT on every view: by design (bt.poll every 5 ms in all views, DMM rail kept on while
        linked); check tool `tools/dm40scpi.py --bt btpoll -p 50` (890047b). Not during an FR-38
        logger session (BT frames are not served then, by design).  (verify on device)
  - [x] UF-38 tools/dm40scpi.py: info / conf / read / log CSV / scpi / btpoll, USBTMC or --bt (890047b).
        (verify on device)
  - [x] UF-41 sin(x)/x: src/sinc.rs (Lanczos-3) + tools/sinc-test (44f035c), wired (553e7a4): the
        "fewer samples than columns" branch (100..500 ns/div). Emulator: frame ~5.2 ms avg at 100 ns/div
        vs ~3.3 ms at 1 us/div (linear path). (verify on device)
- Community-confirmed existing items (priority signal only): native boot/recovery; FR-26 temperature offset (raise to P1);
  FR-36 user calibration; FR-13 sub-mode memory; FR-01/FR-02 (AVG on chart); FR-04/CSV record + FR-38 logger; FR-05;
  FR-09/FR-24 stray-C REL; FR-10; FR-14/FR-17; FR-19; FR-21; FR-28/29/32; FR-34; USBTMC; ui-review #1, #4, #5, #8, #10, #11.

## User decisions 2026-10-13 (on community feedback)
- [x] Temperature: user offset (cold-junction / reading), plus sensors PT100, PT1000 and common NTCs
      (10k B3950, 10k B3435, 100k, ...). (device-verified on the calibrator, 19:10 run)
- [x] Gestures: 3-finger swipe = screenshot to the SPI disk, 2-finger swipe = power off (as stock).  (a20a52d; verify on device)
- [x] AC+DC on ALL models (DM40A hardware supports it).  (verify on device)
- [x] Always 60000 counts (DM40C behaviour) regardless of the model byte; IDN/Device keep real model,
      + physical power-key short/long/double press as in stock; PD6 pressed level.  (verify on device)
- [x] Scope UI redesign (docs/scope-ui-plan.md): superseded by the stock scope port
      (docs/stock-scope-port-plan.md, 9a6baae..29ba03c, S1-S9) + fw-rs extras (b9c6ddd, e4bae33).
- [x] Calibration (0x080C0000) + boot data (0x080A4000) + UID copied to the SPI disk at every boot
      (only when missing/changed) so they're reachable over USB.  (device-verified: CALBK WRITTEN in T3)
- [x] Analog rails: default OFF (stock-like alternation per page); both on automatically in the
      DMM+Scope combined view; SCPI command to force both on (e.g. SYST:RAIL BOTH|AUTO). Consumption/
      heating vs stock: UF-02 / UF-19.  (verify on device)
- [x] +1 display digit (at least DC voltage), smaller font after the main digits, always on.  (776003c; verify on device)
- [x] Milliohm resolution on the lowest resistance range (units of mΩ).  (776003c; device-verified: 10.117 Ohm, 19:10 run)

- [x] AutoHold idle threshold absolute on small ranges (meter.rs ah_cfg): DC V/mV idle below max(300 counts,
      5 mV) (60 mV range: 300 counts = 0.3 mV < the 0.487 mV open-lead residual, docs/acdc-stock-vs-fwrs.md 6c),
      DC A below max(300 counts, 1 uA). Device: open leads on DC mV manual 60 mV / uA never capture.  (86a408a; verify on device)

- [x] BT phone-app function change (cmd 06) goes through the app's function-change path (App::host_func_changed
      = func_changed: meter reset, chart restart, function bar / static view redrawn), like touch and SCPI.  (b5595d1; verify on device)

- [x] Temperature sensor type shown large (owner): TEXT22 chip with the full sensor name in the DMM page's aux2
      slot and as the meter strip's second sub row.  (875d958; verify on device)

- [x] Temperature offset slider (owner: +/- buttons "absolutely unintuitive"): ui::Slider, -20..+20 C in 0.1
      steps, drag / touch-down sets, -/+ end buttons step 0.1, "Offset 0" resets.  (5ba89b3; verify on device)

## Backlog (user ideas)
- [x] SCPI over Bluetooth serial: the same parser as USBTMC (session per transport, DBG:* refused on BT),
      stock binary frames (0xAF) dispatched alongside, DMA TX ring. docs/bt-scpi.md,
      tools/dm40bt_scpi.py.  (74b8890, 4c2f0a6; verify on device incl. READ? / WAV:DATA? throughput)

## Found in the screenshot session 2026-10-13 (native db679a3)
- [x] READ?/MEAS? take ~8 s (wait for a full averaging window that never fills -> 20 conversions).
      (e07c11a; device-verified: READ? ~0.5 s, 19:10 run)
- [x] USBTMC wedges after a timed-out query (pending waits expire, new commands cancel them, class
      CLEAR/ABORT resync).  (e07c11a; device-verified: survives a libusb timeout, 19:10 run)
- [x] SCPI function change (CONF/MEAS) does not redraw like a touch.  (e07c11a; device-verified, 19:10 run)
- [x] AC+DC sub-readings line: AC tilde / DC sign placement.  (40d9a76; device-verified, 19:10 run)
- [x] Temperature sensor label in the status bar: glyphs overlap for PT100/PT1000/NTC names.  (40d9a76; device-verified, 19:10 run)
- [x] AC+DC current reads ~150 uA AC with nothing connected: stock overload guard ported, reads 0 on
      device (per owner; commit not identified). The AC V decay half moved to the crosstalk item.
- [x] 3-finger screenshot blocks the main loop ~8 s (BMP write to SPI flash); USB and DMM stall.
      Now incremental (screenshot.rs state machine, <= ~3 ms per main-loop pass; frame captured
      run-length coded into 96 KiB SRAM, screen frozen only while captured); SPI1 /16 -> /4 (12.5 MHz).
      (5fd7871, e3fe548; verify: loop max in DBG:DIAG? during a shot, SCPI READ? keeps answering, S00x.BMP correct)
- [x] Data logger (LOG:STAT ON) has no on-screen indicator: "DATA LOGGING" panel (dialog::log_panel,
      rate + sample count, 1 s refresh), view repainted on stop.  (5fd7871; verify on device)
- [ ] sigrok/SmuView (merged from three items): libsigrok scpi-dmm DM40 profile exists as
      ../libsigrok-dm40/0001-scpi-dmm-add-support-for-Alientek-DM40A-B-C-dm40-fw-.patch (command set
      like the 34401A: CONF:<f>, READ?, FUNC?; 3 s delay after RES/CAP/TEMP/FREQ/CONT/DIOD). DC V works
      on the device via usbtmc/1209.0001; RES was blocked by H1 (fixed in 258c833). 2026-10-21: every
      function passes with sigrok-cli in the emulator (emu/usbip_client.py scpi-tcp, tcp-raw; replies
      <= 0.86 s), AC+DC now also waits 3 s (first reading 1.4-1.6 s); patch regenerated as one squashed
      commit (branch alientek-dm40-upstream). Done in fw-rs (809000a): CONF? (Agilent-style reply),
      INIT/ABORT no-ops, SYST:IDN:EMUL OFF|34401A (persisted, default OFF) for unpatched
      libsigrok >= 0.5.2 / SmuView (verified with the distro libsigrok 0.5.2 in the emulator).
      Remaining: device retest over USBTMC (sigrok-cli -d scpi-dmm:conn=usbtmc/<bus>.<addr>, SmuView),
      send upstream. Open: a function change on the meter during a native-profile acquisition makes
      READ? wait for the settle (up to ~3 s, past the 1 s USBTMC timeout) and sigrok stops.
- [x] Scope touch: pinch = scale (timebase / V/div, 1-2-5 snap, value bubble), one-finger drag =
      offset, draggable trigger level, no clash with view swipes.  (c470be7, 419fd96, ebff9b4, 81c9dd8;
      verify on device, two-finger report rate first)
- [x] Combined DMM+scope view design: meter strip inside the scope page, 3-size type hierarchy, gesture
      map, renderer choice (own layered compositor, no GUI library) -> docs/combined-view-design.md (8714bdd).
- [x] Combined view C1-C5 (aa51ae5), later the stock page + meter strip (d323565); G1 drag/grab switch
      (65a82fe), G3 pinch (above), R1 redraw profiling (ba9960e).  (verify on device; G4 A/B on device)
- [x] AC+DC: the AC part read ~5% low and wandered phase to phase (4.5..5.3 V on a 40 Hz 0..9.6 V
      square): AD3 enabled before AD2 (3b9251c). (device-verified: AC+DC 6.8000..6.8004 V, sd 0.3 mV;
      docs/acdc-stock-vs-fwrs.md §10.) FREQ 13.3 Hz in the combined view: moved to the crosstalk item.

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
- [x] H1: MEAS:RES? (and CONF:RES + READ?) answered 9.9E37 "Data stale" (ohm settle 2.5 s, wait expired
      at 1.9 s): wait from max(now, dmm.deadline()), single READ? capped at 850 ms for sigrok.
      (258c833; verify on device)
- [x] Review M1 (CHECK_CLEAR_STATUS pending only while a reply is in flight), L1 (ABORT_BULK_IN marks
      cleared), L2 (Bulk-OUT left while a command is pending), L3 (toggle reset never writes EPENA/EPDIS)
      from docs/native-review-2026-10-13.md "Pre-flash review e07c11a".  (258c833, 636bf88; verify on device)
- [x] AC agent questions (a) 40 Hz PWM AC part low, (b) AC+DC AC part = plain AC V, (e07c11a wander):
      answered by 3b9251c (above). (c) A input open: the 150 uA item. (d) FREQ: the crosstalk item.
- [x] firmware/dm40fw.py is untracked but build.sh needs it (packs the native image): commit it. (165b37a)

## UI toolkit decision (user, 2026-10-13)
- Keep our own renderer + layer compositor (docs/combined-view-design.md §7) for now. Considered:
  Slint (native Rust, MCU line renderer, real widgets/swipes; GPLv3 or commercial for embedded),
  LVGL via lv_binding_rust (C, MIT, experimental bindings), egui (needs std + framebuffer/GPU-style
  rasteriser: does not fit 125 KB SRAM). Revisit with a one-screen Slint/LVGL prototype if the
  hand-made widgets stay clumsy. Meanwhile: layout rules + automated overlap check for every screen.
- [x] Measure the real DMM conversion period per rate — resolved by the datasheet + emulator
      (emu/hy3131/RESEARCH.md 3), device confirmation pending: AD1 period = OSR * 30 / SYSCLK, x2
      with AD1CHOP (set in every stock R22): 0x15 (OSR 8192) 100 ms, 0x17 (OSR 32768) 400 ms. Both
      old guesses were wrong (~9.4/s at OSR 32768 and OSR / SYSCLK ~150/s). fw-rs: FAST 10/s, MED
      10/s moving / 2.5/s steady, SLOW as MED (one reading per conversion, 16-conversion window, 32
      on ohm r0..r3); hy3131::ad1_period_us, dmm.rs FR-05 table. Device: SENS:RATE:PER? per rate on
      a steady input (expect 0.4 s for MED and SLOW, 0.1 s for FAST).
      Decided 2026-10-14: SLOW reports every conversion (it was one per 4 = 0.625/s, sized for the
      old ~107 ms guess, so READ? never got a fresh SLOW reading). MEAS:<f>? after a function change can no
      longer reach a full +1-digit window within SCPI_SETTLE_MS 1.5 s (needs ~3.2 s); kept for
      the 2 s PyVISA default.
- [x] Data logger runs at 38 Hz at OSR 2048 — that is the chip, not the SPI link (datasheet +
      emulator, device confirmation pending): OSR 2048 chopped = 25 ms = 40/s. The SPI read is ~122
      us/sample. Faster rates are now opt-in: LOG:OSR 0..2 (OSR 256 chopped 320/s) and LOG:CHOP OFF
      (AD1CHOP = 00, x2: OSR 256 unchopped 1.5625 ms = 640/s, the chip's max; converter offset not
      cancelled, more low-frequency noise); LOG:PREamble? reports chop as a 6th field. The logger's
      nominal period (gate, `missed`) was 16x too short before: `missed` counted ~60 phantom
      samples per real one. Device test: LOG:OSR 0..3 x LOG:CHOP ON/OFF sweep with LOG:RATE?,
      LOG:COUNt? (missed) and the noise / offset (counts) per setting on a steady 0.5 V and a
      shorted input, to fix the real rate / noise / offset table.

## Owner hands-on feedback 2026-10-13 night (build a9d200f / experiment e0272ef)
- [x] Scope drag lags: while a drag is in progress freeze acquisition/redraw of new frames and only
      re-render the shift of the frame already shown (column diff); resume on release.
      (419fd96, 62493e0, ba9960e; verify on device with DBG:PROF?)
- [x] Scope trigger mode Auto and scope AUTO (autoset) "strange": trigger position counted twice +
      blind 800-sample window (ff0e568), stock AUTO-set (419fd96), comparator trigger on by default
      (49ec62e, device-verified), AUTO period search / clip checks / mid-swing trigger (e0db212,
      9d4584c, 5cba114; verify on device).
- [x] Temperature PT100/PT1000/NTC: show the sensor type much larger.  (875d958; verify on device)
- [x] Thermocouple/temperature offset setting: slider instead of +/- buttons.  (5ba89b3; verify on device)
- [x] Buttons feel imprecise to press: touch-down confirmed on any two pressed frames (b8a48ae), hit
      slop on lists/keypad/menu/range + tap after an uncaptured long press (f4a5b66, d7e31bf),
      DBG:TOUCH? log + tools/touch-log.py.  (verify with the owner's finger)
- [x] DMM frequency: ~700 Hz shown as "1 kHz": Hz readout decimals.  (da0f874; verify on device)
- [x] Pinch-to-zoom on the scope: see "Scope touch" above (c470be7, 419fd96).

## Owner notes 2026-10-14 (../замечания.txt)
Firmware:
- [ ] °F shown in the top-left status slot is odd; move the °C/°F choice to a global Settings option and show only the chosen unit.
- [ ] Temperature settings: remove the unclear "Offset 0" button (reset via the slider/long-press instead).
- [ ] "NTC user" is unclear: make its R25/Beta editable right there (or remove); explain on screen.
- [ ] Second resistance mode present in the stock is missing in fw-rs — check stock parity (dmm_v2_6.h) and add it.
- [ ] Tap on the DMM value cycles averaging/resolution: fewer digits = faster readings; compute digits vs OSR/noise from the
      HY3131 timing + measured noise (not ad hoc); the +1 digit always drawn small at the end.
- [ ] Power-meter mode: HY3131 can measure V and I at once (AD2/AD3 + MUL → active power in RMSDATA); Vrms, Irms, P (and
      energy) by switching internal modes — check feasibility with the DM40 front-end wiring first.
      Researched: GO, docs/power-meter-and-misconnect.md §3 (I on the stock OP1 path, V via OP2→AD3 DC-coupled,
      signed 38-bit X·Y RMS read, ~10-12 days). Gate: device test T1 (OP2 on SENSE). Owner: fuse/CAT/10 A limits.
- [ ] Detect wrong connection (voltage on the A jack / current mode with voltage present, leads in the wrong jacks) and warn.
      Researched: GO for current-in-A-jack in non-current modes (idle comparator on PB1, 47.5 Ω shunt, trips ~0.44 mA),
      partial for "V leads while in A mode"; blown fuse not detectable (fuse-check dialog instead). Doc §4, ~4-5 days.
- [ ] (B) AD2 fast data logger: 600..9600 S/s above AD1's 640 S/s, ~14 ENOB, gaps marked (seq/timestamps); ~2 days.
      Doubles as the device experiment for (A): torn-read histogram, loss per rate, SPI2 read-back.
      docs/dmm-input-scope-feasibility.md.
- [ ] (A) DMM-input "slow floating scope" via AD2 (after B): ~5-10 ms/div..50 s/div, 2.4-9.6 kS/s, BW ~1-2.5 kHz,
      V/mV/A on the isolated jacks (current waveforms). Scope UI reused ("Wave" from DMM, MCX|DMM source badge),
      software trigger. Needs one ISR (owner decision; fw-rs is ISR-free) + HY3131 on hardware SPI2; ~2.5-3 weeks.
- [ ] Device 2026-10-21 (docs/device-check-2026-10-21.md): stack 37468/46048 B (81 %) on 16b3eb9 — the stack
      region shrank 59152 -> 46048; re-check on 24f7080 and restore headroom (>= 16 KB).
- [ ] Scope 32 ms loop passes ~3/s: measurement maths (11.4 ms) + 4 measurement cells redraw (12.3 ms) in one
      pass; split across passes and redraw only changed cells (3-4 % of touch/USB wait up to 33 ms).
- [ ] Scope ~9 fps at 1 ms/div 2 V/div after AUTO vs 36 fps before at the same timebase — investigate.
Emulator (emu/):
- [ ] HY3131 model at terminal level: the user sets what is connected to V-Ω/COM/A (sources/loads, incl. voltage AND current
      at the same time); the firmware's mux/relay choices route it through a modelled DM40 front-end (dividers, shunts, relays,
      switches) — no per-mode magic inputs.
- [ ] Show the firmware's current mux/relay/switch selection as feedback in the GUI (draw the front-end path).
- [ ] Cold-junction temperature and the HY3131 die temperature are separate inputs (die temp always set; default equal).
- [ ] GUI: Apply on Enter in the HY3131 input fields; show only the fields relevant to the chosen source (no freq/DC offset
      for plain DC etc.).
- [x] Virtual backup files (owner 2026-10-21): second USB MSC LUN = read-only FAT16 volume DM40-INFO
      generated from the MCU memory (src/vdisk.rs: CALIB / BOOTDATA / UID / OPTBYTES / OTP / ROMBOOT / LOADER .BIN,
      BACKUP.TXT, FIRMWARE.BIN = running firmware as update file); per-LUN SCSI in msc.rs (INQUIRY names, WP bit,
      WRITE -> 07/27/00, PREVENT / UNIT ATTENTION per LUN). SPI disk: only a write-once safety copy of
      CALIB/BOOTDATA/UID (DBG:BOOT? CALBK SAFECOPY_PRESENT/WRITTEN + differs). Host-tested (fatren-test) and
      emulator-tested over USB/IP (userspace client). docs/native-boot.md "Unit data".
      Open: device test (Linux kernel attach + mount ro, Windows, macOS); vdisk::init time on the device;
      generated CSV files from the binary logs via vdisk::set_extra (hook ready, no caller).

## DMM page / settings batch (owner notes ../замечания.txt, 2026-10-21)
- [x] Second stock resistance mode = the stock m+8 "ON" variant (p8set rows, lower test current for
      in-circuit use). It was ported (second tap on the ohm button, persisted in p[2]) but invisible:
      now the stock "ON" tag (ON_TAG font, glyph 0x20) on the ohm button (DMM page + combined band)
      and Normal / "Low current ON" buttons on the Resistor Settings page. (verify on device: tag shows,
      the ~0.27 mA vs ~0.85 mA test current)
- [x] ui-review #15 dead code: ui::bar / button_row / DMM_BUTTONS / sub_label / val_line and four unused
      layout constants, src/fonts.rs and src/text.rs (the anti-aliased Roboto set; nothing else used them),
      tools/mkfont.py and fonts/Roboto-*; host-test module lists updated (ui::button_hit stays: live).
- [x] Continuity: a negative result (source / reversed diode / charged capacitor on the jacks) clamps to
      0.00 ohm as the stock (was -666 / -1114 ohm; emulator cross table). (emulator-verified: V 1.234|cont = 0)
- [ ] AC V on a pure DC source reads ~18 mV in the emulator (stock ~0): NOT a firmware bug. The model's
      coupling capacitor charges to the raw jack DC (UpdateAcNode: src = dc * NodeRatioAc) while the
      seen step uses the calibration-folded node DC (NodeDcAc: DcQ(...)), so step = dcNode - vc never
      reaches 0: residual = 1.5 x V x (cal gain/offset error) = 17.7 mV at 1.234 V. With `cal:0` fw-rs
      settles at 0.64 mV (the model's noise floor). The stock escapes it because its mV-first AUTO steps
      to AC mV (DC coupled in the model, no coupling term); fw-rs stays on the 6 V range (AUTO+, UF-21).
      Fix belongs in emu/hy3131/HY3131.cs (charge vc towards the same DcQ-folded level).
- [x] UF-30 bargraph wired: in the history row's place (8,154,464,18; the 7 px strip under the digits
      cannot keep 3 px from the F78 ink and the history boxes), Settings 2/2 "DMM row" Bargraph (default) /
      History (p[38] bit 10). Fed in App::reading with the raw reading (pre-REL, pre-smoothing; keeps
      moving while HOLD / AutoHold freeze the digits); bipolar in DC V / mV / A (not AC+DC), unipolar
      elsewhere, empty track in temperature, red track on OL. History taps / long press only in History
      mode. DBG:PROF? section "d.bar". (verify on device: d.bar cost, bar at 10/s moving / 2.5/s steady)
- [x] Temperature unit deg C / deg F as one global setting (owner: the deg F readout in the top-left status
      slot was odd): Settings 2/2 "Temp unit", SCPI UNIT:TEMPerature C|F / ?, persisted (p[38] bit 9).
      Applied to TC / PT100 / PT1000 / NTC readings, the cold junction (aux row, temperature page, MEAS:TEMP:CJ?)
      and SYST:TEMP? (MCU die). The second deg F readout is gone (status slot shows AUTO / MANU again).
      A unit change restarts the meter like a function change and converts the temperature limits; offsets
      stay in C (0.1 C trim steps). Bluetooth keeps the stock frame (main C, CJ C, value 3 deg F), converted
      back in bt.rs. (verify on device: phone app in F mode still shows C/F correctly)
- [x] Temperature page redesign (owner notes): "Offset 0" key removed; the offset is a settings-style
      stepper "Offset [-] +7.3 C [+]" (0.1 C steps) and a tap on the value opens the signed keypad (the
      0.12 C/px slider is gone); slot 8 of the sensor grid = the global unit (Unit: C / F). "Fuzzy" buttons
      found: the page's hit boxes were exactly the drawn boxes (4302 dead px in the 5..6 px gaps, touch-test
      reach L+0 T+0 R+0 B+0) and nothing showed a press: now 3 px slop on every key (dead px 953, all in
      the wide gaps) and pressed feedback from touch-down (ui-review #11 rule). The sensor type stays the large
      TEXT22 chip on the DMM page (875d958).
- [x] NTC user R25 / Beta editable on the temperature page (fields shown with "NTC user" selected; numeric
      keypad, R25 in kohm, limits 100 ohm..10 Mohm / Beta 1000..10000 K as dmm.rs set_ntc_user), persisted
      (p[16] / p[17], existing), SCPI [SENS:]TEMP:NTC <R25>,<Beta> (existing). A change while NTC user is
      active re-enters the sensor (NTC entry range follows R25).
- [x] Fix of 1a3efbd: Settings 2/2 had the "Temp unit" value but not its row name (rows shifted by one:
      tapping "Scope swipe" toggled the unit). Row added.
- [x] Tap on the DMM main value = resolution step FAST -> MED -> SLOW (fewer digits = faster), with a
      1.5 s toast of what the step really gives here: FAST "60000 counts / 10/s" (one OSR-8192 chopped
      conversion: 163.84 kHz / 8192 / 2); MED / SLOW "600000 counts / <window> s" where the range has the
      +1 digit (8 / 16 steady OSR-32768 conversions of 400 ms, ohm 16 / 32), else "60000 counts / 2.5/s".
      The +1 digit stays the small half-size glyph. Functions without a rate choice: low beep + toast.
      Not added: a 6000-count / ~40/s step (OSR 0x13, the logger's measured 38/s): its noise on AD1 is not
      measured (dmm.rs: noise floor open); add it after a device noise run.
- [x] UF-30 bargraph restyled (owner: "unclear what it is"): a classic segmented DMM bargraph: 50 segments
      (2 % each; bipolar 25 per side from a centre zero), the 10 % marks as full-height segments (ruler), lit
      segments orange, all red + a red "OL" marker on overload, F14 scale labels under it: 0 / half / full
      scale with unit (bipolar -FS / 0 / +FS unit), full scale = the range's (1000 V / 10 A where limited).
      Rows 147..172 (3 px from the F78 ink and the chart frame), probed for the ui-render ink check;
      incremental update repaints only the changed segments (98..150 bus words / update; bargraph-test:
      every update equals a fresh draw).
- [x] ui-review #13 remainder: the DMM page value turns the stock red (0xC9A0) in the V functions from 30 V
      (live reading, also while HOLD; the stock cue, gui_v2_6.h:157/175), lightning still from 100 V. No bar
      tint: the stock has none. (verify on device: 30 V on the calibrator)
- [x] Device bug (16b3eb9, 40 Hz square 0..10.4 V): CONF:VOLT:ACDC then CONF:FREQ read 40.002 / 13.334 /
      26.668 Hz with duty 100.0 % for ~11 s (MIN / AVG polluted); DC V -> Hz clean. Not reproduced in the
      emulator (its counter model has no settling input path: clean 40.0007 Hz at once). Firmware side:
      the frequency mode's two-source choice kept its history across functions (last / change / source
      phase never reset) and accepted results with duty 100 % (periods counted while the input is never
      seen low = edges missed). Now: source history reset on every function change; a result with f > 0
      and duty >= 99.95 % neither wins the choice nor reaches the readout / MIN / AVG (no reading that gate).
      Cause of the missed edges (input path settling after the AC+DC relay pulses?) still open: retest on
      the device; if the wrong readings persist, compare the stock's AC+DC -> Hz entry (R20 / relay order).
- [ ] DMM batch polish (open): bargraph full-scale labels in diode / continuity say the 60000-count scale
      (6 V / 600 ohm), not their real ceilings (3 V / 1000 ohm); Help "Tap history row" applies only with
      Settings 2/2 DMM row = History; a debug_assert in ui::list_page (vals.len() == names.len()) would have
      caught the 1a3efbd row mis-mapping.
