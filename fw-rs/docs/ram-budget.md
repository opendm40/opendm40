# RAM budget: who is live when (2026-10-22)

The owner's question: do all big RAM users have to be live at the same
time? This note lists every static of 1 KB or more on the native build,
when it is live, what else is live then, and whether it can be overlaid
or shrunk. The memory map and the link checks are in
docs/native-boot.md "Memory budget". RAM is 64 KiB TCM (.data / .bss and
the stack) plus 192 KiB SRAM (.dmabuf, .sram_bss).

## Result of this round

| | before (367b933) | after |
|---|---|---|
| SRAM free (above .sram_bss, link check >= 8 KiB) | 10 848 B | 60 648 B |
| SRAM statics (`screenshot::CAP` 49 152, `screenshot::JOB` 1 088 -> 436) | | -49 804 B |
| TCM stack / static worst path / headroom | 45 640 / 24 236 / 21 404 B | 45 632 / 24 236 / 21 396 B |
| region B free (zero-wait flash) | 23 536 B | 24 248 B |
| region C free (.cold, below 0x080A0000) | 7 984 B | 8 456 B |

(The capture / RLE code left region C; `scratch()` is now inline, with its
generation bump, in its ~60 callers.)

What changed:

1. **`screenshot::CAP` (48 KiB) is gone.** The screenshot streams the
   panel straight into the block it programs (below).
2. **The scope FFT work buffer (8 KiB) moved into the disk scratch arena**
   (`calbackup::SCRATCH`, 12 KiB), with an explicit ownership check. It used
   to be the tail of CAP.
3. `screenshot::JOB` lost its 642-byte row offset table (1 088 -> 436 B).

## The screenshot: streamed from the panel

How it worked: there was never a frame buffer. The image has always been
read back from the ILI9488's GRAM (RAMRD 0x2E, `lcd::read_span`: a dummy
word, then RGB666 packed two pixels per three words, converted to RGB565).
CAP was only a run-length coded copy of the rows so that the screen could
unfreeze after ~0.1 s. Screens that did not fit in CAP were already read
from the GRAM while the file was written, with the screen frozen for the
whole write. So the streaming path is not new. It is the old fallback,
and it is now the only path:

- Data phase, per 4 KiB block: fill it with the header bytes and the
  pixels read from the panel (`read_span`, ~2 000 pixels, ~1.3 ms), into
  `scratch()[0]`, then `write_step` it (compare, erase, 16 page programs,
  read back), in slices of <= ~2 ms of CPU per main-loop pass as before.
- Consistent frame: `frozen()` is true from the gesture until the last data
  block has been filled. main.rs passes `can_draw = false` and App::gesture
  drops input in that time, so nothing draws over rows that are not read
  yet. The main loop keeps running: DMM, USB, BT and the watchdog. The
  verify pass compares CRCs and does not need the panel. Three draws
  bypassed `can_draw` (App::toast, the auto power-off warning, the USB
  firmware-file dialog); with the freeze now always ~5 s they are held off
  while `frozen()` (a toast is dropped, the dialog comes after the shot).
  The power-off / reset paths (`finish_disk_writes`) draw nothing before
  they finish the shot.
- Cost: the screen stays frozen for the write, ~4-5 s on the device
  (emulator with W25Q128 timing: 5.0 s). That is 76 blocks, each a 4 KiB
  sector erase (~45 ms typ.) plus 16 page programs. Now ~1.5 s: the next
  shot's clusters are erased in idle time (next section). Before, it was ~0.1 s for the 91 % of screens
  whose RLE fitted, and the same 4-5 s for the AC+DC and power views and
  noisy scope traces, which did not fit.
- Disk held: a fast-log session or an upload holds the disk for an
  unbounded time. A shot can no longer wait for them with the screen
  frozen, so it is refused at once with the toast "Disk busy" (new
  `Shot::Busy`). It used to be captured and written after the session. A
  csvlog commit (a few blocks) is still waited for, frozen.
- Bytes: identical to the old path. Both read the same GRAM through the
  same `read_span`, and the RLE was lossless. Checked on the host
  (tools/fatren-test: the BMP equals the stub screen pixel for pixel,
  bottom-up, with the stock header) and in the emulator (emu/shottest.py:
  the BMP equals the panel at the gesture; old build vs new build
  byte-identical, see "Tests").

