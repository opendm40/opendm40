# Compact log format (DLG): binary on the disk, CSV on read

opendm40 stores its logs on the SPI flash in its own compressed format, DLG,
and produces CSV only when a file is read (USB volume, SCPI, host tool). This
page specifies the format, the writers and the readers.

Users of the format: the DMM recording ([`src/csvlog.rs`](../src/csvlog.rs),
DMM page swipe down / `LOG:CSV ON`), the scope record
([`src/scope_csv.rs`](../src/scope_csv.rs), "Save CSV") and the fast logger
to the disk ([`src/fastlog.rs`](../src/fastlog.rs), section "Fast stream
(kind 3)"). Codec and CSV generator: [`src/dlog.rs`](../src/dlog.rs).

## 1. Why DLG

A CSV row is ~21 bytes (`1234.567,1.23456,V,\r\n`; ~25 for the fast
logger). DLG codes the typical row ("time +-3 ms, value +-few digits") in
~9-12 bits with delta / zigzag / adaptive Rice: 11-26x smaller than CSV on
synthetic sets, against 3-3.5x for an MCU-sized deflate (a 32 KiB gzip window
does not fit the RAM). The standard formats (gzip CSV, sigrok session, FLAC,
ASAM MDF4, HDF5 / Parquet / TDMS) were compared in
[log-format-standards.md](../../docs/history/log-format-standards.md): none
is appendable in small power-cut-safe commits from the MCU and compresses
as well, so DLG stays; the CSV export makes the format invisible to users.

RLE is implicit: a repeated value with an unchanged interval costs 2 bits
(k = 0 codes for 0 and 0).

Hours of logging in ~13 MB of free SPI disk (16 MiB FAT16 minus the firmware
backup `_DM40PRV.BIN`, the calibration safety copy and a few screenshots):

| rate | CSV (21 B DMM / 25 B fast logger) | DLG | gain |
|---|---|---|---|
| 1/s DMM | 172 h (7 days) | 2800 h (1.29 B/row; one recording ends at 49.7 days, the u32 ms time) | 16x |
| 10/s DMM | 17 h | 325 h (1.11 B/row) | 19x |
| 40/s fast logger | 3.6 h | 41 h (2.2 B/row) | 11x |
| 634/s fast logger | 14 min | 2.6 h (2.15 B/row) | 11x |
| scope record | 690 KB / record, 18 records | ~9-14 KB / record, ~1000 records | 50-75x |

## 3. The DLG format (version 1)

All integers little-endian. A file is a file header and a sequence of
**chunks**. One chunk per csvlog commit (every 60 s, a full 4 KiB of data, or
FIFO 3/4 full): the chunk is the unit of power-cut safety. Every chunk is
decodable on its own (the coder state restarts), carries its CRC-32, its
first row number and the CSV byte offset of its first row, so:

* a power cut / a full disk / a truncated copy loses only the chunk that was
  being written; every complete chunk before it decodes (tested: truncation
  at every byte);
* random access for the CSV export: chunk headers form a skip list (offset in
  the CSV, number of rows), no full decode needed to find an offset;
* the CSV length is the sum of the headers (exact, for FAT directory entries).

### File header (kind 1 = DMM readings)

| off | size | field |
|---|---|---|
| 0 | 4 | magic `DLG1` |
| 4 | 1 | kind: 1 DMM readings, 2 scope record, 3 fast logger stream |
| 5 | 1 | unit length `ul` (<= 12) |
| 6 | 2 | header text length `hl` |
| 8 | 4 | CRC-32 (zlib) of bytes 12 .. 12 + 12 + hl |
| 12 | 12 | unit (ASCII, `ul` bytes used, rest 0) as in each CSV row |
| 24 | hl | the CSV header text exactly as the old writer printed it (the two `#` lines and `time_s,value,unit,flags`, CRLF); an interval recording (Record page, [decisions-2026-10-22.md](../../docs/history/decisions-2026-10-22.md) #1) has a third `#` line before the column names: `# interval N s: one row per interval with readings, value = mean of its readings, time_s = end of the interval; OL = an overload in it` |

### Chunk (kind 1)

| off | size | field |
|---|---|---|
| 0 | 2 | magic `DC` |
| 2 | 2 | payload length `pl` in bytes |
| 4 | 4 | CRC-32 of bytes 8 .. 28 + pl |
| 8 | 4 | row number of the first row (0-based, over the file) |
| 12 | 4 | CSV byte offset of the first row (from the start of the CSV, header included) |
| 16 | 4 | CSV bytes of this chunk's rows |
| 20 | 4 | time of the first row, ms since the start (as `time_s`) |
| 24 | 2 | rows in the chunk |
| 26 | 1 | flags at the chunk start (bit 0 OL, 1 REL, 2 HOLD, 3 HI, 4 LO, 5 GAP) |
| 27 | 1 | reserved 0 |
| 28 | pl | bit stream (MSB first, the last byte zero-padded) |

### Rows (bit stream)

Coder state at the chunk start: `t = t0`, `dt = 0`, flags from the header,
no scale (`k` unset), `pv = 0`, two adaptive Rice parameters (LOCO-I: A, N;
A0 = 4 time / 16 value, N0 = 1, halved at N = 32; `k` = least k with
N << k >= A).

Per row:
1. **Time**: `R_t(zz(dod))`, dod = (t_row - t_prev) - dt_prev, zigzag.
   The first row of a chunk codes dod = 0 (its time is t0).
2. **Flags / value**:
   * If the current flags have OL: one bit, 0 = same flags (empty value),
     1 = six bits of new flags follow, then step 2 again with them.
   * Else a value symbol `R_v(u)`: u = zz(dv) + 1 with dv = n - pv; u = 0 is
     an escape followed by 2 bits:
     * 0: empty value (non-finite reading without OL);
     * 1: new flags (6 bits), then step 2 again;
     * 2: new scale: k (8 bits, signed) and n (sign bit + 24 bits), this row's
       value; pv = n;
     * 3: "-0" (a negative value below 1e-30, which the old writer prints
       as `-0`).
   Rice code R(x) with parameter k: q = x >> k in unary (q ones, a zero), then
   the k low bits; q >= 24: 24 ones, then x in 32 bits.

**Values**: the old writer prints 6 significant digits, i.e. a decimal
(sign, m = 100000..999999, e) pair computed from the f32 (`W::num`). The
encoder stores exactly that pair, as the integer n = +-m * 10^(e-5-k) at the
chunk's current scale k. It changes k (escape 2) when the value is not an
integer at k (a finer decade: k = e - 5) or when e - 5 >= k + 2 (100x coarser,
so a noisy decade boundary does not toggle the scale); hence |n| < 10^7.
The decoder turns n back into (m, e) and prints it with the same routine:
**the CSV is bit-exact by construction**, not by float round-tripping (host
test: the decoded CSV vs the old writer for the same `push` calls).

Bytes of a row are known at encode time (the old row formatter runs into a
96-byte scratch, as today), so each chunk header carries its CSV length.

### Scope record (kind 2)

File: the file header (kind 2, unit "V", the CSV header text incl.
`time_s,volts`), then 24 bytes of parameters (f32 a, f32 b: volts = a + b *
code; f32 dt, s/sample; u32 t0, the index of time 0; u32 n; u32 CRC-32 of
these 20), the block table (per 4096-sample block: u32 coded bytes, u32
CRC-32 of them; then u32 CRC-32 of the table), then the coded blocks.

A block ([log-format-standards.md](../../docs/history/log-format-standards.md) section 4): 3 bits mode, 0..3 = the
FLAC fixed polynomial predictor of that order (chosen per block: the least
sum of |residual|), 4 = raw 10-bit codes (used when it is smaller, so a
block is never above 1.25 B/sample); then per sample the adaptive Rice code
(the DMM coder's) of the zigzag residual; zero-padded to a byte. Predictor
and Rice state restart per block (sample j < order uses order j, order 0
predicts 512), so the CSV export seeks to a block through the table and
decodes forward from its start.

File length up front (the screenshot writer allocates the chain before
writing): `scope_csv::set` codes every block once to get its length and
CRC (~1-2 ms per block on the device, a one-off at "Save CSV"); the writer
then calls `fill` in 512-byte pieces, one per main-loop pass, and a resumable
encoder (dlog::SEnc) continues where the last piece stopped, so no sample is
coded twice and a pass costs ~2500 samples at most. No truncation, no
over-allocation.

Size ([`tools/dlog-test`](../tools/dlog-test), synthetic 26624-sample records), bytes per sample:

| record | u16 (before) | DLG | vs u16 |
|---|---|---|---|
| DC + 1 LSB noise | 2.000 | 0.370 | 5.4x |
| sine 13 periods, 0.7 LSB noise | 2.000 | 0.345 | 5.8x |
| square 10 periods | 2.000 | 0.355 | 5.6x |
| mains + harmonics, 2 LSB noise | 2.000 | 0.539 | 3.7x |
| sine 400 periods (fast) | 2.000 | 0.531 | 3.8x |
| noise burst on DC | 2.000 | 0.377 | 5.3x |
| white noise, full scale | 2.000 | 1.250 (raw blocks) | 1.6x |

A full record is ~9-14 KB instead of 53 KB (the CSV: 692 KB); the
fatren-test record (a clean sine) is 5908 bytes. Emulator (native image,
1 kHz 3 V sine, Single trigger, MF > Settings > Save CSV): SCOP000.DLG 4231
bytes for 26624 samples (0.16 B/sample; the model has no ADC noise; u16
would be 53624), decoded by dlog.py to the 692552-byte CSV. The generated CSV is byte
for byte the old SCOPnnn.CSV (fatren-test md5, dlog-test vs scope_row).

### Fast stream (kind 3: the fast logger)

The FR-38 / AD2 stream to the disk (`fastlog.rs` session,
[`src/fastenc.rs`](../src/fastenc.rs) coder + pager): `LOGS/FASTnnn.DLG`,
served as `LOGS/FASTnnn.CSV` on the USB volume and by
`MMEM:DATA? "FASTnnn.CSV"`, decoded by [`tools/dlog.py`](../tools/dlog.py).

File: the file header (kind 3; unit `V` / `A` for AD2, `cts` for AD1; the
CSV header text), then 32 bytes of parameters:

| off | size | field |
|---|---|---|
| 0 | 4 | f32 a |
| 4 | 4 | f32 b: value = a + b * raw (AD2: the DMM's AD2 calibration, dmm::ad2_scale, as the wave view) |
| 8 | 4 | nominal rate, mHz (Logger::nominal_mhz) |
| 12 | 1 | flags: bit 0 = the value column exists (AD2) |
| 13 | 1 | source: 1 AD1, 2 AD2 |
| 14 | 1 | DMM mode |
| 15 | 1 | range |
| 16 | 12 | 0 |
| 28 | 4 | CRC-32 of bytes 0..28 |

then chunks of the kind-1 framing with other header fields:

| off | size | field |
|---|---|---|
| 0 | 2 | magic `DC` |
| 2 | 2 | payload length `pl` |
| 4 | 4 | CRC-32 of the payload FOLLOWED by bytes 8..28 (the writer patches the header in after its payload went out) |
| 8 | 4 | row number of the first row |
| 12 | 4 | t0 low: device us of the first row |
| 16 | 2 | t0 high (48-bit time: the logger's u32 stamps, which wrap every 71.6 min, unwrapped by the writer) |
| 18 | 2 | rows |
| 20 | 4 | dt0: us from the previous row (0 for the file's first) |
| 24 | 4 | v0: raw value of the first row (i32) |
| 28 | pl | rows 1..: Rice(zz(dt - dt_prev)) then Rice(zz(raw - raw_prev)), the kind-1 adaptive coder (time A0 4, value A0 16; dt_prev starts at the nominal period) |

A chunk closes at 4 KiB of payload, after 1 s with >= 64 rows, after 10 s,
or at 65535 rows; the power-cut loss is at most that.

CSV, fixed width (size = hl + rows * width, a seek is a division; the
columns of dm40scpi.py `log`, without its seq):

    t_s,raw,value_V,lost_before          (AD2, 46 bytes per row)
          0.000104,   12345,+5.88634e-02,      0
    t_s,raw,lost_before                  (AD1, 33 bytes per row)

t_s = the device clock / 1e6 (6 decimals); lost_before = round(dt /
nominal period) - 1: the conversions missing before the row (a main-loop
stall of the AD1 sampler, the ring full because the flash fell behind, a
stall of the run): every loss is visible, none silent.

Size (`tools/dlog-test` fast sets):

| set | B/sample | file B/s |
|---|---|---|
| AD2 38400, 50 Hz sine + 6 LSB noise | 1.92 | 73 600 |
| AD2 9600, DC + 4 LSB noise, ring holes | 1.00 | 9 000 |
| AD2 4800, full-scale white noise (worst) | 2.93 | 14 100 |
| AD1 640, ramp + 25-count noise | 2.03 | 1 300 |
| AD1 40, steady | 2.46 | 98 |
| AD2 1200, stamps across the 2^32 us wrap | 1.95 | 2 300 |

(The CSV is 46 / 33 bytes per row: 15-25x.) The plan for the allocation
uses 3 B/sample (fastlog::BPS_*); a file that fills up first ends Full.

#### Writing it: pre-allocated, pre-erased, programs only

    Prep   boot sector, LOGS/, a FAST number + free slot (one block per
           pass); free clusters first fit (one FAT block per pass, <= 24
           extents) for duration x rate x 3 B + 8 KiB, bounded by the free
           space (LOG:DURation 0 = all of it); per pass one blank check (a
           4 KiB read) or one erase start (an aligned 64 KiB block that is all
           ours: one BE64, else SE4K per dirty sector; blank ones are not
           erased); the chain in both FATs; the LOGS/ entry with size = the
           allocation;
    Run    per pass <= 300 us of its own CPU (wall time minus the stream
           ISR's busy time; 600 us while the ring is over half full; 2 ms
           of wall time at most): drain the ring (Logger::drain_recs, as
           LOG:DATA?) into the coder while the 8 KiB FIFO (csvlog's, lent)
           and the file have room; one page program when RDSR says idle. No
           erase, no FAT or directory write, no wait, no `.cold` code
           (region B). Above 38.4 kS/s the writer trial first (below);
    Stop   the ring's rest, the last chunk and pages; the entry's size = the
           bytes written; the unused clusters freed in both FATs.

Power cut ([`tools/fatren-test`](../tools/fatren-test) "fastlog": a cut before every block write,
page program and erase of a session): before the chain nothing is visible;
during the chain / entry at worst lost clusters; during the run the file has
the allocation's size and every chunk programmed so far decodes (the rest
reads 0xFF: a torn tail); between the entry and the header's first page
program (one main-loop pass after the logger starts) the file is all 0xFF,
not a DLG (left out of the USB volume, deletable); during Stop: a chain
longer than the file (fsck truncates) at worst.

#### Rate limits (the flash)

The chip: W25Q128JV page program tPP 0.4 ms typ / 3 ms max (256 bytes);
the 12.5 MHz SPI clocks a page out in ~0.17 ms of CPU (polled, back to back:
`tx_burst`); a chunk header costs one
more short program per 4 KiB chunk.

| | per page | pages/s | bytes/s |
|---|---|---|---|
| typical (tPP 0.4 ms) | ~0.6 ms | ~1650 | ~420 KB/s |
| worst case (tPP 3 ms every page) | ~3.2 ms | ~310 | ~79 KB/s |

| stream | file B/s (measured coder) | vs worst case | CPU for the pages | 13 MB lasts |
|---|---|---|---|---|
| AD2 76800 | ~146 KB/s (1.9 B) .. 225 KB/s (2.93 B) | 2-3x over: only after the tPP probe (below) | ~12-18 % | ~1.5 min |
| AD2 38400 | ~74 KB/s (1.9 B) .. 113 KB/s (2.93 B) | at / over the limit | ~6 % | ~3 min |
| AD2 19200 | ~37 KB/s | 2x margin | ~3 % | ~6 min |
| AD2 9600 | ~10..28 KB/s | 3x+ | ~1.5 % | ~12 min+ |
| AD2 4800 / 2400 / 1200 | <= 14 KB/s | large | < 1 % | 25 min .. 2 h |
| AD1 640 | ~1.3 KB/s | large | | ~2.8 h |
| AD1 40 | ~0.1 KB/s | large | | ~1.5 days |

76800 S/s ([dmm-input-scope-feasibility.md](dmm-input-scope-feasibility.md)
11.12, decision 2026-10-22 #9): in the emulator with the disk's SPI timed, a
typical chip keeps up to 196 KB/s (767 pages/s), a chip at tPP 3 ms on every
page loses 28-54 %. So a disk session above 38400 S/s first times 4 page
programs at the end of Prep (`fastlog::set_probe`, LOG:DISK? tpp fields) and
runs at 76800 only when the mean is <= 800 us (`TPP_OK_MEAN_US`) and the max
<= 1.5 ms (`TPP_OK_MAX_US`), else at 38400.

**The CPU.** At 76800 the stream ISR takes ~52-62 % of the CPU. The writer
needs ~2.5 us per row for the coder and ~0.17 ms per page, ~60 % of the main
loop's CPU at 1.8 B/sample (emulator with the device's speed model,
`--mips 110 --isr-stall-ns 700`, emu/peripherals/CpuCost.cs). The Run pass
budget counts its own CPU (wall time minus the ISR's busy time). Before this
was tuned (device 20a91d0) the 76800 rung lost 54 % on the device.

**The writer trial** (fastlog.rs `Gate`, every disk session above 38.4
kS/s whose tPP probe passed): the stream's first 1 s (after a 100 ms
warm-up) is coded as in the run, its pages clocked out as reads with the
probe's tPP as the busy time (nothing reaches the file); the rung stays
when the ring dropped nothing and its low water (the fill at a pass's start)
did not rise from the first half of the window to the second by more than
128 records (a writer short by ~0.4 % of the rate); else the session runs at
38400 (LOG:SRATe? / LOG:PREamble? say so, as for a slow tPP). Kept: the
coder starts again with the header at the window's end and the same logger
session runs on (the run has the trial's stream: its Pll, its ISR load);
fell back: the logger session restarts at 38400 and the file begins then.
Either way the trial's samples are not in the file. The high water is reported, not judged: a slow pass
(a panel part, ~18 ms) lifts it to ~1400 of 2048 once in a while. LOG:DISK?
fields 11..13: the verdict (0 none, 1 kept, 2 fell back), the window's ISR
permille, the ring's high water. A stop during the trial abandons the
session as one in Ready (the file is removed).

Emulator, 76800 to the disk, flash typ (`emu/fastlogtest.py --mips 110
--isr-stall-ns 700`, 2026-10-23; "before" = 540e056):

| firmware | input | rows | lost | B/sample | pages/s | ISR % | trial |
|---|---|---|---|---|---|---|---|
| 540e056 (before), --mips 110 alone | 0.5 V 50 Hz | 96 520 of 153 602 | 57 081 (37 %) | 1.99 | 596 | 46 | - |
| 540e056 (before) | 0.5 V 50 Hz | 82 977 of 153 639 | 70 662 (46 %) | 2.12 | 636 | 52 | - |
| after | 0.5 V 50 Hz | 153 602 | 0 | 1.22 | 366 | 52 | kept |
| after | + noise 40 / 200 / 1000 | 768 002 / 307 201 / 306 422 | 0 | 1.81 / 2.09 / 2.45 | 544 / 628 / 734 | 52 / 52 / 62 | kept |
| after, the trial at 12.7 s uptime (--start-at 11) | 0.5 V 50 Hz | 153 601 | 0 | 1.21 | 363 | 52 | kept |
| after, --isr-stall-ns 1500 | 0.5 V 50 Hz | 307 201 | 0 | 1.24 | 372 | 59 | kept |
| after, --mips 80 / 70 (a slower main loop) | 0.5 V 50 Hz | 153 602 / 76 802 at 38400 | 0 | 1.27 / 1.34 | 190 / 201 | 27 / 30 (63 / 69 in the trial) | fell back |

So: up to 19200 S/s the disk keeps up even at the datasheet's worst page
time; 38400 S/s keeps up with typical parts (~5x margin) but not with a
chip at tPP max on every page, where the ring (53 ms at 38400) overflows
and the CSV shows lost_before rows. Emulator measurement below (busy model in
[emu/peripherals/W25Q128.cs](../../emu/peripherals/W25Q128.cs),
[emu/fastlogtest.py](../../emu/fastlogtest.py)); the device check is open
([TODO.md](../TODO.md) "Fast logger device check").

Emulator (`emu/fastlogtest.py`, 2026-10-22; 200 MIPS, AC V 0.5 V 50 Hz on
0.2 V DC / DC V for AD1, LOG:DURation 2; the model's signal is cleaner than
a real input, so its B/sample is low):

| flash | stream | Prep (allocation ~3 B/sample x 2 s + 8 KiB) | rows | lost | B/sample | pages/s |
|---|---|---|---|---|---|---|
| typ | AD1 640 | 0.3 s | 1282 | 1 (an AD1 main-loop gap) | 2.50 | 6 |
| typ | AD2 4800 / 9600 / 19200 / 38400 | 0.5 / 0.7 / 0.8 / 0.8 s | 9602 / 19202 / 38401 / 76801 | 0 | 1.84 / 1.68 / 1.55 / 1.44 | 35 / 63 / 116 / 216 |
| max | AD1 640 | 2.6 s | 1282 | 0 | 2.48 | 6 |
| max | AD2 4800 / 9600 / 19200 / 38400 | 4.6 / 6.2 / 7.4 / 9.4 s | 9601 / 19201 / 38402 / 76802 | 0 | 1.84 / 1.68 / 1.55 / 1.45 | 35 / 63 / 116 / 217 |

Every session kept up, also 38400 S/s with tPP = 3 ms on every page (217 of
the ~310 pages/s; the ring never overflowed): at this signal's 1.45
B/sample. A noisier input (2.93 B/sample worst case: ~440 pages/s) at
38400 would not keep up with a worst-case chip (untested on the device).
The Prep time is the erase of a zero-filled emulator disk (worst: every
sector dirty); a fresh area that is already blank skips it. Over Bluetooth
AD2 and disk sessions are refused ([bt-scpi.md](bt-scpi.md#throughput)).

## 4. Export: CSV on read

The DLG files live on the SPI flash (internal storage,
[`src/fatdir.rs`](../src/fatdir.rs)) as `LOGS/LOGnnn.DLG` / `LOGS/FASTnnn.DLG`
/ `SCOPE/SCOPnnn.DLG` (files of earlier builds in its root); the firmware
writes no text CSV files there. CSV is produced three ways from one
generator in `dlog.rs`:

1. **Generated files on the USB volume** ([`src/vdisk.rs`](../src/vdisk.rs)): `LOGS/LOGnnn.CSV` for each
   finished `LOGnnn.DLG` and `FASTnnn.DLG`, `SCOPE/SCOPnnn.CSV` for each `SCOPnnn.DLG`, exact
   size from the chunk headers / block table. Section 5.
2. **SCPI** `MMEMory:DATA? "<name>"[,<offset>[,<length>]]`: definite-length
   block `#4nnnn` of up to 6000 bytes of the file starting at `offset`
   (default 0); a name ending in `.CSV` whose `.DLG` exists is served as the
   generated CSV, any other file as its raw bytes (the name decides the
   directory; a `LOGS/` / `SCOPE/` / `SHOTS/` prefix is accepted). The host
   loops until a short block. `MMEM:CAT?` lists the gallery files (S*.BMP,
   LOG*.DLG, FAST*.DLG, SCOP*.DLG; those in a directory with it), all as `BIN`:
   `"LOGS/LOG000.DLG,BIN,<bytes>"`.
3. **Host decoders**: [`tools/dlog.py`](../tools/dlog.py) (standalone, stdlib only:
   `dlog.py LOG000.DLG > LOG000.CSV`, `--check` reports chunk errors) and
   `tools/dm40scpi.py get LOGS/LOG000.CSV` (over USBTMC, `MMEM:DATA?`).

The gallery reads `.DLG` directly (chunk decoder into its min / avg / max and
sparkline passes).

## 5. The USB volume's CSV files (`vdisk::refresh`)

`vdisk::refresh(exclude)` scans the SPI directories (dlog.rs `each_entry`
over cluster chains, `dir_start`) and keeps a table of the generated files
([native-boot.md, Unit data](native-boot.md#unit-data-the-usb-volume-srcvdiskrs-the-spi-disk-srcfatdirrs-and-the-calibration-safety-copy-srccalbackuprs): existing files keep their slot and
clusters, new ones are appended, a compaction only while unmounted); a new
DLG's CSV size comes from `Gen::open` (every chunk header + CRC; once per
file, not per refresh; kind 3: the chunk headers and the CRCs of the last 4
chunks only, see below), `exclude` = the file being recorded (it appears when
it ends). Reads go through one cached `Gen` (sequential 512-byte sector reads
O(1), a seek walks the chunk headers); it reads the SPI flash itself
(spiflash.rs `read_at`, through `dlog::FnRaw`; no 4 KiB block buffer, no
calbackup scratch) and waits for a running erase like every SpiFlash read.

Main loop (main.rs, after `csvlog::poll`): `dlog::set_stale()` (a recording
ended, a screenshot / scope record was written, a file deleted, an upload)
bumps `dlog::gen()`; once it has not moved for 1 s and no csvlog commit /
screenshot job runs: `vdisk::refresh(csvlog::recording_name())`. Cost: the
directory scan plus `Gen::open` of the new DLGs, ~0.1 s per 100 KB of new
logs at the 12.5 MHz SPI clock. The volume is 1.98 GiB (32 KiB clusters):
CSVs are ~10-20x their DLG, so a full 16 MB SPI disk of logs fits.

Large FAST files (a 2.2 MB DLG is a ~50 MB CSV):

- `Gen::open` checks the chunk headers and only the last 4 chunks' CRCs
  (~34 KB of reads, ~0.04 s), at the volume refresh and at the first read.
  A chunk corrupted inside the file (not the torn tail) is then not
  detected by the volume / MMEM:DATA? (its rows decode as garbage, their
  count right); `tools/dlog.py` checks every CRC.
- `fast_seek` / `seek` go on from the next chunk (no FAT walk from the
  start): ~13 k SPI reads for a 50 MB CSV, ~1.5 s of SPI time (host
  estimate, `tools/fatren-test` `vread`).
- The sector read path is `.text.vdisk_rd` in region B (zero-wait flash).
- main.rs keeps polling USB while a READ's data phase runs, 2 ms per pass
  (0.5 ms in a logger session). The device throughput is not measured
  (TODO.md "Fast logger device check").

Tested: `tools/fatren-test` (the USB volume over a SPI image with logs, scope
records and screenshots: dumped, fsck, mtools, LOGS/LOG000.CSV == the old
writer's file by md5, SCOPE/SCOP000.CSV too, every CSV == dlog.py) and the
emulator (USB/IP `mscdump`).

## 6. Cost on the MCU

* Encoder: per row the text formatter (for the CSV length) + ~40
  instructions of bit coding; the coded bytes go to the 8 KiB csvlog FIFO,
  the chunk header is patched there before the commit. The FIFO holds ~2 min
  (worst case) to ~10 min of readings while a commit is deferred.
* Code: ~19 KB of native .text (codec, generator, USB-volume table,
  MMEM:DATA?, gallery reader).
* Gallery: same decoder, one chunk per main-loop pass.

## 7. Results

Host (`tools/dlog-test`, 6000 rows per set, chunks of 1..1200 rows):

| set | CSV B/row | DLG B/row | ratio |
|---|---|---|---|
| noisy DC V 10/s | 19.8 | 1.23 | 15.7x |
| steady ohm 2.5/s | 22.5 | 0.92 | 23.6x |
| ramp 2.5/s | 20.5 | 1.52 | 13.2x |
| AC V 230 10/s | 19.8 | 1.55 | 12.5x |
| Hz 3/s | 21.4 | 1.30 | 16.1x |
| ohm + OL bursts | 19.0 | 1.29 | 14.3x |
| MEDIUM + autorange + flags | 22.5 | 1.50 | 14.6x |
| decade sweep (worst case) | 22.5 | 7.20 | 3.1x |
| 10 h at 10/s, 1-min chunks | 21.7 | 1.16 | 18.7x |

Emulator (native image): `LOG:CSV ON` for 60 s on a 1.234 V input (154
readings, MEDIUM 2.5/s): LOG000.DLG 455 bytes (248 of them the header), CSV
3108 bytes; `MMEM:DATA? "LOG000.CSV"` == `tools/dlog.py` of
`MMEM:DATA? "LOG000.DLG"` == the USB volume's LOG000.CSV.

Status and the device checklist: [TODO.md](../TODO.md) "Compact binary logs".
