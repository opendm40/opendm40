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
   AC V of a 0.5 V DC source decaying. Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 7): DC 1 / 5 / 10 V within 0.04 %, FREQ
   40.0017 Hz, AC V of 0.5 V DC 0.3-0.5 mV; **the 40 Hz 0..9.6 V square reads +0.19 % with the scope rail on**
   (DC V / AC V / AC+DC, reproducible): to explain (ground return / PC4 rail noise?).
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

## Memory budget (2026-10-22 rework, docs/native-boot.md "Memory budget")

- [x] Stack headroom 16.8 -> 24.9 KB (ad2-isr), SRAM free 1.8 -> 12.9 KB, region B free 0.3 -> 24.5 KB;
      build checks: stackdepth --check 20480, TCM stack >= 42 KiB, SRAM >= 8 KiB free, region B >= 8 KiB
      free (warning < 16 KiB).
- [x] Device: DBG:BOOT? STACK high-water after a scope walk + FFT + screenshot (static path 21.8 KB):
      a239aa6 21 616 / 46 620 B (46 %) after scope page, popups, FFT on, AUTO, Save CSV, 3-finger shot
      (boot 18 328); docs/dmm-input-scope-feasibility.md 11.11.
- [x] Device: screenshot with the scope FFT on (40 KiB CAP while the FFT holds the tail): a239aa6
      SHOTS/S007.BMP 307 266 B holds the spectrum, the FFT keeps updating after the shot (11.11).
- [x] Device: a shot of a big screen (AC+DC / power view) then the FFT: the spectrum resumes after the shot.
      f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 4): W page AC+DC shot as SHOTS/S009.BMP, back on the Scope page s.fft 5012 -> 5866 in 3 s.
- [x] `.cold` (region C, not zero-wait) audit 2026-10-22, emulator hooks on every function of C (a Renode
      AddHook per symbol from t = 6 s, hit counts logged at powers of 2; DMM idle, swipe-down CSV recording
      16 s, combined view, settings pages, generator page): vwrite::poll (B) called upload::disk,
      vdisk::entries and vdisk's generic `[VF; 400][..n]` index on EVERY main-loop pass (~24k calls/s in
      idle) -> inline(always) / a plain slice; the DLG row coder (Enc::row, BitW, Ad, fmt_row's CSV length,
      crc32_upd, ~1.4 KB) ran per reading while recording -> `.text.dlog_row` (B); app_press's guard runs on
      every touch report (scope drags) -> inlined into App::gesture, the body (press_track, taps only)
      stays in C. fatdir runs ONCE per recording start (csvlog bpb), not per commit (the earlier note was
      wrong): stays in C. After: no C function runs in idle / combined view / while recording; C runs at a
      recording start / stop (fatdir, the vdisk refresh), on taps and boot. Not covered by the run: MSC reads
      by a host (vdisk read_block in C per sector: host-driven I/O, fine) and the scope FFT / AD2 stream
      (the budget run of b21756f: none in C). Region B: -3 KB for decision #6 + the audit (20.0 KB free on 090f845).
- [x] Device: DBG:PROF? / lat_pc while recording a CSV and with the USB volume mounted (Files live: a
      vdisk refresh per commit, ~60 s) to confirm no stall from region C.
      f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 5): no region C stall; loop max1s 1-2.5 ms, one 74 ms pass per commit (84 ms at the
      start) in `csv` = the blocking FAT / dir write_block erase (by design); a recording appears on the
      volume at its stop, not per commit (finish -> set_stale). Region C found elsewhere (6a / 6c):
      vdisk::file_read's closure (host volume reads during a stream, max_lat 9 us: fixed) and the volume
      refresh under the Wave view's stream (below).
- [x] Region B audit #2 2026-10-22 (after the fast logger: 9.4 KB free): 30.4 KB free (ad2-isr; 37.8 KB
      without), no change in function (docs/native-boot.md "Region B audit #2"). ~20 KB of `.cold` modules'
      code was inlined into hot callers (Gallery::poll + DLG decoders in App::tick, VDisk::write_block in run,
      boot::put_info in Scpi::handle, help / temperature pages, scope Save CSV, csvlog delete_on /
      catalog_scpi_on) -> `#[inline(never)]` / patterns; poll_cap's f32 quicksort -> insertion sort. Region C
      31.7 KB left. Hooks on every C function (per-second counts): idle, CSV recording, combined view, AD2
      stream 38400 (DMA), fastlogtest typ: C only at a start / stop, but LOG:DISK? (polled during a disk session) ran
      fast_scpi in C (5d5dba0) -> its reply fast_status in B. `Logger::drain` has one copy (no
      duplicate to drop). build.sh: a banner at the end below 16 KiB. emu/hy3131/ad2streamtest.py and
      emu/fastlogtest.py take `--extra-script FILE` (e.g. the hook lines).
## Remaining work

- [x] `tools/dm40dbg.py` ported from CDC/pyserial to `/dev/usbtmc` + `DBG:` commands (a3427d4).
      `tools/reload.sh` is the native reload flow (fa29a80); no serial/ttyACM references remain.
- [x] On-device test of USBTMC (`*IDN?`, `DBG:PING?`/`DIAG?`/`LCD?` screenshot, `DBG:TAP`/`SWIPE`) and
      the mass-storage disk (W25Q128, 1f2bf30).  (device-verified: used in every device run since;
      CALBK files written, dm40_patched.bin copied onto the disk for DBG:UPDATE)