Why not a smaller CAP: the RLE size of the 148 host renders of
tools/ui-render was measured, both as coded now and with an extra
"row = previous row" token (the anti-aliased fonts and traces make rows
differ):

| CAP size | renders that fit (plain RLE) | with a row-repeat token |
|---|---|---|
| 8 KiB | 0 / 148 | 2 |
| 12 KiB | 4 | 5 |
| 16 KiB | 9 | 9 |
| 24 KiB | 24 | 30 |
| 32 KiB | 75 | 94 |
| 48 KiB | 134 | 135 |

The median is ~30 KiB and the largest is 68 KiB (d02-dmm-acdc). A small
buffer buys nearly nothing. It is all (48 KiB) or nothing.

Re-render in strips was not attempted. The drawing code paints straight to
the panel with diff state (scope_ui `d_*`, the page caches), not a region
renderer, so a strip could not be reproduced deterministically. Strips
captured one at a time without the freeze would be consistent within a
strip but not across the frame.

## The screenshot freeze: next shot's clusters erased in idle time (shot-preerase)

The owner's option 3. Design (screenshot.rs "idle-time preparation"):

- **Nothing reserved.** A shot allocates first fit from FAT 1, so the
  preparation keeps the *first 76 free clusters* blank (0xFF). They stay
  ordinary free clusters: no FAT entry, no directory entry, no hidden file.
  So a power cut mid-prepare cannot leave lost clusters or a FAT
  inconsistency (at worst a half-erased free sector; nothing for fatren or
  fsck to do), the USB volume (vdisk.rs) shows nothing new (free clusters
  are not files; only the raw DEBUG/SPIFLASH.BIN shows them, as 0xFF), and
  any other writer (csvlog commit, fast-log Prep, a firmware file copied in
  "PC can edit" mode) may take them and is just faster for it. Flash wear
  is unchanged: each cluster is still erased once per reuse, only earlier.
- **Steps** (main loop, `prep_poll`, inline in region B; the step itself,
  `prep_step` 644 B, is region C): boot sector; FAT 1 entries 64 per read
  up to the next free cluster; that cluster's block checked blank 1 KiB per
  step (~0.7 ms); a dirty block erased (SE4K start, the cluster's FAT entry
  re-read in the same step first), checked again once the chip is idle (a
  block that stays dirty is skipped). Never waits for BUSY (one RDSR, then
  the next pass). Ready after 76 blank clusters, or all free ones on a
  nearly full disk.
- **Restart.** `msc::FLASH_WRITES` counts every program / erase command
  (spiflash.rs `write_enable`). When it moved since the preparation's last
  step, another writer touched the disk: it starts over once the disk has
  been quiet for 2 s (a full re-check is ~76 x 4 KiB of reads, ~0.2 s of
  SPI in ~1 ms steps; after a 60 s CSV commit, after a shot). `Off` (no
  disk, a read error, an erase timeout) retries after 60 s.
- **When it runs** (main.rs): vdisk builds only (a host that writes the
  raw SPI disk could be filling a free cluster); not during a shot (checked
  inside), a csvlog commit, a fast-log session, an upload or a PC staging
  (those keep data in free clusters until they link it), a logger session
  (constant interval; the AD2 stream ISR, as region C code must not run
  then), nor within 0.5 s of host USB disk I/O (a READ data phase or a
  WRITE). SCPI MMEM:DATA? / the on-device gallery read through an erase in
  flight wait for it (<= 45 ms typ., once).
- **The shot.** At the end of Alloc, if the preparation is Ready and
  nothing has written since (`prep_blank`), the shot's first clusters are
  exactly the proven-blank ones: their blocks go straight to the page
  programs (write_step phase 1, no compare, no erase). Every other block is
  compared / erased as before. Any data block whose read back fails is
  written once more with a forced erase (an erase cut short by a power cut
  that still reads blank, or a stale assumption), once per block.
- Power-off / reset (`finish_disk_writes`): an erase in flight is waited
  for (`prep_finish`, <= 0.4 s).