- [x] Faster redraw of the main DMM value and trend chart (target ~6 ms like stock: render to RAM,
      single window flush, chart column ring). 4d2dc40 sped it up; measure with DBG:PROF? (95287a8).
      Done: be9425b (d.read / d.chart prof sections), 1f19f05 (chart column-span cache, aux rows repainted
      only on change, faster stockfont::label; pixel-identical). (verify on device: DBG:PROF? d.read / d.chart)
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
  - [x] RAM build removed (owner 2026-10-22; it no longer linked, ~200 KB over): build.sh builds
        only the native image (`native` kept as a no-op argument), link.x / link-test.x /
        startup.rs, the test-initcore / test-recovery (T1/T2) images, stub/, tools/legacy/
        (flash.py, dm40_loader.py) and reload.sh --ram are gone; features native /
        embedded-fonts / calbackup / fatren-live dropped (always on / unused), vdisk is default.
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
      - #11 pressed-key feedback on the keypads (7dad1b9); since 2026-10-22 also the menu tiles /
        rows (stock tile style's pressed 0x4419, an RE guess), the list-page rows of Settings 1-3,
        Hold & Limits, Record (0x04FC, value white), their -/+ stepper keys and the Start / Stop row
        (0x7BEF, the stock popups' pressed colour) and the generator boxes + wave drop-down (0x04FC):
        drawn from touch-down until release / roll-off, one item redrawn (app_press.rs, in .cold;
        ui::set_row_only = the page's own draw call limited to one row). ui-render: every item
        pressed, then released = the page as drawn. (verify on device)
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
      decided: see docs/decisions-2026-10-22.md #1 (Record page: intervals, mean per row, duration, estimate)
  - [x] Record page (decision #1): "Record" row on Hold & Limits -> View::Record: Interval (every reading /
        1 s / 10 s / 1 min, -/+ stepper), Duration (until stopped / 10 min / 1 h / 8 h / 24 h), "Free: about
        N h left" (free clusters x 2 B/row at the interval or the measured reading period, scanned on page
        entry, "disk busy" while a commit holds it, capped at the 49.7-day limit), Start / Stop (Start returns
        to the DMM page). csvlog.rs: interval rows = mean of the readings, time_s = interval END (k x I),
        flags ORed; an OL in the interval makes it an OL row (the DLG / CSV format carries no value on an OL
        row: decision text "mean of the non-OL readings" is not representable without a format change);
        no row for an interval without readings; the partial last interval at stop is written at its
        nominal end; the duration ends the recording (push and poll) with the normal "Saved" toast. One
        extra "# interval N s: ..." header line in interval mode; every-reading mode byte-identical (the
        fatren-test md5). SCPI LOG:CSV:INTerval / DURation (discrete values; Settings conflict while
        recording) / FREE?. Persist v8 word W_REC. The idle auto power-off is held while recording (it was not). Swipe REC uses the same settings. Help page 5.
        fatren-test "interval recording". (verify on device: checklist in the decisions doc #1)
        Device 5f14d38 (docs/device-check-2026-10-21.md "5f14d38" 3): Hold & Limits 9th row, Record page, 1 s via
        the page 131 s -> LOG000.CSV 132 rows t = 1..132 on the USB volume == MMEM:DATA?; 10 s / 10 min stopped
        itself, 60 rows t = 10..600; INT / DUR / FREE? and Settings conflict OK; settings survive DBG:RESET.
        Not verified: swipe REC with the interval, the estimate within 2x, the power-off hold (no DBG field,
        Shutdown Never on the unit).
      src/csvlog.rs: one-finger swipe down on the DMM page = start / stop,
      toasts, red REC in the status blocks, LOG:CSV ON|OFF / LOG:CSV?, power-off / reset commit the tail.
      (Since the USB storage redesign the PC cannot write or mount the SPI disk: no "Disk in use"
      refusals any more; files in LOGS/ on the SPI flash.)
- [x] Compact binary logs, CSV only on read (owner 2026-10-21: "not CSV on the disk, RLE or a
      decided: see docs/decisions-2026-10-22.md #2 (timestamps stay real main-loop time, no grid snap)
      standard compressed format; CSV only for export"). docs/compact-log-format.md: own DLG format
      (adaptive Rice of delta-of-delta time + 6-digit value deltas, CRC chunk per commit) chosen over
      gzip / sigrok / FLAC on measured data: 0.9..1.9 B per DMM reading vs ~21 B CSV (11..26x; 13 MB of
      disk = 325 h at 10/s instead of 17 h); scope record 53 KB vs 692 KB.
      - [x] src/dlog.rs codec + CSV generator; tools/dlog-test: decoded CSV byte-identical to the old
            writer for 9 data sets, random access, truncation at every byte keeps every whole chunk, bit
            flips contained. tools/dlog.py: standalone decoder (same bytes).
      - [x] csvlog.rs records LOGnnn.DLG (same swipe / LOG:CSV UI); numbers shared with legacy LOGnnn.CSV;
            fatren-test: md5 of the decoded CSV == the old writer's LOG000.CSV, cut sweeps, disk full.
      - [x] scope "Save CSV" writes SCOPnnn.DLG, CSV md5 == the old SCOPnnn.CSV. Coded (owner
            2026-10-21, docs/log-format-standards.md 4): fixed predictor order 0-3 per 4096-sample
            block (or raw 10-bit) + adaptive Rice, 3.7-5.8x smaller than u16 (emulator record 4231 B);
            the length is planned at Save, the writer fills 512-byte pieces per pass.
            (verify on device: a real noisy record's size; the ~10-20 ms planning stall at Save)
      - [x] Owner 2026-10-21: no text LOG*.CSV / SCOP*.CSV on the SPI disk any more (gallery CSV
            parser, shared numbering, list / delete removed); CSV only as the export.
      - [x] Export: virtual LOGnnn.CSV / SCOPnnn.CSV on the read-only LUN 1 (dlog::refresh ->
            vdisk::set_extra, refreshed when a recording ends / a delete / a host write ends);
            SCPI MMEM:DATA? "<name>"[,off[,len]] (LOGnnn.CSV generated, any file raw);
            tools/dm40scpi.py get LOG000.CSV. Gallery reads .DLG (exact counts).
      - [x] Emulator: LOG:CSV ON 60 s (154 readings at 2.5/s): LOG000.DLG 455 B, CSV 3108 B; MMEM:DATA?
            CSV == dlog.py of the DLG == LUN-1 LOG000.CSV read over USB/IP MSC.
      - [ ] Verify on device: swipe REC 10 min, the USB disk shows LOGS/LOG000.CSV on Windows / Linux
            right after the recording (UNIT ATTENTION at once, also while mounted), open in a
            spreadsheet; scope Save CSV -> SCOPE/SCOP000.CSV; gallery tile of a .DLG; DBG:PROF? csv lap
            during a refresh with a large new log (~0.1 s per 100 KB expected); DBG:DIAG? STACK
            high-water after a refresh with several logs.
            (Device 9dd1222, the old LUN-1 layout: scope Save CSV -> no SCOP*.CSV on DM40-INFO after
            unmount/mount; the Save CSV job never called dlog::set_stale. Re-check on the new layout.)
            Emulator on e832a86 (new layout): scope STOP -> MF -> Settings -> Save CSV -> SCOPE/SCOP000.CSV on
            LUN 0 at the next read (UNIT ATTENTION 06/28 first, then the listing has it; screenshot.rs finish()
            calls set_stale since 4a484f7); LOG:CSV ON/OFF -> LOGS/LOG000.CSV likewise. Device re-check pending.
            Device 5f14d38 (Linux): SCOPE/SCOP000-002.CSV and LOGS/LOG000-001.CSV on the mounted volume right after
            the recordings, no remount, == MMEM:DATA?. Open: Windows, gallery .DLG tile, PROF csv lap, STACK.
      - [x] Owner 2026-10-21: drop legacy text LOG*.CSV / SCOP*.CSV support on the SPI disk (gallery parser,
            numbering, list/delete): only .DLG is written and read; CSV exists only as the generated export.
            (4544d4e)
      - [x] FR-38 logger to disk: src/fastlog.rs + src/fastenc.rs, DLG kind 3 (docs/compact-log-format.md
            "Fast stream (kind 3)"): LOGS/FASTnnn.DLG pre-allocated (duration x rate x 3 B, bounded by the free
            space) and pre-erased (blank sectors skipped, BE64 where a 64 KiB block is all ours) before the
            logger starts; the run only programs pages (<= 300 us + one page per pass, no erase / FAT / dir
            write, region B); entry size + FAT trim at stop. LOG:DEST DISK|HOST, LOG:DURation, LOG:DISK?;
            Record page interval "Fast" (AD2 in AC rows, AD1 in DC V/mV/A, LOG:SRATe / OSR rate; tap to
            stop / cancel). CSV on the USB volume (LOGS/FASTnnn.CSV, fixed width: t_s,raw,value,lost_before),
            MMEM:DATA?, dlog.py. Host: dlog-test fast sets (round trip, truncation, power cut per chip op),
            fatren-test fastlog (sessions, ring holes, Full, abandon, the USB volume's CSV, a cut before
            every block write / program / erase). Emulator: emu/fastlogtest.py (W25Q128 busy model).
            Device test plan: "Fast logger device check" below.

## Feature requests to add (from ../docs/dm40-feature-request.md; user: add autohold + useful ones)
- [x] **FR-01 AutoHold (P0, critical).** Auto-freeze the reading when it stabilises, re-arm on a new
      signal, beep on capture (Fluke/Keysight style). (adcfa23; verify on device) (26e964a re-capture: verified on
      device 9dd1222: 1.10 -> 1.20 -> 1.10 -> 1.12 V captured, 1.11 V not, as meter-test)
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
        logger session (BT frames are not served then, by design).  Device 0667175 (docs/device-check-2026-10-21.md "Bluetooth (0667175)"): 400 / 400
        replies at 50 ms on DMM, DMM+Scope, Scope, Gen (median 37 ms). Caveat: a link opened while on Scope /
        Gen does not wake the DMM (stale value; see the open item below).
  - [x] UF-38 tools/dm40scpi.py: info / conf / read / log CSV / scpi / btpoll, USBTMC or --bt (890047b).
        Device 0667175: --bt info / read -n 20 / btpoll OK.
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
      tools/dm40bt_scpi.py.  (74b8890, 4c2f0a6; device 0667175: SCPI, POW / CALC:LIM, LOG:DATA? over BT OK;
      LOG:DATA? drains ~918 records/s = ~11 KB/s; WAV:DATA? throughput not measured)
- [x] Compound SCPI messages (`POW:PHAS:CAL RES;:SYST:ERR?` hung the USBTMC read): Scpi::handle kept only
      the last command the main loop runs (Instr / POW / LOG / MMEM / DBG / READ?) and main.rs cleared the
      text replies before it. Now scpi_msg.rs runs a message one command at a time (USBTMC and BT alike):
      replies in order, ';'-joined, one response; READ? / MEAS? / FETC? pause the rest until the reading;
      a message with a query always gets a response (empty if none could answer, reason in SYST:ERR?; also
      a lone MMEM:DATA? / MMEM:CAT? that failed, which hung before); block replies (MMEM:DATA? / CAT?,
      LOG:DATA?, DBG:LCD? / PROF? / TOUCH? / HYSPI? / AD2?) only as the sole query ("Query error" next to
      others); joined reply <= 512 B ("Out of memory"). tools/scpi-msg-test, usbtmc_stress.py compound.
      Open (pre-existing): headers after ';' are absolute (no IEEE 488.2 relative-path rule); a ';' inside
      a quoted argument splits; a BT line refused as DBG:* that holds a query gets no response.
      Device 0667175 over BT and USB: `POW:PHAS:CAL RES;:SYST:ERR?;*OPC?` -> `-100,"Settings conflict; RES:
      ..."; 1` in one line, `*IDN?;SYST:ERR?` -> `...;0,"No error"`.
- [x] Auto AC/DC for V and I (as UNI-T UT197 "Auto AC/DC Voltage"): an AUTO coupling next to DC / AC /
      AC+DC that picks DC or AC by itself from the signal and shows which one it chose. Decisions
      (docs/acdc-stock-vs-fwrs.md section 11; dmm.rs AUTO_*):
      - measurement: AUTO runs the AC+DC alternation (the only place both parts are seen: AC V is
        AC-coupled) and shows one part, the DC part (signed) or the AC part's RMS; autorange on the total.
      - rule / hysteresis (once per ~1.9 s cycle, both parts fresh): to AC when AC > 1.5 x |DC| and above
        the floor (V 1 %, A 3 % of the range), to DC when AC < 0.75 x |DC| or below the off floor (V 0.5 %,
        A 2 %); between: unchanged (factor-2 band; a half-wave rectified sine keeps what it showed).
      - switch time: 2 evaluations in a row (a probe moved between two DC-dominant sources must not flash
        AC for the one cycle that mixes old and new parts): the switch shows 2..6 s after the change
        (emulator: 2.2..4.5 s); the first evaluation after entry decides at once (~2 s, as AC+DC's first
        reading).
      - display: the glyph above the unit and on the V / A button = the part shown, "AUTO" in the limit
        badge's slot (a verdict wins); aux row = AC part / DC part tagged, as AC+DC (it does not move on a
        switch); strip badge AutoDC / AutoAC. A switch restarts REL, statistics, the AutoHold window (HOLD
        and a capture stay); limits use the slot of the part shown; a CSV recording ends on a switch.
      - touch: V / A button cycle DC -> AC -> AC+DC -> AUTO -> DC; range page coupling row DC / AC / AC+DC
        / "AC/DC" (4 buttons on the range grid).
      - SCPI: [SENS:]VOLT:COUP / CURR:COUP DC|AC|ACDC|AUTO (select the function), ? -> DC / AC / ACDC /
        AUTO,DC / AUTO,AC; CONF / MEAS / FUNC VOLT:AUTO / CURR:AUTO, FUNC? "VOLT:AUTO" (codes 19 / 20).
        BT: the mode byte reports the part shown (the app echoing it keeps AUTO). LOG / wave view refuse it
        as AC+DC.
      - persistence: the function code (19 / 20) in p[2], no layout change; V / A keep no coupling of
        their own (entry from another function = DC, stock).
      Tests: emu/hy3131/couptest.py v / i / tap (DC -> AC -> mixed both ways through the band), ui-render
      auto_* cases (d02b..d02d, c19 / c20, m03c), touch-test coupling row.
      Not on the UT197 model: there it is "Auto-V LoZ", the auto selection comes with a ~2 kohm input
      load (no LoZ hardware here, feature-request HW-6), so ours is the 10 Mohm variant: ghost voltages
      (hum on open leads) still read, as AC above the floor.
  - [ ] Device check (AUTO coupling): see docs/acdc-stock-vs-fwrs.md section 11 "Device check".

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
- [x] °F shown in the top-left status slot is odd; move the °C/°F choice to a global Settings option and show only the chosen unit.
      (1a3efbd: Settings 2/2 Temp unit, UNIT:TEMP; verified on device 9dd1222: UNIT:TEMP F/C, reading, CJ, SYST:TEMP?)
- [x] Temperature settings: remove the unclear "Offset 0" button (reset via the slider/long-press instead).
      (bdbbfcb: offset stepper + keypad, no Offset 0; verify on device)
- [x] "NTC user" is unclear: make its R25/Beta editable right there (or remove); explain on screen.
      (bdbbfcb: R25 / Beta fields on the temperature page; verify on device)
- [x] Second resistance mode present in the stock is missing in fw-rs — check stock parity (dmm_v2_6.h) and add it.
      (d6fc216: stock "ON" low-current variant made visible; verify on device)
- [x] Tap on the DMM value cycles averaging/resolution: fewer digits = faster readings; compute digits vs OSR/noise from the
      HY3131 timing + measured noise (not ad hoc); the +1 digit always drawn small at the end.
      (24f7080: FAST/MED/SLOW with a counts / rate toast; verified on device 9dd1222)
- [x] Power-meter mode: HY3131 can measure V and I at once (AD2/AD3 + MUL → active power in RMSDATA); Vrms, Irms, P (and
      decided: see docs/decisions-2026-10-22.md #4 (energy counter resets on boot)
      energy) by switching internal modes — check feasibility with the DM40 front-end wiring first.
      Researched: GO, docs/power-meter-and-misconnect.md §3 (I on the stock OP1 path, V via OP2→AD3 DC-coupled,
      signed 38-bit X·Y RMS read, ~10-12 days). Gate: device test T1 (OP2 on SENSE). Owner: fuse/CAT/10 A limits.
      Done (powermeter.rs / ui_power.rs / app_power.rs, doc §3.11): function 18, row B with SPHACAL before the
      AD3-first enables, P→V→P→I on R34 (2 discards), DC parts from the LPFs, S/Q/PF, Wh/VAh, per-channel autorange,
      burden compensation, current-limit countdown, auto-off held; fallback (stock AC V / AC A alternated, VA only)
      behind POW:METH / the page; long press on A or "Power W" on the Current page → wiring dialog; Power Settings
      page; SCPI CONF:POW / FETC:POW:ALL? / [SENS:]POW:…; DBG:POW:ROW?/POKE/STAT/RAW?/DISC. Emulator Power role
      + emu/hy3131/powtest.py (11 scenarios OK). (verify on device: T1, T2, T3 of doc §3.11, then T5/T6)
      Device 2026-10-22 (7dce69f, calibrator on V-COM, A not wired; doc §3.11 "Device results"): T1 GO -
      OP2 on SENSE linear, default row best (POKE 14/15 kills the signal, R35 0x44/0x74 no better), but raw
      gain -2.05 % / -1.63 % / -0.33 % (6 / 60 / 600 V) -> 0.5 % only after a per-range gain; T3 at zero
      current: YY clean from the 1st result, XX needs the 2 discards (keep DISC 2); UI dialog / settings OK.
      T2, T3 with current, T4-T6 pending (need current through A).
  - [x] Device follow-ups (doc §3.12): per-range V gain derived from the factory AC V / DC V gains (residual
        -0.03 / -0.08 / +0.12 % vs T1) + POW:VOLT:CAL self-cal (AC V / DC V reference, persisted); LPF zeros by
        POW:ZERO (leads open; internal AGND zero assessed, not default); f in power mode (adaptive R21 window
        around the tap's DC, PB4 = the current as fallback; cause: the unipolar T1 square); V shown + "I ranging"
        on entry, per-channel parts kept, own statistic first (~0.9 s per autorange step); I_dc with the AC A RMS
        gain while the LPF gain is uncalibrated; DISC 2 kept (XY / XX with current untested).
  - [x] DC power (owner): POW:COUP DC|ACDC|AUTO; DC = one fixed row (I on AD1 with the DC A path / factory gain
        and its own POW:ZERO zero, V on OP2 -> AD3 LPF at the same time, AD1 enabled last: note 7), a reading per
        100 ms, burden comp; AUTO by the AC/DC ratio (B) and the Y ripple (DC row). Persist v9.
  - [x] 7th DMM function button "W" (64 px buttons, 68 px pitch, DMM page + combined band); the wiring dialog
        only until confirmed once; restored after a reboot; "Power W" on the Current page removed. Persist v7.
  - [x] Power page rounds 2 / 3 (doc §3.12): real W glyphs (stockfont_w.rs); coupling the stock way (active W
        button: DC -> AC+DC -> AUTO, glyph on the button / above the unit, Settings row DC / AC+DC / AUTO); V / I
        on top, the energy box below (E | PF | S | Q | f), tap on the main value cycles P / Wh / VAh, reset with a
        confirm; the energy is not persisted over a power cycle (owner's call).
  - [x] Owner round 4 (doc §3.14): no false OVERLOAD in the power meter with I AUTO (an over range on the uA / mA
        shunt is a one-hop up-range to 6 A, no alert; the red OVERLOAD stays for a manual I range); coupling DC /
        AC / AC+DC (AC = the AC parts only, DC parts and zeros not used; AUTO dropped, POW:COUP AUTO = ACDC,
        deprecated; persist value 3 = AC, the former AUTO loads as AC+DC); stock DC / AC / AC+DC glyphs; Power
        Settings readable ("At load" with its help line, Zero / V cal dialogs, coupling meaning, status text);
        Help 7/7 "Power meter". powtest istep / istep-ua / iman-ovl / ac, senttest ovl-pow-auto-*.
        (verify on device: the check list of doc §3.14) Device f3aea2f: items 3 and 5 PASS (docs/device-check-2026-10-21.md "f3aea2f" 1);
        1, 2, 4 need current through A; 6 not run.
  - [ ] Verify on device: the re-check list of doc §3.12 (zero, V cal, f on the square, W entry, DC method /
        AUTO, then T2 / T3 / T4 with current through A).
        Device 5f14d38 (device-check "5f14d38" 5): PASS for W entry (dialog once, then direct), tap-W coupling
        cycle + glyphs, POW:ZERO (Y 1072, X ~890 / ~160), V vs DC V <= 0.063 % on 6 / 60 V in DC and AC+DC, f 40.0018
        Hz on the square (R21 0xBF), P / Wh / VAh tap, "Reset energy?" (1st Yes did not reset, 2nd did), reboot in W.
        Open: POW:VOLT:CAL, T2-T4 with current through A.
        Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 1): PASS for the coupling tap cycle DC -> AC -> AC+DC (round 4), Power Settings
        dialogs (At load, Zero -> Start, V cal -> Cancel, Wiring, Limits), POW:ZERO, f 40.0018 Hz, energy
        reset with Yes 13 s after the long press, no "-0"; POW:VOLT:CAL DC reference 6 V: 1.020998 (doc
        1.0209), 60 V with 10 V: 1.016612 (doc 1.0170; over-corrects 0.06 %); 60 V with 5 V = SMALL (the list
        above is wrong: 8 % < the 10 % window); AC reference blocked (calibrator: 4.77 V AC max). Gains set
        back to factory. Still open: T2-T4 with current through A.
        - [ ] V cal on 6 V AC-coupled with the 0..9.6 V square returned OK (1.021410) although the peak clips
              the 6 V range: the DC-kind path has no clipping guard. The dialog says 10-70 %, DC takes 100 %.
        Fixed after 5f14d38: the 1st Yes came ~13 s after the long press (screenshot 38 -> 39: 12.9 s; the
        2nd try 2.4 s) and the confirm auto-cancelled at 10 s, so the tap hit the bare page: now 60 s
        (dialog::ENERGY_MS, as the wiring / firmware questions). Emulator: powtest.py ereset (Yes 13 s
        after the long press resets; FAILs on 06d6f62). Negative zero: every power readout (main, E box,
        PF / S / Q, AVG, V / I) shows +0 for a value that rounds to zero (pw::no_neg_zero; ui-render
        p15 / p16, meter-test). Generic DMM readouts unchanged (dmm::format). Unit case: the E box already
        says "kWh" (F14 has k); the main readout / aux row now too: our LETTERS 'k' (tools/mkwglyph.py,
        FreeSans Bold k scaled by the stock h ink box, stockfont_w.rs OWN_GLYPHS); unit_text keeps 'k'
        only in the energy units (KΩ / KHz stay as the stock writes them). The chart axis labels and
        history boxes (ui::num_unit, all charts) show +0 too ("-0.000mW" on the power chart).
  - [x] TODO remainder (doc §3.13): BT cmd 09 / 06 = the DC V byte (0x28 auto) with P as a V-group value
        ("230.02 V" = 230.02 W; mW as mV; value 2 / 3 FFFF; cmd 06 echoing the byte keeps the power meter),
        BT SCPI runs every POW: / FETC:POW / CALC:LIM command (only DBG:* refused); limits in W: 10th slot
        "Power" (CALC:LIM in the power meter, the "Settings conflict" refusal removed; Hold & Limits from
        Power Settings > Limits; badge in the AUTO label's slot; fail beep; persist v10 p[62..64]); Wh limits
        not offered (one-shot budget, not a band); RESistive phase cal stays refused, now with the reason
        (flat cos optimum: the step fit needs ~3e-5 of P / S vs 1e-2 block ripple; OSR group delays cancel),
        REAC refuses NOTREACTIVE (|P/S| > 0.32) / RANGE / FLAT instead of storing a clamped value,
        POW:PHAS:CAL? = outcome,SPHACAL; "tap cycles P / S / PF / E" superseded by the owner's P / Wh / VAh.
        powtest limits / phasecal-ref / bt (all 21 OK); ui-render badge matrix + p17 / p18 (0 violations).
        (verify on device: the check list of doc §3.13 - phone app, W limits + beep, cal refusals, BT SCPI)
        Device 0667175 without current (docs/device-check-2026-10-21.md "Bluetooth (0667175)"): cmd 09 = 0x28, "0.0000 mV" for 0 mW, value 2 / 3
        FFFF; cmd 06 0x28 keeps POW; BT CONF:POW / FETC:POW:ALL? / CALC:LIM:UPP 100 (FAIL? 0) / UPP -1 (FAIL?
        1, HI badge) OK; RES refusal text OK. Left: the phone app itself, limits / beep with a real load,
        persist v10 over a reboot, REAC cal on a capacitor.
- [x] Detect wrong connection (voltage on the A jack / current mode with voltage present, leads in the wrong jacks) and warn.
      decided: see docs/decisions-2026-10-22.md #5 (A-JACK / LIVE? alternate with HOLD in the status slot)
      Done (decision #5): ui::slot_alert / alert_alternates; DmmView.alt = the app's 1 s strip_alt (now
      toggled in every view); the live alert (meter.live) is drawn while HOLD / AutoHold freeze the readout
      and an alert change under HOLD repaints at once (shown_state); strip_data the same rule. ui-render:
      both phases over the DMM page / strip matrices + a phase-flip stale-pixel check (0 violations).
      (verify on device: HOLD then current into A in DC V -> HOLD / A-JACK alternate 1 s each)
      Device 5f14d38: LIVE? / HOLD alternate ~1 s in continuity with 1 V, slot clean (A-JACK needs the A jack
      wired; Ω with a positive source never shows LIVE?: by design, needs a negative result).
      Researched: GO for current-in-A-jack in non-current modes (idle comparator on PB1, 47.5 Ω shunt, trips ~0.44 mA),
      partial for "V leads while in A mode"; blown fuse not detectable (fuse-check dialog instead). Doc §4, ~4-5 days.
      Done (sentinel.rs, doc §4.6): A-JACK by the comparator on PB1 (DC V / mV / ohm / temp) and the AD1 peak
      hold on PB1-PB3 (AC V / mV); V-JACK? by the comparator on the /1000 tap (DC A, SS5 only); LIVE? from the
      ohm / continuity / diode results; status-slot / strip badge, toast, 3x50 ms beep every 2 s; Settings 3/3
      "Jack alerts"; SYST:ALER? / :ENAB, DBG:SENT?. Emulator model + 18 USB/IP scenarios; cross table unchanged.
      (verify on device: T7, T9, T10 of the doc; T9 decides PKH_RESTART)
      Device 9dd1222 FAILS (docs/device-check-2026-10-21.md "9dd1222" 4): AC V 6 V range reads a ~37.2 mV floor
      with alerts ON (0.15-0.35 mV OFF, any DC input) -> PKH_RESTART / AD1 in AC V moves the RMS; A-JACK false
      trip in DC V with 10 V on V-COM, A jack open (calibrator 1 <-> 10 V autorange soak: streak 96, AJACK).
      T7 equal within noise for DC V / DC mV / ohm / AC mV / Hz. T9 / T10 not run (A jack not wired).
      Fixed (doc device-check "Fixes after 9dd1222"): AC V sentinel row holds OP1CHOP = 00 (OP1's 2 kHz chopper
      starts with AD1: offset square x10 -> 37 mV RMS floor; AC mV reads OP2, clean); DC V 60 / 600 / 1000 V
      carry no comparator sentinel (those rows drive the power bus from OP3; r0 keeps it, and leads in A/COM
      leave V-COM open = r0). DBG:SENTinel:VARiant 0..7 restores the 9dd1222 rows per bit for the re-check.
      Host tests + senttest dcv-step. (verify on device: re-check list of that section)
      Verified on device 5f14d38: AC V ON == OFF within 0.04 mV (VAR 1 brings back 37.15 mV: the chopper was the
      cause); DC V 1 <-> 10 V soak 5 min +0 hits, kind OFF at 10 V; VAR 4 on 60 V trips again; 10 V on manual 6 V
      no trip.
  - [x] The rest of §4 (doc §4.7, 2026-10-22): OVERLOAD shunt rule (uA / mA over range -> 10 A at once,
        latched, OVLD badge + 300 ms beep + toast, not maskable), sentinels in AC+DC (both phases), AC A (AD1 on
        the /1000 V tap), Hz (AD1 peak on PB1), continuity / diode / cap r0..4 (AD2 + OP2 on PB1); cap r5..7 and
        DC V 60 V+ stay uncovered (documented gaps); DC? (AC V) / AC? (DC V 6 V) / FUSE? hints; fuse check
        (Current Settings "Fuse check", SYST:FUSE:TEST / SYST:FUSE?); STATus:QUEStionable bit 9 (+ bit 1
        OVERLOAD). Emulator: model + 27 new senttest scenarios, 362 model checks; host tests; ui-render 0.
        (verify on device: doc §4.7 device test plan T8, T11..T16; DBG:SENTinel:VARiant +8 / +16 back out the
        new rows / the alternation if a reading moves)
  Compared with UNI-T UT197 (2026-10-22, its manual §16.2 + the A / mA fuse notes): the UT197 senses a PLUG in
  the mA/uA or A jack (no signal needed) and flashes "InErr" + beeps in any non-current position, i.e. before
  the probe touches a circuit; the sense path runs through the fuse, so "InErr shows" doubles as the fuse
  check. Ours sees only a current already flowing (mains on A-COM in V mode blows the fuse first, §4.3), and an
  unconnected plug is electrically invisible. Follow-ups, in order:
  - [ ] Device T9 / T10 with the A jack wired (never run), plus the 9dd1222 re-check list (AC V floor with
        alerts ON, DC V 1 <-> 10 V soak without A-JACK). Gate for everything below.
        (re-check list PASS on 5f14d38; T9 / T10, AC mV / TC soaks still open)
  - [x] "Leads in A?" reminder (closest firmware stand-in for InErr): on a change from a current function to
        a non-current one, toast "Leads in the A jack? Move to V" + a beep, at the moment the UT197 would warn.
        Fires also when the lead was already moved: own option in Settings 3/3 "Jack alerts" (and in the
        SYST:ALER:ENAB mask); decide whether it repeats or needs a tap to dismiss.
        Done (doc 4.9): any path A (DC / AC / AC+DC A, power meter) -> non-A; one 200 ms beep at once + one
        toast "Leads in A jack? Move to V" (the TODO text leaves 1 px to the frame) after the new function's
        first reading (<= 1 s). Decided: no repeat, no tap (the firmware cannot see whether the lead moved;
        A-JACK takes over when a current flows); no alert state, so decision #5's HOLD alternation is
        untouched. Fuse check exempt. Settings 3/3 row "A-lead reminder" ON / OFF (default ON, p[38] bit 15),
        SYST:ALER:ENAB bit 3 (ON = 15), DBG:LEADs?. Host test leads_reminder, emu/hy3131/leadstest.py, ui-render.
        (verify on device: doc 4.9 device check list)
  - [x] Fuse check (UT197's procedure, our doc §4.3 design): guided dialog in ohm mode (600 ohm, PE = 2),
        lead V/ohm -> A, ~48 ohm = OK, OL = FUSE OPEN, else "Check leads"; the 0.05 ohm (PE0) step left out
        (it tests the MOSFET, not the fuse). FUSE? hint: 60 s below 0.1 % of the range (AC 5 %). Doc §4.7.
        (verify on device: T14, T15)
  - [x] Sentinel coverage: AC+DC, AC A, Hz, continuity, diode, cap r0..4 done (doc §4.7); open: cap r5..7
        (both op amps and AD1 busy: no path) and DC V 60 V+ (the 9dd1222 false trip). T8 gates the AD2 rows.
  - [x] OVERLOAD shunt rule (§4.3 / §4.7). (verify on device: T11, with a current-limited supply first)
  - [ ] Hardware note (HW-1, for a board revision / mod): a sense contact in the A jack wired to a spare
        HY3131 input through a high-value resistor would give a real InErr and the fuse check with it.
- [x] (B) AD2 fast data logger, main-loop (E1, no ISR): LOG:SOURce AD1|AD2, LOG:SRATe 600..9600 (R26 ladder,
      hy3131::ad2_r26_for), AD2F-gated 19-bit reads with a sign-copy torn-read check, AD3/RMS/LPF off, burst
      polling (500 us), LOG:COUNt? lost (span-based), LOG:GAPs?, LOG:PREamble? source/req/nominal/r26/coupling,
      panel "AD2 meas/nominal Sa/s" + loss row; ring unchanged (2048 x 8 B: 213 ms at 9600/s). Rows: AC mV / AC A /
      mA / uA (DC-coupled), AC V (AC-coupled). Host: dm40scpi.py `log --source ad2 --rate N` (CSV with
      lost_before), `ad2 torn|noise`; emulator AD2 grid + noise model. Hooks: DBG:AD2:TORN? / NOISE? / DRAW.
      Emulator only so far (50 MIPS: ~2 kS/s readout, not the device's): docs/dmm-input-scope-feasibility.md 7.
- [ ] (B) device session (30 min, feasibility doc 7.3): torn reads per OSR, loss per rate with / without redraw,
      decided: see docs/decisions-2026-10-22.md #3 (one TIMER ISR, exclusive bus hand-off, gated on SPI2)
      shorted-input ENOB, a waveform; then decide the rate cap and the ISR (E2-E4).
      Partly done on device 9dd1222 (docs/device-check-2026-10-21.md "9dd1222" 1): torn reads none at any rung;
      waveforms AC V / AC mV ok; converter floor ENOB 16-17.6 (AC A open). FOUND: the chop-ON rungs give AD2F
      ~2x faster than ad2_r26_for's nominal (600 -> 1128/s, 2400 -> 3832/s measured; 0x24 = 0x04 readout), so
      LOG:COUNt? lost clamps at 0 below 9600; link ceiling ~6.5 kS/s; DRAW 2 = 26 ms gap per 100 ms. Open: the
      --chop off pair, a shorted-input mV noise run, then the rate cap.
      Fixed: ENCHOPAD2 does not slow AD2 (0x25 = 0x05, 0x24 = 0x04): ad2_period_ns ignores it, the ladder is
      1200 / 2400 / 4800 / 9600 S/s with or without chop (600 does not exist: /4 OSR 1024 = 1200), so nominal /
      lost are right; the bit-banged link topped out at ~6.5 kS/s (9600 showed ~32 % lost; SPI2 (E2) should lift that). Emulator model
      and tests follow. (verify on device: log 1200 / 2400 / 4800 measured == nominal, lost 0; 9600 lost vs the bus build)
      Device 5f14d38 (ad2-isr): ladder right, captured + lost = nominal at 1200 / 2400 / 4800 / 9600, chop on / off.
- [x] (E2) HY3131 on the hardware SPI2 (src/hy3131.rs; feasibility doc 8): SPI2 @ 0x4000_3C00, PC10/11/12 AF6
      (VERIFIED: GD32F450xx DS table 5, UM V2.2), mode 0, /16 = 3.125 MHz, CS PD2 GPIO, the read-period clock as
      one GPIO pulse on PC10 between SPI bytes (exact datasheet frame). It is THE bus (no runtime switch); the old
      bit-bang stays only as the fallback build feature `hyspi-bitbang`. DBG:HYSPI:SELF? n[,div]: constant-row /
      write-pattern / AD2-sign check + us per frame (div 8 / 16 / 32 for one call). Emulator: HY3131SpiPort on
      spi3; AD2 logger loss at 2400 / 9600 S/s 16 % / 79 % (BB) -> 1.5 % / 8 % (SPI2), plumbing only.
- [x] (E2) device check (10 min, feasibility doc 8.4): SPI2 build, SELF? at 16 / 32 / 8, readings vs the
      bit-bang fallback build, AD2 logger loss; on failure flash the `hyspi-bitbang` build. Then drop the
      fallback feature and the bit-bang code.
      8e5efa6 on the device FAILED (doc 8.5): DC V wrong, w1 write/read-back ~25 % bad at /16 and /32, reads
      fine -> frame-boundary timing (CS hold / idle vs the HY3131's 4.9 MHz SYSCLK), not the bit clock.
      DIAG image fca31fd on the device (doc 8.6): SPI2-written data bytes with bit 7 = 1 and bit 0 = 0 arrive
      with bit 0 = 1 (64 / 256 values, every divider / CS timing / pin speed); SPI2 reads clean at /8 /16 /32.
      Fixed: reads on SPI2 /16, writes bit-banged (pins switched per write frame); DIAG / RAW removed.
- [x] (E2) device check of the read-SPI2 / write-bit-bang image 7889e6c: PASS 2026-10-22. DC V 0.99979..0.99990 V
      (1 V cal), SELF? 2000 at /16 and /32: all error counts 0, 23 stable bytes, 7.65 us 1-byte / 21.4 us AD1+AD2
      read; AD2 logger 9600 S/s -> 8534 S/s measured, 11.1 % lost, 0 bad, max gap 2.05 ms (bit-bang ceiling was
      ~6.5 kS/s); 4800 S/s -> 8.0 % lost (main-loop gaps: the ISR decision).
- [x] Drop the `hyspi-bitbang` feature (bit-banged reads) now that 7889e6c passed: done (bit-banged WRITES stay,
      DBG:HYSPI:SELF? stays).
- [x] Pure-SPI2 writes (feasibility 11.6 / 11.7): device 4b98128 DBG:HYSPI:WR?: the chip takes each byte's LSB
      late (the next MOSI level); b81abf2 writes one frame per register with a CS-cut dummy whose MSB = the LSB:
      0 bad in 8 x 512 writes at /8, /16, /32, DC V 0.9997 V on 1 V, SELF? w1 9.9 us (bit-bang 57 us). The
      bit-banged write is removed.
- [x] (E3/E4) AD2 stream on ONE timer ISR, decision #3 (docs/dmm-input-scope-feasibility.md 9): Cargo feature
      `ad2-isr` (OFF by default until the device acceptance below), TIMER6 = TIM7 IRQ 55 the only NVIC line
      (shim.rs table to IRQ 55, everything else ICER'd + SysTick / PendSV cleared at the hand-off, PRIMASK
      cleared only while streaming), exclusive HY3131 bus hand-off (hy3131.rs `stream`, every main-loop
      read / write stops the stream first: DBG:ISR? forced), SPSC ring (logger.rs `RING`, the same 16 KiB,
      full = newest dropped), ISR = INTF + R03..R05 over SPI2 with capped waits, tick 4 x the rate + 7/8 P
      read gate (DEVIATION from "~1.05 x": 1.05 x gave ~P stamp jitter and 5 % false gaps, doc 9.2), default
      LOG:SRATe 4800 with the feature. DBG:ISR?. Stack check counts the ISR (+284 B; stackdepth.py also
      sees IT-block `bl` now: main path +336 B, headroom 17456). Emulator (200 MIPS, DRAW 2): 4800 / 9600
      / 1200 S/s lost 0, no stamp gap (main-loop sampler: 23 % lost at 4800 DRAW 2);
      emu/hy3131/ad2streamtest.py PASS.
- [x] (E3/E4) device acceptance of the `ad2-isr` image. 5f14d38 FAILED (1.4 % / 2.6 % lost in bursts: the INTF
      read-clears race at a +63 ppm tick; max_lat ~20 us). Fixed in 4b98128 (feasibility 11): the ISR is
      phase-locked to the conversion grid (src/ad2pll.rs: blind R03..R05 read half way between edges, INTF
      probe every 4th period), hot code / data below 0x08080000 (link-native.x region B / .cold; the fonts had
      grown into the non-zero-wait flash). Device 4b98128, 60 s each: lost 0 of 288 197 at 4800 DRAW 2, 0 of
      576 092 at 9600, 0 of 1 153 647 at 19200 (/16, DRAW 2, CPU 35.6 %); max_lat 4.7 us (was 21.8); chip
      crystal -1 ppm; *IDN? max 0.73 ms; READ? / SELF? clean after. /8 reads NOT clean (61 / 100 sign errors in
      1.15 M / 2.36 M): stream stays /16. `ad2-isr` is DEFAULT since b81abf2.
- [x] (E3/E4) device check of b81abf2 (feasibility 11.9): critical-section without masking -> max_lat 0.24 us;
      CPU /16 4800 DRAW 2 / 9600 / 19200 DRAW 2 lost 0; DMA frames 9600 lost 0 at 5 % CPU; /8 + CKPH = 1 data phase
      clean (SELF?, 19200 lost 0, 38400 35.8 M samples 0 bad at 14 % CPU) -> the stream default (CPU frames).
      DMA at 38400 lost the lock (CPU probe frames overran): probes as DMA frames since a4907d0.
- [x] Device a0cdb4e (feasibility 11.10): DMA /16 38400 lost 0 of 2.3 M at 17.5 % CPU, 19200 DRAW 2 lost 0 at
      7.8 %; CPU /8 + CKPH 38400 lost 0 at 51 %, 4800 DRAW 2 / 9600 lost 0. Host: a raw drain tops out at ~37 kS/s.
- [x] Decide: DMA frames as the stream default; the wave view gains the most.
      decided: see docs/decisions-2026-10-22.md #7 (DMA frames default, gated on one device run with the
      default flipped: 60 s runs, a 15-min 38.4 k sharing matrix with gen DAC + BT, the wave bands)
      done a239aa6 (`set_dma_req` default 1; DBG:AD2:DMA 0 = CPU frames). Device 11.11: 60 s 4800 DRAW 2 /
      9600 / 19200 DRAW 2 / 38400 lost 0 at 1.9 / 3.9 / 7.8 / 17.5 % CPU; 900 s 38400 with the generator
      DAC at 10 MS/s lost 0 of 34.56 M at 17.6 %; Wave view 5 ms..1 s/div bad 0, the same measurements as
      CPU frames, first sample 14..51 ms after a band change (CPU 10..22 ms); DC V / SELF? clean after.
      Emulator ad2streamtest +-100 ppm (default, no :dma) + wavetest PASS.
- [x] Device: decision #7 sharing matrix, BT part: 38400 for 15 min with BT connected (phone or host SPP),
      lost / bad 0, BT answers after the stream. Not run on a239aa6: the host got "Host is down" from the
      meter's SPP address for the whole session (11.11). Cause: the next item.
      Done on 0667175 (docs/device-check-2026-10-21.md "Bluetooth (0667175)"): 900 s 38400 DMA frames with a host btpoll (50 ms) linked the whole time:
      34 560 269 records, lost 0, bad 0, seq / stamp gaps 0, max gap 27 us, CPU 17.5 %; BT cmd 09 replies
      during the stream 0 (by design, UF-36), BT answered right after.
- [x] BT module power (PC0): native fw-rs (since e3a457d) left PC0 high from board::init, the stock boot
      level = module off, and never lowered it (the stock does at its BT UART init); the RAM-loaded builds
      had inherited a powered module from the stock app. bt.rs `Bt::power` now follows the Bluetooth setting
      (low = on, high = off), waits 1 s after power-on before the AT init, pulses PC0 high
      100 ms after a successful cmd 07 (stock); the status-bar BT glyph needs Bluetooth ON (PC14 is
      meaningless with the module off). Emulator BtModule models PC0 (on only while driven low; a host
      connect waits for power, else refused): powtest bt FAIL on the old image, OK fixed; modetest fwrs dcv/acv OK; stock dcv
      OK (the stock's PC0 handling passes the model). (verify on device: BT glyph off with no host; the
      classic XX:XX:XX:XX:XX:XX answers `dm40scpi.py --bt info`; then the pending BT checks: doc 3.13 power
      meter list, decision #7 BT part, phone app (BLE "DM40A"))
      Device 0667175 (docs/device-check-2026-10-21.md "Bluetooth (0667175)"): glyph off with no host, on while linked, off after; --bt info OK;
      reconnect 0.7-1.3 s; Settings Bluetooth OFF -> not discoverable, connect times out; ON -> discoverable,
      answers. Phone app (BLE) not run.
- [x] BT link opened while the DMM rail is off (Scope / Gen page) does not wake the DMM: cmd 09, BT FETC? /
      READ? and USB READ? return the reading from before the rail went off and ignore a 1 -> 2 V step (3/3).
      After >= 3 such connect / disconnect cycles the DMM stays frozen even back on the DMM page (> 9 min,
      PROF d.read 0, "AC?" hint; 2/2), until `DBG:HYSPI:SELF?` (its `app.dmm.start`). A link held while
      leaving the DMM keeps it live. Suspect: `Dmm::start` from `set_suspended(false)` right after PE2 goes on.
      Repro: docs/device-check-2026-10-21.md "Bluetooth (0667175)" §1.
      Cause (emulator, PE2 now power-gates the HY3131 model): a wake that leaves PC4 on (BT link / host query
      on Scope / Gen) wrote the row in the same pass as the PE2 rise (the page switch has the 10 ms
      make-before-break pause, the wake had none): the chip powered up with its POR row, nothing ever
      rewrote it; with the rail then kept on (host_dmm after a READ?, the link) the DMM page did not restart
      it either. Fix (dmm.rs): the row goes out RAIL_SETTLE_MS = 50 ms after any rail rise (`wake_at`,
      the deadline moves at once so READ? waits for the fresh reading); a stall watchdog (no reading for 2 s
      past the deadline -> R2A..R31 read back, all 0x00 / 0xFF = the row is lost -> restart; no reading for
      max(10 s, 8 x period), not in capacitance -> restart). Counters in SYST:RAIL:STATe? (wakes, lost,
      silent). emu/hy3131/btwaketest.py: 3bae462 fails 1g / 1h / a chip POR, the fix passes. The device 1h
      (no READ?, the rail toggling, the page switch's 10 ms pause) is not fully explained by the model: the
      counters on the device re-check discriminate (wakes per link cycle; lost / silent).
      Device 036cc55 (device-check-2026-10-21.md "DMM rail wake and fast logger"): 1g / 1h PASS, wakes +1 per
      link (the rail follows PC14), lost / silent 0 everywhere incl. 35 s idle in Hz / temp / ohm / power /
      diode / cap. AC V: the first reading after a wake was +8 % -> ea29bc4: 2 s settle after a wake in the AC
      functions, and leaving the Wave view no longer drops the DMM rail (wavetest "DMM resumed" regression).
      Device ea29bc4: AC V wake READ? -0.8 % (first answer after 2 s), Wave view MENU -> READ? correct with no
      rail drop, 1g / 1h PASS again.
- [ ] DMM reads at /8 + CKPH = 1 too (2.6x faster bus); needs its own device check of every function.
      decided: see docs/decisions-2026-10-22.md #8 (DMM stays at /16 mode 0; acceptance check defined for the
      revisit with decision #3's request queue)
- [ ] B (zero-wait flash) is FULL: the branch overflowed by 4 bytes at a4907d0; the cold modules' named
      .rodata moved to `.cold` (28 bytes left), the bit-banged write removed. A real reclaim is needed; never put
      per-pass code in `.cold` (feasibility 11.4 rule).
- [ ] 76.8 kS/s: /8 + CKPH works for CPU frames; the period (13 us) leaves no room for the CPU INTF probe
      (5.8 us at /8): needs a DMA data phase started after the CPU command byte + CKPH switch, and DMA probe
      frames at /8 mode 0 (probe noise tolerated). Not done.
- [ ] (B) DC-coupled V on AD2 (new row, feasibility 1.3 last row): needs device test 6.5 first; not implemented.
- [x] (B) AD2 samples to the SPI disk (.DLG): the fast logger (LOG:DEST DISK, above / docs/compact-log-format.md
      "Fast stream"). Over BT: the ~870 records/s link ceiling (docs/bt-scpi.md) is below AD2's slowest rung
      (1200 S/s): LOG:STATe ON from BT is refused for AD2 / > 800 S/s ("Rate too high for Bluetooth"), AD1
      (<= 640 S/s) streams over BT LOG:DATA?; BT LOG:DATA? of a faster USB session: empty block + the error.
      Device 0667175 (before this refusal existed): AD2 1200 S/s started and drained over BT gave 918 records/s,
      lost 0, 6 593 of 36 418 overwritten (ring full): confirms the ceiling (device-check "Bluetooth (0667175)").
- [ ] Fast logger device check (fastlog.rs; ~30 min; emulator results in docs/compact-log-format.md):
      1. DC V, LOG:SOUR AD1, LOG:OSR 0, LOG:CHOP OFF, LOG:DEST DISK, LOG:DUR 60, LOG:STAT ON: LOG:DISK? goes
         1 (prep, note the seconds) -> 3 -> 5; LOGS/FAST000.CSV on the USB volume == dlog.py of MMEM:DATA?
         "FAST000.DLG"; lost_before all 0 (640 S/s), LOG:COUNt? overwritten 0; DBG:PROF? loop max.
      2. AC V 0.5 V 50 Hz, LOG:SOUR AD2, LOG:SRAT 4800 / 9600 / 19200 / 38400, LOG:DUR 30 each: sum of
         lost_before and LOG:COUNt? overwritten per rate (expect 0 up to 19200; 38400 = the chip's tPP);
         DBG:ISR? max_lat while pages program (SPI1 is not the HY3131 bus: expect unchanged ~0.24 us).
      3. Record page: interval "Fast", Duration 10 min, Start in DC V: the panel shows "Preparing disk" /
         %, then rate / samples / FASTnnn KB; a tap stops it; toast "Saved FASTnnn.DLG"; a tap during
         Prep cancels (no file).
      4. LOG:DUR 0 on a disk with ~1 MB free: Prep allocates all, the session ends Full (LOG:DISK? 6).
      5. Power: pull the battery mid-run once; after boot fsck the SPI image (DEBUG/SPIFLASH.BIN):
         FAST file with the allocation's size, decodes up to the cut.
      6. BT (dm40bt_scpi.py): AD2 LOG:STAT ON -> "Rate too high for Bluetooth"; AD1 OSR 0 CHOP OFF over BT:
         dm40scpi.py --bt log --source ad1 --osr 0 --chop off for 60 s, LOG:COUNt? overwritten 0 (the
         870/s ceiling: 640/s should hold), then OSR 0 chop on (320/s).
      7. Prep time on the real chip for LOG:DUR 60 at 38400 (~7 MB): erase time vs the emulator's model.
      Device 036cc55 (device-check-2026-10-21.md "DMM rail wake and fast logger"): (1) AD1 640/s: 1 122 of
      ~38 400 lost (1.8 %, g2 gaps, max 4.7 ms) - FAIL, to investigate; CSV on the volume == the tool's.
      (2) AD2 4800 / 19200: 0 lost; 38400: 1 lost; max_lat 1.6-12 us (not ~0.24). Host stop mid-run: clean
      (done, decodes to the last row). 3-7 not run.
      Fixes (2026-10-22, emulator / host; device re-check = items 8-11 below):
      - (1) AD1 unchopped 160 .. 640 S/s runs on the stream ISR (logger.rs `ad1_on_isr`): R00..R02 read blind
        mid-period, phase-locked by INTF.AD1F probes (ad2pll.rs, the timer at 40 / 80 / 160 ns per
        count, PSC 4 << KSH - 1, so a 1.56 .. 6.25 ms period fits the 16-bit ARR; CPU frames at /16).
        The slower AD1 settings keep the main-loop sampler, which now reads R00..R02 blind once when
        AD1F does not come 1.25 periods after the last sample (the read-clear race; DBG:ISR? `late`).
        Emulator (AD1F race on, `IntfRaceAd1`): AD1 640 / 160 to disk 0 lost.
        Device 5394b21 (2026-10-22): AD1 640 to disk FAILED: no AD1 edge in the 3-period search, the
        gated CPU stream then never probed (probes 0, captured 0 for 700 s) -> probes while gated, an
        8-period AD1 search. AD1 40 (OSR 3 chopped, main loop) host session 60 s: 2320 samples, LOG:GAPs?
        all 0, 83 blind rescues (`late`; 3 / 91 equal to the previous value, as elsewhere): the device
        DOES lose ~4 % of AD1F flags to the read-clear race at a tight poll; the chip's real period
        there is 26.3 ms (nominal 25: the 3 % "lost" is the span vs the nominal) -> rescue stamps /
        window from the measured period; the ISR only for the unchopped settings (the Pll locks to
        the nominal within +-1000 ppm). AD2 38400 / 19200 to disk 30 s: 0 lost (was 1); max_lat 6.8 /
        6.1 us, lat_pc VDisk::block_size (region C: the desktop polls the mounted volume) -> every
        VDisk BlockDevice method and the directory sectors in region B. Volume read of FAST006.CSV
        (53 MB): ~39 KB/s, the main loop's USB section ~15 ms per pass -> DBG:VDISK:RD? to see where.
        Device 046e4b9: AD1 640 / 320 / 160 (chop off) to disk 60 s: 38402 / 19201 / 9602 samples,
        0 lost, gaps 0, locked, -0.9 ppm, max_lat 0.24 us. AD1 40 host 60 s: gaps 0, 76 rescues,
        stamps 26 ms apart (LOG:COUNt? still says 73 lost: the span vs the nominal 25 ms). AD2 38400
        / 19200 to disk with the volume mounted: 0 lost, max_lat 0.28 us. CSV read still ~40 KB/s,
        also with strictly sequential 64 KiB reads (FADV_RANDOM): DBG:VDISK:RD? 6.4 ms per
        read_block, ~44 SPI reads of ~28 bytes (header-sized) per sector, 1 open; a host replay of
        the device's own SPI image (fatren-test vread) reads 0.07 per sector: not reproduced. ->
        two CSV cursors + seek checkpoints (a head-reading indexer no longer drags one cursor), a
        lock timeout (re-acquire after 2 s unlocked with nothing captured, then end the session),
        DBG:VDISK:RD? also counts non-cursor seeks, headers read, rows decoded: to be measured.
        Device f3aea2f: FAST003.CSV sequential 64 KiB reads 571 KB/s (was ~40), RD? delta per sector
        0.21 ms, 0.1 SPI reads, 200 non-cursor seeks / 842 headers in 68 k sectors; `dd bs=1M` of
        FAST002.CSV (26 MB) 1.4 MB/s. The 046e4b9 slowness (one cursor walked headers per sector)
        matches the indexer interleave the second cursor + checkpoints address; not proven which.
        Sanity: AD1 640 and AD2 19200 to disk 30 s, 0 lost, locked, max_lat 0.24 / 0.28 us.
      - Emulator: the AD2 19200 DMA lock never converged in 3 of 9 sessions (5394b21; 0 of 9 on
        f66af0d): a coin-toss probe around the edge geared kp up before it reached kp_min ->
        ad2pll `result`: locked also at kp <= kp_max / 4 after 64 probes (9 of 9 pass).
      - (2) max_lat 12 us at the disk session's start / stop: region C code ran while the ISR streamed
        (`fast_events` / `fast_start` (.cold) started the logger and went on in C: `fast_head`,
        `prep::run_begin`; at the end `fast_events` ran before it stopped the logger). Now
        `Logger::plan` + the header first (C), `logger.start` / `logger.stop` in main.rs (B). Emulator
        hooks on every region C function, flagged when entered with the bus handed to the ISR (the
        hy3131 HANDED byte): f66af0d `fast_events` at the stop of an AD2 38400 disk session; now none
        in disk sessions AD1 640 / 160 / 40, AD2 38400 (fastlog::prep / fatren / fatdir left unhooked:
        per-pass Prep code, too slow under hooks). Also the USB volume's read path is in B now
        (a host reading the volume while a stream runs no longer stalls the ISR).
      - (3) FAST CSV reads: no full CRC scan at the volume refresh / open (headers + the last 4
        chunks), no FAT re-walk per chunk (quadratic), the read path in region B, several USB packets
        per pass while a READ runs (docs/compact-log-format.md 5). Host (fatren-test `vread`, 50.6 MB
        CSV): SPI reads 171 k -> 13 k, ~1.5 s of SPI time; refresh ~1.5 s -> 0.04 s. Emulator
        (fastlogtest --vol-read, MSC READ(10) of FAST000.CSV, 3.5 MB): == dlog.py; ~2.1 MB/s of
        virtual time before and after (its USB moves a whole URB per poll and its flash is all
        zero-wait: the packet pumping and region C are the device's to measure, item 10).
      Open (emulator, once in 4 runs of the max-flash profile, 25f3879, 4 emulators sharing the CPU):
      an AD2 19200 disk session's DMA-frame lock never converged (DBG:ISR? locked 0, probes alternating,
      max_run 7: the phase step geared up and down), so the gated stream took no sample and the session
      never ended; two reruns of the same sequence passed, as did f66af0d's once. AD2 path untouched by
      this change. Wants: a lock timeout (restart / error after N s unlocked) and a repro with --ppm.
      8. Device: AD1 640 S/s to disk 60 s (as 1): LOG:COUNt? lost 0, LOG:GAPs? all 0; DBG:ISR? active 1
         during it, rate_mhz ~640 000, locked 1, late 0, max_lat < 2 us; also 320 / 160 (OSR 1 / 2, chop
         off). AD1 40 (OSR 3) host session over USB 60 s: gaps 0, rescue stamps ~26.3 ms apart.
      9. Device: AD2 38400 / 19200 to disk 60 s: max_lat < 2 us (lat_pc if not), 0 lost; also with the
         USB volume read meanwhile (cp a big CSV): max_lat still < 2 us.
      10. Device: copy a ~50 MB FAST CSV off the volume (`time cp`): >= 500 KB/s, DBG:VDISK:RD? before
          / after (read_block calls, ms, max us, opens, flash reads / bytes); also a small one
          and LOG000.CSV; `cmp` with `dm40scpi.py get` / dlog.py of the DLG. The volume refresh after a
          large session: no visible stall (was ~1.5 s per 2 MB of DLG).
      11. Device: DMM page while a host copies: the reading keeps updating (2 ms of USB per pass).
      Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 5 / 6): 11 PASS (READ? every 0.40 s through a 45 s copy at 584 KB/s). 9 with a
      host reading the volume: AD2 19200 10 s to disk 0 lost but **max_lat 8.98 us, lat_pc
      vdisk::file_read::{closure#0} in region C** (the `near` closure got its own symbol, matched by the
      `*7dm40_fw5vdisk*` .cold rule) -> a fn in .text.vdisk_rd (fixed, to re-check). Still in region C on
      the volume read path: the DMM-log / scope CSV generators (Gen::advance, RowDec::*, SDec::*, chunk_at,
      Gen::open): a host reading LOG*.CSV / SCOP*.CSV during a stream runs C per row.
      - [ ] **Device hang + watchdog reset (once)**: `dm40scpi.py fast --source ad2 --rate 19200 --dur 30`
            with a host reading FAST003.CSV over the volume: the session completed (FAST005.DLG intact),
            the volume's UNIT ATTENTION at the end failed the host's reads, the tool's MMEM:DATA? of
            FAST005.CSV timed out, usb-storage reset the device 33 s later, then FWDGT (DBG:BOOT? WDT_RETRY).
            Not reproduced: MMEM:DATA? alone / with a volume read of another or the same file / the session
            end with a volume read but no MMEM:DATA?. Suspect the overlap of the session end, the volume
            refresh, the host's revalidation reads and Gen::open of the new FAST file over USBTMC; repro in
            the emulator (fastlogtest --vol-read + MMEM:DATA? right after done).
- [x] (A) DMM-input "slow floating scope" via AD2 = the Wave view (E5/E6; docs/dmm-input-scope-feasibility.md 10,
      src/app_wave.rs; feature `ad2-isr` only, hidden without it): MF range page "Wave" (V / A) and the scope page's
      "MCX | DMM" source badge; the AD2 stream drained into the scope record (continuous, 10-bit codes), the stock
      page unchanged around it: 5 ms..50 s/div (9600 / 4800 / 2400 / 1200 S/s per band, roll from 200 ms/div),
      V/div = the DMM range (+ / - step it), coupling = the row's (AC V AC-coupled, mV / A DC-coupled, no DC V
      row: says so), units V / A, measurements / cursors / STOP / zoom / Save CSV / FFT as the MCX scope; trigger
      (edge / pulse / runt) detected per sample IN the stream ISR (trigsoft::Stream; no dead time; decision #3
      amended: config words + event ring), HY3131 comparator evaluated, not used (10.2). DMM paused while open,
      row rewritten on exit; a host CONF closes it. Emulator: emu/hy3131/wavetest.py (sine V, square A, trigger,
      roll, range steps, exit, conflicts) PASS; ui-render wave scenes 0 violations; stock pixel diff unchanged.
- [x] Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 8): Wave view 10.6 on the 40 Hz square: entry, Freq / Vrms (-0.6 %), trigger
      Normal / Single, range 2 / 20 V/div / 200 mV/div, STOP + Save CSV, MENU exit, conflicts PASS. Fixed:
      roll at 5..50 s/div moved at the 2 s/div speed (w.per from the clamped `screen`), the DC V entry toast
      and two more toasts were wider than the box. Current steps blocked (A not wired).
      - [ ] 5 ms/div: Freq / Cycl "--" with two periods on screen; Vrms -1.6 % from 50 ms/div (1200-2400 S/s).
      - [ ] A volume refresh (Save CSV, a screenshot) runs region C (vdisk::ours / refresh / scan_spi) under
            the Wave view's stream: max_lat 21.7 us, 15 overruns at 4800 S/s. Defer the refresh while the
            wave streams, or move the refresh path to region B (~3 KB).
      - [ ] CONF? in the wave view on 200 mV/div (AC mV) still answers VOLT:AC +6.
- [ ] (A) device test of the Wave view (feasibility 10.6, 15 min) - after the ISR loss fix (E3/E4 device item:
      its holes restart the wave record, 10.4). Follow-ups: 16-bit record, DMM reading alongside (request
      queue), DC-coupled V row (E7). Done: the timebase list starts at 5 ms/div in the wave view
      (scope_ui::set_tb_lo from app_wave; touch-test + ui-render s01-tb-list-wave).
- [x] Device 2026-10-21 (docs/device-check-2026-10-21.md "Fixes"): stack 37468/46048 B (81 %) on 16b3eb9.
      d957027: no USBTMC tx copy, 8-byte logger records, .sram_bss (EP_MEMORY, MSC_IO, MSC block, touch
      log, dlog TAB); build.sh native checks the static worst path (tools/stack-depth) >= 16 KiB headroom,
      link-native.x stack >= 28 KiB + 16 KiB. Emulator walk 38580/43448 -> 28532/48344 B.
      (verify on device: DBG:BOOT? STACK after the scope walk, expect ~28.5 K used of ~48.3 K)
      device a239aa6 (after the 2026-10-22 rework): 21.6 K used of 46.6 K (feasibility 11.11)
- [x] Scope 32 ms loop passes ~3/s (meas 11.4 + frame 5 + cells 12.3 ms in one pass): a33e3ad integer
      measurement maths, graph paint and cells on later passes (one changed cell per pass); fa3358d a
      trace-only graph flush stops after 4 ms. Emulator 1 ms/div loop max: noise 10.9 -> 5.7 ms, 40 Hz
      square 21.5 -> 8-10.5 ms. (verify on device: DBG:PROF? loop max <= 12 ms, 400 DBG:PING? p99) Device 9dd1222: no 20-50 ms passes any more;
      scope page open input loop max 12.38 ms (marginal), ping p99 0.7 ms; square after AUTO 10.65 ms. Combined
      view: loop max 11.4 ms but ping p50 11.8 / p99 16.5 ms.
      Cause (device profile): not the view but a noise-triggered trace at 49.8 fps: every column changes, the
      paint is ~10 ms per frame (frame pass 6.2 ms incl. a 4 ms flush slice + 2-3 more 4 ms slices), so the loop
      ran 2-10 ms passes back to back (scope 98 % CPU, 56 % of it in sp_tick outside the nested sections).
      Fix: flush slice 1.5 ms, the paint starts on the pass after the frame (frames in back-to-back passes still
      paint at once), DBG:PROF? "s.paint" (was x1) times it. Not measurable in the emulator (it does not reach
      that frame rate at 1 ms/div). (verify on device: combined view, open input, 1 ms/div: ping p50 <= 2 ms)
      Device 5f14d38: 56.6 fps, s.paint max 1.68 ms, loop max 12.3 ms, ping p50 3.3 / p99 18.1 ms (was 11.8 /
      16.5): better, p50 still above 2 ms.
      Fix after 5f14d38: the combined view paints at most 25 fps (app.rs COMBO_FRAME_MS 40; Auto / Normal
      running records; Single, STOP, roll, the wave view and the Scope page uncapped; a frame not drawn
      still measures and its paint pass goes on) and its flush slice is 0.8 ms (scope_ui
      set_flush_short; Scope page 1.5 ms as before). Expected: paint ~11 ms x 25 = ~28 % CPU instead of
      63 %, most passes short. Emulator (~9 fps there, cap not reached): with the cap at 200 ms the
      combined view drops to 4.7 fps and the Scope page stays at 9.5. (verify on device: combined view,
      open input, 1 ms/div, 400 x DBG:PING? p50 <= 2 ms, DBG:PROF? ~25 fps, s.paint max <= 0.9 ms)
- [x] Scope ~9 fps at 1 ms/div after AUTO: three comparator misses (AUTO's fast-timebase sweep) disabled
      the hardware trigger for good; 53bdb9d suspends it until the next range / timebase / coupling /
      edge / mode / type change. The emulator's comparator model barely fires (ScopeFeeder TIM2 gate
      timing; separate), so the fps itself is unverified. (verified on device 9dd1222: AUTO on the 40 Hz square,
      then 1 ms/div: 40.0 fps, DBG:HWTRIG? fields 5/6 = 1,1, 38 fps during level drags)
- [x] STOP then Save CSV gave "No record to save" (Auto untriggered / STOP mid-acquisition): f09b3b0 the
      samples captured so far become an untriggered stored record on STOP. (verified on device 9dd1222: Auto open
      input, Auto square mid-acquisition, Normal WAIT -> SCOP000..002.DLG, "untriggered" headers, dlog.py == MMEM)
- [x] Experiment 2026-10-21 (6e32fe7 on the device, Linux 7.0, GNOME/udisks): a UNIT ATTENTION 06/28/00 on the
      MOUNTED read-only LUN 1 (DBG:VDISK:FORCE? 1 + ADD?) -> kernel rescans sdb, udev change DISK_MEDIA_CHANGE=1,
      the mounted vfat lists the new files at once with correct content, no I/O errors, no unmount. Held UA (the
      default) shows nothing until unmount. Simulated eject (EJECT? 3: NOT READY x3): capacity 0 -> back, stays
      mounted. Nautilus: no auto refresh, F5 shows the new files (owner: acceptable).
      => live file lists on a virtual read-only volume work on Linux without MTP if the UA is NOT held
      while locked; keep existing files' clusters fixed (cached inodes). Windows / macOS / Nautilus refresh: untested.
- [x] USB storage redesign (owner 2026-10-21): stop exporting the SPI FAT as LUN 0. The SPI flash becomes internal
      storage (FAT16 kept for the vendor FS-loader: dm40*.bin in the root + the calibration safety copy) with
      subdirectories (LOGS/, SHOTS/, SCOPE/) in our FAT code. USB exports ONE generated volume: LOGS/*.CSV,
      SCOPE/*.CSV (from DLG), SHOTS/*.BMP, INFO/ (today's LUN-1 files), DEBUG/SPIFLASH.BIN (raw image), with the
      media-change UA sent at once (not held while locked; existing files keep their clusters). Then: delete from
      the PC (recognise directory-entry 0xE5 writes) and dropping dm40*.bin for an update (DAPLink-style write
      recognition) as follow-ups. Removes "disk in use" refusals and host-side FAT corruption.
      docs/native-boot.md "Unit data".
      - [x] src/fatdir.rs: subdirectories in our FAT code (create with . / .., grow, rmdir, directory views
            for the writers, for_each over root + dirs); LOGS/ SHOTS/ SCOPE/ made at native boot (8 clusters =
            1002 slots each), DBG:BOOT? FATDIR. Writers -> directories, numbering over root + directory;
            files of earlier builds STAY in the root (no boot-time move: two-live-entries / lost-file window),
            every reader lists both. Host: create / grow / power cut before every write, fsck after each.
      - [x] src/vdisk.rs: ONE read-only LUN "DM40 files", 1.98 GiB FAT16 / 32 KiB clusters: README.TXT,
            INFO/ (the old LUN-1 files), DEBUG/SPIFLASH.BIN, LOGS/ SCOPE/ (CSV from DLG), SHOTS/ (BMP
            streamed); table of 400 files (SRAM): existing files keep slot + clusters, append-only, compaction
            only when out of space / after a bus reset AND unlocked; UA at once also while locked; refresh 1 s
            after the last change. Host test: sparse dump, fsck, mtools, CSV == dlog.py, BMP == SPI file,
            SPIFLASH.BIN == the SPI image, places kept across refreshes, compaction. Emulator (USB/IP): luns,
            mscdump (fsck clean, files equal their sources), screenshot under PREVENT -> UA after 4 s, new
            SHOTS/S003.BMP appended, the rest in place.
      - [x] "Disk in use by PC" refusals removed (screenshot, scope record, recording start / commits, gallery
            and MMEM delete); MMEM:CAT? lists "LOGS/LOG000.DLG", names accept a LOGS/ SHOTS/ SCOPE/ prefix.
      - [x] Firmware update without the writable disk: DBG:UPLOAD BEGIN / DATA (raw binary, before the SCPI
            parser) / END? / ABORT (src/upload.rs: DM40FW.BIN in the SPI root, other dm40*.bin removed, CRC
            re-read, header checked, data -> FAT -> directory); tools/dm40dbg.py upload; tools/reload.sh uses it
            (falls back to the USB disk for an older fw-rs); emu/usbip_client.py upload (emulator: 480 KB in
            7.9 s). Recovery path (the loader's own disk) untouched.
      - [ ] Device: (the default "Files live" volume, decision #6) Linux kernel attach + udisks ro mount of the 2 GiB volume; a recording / screenshot while
            mounted -> new file visible after F5 (UA at once); Windows Explorer and macOS Finder with the UA
            while mounted; `fsck.fat -n` on a copied DEBUG/SPIFLASH.BIN; first boot after the update: FATDIR
            0x7 created 0x7 (~1 s extra boot, once), boot time of vdisk::boot_refresh with many logs;
            reload.sh end-to-end (upload ~10-20 s?, DBG:UPDATE, "installed OK"); MMEM:CAT? / DATA? / DEL with
            directories (dm40scpi.py get LOGS/LOG000.CSV).
            If Explorer does not show new files after the UA: try a volume serial that changes with every
            refresh (Windows may key "same medium" on it).
            Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 3a, Linux): PASS: udisks ro mount, a device screenshot listed ~9 s after the
            gesture, a recording at its stop (== MMEM:DATA?); fsck of SPIFLASH.BIN clean (dirty bit only).
            A host read in flight at the UA gets EIO (reopen works). Windows / macOS, reload.sh not run.
      - [x] Follow-ups (2026-10-22, src/vwrite.rs; docs/native-boot.md "Host writes"): the volume is
            writable (no WP bit; DBG:VDISK:RO 1 = the read-only volume as before). Every 512-byte WRITE is
            diffed against the generated sector: a LOGS / SCOPE / SHOTS delete (entry 0xE5 + chain freed ->
            the SPI file deleted; a move to the trash = deleted at the eject), a DM40*.BIN in the root
            (staged in free SPI clusters by content, any write order; checked as the loader does, linked as
            DM40FW.BIN, dialog "Firmware received / Install it now?" Yes = DBG:UPDATE, No / 60 s =
            deleted), the hosts' own trash / indexer / Finder files (kept for the session in hidden
            _VHOSTnn.TMP files, deleted at the eject / next boot), dirty flags and dates; everything else
            DATA PROTECT. Layout frozen while the host has it mounted (new recordings / screenshots appear
            at the eject / re-plug: the cost of writability; a UNIT ATTENTION would make Linux drop writes).
            decided: see docs/decisions-2026-10-22.md #6 (read-only "Files live" by default; Settings 3/3
            "USB disk" = "PC can edit" -> the writable volume, volatile, applied at the next USB plug)
            IMPLEMENTED 2026-10-22: vwrite.rs mode in force / asked for (applied at the bus reset), Settings 3/3
            row + toast "Re-plug USB to apply", Help 6/6 "USB disk", README.TXT, DBG:VDISK:RO? = in force,
            asked for; fatren-test vwrite "mode" + "rw-ro" scenarios (captures unchanged, they run writable).
            Emulator + kernel attach: RO mount, LOG:CSV while mounted -> listed at once; row tapped, re-attach
            -> Write Protect off, rm 2 files -> deleted on the SPI disk, cp dm40fw.bin -> install dialog, No.
            Linux sequences captured with a real kernel (usbip_msc.py: rm, cp of 500 KB, trash + cp,
            trash + empty) and replayed in run.sh; fsck after every scenario, power cuts in staging,
            commit, park, purge. Emulator (real firmware, kernel attach): rm 2 files, cp 500 KB (47 s in
            the emulator), dialog Yes -> reset into the loader.
      - [ ] Device: (in "PC can edit" after a re-plug, decision #6) Linux rm / Nautilus Delete (trash) / cp dm40fw.bin + Yes -> installed + No ->
            DM40FW.BIN gone; DBG:VDISK:WR? after each; DBG:BOOT? STACK after a cp; SPIFLASH.BIN fsck; DMM
            and a CSV recording during a cp (staging waits / busy retries); Windows Explorer (System
            Volume Information, delete, copy) and macOS Finder (.fseventsd, .Trashes, ._ files): untested,
            expected per docs/native-boot.md "Host OS notes".
            Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" 3b, Linux): PASS for rm (deleted at once), gio trash (deleted at the
            eject), cp dm40fw.bin (601 KB in 29.8 s) -> dialog -> No -> no dm40*.bin on the SPI disk, WR?
            refused 0, stack 23 376 B, fsck clean, Files live restored. Re-plug without the cable: `usbreset
            1209:0001` (the bus reset applies the mode) + DBG:VDISK:EJECT 3 (the kernel re-reads WP only at a
            medium change). Yes (install), the DMM / a recording during a cp, Windows / macOS not run.
- [x] A-jack protection hybrid (owner 2026-10-22): non-current modes keep the stock resting shunt state (PE0 = PE1 = 0,
      ~48 ohm A-COM: sensitive A-JACK from ~0.44 mA); when the sentinel measures a current that would overheat the
      47.5 ohm resistor (owner: package looks like 2512 / ~2506 -> assume 1 W, derate to 0.5 W continuous: > ~100 mA for
      > 0.5 s, or > ~300 mA (4.3 W) at once) switch to the 10 mohm shunt (PE0 = 1): a weak source then flows harmlessly,
      a stiff one blows the 10 A fuse instead of burning the resistor / MOSFETs (30 V parts). Latch with a red alert +
      beep until the user picks a function/range and the current is gone; measure the current on the 10 mohm path
      while latched (AD1/AD2 on PB2) to know when it is safe to return. >30 V on A-COM (mains) cannot be protected by
      switching (MOSFET breakdown first); document. Doc: power-meter-and-misconnect.md section 4 (update the old
      "do not auto-switch MOSFETs in V modes" note).
      Done (doc §4.8): the probe (OP2 x1 on PB2 = the 10 mohm element, AD2 peak, 4 194 codes / A) enters the
      DC V / mV / ohm / temperature row at the first comparator hit; >= 300 mA at once or >= 125 mA for 400 ms
      from the first hit (~0.5 s from the onset; 125 mA keeps 5 V = 104 mA a warning) -> PE0 = 1, red A-PROT
      (badge / toast / beep, SYST:ALER? APROT, QUES bit 9), latched until the user acted AND the probe read
      < 50 mA for 1 s; a current function / the power meter ends it. Gaps: > 30 V (MOSFETs), Jack alerts OFF,
      AC V / Hz / cont / diode / cap (no free pair). Emulator: PB2 node + AD2 probe; senttest aprot-* (12 V:
      409 ms, 24 V: 105 ms, ohm / temp ~0.41 s, 5 V: A-JACK only).
      (verify on device: the A-jack protection check list of doc §4.8)
      - [ ] Device f3aea2f (docs/device-check-2026-10-21.md "f3aea2f" alerts): **A-PROT found latched with the
            A jack open** at the end of the run (DBG:SENT? field 11 = 105 ms: the ">= 300 mA at once" rule
            in the first probe window; probe 15 mA afterwards; CONF:VOLT:DC over SCPI did not release it,
            DC A did). When / which function is unknown (DC V 1 / 5 / 10 V and the square on the DMM page,
            combined view and Scope page did not trip it again). Log the trip (function, probe mA, time).
      - [ ] **False A-JACK in Hz** (A jack open, reproducible): FREQ with 1 V DC or the 40 Hz square on V-COM
            -> SYST:ALER? AJACK (kind AD1PK_PB1), DMM page and combined view. Also an "AC" hint in DC V on
            the calibrator's 5 V DC (not at 1 / 10 V).
- [x] Burden voltage in the A / mA / uA modes (as the EEVblog 121GW, owner 2026-10-22): I * R_path of the
      shunt in use in the aux row, "Burden" caption: DC A aux1 (signed), AC A aux2 instead of the duty
      (RMS); none in AC+DC (both rows hold the parts) and in the power meter (its own At-load line). R_path
      = the power meter's per-shunt setting (default the bare shunt = a lower bound), POW:BURDen now sets
      the shunt of the DMM A function in use. Settings 3/3 "Burden voltage" (persisted p[38] bit 14,
      default OFF = stock look), DISPlay:BURDen ON|OFF / ?, FETCh:BURDen? (V, pre-REL, works with the
      readout off). Not sent over BT. Emulator emu/hy3131/burdentest.py, ui-render d21..d24 / c16..c18
      (docs/power-meter-and-misconnect.md 3.15). Left: the device check (3.15 list: a second meter across
      A-COM per range, then POW:BURD the measured R_path); the guided "measure R_path" step is not built
      (own ohm function reaches only the uA chain: the fuse check value could seed it later).
- [x] Inrush current (as the Fluke 376 / 323 clamps, owner 2026-10-22): arm in the 10 A range (autorange
      off), wait for the current to step up from ~0 past a threshold, then show the RMS over the first 100 ms
      after the trigger (Fluke's window) plus the peak, and keep the captured trace (a short chart / scope-like
      view, saved like the FAST*.DLG records). Data path: AD2 on the 10 mohm shunt at x1 (+-125 A window,
      dmm-input-scope-feasibility.md 1.3) at the fast logger's 9.6 kS/s (src/fastlog.rs; ~960 samples in
      100 ms), with a pre-trigger ring. Open: the trigger rule (absolute A vs a step from the idle level), AC
      and DC loads (RMS of the raw samples covers both), what the 10 A fuse tolerates (rating / type UNKNOWN:
      a motor start can be 5-10x the run current), AD2 bandwidth and offset at x1 vs the DMM reading, UI entry
      (A mode sub-function vs Record page), SCPI (arm / fetch RMS, peak, trace), emulator scenario (a stepped
      current into A), device check with a lamp / motor and a second meter or the scope + shunt.
      Done (docs/inrush-current.md): MF in A -> Current Settings -> "Inrush" (A sub-function, as the Fluke
      clamps; not the Record page) opens the Inrush Current page: AC A row of the 10 A range (manual), the
      AD2 stream (wave-view style session, 9600 S/s, the section 11 ISR), the DMM paused while armed.
      Trigger: the ~1 ms mean of |I| exceeds the largest such mean of the previous ~100 ms by the step
      (default 1 A, stepper 0.2..10 A, INR:THR 0.1..50 A): an absolute threshold from ~0, a rise by the
      step on a running load (a running 2 A rms AC load does not trigger 1 A); either polarity; t = 0 = the
      window's first sample (<= ~1 ms early). Trace: 200 ms pre + 1.8 s post as 16-bit codes (raw >> 3,
      3.8 mA) in the scope's BUF (lent: the MCX acquisition idles, the stored scope record dropped; no new
      SRAM). Results: Irms over 100 ms from t = 0 (AC + DC rms), the signed peak (OL at the AD2 rail),
      idle rms, lost samples. Saved through the fast logger as LOGS/FASTnnn.DLG (kind 3: header with the
      results, t_s from the first sample, t0 = 0.2 s; FASTnnn.CSV on the USB volume / MMEM:DATA?), the GUI
      live meanwhile. SCPI INRush[:STATe] / :ARM / :THReshold / :CONDition? / :FETCh? / :FILE? /
      :PREamble? / :DATA? (setters refused over BT). Closing restores the A sub-mode / autorange and BUF.
      Emulator emu/hy3131/inrushtest.py PASS: DC 12 A 20 ms / 6 A 80 ms / 3 A step -> Irms 7.568 A (exact
      window 7.589), peak 12.012 A, idle 0.050 A, CSV 19 200 rows, t0 0.200000 s; 2 A rms AC running 6 s no
      trigger, 5 A rms step -> Irms 5.104 A (the DMM's AC A 5.000 A: the model's raw AD2 sine is 2.2 % above
      its DC scale, an emulator artefact, doc section 8), host CONF stops it. Host tests (tools/meter-test inrush), ui-render i01..i07.
      Budgets (on 6dfffec): region B 13.9 KB free (19.4 before: +5.4 KB), SRAM unchanged (10.9 KB free),
      stack headroom 21.2 KB (21.4 before).
      (verify on device: docs/inrush-current.md section 9: lamp, motor, a second meter / clamp)
Emulator (emu/):
- [x] Front-end schematic: label the jack wires with the HY3131 pins the firmware's register settings actually
      use per function (from the rows / mux selections: e.g. which PA tap, SENSE, PB0..PB7, FB/RLU), not "the
      datasheet's typical circuit" (owner 2026-10-21). (0487584 first cut; b540bdc
      "jack-wire pins from the registers only": no role table left - the pins the enabled blocks
      select (ENCMP alone for the comparator, SAD1I for AD1, SDIO) put on a jack by a per-pin board table,
      PB4 follows OP1's input, RLD on the Ω net, every closed PA switch with its P / D / F / S letters, the
      divider ratio from the SS taps (power meter /1000 etc.), the test-source band names the node by its
      pins and derives its AD1 / comparator chain; columns reserved so nothing moves between functions.
      Checked on 24 fw-rs emulator states + all 47 table rows at 700 / 1000 / 1400 px. Visible change for the
      owner to confirm: an enabled comparator nothing reads (stock R20 0xf0, SCMPI = PB4) now shows PB4 into
      CMP in the DC rows, and AC V r0 says divider /100 (PA6 tap; the old range table said /10))
- [x] HY3131 model at terminal level: the user sets what is connected to V-Ω/COM/A (sources/loads, incl. voltage AND current
      at the same time); the firmware's mux/relay choices route it through a modelled DM40 front-end (dividers, shunts, relays,
      switches) — no per-mode magic inputs. (0b464bd DM40 front-end model + connection editor, 213fc83 cross table)
- [x] Show the firmware's current mux/relay/switch selection as feedback in the GUI (draw the front-end path).
      (0b464bd schematic, 0b2e01f HY3131 pins + internal switching, 6df6db9 Frontend state line)
- [x] Cold-junction temperature and the HY3131 die temperature are separate inputs (die temp always set; default equal).
      (0b464bd CJ/die split)
- [x] GUI: Apply on Enter in the HY3131 input fields; show only the fields relevant to the chosen source (no freq/DC offset
      for plain DC etc.). (0b464bd GTK connection editor: Enter applies, per-source fields)
- [x] Virtual backup files (owner 2026-10-21): second USB MSC LUN = read-only FAT16 volume DM40-INFO
      generated from the MCU memory (src/vdisk.rs: CALIB / BOOTDATA / UID / OPTBYTES / OTP / ROMBOOT / LOADER .BIN,
      BACKUP.TXT, FIRMWARE.BIN = running firmware as update file); per-LUN SCSI in msc.rs (INQUIRY names, WP bit,
      WRITE -> 07/27/00, PREVENT / UNIT ATTENTION per LUN). SPI disk: only a write-once safety copy of
      CALIB/BOOTDATA/UID (DBG:BOOT? CALBK SAFECOPY_PRESENT/WRITTEN + differs). Host-tested (fatren-test) and
      emulator-tested over USB/IP (userspace client). docs/native-boot.md "Unit data".
      Superseded by the USB storage redesign (one generated volume with INFO/ etc., above).

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
      0.00 ohm as the stock (was -666 / -1114 ohm; emulator cross table). (emulator-verified: V 1.234|cont = 0; verified on device 9dd1222: 0.00 ohm + LIVE?)
- [x] AC V on a pure DC source reads ~18 mV in the emulator (stock ~0): NOT a firmware bug. The model's
      coupling capacitor charges to the raw jack DC (UpdateAcNode: src = dc * NodeRatioAc) while the
      seen step uses the calibration-folded node DC (NodeDcAc: DcQ(...)), so step = dcNode - vc never
      reaches 0: residual = 1.5 x V x (cal gain/offset error) = 17.7 mV at 1.234 V. With `cal:0` fw-rs
      settles at 0.64 mV (the model's noise floor). The stock escapes it because its mV-first AUTO steps
      to AC mV (DC coupled in the model, no coupling term); fw-rs stays on the 6 V range (AUTO+, UF-21).
      Fix belongs in emu/hy3131/HY3131.cs (charge vc towards the same DcQ-folded level).
      Fixed in cd80de8 (vc charges to the calibration-adjusted level; test settles to the floor).
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
      (verified on device 9dd1222: visible, segmented, bipolar; d.bar 1 us)
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
      (verified on device 9dd1222: AC+DC -> Hz 2 x 43 reads all 40.002 Hz, duty 50 %, MIN/MAX clean)
- [x] DMM batch polish: bargraph full scale in continuity / diode = their OL ceilings 1000 ohm / 3 V
      (bargraph::mode_full_scale; ui-render d18 / d19, bargraph-test); Help 3/5 rows "Tap / hold history:
      Store / clear" + "History row shown: Settings: DMM row"; ui::list_page asserts one value per row on
      the host (ui-render / touch-test; touch-test passed 11 values to the 9-row pages, fixed).