Freeze, emulator (emu/shottest.py, W25Q128 timing tPP 0.7 ms, tSE 45 ms;
the firmware's own figures, DBG:FLASH? fields 4-7):

| build | screen | frozen | data blocks erased | slices while frozen |
|---|---|---|---|---|
| 87f2c20 + instrumentation (before) | DMM HOLD | 5 001 ms | 76 / 76 | 2 406 |
| 87f2c20 + instrumentation (before) | menu | 5 001 ms | 76 / 76 | 2 406 |
| 87f2c20 + instrumentation (before) | combined view | 4 374 ms | 61 / 76 | 2 098 |
| 8f1d887 = 90bfeab before the rebase (after, prepared) | DMM HOLD | 1 403 ms | 0 | 681 |
| 8f1d887 | menu | 1 403 ms | 0 | 681 |
| 8f1d887 | combined view | 1 403 ms | 0 | 681 |
| 8f1d887 | the same view again, ~10 s after the last shot | 1 403 ms | 0 | 681 |
| rebased onto 3028923 (the GD32_SPI stall model, 166237c) | DMM / menu / combined / again | 1 408-1 409 ms | 0 | 681 |

(The "again" shot found the preparation done: the test's verify wait and
panel read-back took longer than the 2 s quiet + ~76 erases. A shot taken
before the preparation is done costs the old ~5 s minus 45 ms per block
already blank.) The before build's combined-view shot found 15 clusters
blank from the earlier shots' erases.

Per prepared block ~18.5 ms: the panel read 1.3 ms, 16 page programs
~14 ms (0.7 ms each + 0.2 ms to clock the page out), the read back 2.7 ms.
Without the compare-skip (an earlier build of this round) it was 1 638 ms.
With the datasheet's typical tPP 0.4 ms it would be ~1.1 s. The rest of
the shot (FAT, directory entry, verify) runs after the screen unfreezes.

Every shot's BMP equals the panel read before the gesture (0 px differ).
(`SYST:ERR? -100,"Mass storage busy"` at the end of shottest is its own
MMEM:CAT? polls answered while a shot holds the disk, as before.)

Host tests (tools/fatren-test 12c): preparation on a disk whose free
clusters need erasing (76 erases; FAT / root / directories / every
allocated cluster unchanged), again after a reboot (0 erases), the shot
after it (no data block erased, 76 programmed without the compare; the
next 76 prepared afterwards), power cuts before erase 0 / 1 / 2 / 38 / 75
(fsck clean, the next boot completes), a blank-reading sector that does
not program (the retry), a prepared cluster overwritten behind the write
counter (the retry), another writer linking the first free cluster after
every one of the 1 161 steps of a preparation (its data intact, with and
without the counter: the FAT re-read before each erase), 20 free clusters
(all prepared, the shot still refused "Disk full").

Memory (native build, rust-fw-poc 3028923 -> this change): TCM statics
+~100 B (`screenshot::PREP` 56 B, `STATS` 16 B, `PREP_ERASES`,
`msc::FLASH_WRITES`): stack 45 520 -> 45 424 B, static worst path
24 284 -> 24 292 B, headroom 21 236 -> 21 132 B (>= 20 KiB). SRAM free
60 544 -> 60 528 B (`JOB`, .dmabuf, +16 B). Region B free 71 980 ->
71 332 B (the inlined `prep_poll`, the DBG:FLASH? fields). Region C free
23 820 -> 22 888 B (`prep_step` 644 B, `prep_stats`, the retry).

Gallery (screenshot viewer): it needs no big buffer. `Gallery` is 0.7 KiB
of TCM, and it decodes a BMP row by row from `scratch()[0]` (data) and
`[1]` (FAT) inside one `poll` while the disk is free.

## The disk scratch arena (`calbackup::SCRATCH`, 12 KiB)

The owner asked to overlay it, on the premise that it is calbackup's and
only used at boot. **The premise does not hold.** SCRATCH is the shared
block buffer of every on-device disk user, not calbackup's own:

| user | uses it | across main-loop passes? |
|---|---|---|
| calbackup boot safety copy (pre_main) | buf, tmp, FAT | no (before main) |
| screenshot.rs (shot, scope record) | [0] block being written, [1] FAT cache | yes, Bpb .. Verify |
| csvlog.rs commit | [0], [1] | yes, while `writing()` |
| csvlog catalog / delete / free space (MMEM:CAT?, gallery, vwrite) | [0..2] | no |
| fastlog.rs Prep / Stop | [0..2] | yes (`holds_disk()`) |
| upload.rs (SCPI upload, USB-volume firmware staging) | [0] collects the block | yes (`busy()`) |
| vwrite.rs (host deletes, purge, commit) | [0..2] | no |
| gallery.rs | [0] data, [1] FAT | no (inside one poll) |
| fatdir.rs (SHOTS / LOGS / SCOPE created) | [0..2] | no |

A csvlog commit runs every 60 s while a recording is on, so the arena is
in use regularly, and while the scope's DMA writes `scope::BUF`. Overlaying
it on `scope::BUF` or on CAP (the screenshot used both at once) was not
safe.

What was done instead: the arena takes a second tenant that really is idle
most of the time, the scope FFT's 8 KiB work buffer. It is needed only
while a spectrum job is in flight (a few passes), and the job can be
dropped and redone at any time. Ownership has runtime checks
(calbackup.rs):

- `scratch()` is the disk side's only accessor. Every call bumps a
  generation counter (`GEN`). A disk user never waits for the FFT.
- `fft_work(fresh)` is the FFT's only accessor. It returns None while a
  disk job that keeps data in the arena across passes is in flight
  (`disk_job()`: `screenshot::busy() || csvlog::writing() ||
  fastlog::holds_disk() || upload::busy()`, the same flags the disk lock
  uses). For a job already loaded (`fresh` false), it also returns None
  when `GEN` moved since the load: a single-pass disk user (a gallery read,
  MMEM:CAT?) wrote the arena in between. On None, scope_page.rs drops the
  job, keeps the last spectrum and asks for the frame again once
  `fft_free()`. This is the same retry the CAP lease had.
- Neither side keeps a reference across passes. The arena is main-loop
  only (no ISR touches it).

Visible cost: the spectrum is not updated during a disk job (a commit,
~0.1-0.5 s every 60 s while recording; a shot; a whole fast-log session).

## Every big static (after this round)

Sizes from `llvm-nm -S` of the native ELF (ad2-isr default build).

| static | size | live when | live at the same time | overlay / shrink? |
|---|---|---|---|---|
| scope::BUF (SRAM) | 52 KiB | always: the scope acquires in the background in every view (measurements, SCPI); the stored record while stopped | everything | Lent to inrush (`lend_buf`) while the inrush page is open. Nothing else can take it while the scope runs. Keep. |
| logger::RING (SRAM) | 16.2 KiB | LOG:STAT ON sessions (AD1 / AD2 stream), the Wave view, inrush, fast-log sessions | csvlog is refused by the Wave view, inrush and fastlog::begin, but not by an SCPI LOG:STAT ON session | Idle in plain DMM / scope use. A 12 KiB csvlog overlay (FIFO + TAIL) would need csvlog::start and LOG:STAT ON to refuse each other too, plus an owner token. Not done: a wrong assumption silently corrupts a recording. |
| calbackup::SCRATCH (SRAM) | 12 KiB | any disk job (table above) + the FFT | see above | Now also hosts the FFT work (8 KiB saved). |
| csvlog::FIFO_BUF + TAIL (SRAM) | 8 + 4 KiB | a CSV recording (FIFO also lent to fastlog: `lend_fifo`, never together) | DMM, scope, BT, USB | See logger::RING. Already shared with fastlog. |
| scope_ui::GRAPH (SRAM) | 9.7 KiB | the scope pages (Scope, combined DMM+scope, Wave view): trace / persistence / math rows, and their on-screen diff copies | the scope record, the FFT | Idle on pure DMM pages, but the combined view and the "everything at once" goal rule out an overlay with anything scope-side. Could shrink: the math rows (2 x 1.6 KiB) are only used with math on. Not done. |
| bt::TX + RX (SRAM) | 8 + 1 KiB | TX: while BT is connected (the reply, up to RSP_MAX 6 KiB + LF, plus stock frames, queued for DMA); RX: the DMA ring, always while BT is powered | everything | TX could shrink to ~6.2 KiB if a reply and stock frames are never queued together. Not checked. Keep. |
| vdisk::TAB + STATE + CSV_CUR x2 (SRAM / TCM) | 6.3 + 0.3 + 1.9 KiB | always while USB exports the generated volume (the table regenerates every sector read) | everything | Keep. |
| APP (TCM) | 6.2 KiB | always (the App) | | Keep. |
| scope_page::CACHE (SRAM) | 4.7 KiB | the scope page (persistence a/b, overlays) | GRAPH | Same as GRAPH. |
| scope_page::FFT (TCM) | 4.0 KiB | the last spectrum (`db`) | | The result must survive a dropped job. Keep. |
| gen::WAVE (SRAM) | 3.9 KiB | while the generator outputs (DAC DMA, circular) | everything ("DMM + scope + gen") | Keep. |
| touch::LOG (TCM) | 1.9 KiB | always (DBG:TOUCH?) | | Debug aid; could go to 0.5 KiB. |
| scope::ROLL_COLS (SRAM) | 1.6 KiB | roll mode | | Keep. |
| vwrite::W0 (SRAM) | 1.3 KiB | the writable USB volume session | | Keep. |

Never live together, so worth an owner token if RAM gets tight again:
logger::RING with the csvlog buffers (12 KiB, see above). Everything else
over 4 KiB is either always live or live with the scope.

## Tests (2026-10-22, on 34a5a6e rebased onto 367b933)

- Host: every fw-rs/tools/*/run.sh passes (fatren-test includes the
  screenshot writer on the stub screen: BMP = the stub pattern byte for
  byte, the stock header, power cut / abort after every slice, shots
  interleaved with csvlog commits; and the calbackup boot cases 6-9).
  fft-test, gallery-test, ui-render (148 renders, 0 violations) pass.
- emu/shottest.py (new): DMM with HOLD, the Settings menu, the combined
  view with the scope STOPped. Each BMP equals the panel read over DBG:LCD?
  before the gesture, 0 pixels different, for the new build and for the
  old one (CAP path, 367b933). The menu BMP is byte-identical between the
  two builds. The DMM and scope screens differ between the two runs
  themselves (reading, trace phase), so for those only "BMP = own panel"
  can be compared.
- FFT in the arena (emulator, combined view, math FFT on): the spectrum
  runs (DBG:PROF? s.fft), pauses during a shot and goes on after it and
  after MMEM:CAT? bursts (each one a single-pass disk user that moves
  `GEN`); the peak stays 992-993 Hz, -9.9 dBV.
- calbackup boot (emulator, private disk copy): first boot DBG:BOOT?
  `CALBK,SAFECOPY_WRITTEN,7,0,0`; after DBG:RESET `SAFECOPY_PRESENT,0,0,0`.
- emu/hy3131/tests/run.sh (385 passed, 0 failed), inrushtest PASS,
  fastlogtest --profiles typ PASS (no sample lost up to AD2 38400),
  wavetest PASS, ad2streamtest PASS.
- emu/shottest.py's first version read the combined view with the scope
  running: the trace moved while the reference was read (80 DBG:LCD?
  queries), 204 px differed, in the trace area only. The scope is STOPped
  now; with that, both builds pass.

Device checks still open (the emulator's ILI9488 models RAMRD from its
datasheet; the read-back itself has been in use on the device since
5fd7871 for DBG:LCD? and for the rows CAP could not hold):
1. Shot of a static screen (a menu) and a busy one (AC+DC, a running
   scope trace): the BMP opens, equals `dm40dbg.py shot` of the same
   frozen screen; the screen stays frozen until the write ends (~4-5 s),
   nothing draws or reacts to touch meanwhile, then the toast.
2. Read-back colours: a colour test screen (the scope's colour picker) in
   the BMP vs the panel: RGB666 -> 565, the dummy word.
3. During a shot: SCPI READ? keeps answering, DBG:DIAG? loop max stays
   about as before, the DMM keeps measuring (the screen just does not
   update).
4. A shot during a fast-log session: "Disk busy" at once, nothing written.
5. FFT on the scope page while a CSV recording commits (60 s) and during a
   shot: the spectrum pauses and comes back, no garbage spectrum.
6. calbackup boot: DBG:BOOT? CALBK `SAFECOPY_PRESENT` (the copy is already
   on this unit's disk), CALIB.BIN / BOOTDATA.BIN / UID.BIN unchanged.
