# Compact log format (DLG): binary on the disk, CSV on read

Owner request 2026-10-21: "store the logged data in RLE or something like that
to save space. Not in CSV. CSV only for export, e.g. on read. Or maybe there is
a standard format with compression instead of CSV."

Scope: the DMM recording (src/csvlog.rs, DMM page swipe down / `LOG:CSV ON`),
the scope record (src/scope_csv.rs, UF-13 "Save CSV", c8db910) and, as a
reserved stream kind, the FR-38 fast logger (src/logger.rs: today it never
writes the disk, it streams to the USB host).

## 1. What the CSV costs today

Measured with the csvlog.rs formatter ported bit for bit to Python
(scratchpad prototype, synthetic data below):

| data | rows/s | CSV bytes/row |
|---|---|---|
| noisy DC V 1.2345x V, sigma 30 uV | 10 | 20.7 |
| steady 10.0012 kOhm, +-1 digit 10 % of rows | 2.5 | 22.7 |
| battery ramp 4.2 -> 3.0 V | 2.5 | 21.2 |
| AC V 230 V wandering | 10 | 20.7 |
| Hz 50.000 wandering | 3 | 22.1 |
| ohm with OL bursts | 10 | 20.4 |
| MEDIUM rate (10/s moving, 2.5/s steady) + autorange steps | 2.5 | 21.8 |
| FR-38 raw AD1 counts as `seq,t_s,counts` | 40 / 640 | 24.4 / 25.5 |
| scope record row (fixed width) | 26624 per record | 26 |

The row is ~21 bytes: `1234.567,1.23456,V,\r\n`; time (8..10 chars), value
(7..11), unit and separators. The long-run timestamp grows by a char per
decade of seconds.

Disk: W25Q128 16 MiB = one FAT16 volume, 4087 clusters of 4 KiB = 15.96 MiB.
Taken out: the firmware backup `_DM40PRV.BIN` (~0.4 MB), the write-once
calibration safety copy (~25 KB), a few screenshots (300 KB each). **13 MB
free** is the working assumption below.

Hours of logging in 13 MB:

| rate | CSV (21 B DMM / 25 B FR-38) | DLG (measured, below) | gain |
|---|---|---|---|
| 1/s DMM | 172 h (7 days) | 2800 h (117 days; 1.29 B/row; one recording ends at 49.7 days, the u32 ms time) | 16x |
| 10/s DMM | 17 h | 325 h (13.5 days; 1.11 B/row) | 19x |
| 40/s FR-38 | 3.6 h | 41 h (2.2 B/row) | 11x |
| 634/s FR-38 | 14 min | 2.6 h (2.15 B/row) | 11x |
| scope record | 690 KB / record, 18 records | ~9-14 KB / record, ~1000 records | 50-75x |

## 2. Candidates

Synthetic sets (Python, seed 1; times jittered +-3 ms as the main loop does);
bytes per row:

| set | CSV | gzip -9 | gzip member/min | deflate fixed-Huffman 1 KiB window | xz | bz2 | varint dod+delta | **adaptive Rice** |
|---|---|---|---|---|---|---|---|---|
| noisy DCV 10/s | 20.70 | 4.25 | 4.28 | 5.93 | 2.50 | 3.36 | 2.00 | **1.11** |
| steady ohm 2.5/s | 22.71 | 3.52 | 3.94 | 5.17 | 2.00 | 3.04 | 2.00 | **0.88** |
| ramp 2.5/s | 21.23 | 5.69 | 5.45 | 7.45 | 3.04 | 6.03 | 2.00 | **1.26** |
| ACV 230 10/s | 20.70 | 5.38 | 5.19 | 7.09 | 2.88 | 4.50 | 2.01 | **1.43** |
| Hz 50 3/s | 22.08 | 5.13 | 5.22 | 7.01 | 2.91 | 4.13 | 2.00 | **1.26** |
| ohm + OL bursts | 20.25 | 4.61 | 4.52 | 6.21 | 2.57 | 3.57 | 2.94 | **1.86** |
| MEDIUM + autorange | 21.77 | 4.71 | 4.99 | 6.48 | 3.02 | 3.70 | 2.00 | **1.25** |
| FR-38 40/s | 24.35 | 8.16 | | 11.37 | 4.26 | 6.33 | 2.13 | **2.25** |
| FR-38 640/s | 25.53 | 8.61 | | 12.28 | 4.31 | 6.74 | 2.76 | **2.15** |

Ratios vs CSV: gzip 4..5x (desktop zlib, 32 KiB window), an MCU-sized
deflate 3..3.5x, xz 7..8x (not MCU-feasible), own Rice coding **11..26x**.

**(b) Standard formats.**
* *gzip CSV, one gzip member per commit* (`LOG000.CSV.GZ`): the only standard
  option with broad tool support (zcat, Python/pandas, 7-Zip; Excel does not
  open it). Multi-member gzip is valid and power-cut friendly (each commit a
  member). But a compressor that fits: RAM is the limit (native image:
  ~5 KB of SRAM and ~6 KB of TCM left), so a 32 KiB window is out; a 1 KiB
  window with fixed Huffman (uzlib-class, ~1.5 KB code, ~1.5 KB RAM for the
  window + hash) reaches only **3.5x** (5.9 B/row). And on read the CSV still
  has to be decompressed by the user.
* *sigrok session (.sr)*: a ZIP with float32 analog samples per channel at a
  fixed samplerate; the ZIP central directory is at the end (not appendable,
  not power-cut safe), no per-sample timestamps (DMM readings are irregular:
  MEDIUM switches 10/s <-> 2.5/s), no OL. 4 B/sample before deflate. Good as
  an *export* target for the scope record later, not as the storage format.
* *FLAC*: integer samples at a fixed rate, excellent residual coding (the
  idea used below), but no timestamps / OL / units, and only audio tools.
* HDF5 / Parquet / TDMS: far too big for the MCU.

**(a) Own block format** wins by 3..5x over the best MCU-feasible standard
option, and the CSV export is generated anyway. Choice: own format **DLG**,
CSV produced on read (section 4).

Why adaptive Rice and not byte varints / RLE: the varint coding is floored at
2 bytes per row (one byte for time, one for value); Rice codes the typical
"time +-3 ms, value +-few digits" row in ~9 bits. RLE is implicit: a repeated
value with an unchanged interval costs 2 bits (k = 0 codes for 0 and 0); an
explicit run token would save at most ~0.25 B/row on steady signals, not
worth a second code path.

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
| 4 | 1 | kind: 1 DMM readings, 2 scope record, 3 FR-38 raw (reserved) |
| 5 | 1 | unit length `ul` (<= 12) |
| 6 | 2 | header text length `hl` |
| 8 | 4 | CRC-32 (zlib) of bytes 12 .. 12 + 12 + hl |
| 12 | 12 | unit (ASCII, `ul` bytes used, rest 0) as in each CSV row |
| 24 | hl | the CSV header text exactly as the old writer printed it (the two `#` lines and `time_s,value,unit,flags`, CRLF) |

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

A block (docs/log-format-standards.md section 4): 3 bits mode, 0..3 = the
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

Size (tools/dlog-test, synthetic 26624-sample records), bytes per sample:

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

### FR-38 raw stream (kind 3, reserved, not implemented)

Same chunk framing, rows = (dod of the microsecond timestamp, delta of the
24-bit count) with the same Rice coder; seq is implicit (row number) except a
gap record. Measured 2.15..2.25 B/row (11x vs CSV). Not implemented: at
640/s every commit's FAT / root `write_block` (<= 0.45 s main-loop stall)
loses ~290 conversions (the HY3131 has no FIFO), and a commit per 4 KiB at
1.4 KB/s is one root-block erase every 3 s (100k cycles in ~80 h). Viable at
<= 40/s with the 60 s commit; at 640/s it needs pre-allocated, pre-erased
clusters and one directory write at the end. Left for a later item.

## 4. Export: CSV on read

The DLG files live on the SPI disk (LUN 0) as `LOGnnn.DLG` / `SCOPnnn.DLG`;
the firmware neither writes nor reads text CSV files there (owner
2026-10-21: the CSV is only the export). CSV is produced three ways from one
generator in src/dlog.rs:

1. **Virtual CSV files on the read-only LUN 1** (vdisk.rs `set_extra`, the
   hook the vdisk agent built): `LOGnnn.CSV` for each finished `LOGnnn.DLG`,
   (and `SCOPnnn.CSV` for each `SCOPnnn.DLG`), exact size from the chunk
   headers / block table. Section 5.
2. **SCPI** `MMEMory:DATA? "<name>"[,<offset>[,<length>]]`: definite-length
   block `#4nnnn` of up to 6000 bytes of the file starting at `offset`
   (default 0); a name ending in `.CSV` whose `.DLG` exists is served as the
   generated CSV, any other root file as its raw bytes. The host loops until
   a short block. `MMEM:CAT?` lists the gallery files (S*.BMP, LOG*.DLG,
   SCOP*.DLG), all as `BIN`:
   `"LOG000.DLG,BIN,<bytes>"`.
3. **Host decoders**: `tools/dlog.py` (standalone, stdlib only:
   `dlog.py LOG000.DLG > LOG000.CSV`, `--check` reports chunk errors) and
   `tools/dm40scpi.py get LOG000.CSV` (over USBTMC, `MMEM:DATA?`).

The gallery reads `.DLG` directly (chunk decoder into its min / avg / max and
sparkline passes).

## 5. LUN-1 hook (vdisk.rs `set_extra`, implemented)

src/dlog.rs (feature `vdisk`, i.e. the native image):

```rust
/// Rebuild the LUN-1 extra files: one LOGnnn.CSV / SCOPnnn.CSV per valid
/// LOGnnn.DLG / SCOPnnn.DLG in the SPI-disk root (directory order; with
/// more than vdisk::MAX_EXTRA the newest ones), exact sizes (chunk headers,
/// every CRC checked), then vdisk::set_extra (its one-shot media change)
/// unless the table is unchanged. `exclude` = the file being
/// recorded (its size still changes; it appears when it ends).
pub fn refresh(exclude: Option<[u8; 11]>) -> usize;
/// Extra::read: CSV bytes of table entry `arg`; a cursor makes sequential
/// 512-byte sector reads O(1), a seek walks the chunk headers.
pub fn vread(arg: u32, off: u32, buf: &mut [u8]);
pub fn set_stale();   // csvlog: a recording ended, a file was deleted
pub fn stale() -> bool;
```

Main loop (main.rs, after `csvlog::poll`): a host WRITE marks it stale (the
PC may have copied / deleted logs); when stale, 2 s after the last host
WRITE(10) (a file copy is ~100 of them) and while no csvlog commit or
screenshot / scope job runs: `dlog::refresh(csvlog::recording_name())`.
Cost: a synchronous read of every DLG file (CRC of every chunk), ~0.1 s
per 100 KB of logs at the 12.5 MHz SPI clock (a 10 h log at 10/s is 0.4 MB:
~0.4 s of main-loop stall, once per recording end / delete / host copy). `vread` reads the SPI flash itself
(spiflash.rs `read_at`, through `dlog::FnRaw`; no 4 KiB block buffer, no
calbackup scratch: csvlog holds those across passes) and waits for a
running erase like every SpiFlash read (<= 0.4 s at worst). RAM: the table
(64 x 20 B) and the cursor (~0.4 KiB) in .bss; `refresh` puts the 64-entry
Extra list (~1.5 KiB) on the stack once. Volume size: the vdisk data area is 64 MiB;
CSVs are ~20x their DLG, so a full 13 MB disk of logs would expand past it:
`set_extra` takes the prefix that fits (newest last in directory order), so
very large logs are reachable by SCPI / dlog.py only. Raising the cluster
size to 32 KiB (FAT16 limit 2 GiB) would cover every case.

Tested: tools/fatren-test (`vdisk ... logs <spi.img>`: the LUN-1 volume
dumped, fsck, mtools, LOG000.CSV == the old writer's file by md5, SCOP000 /
SCOP001.CSV too) and the emulator (section 7).

## 6. Cost on the MCU

* Encoder: per row the old text formatter (for the CSV length) + ~40
  instructions of bit coding; no new RAM (the coded bytes go to the
  existing 8 KiB FIFO, the chunk header is patched there before the commit).
  The FIFO now holds ~2 min (worst case) to ~10 min of readings while a PC
  write or mount defers commits (was ~27 s).
* Code: the whole change is +~19 KB of native .text (codec, generator,
  LUN-1 table, MMEM:DATA?, gallery reader); .bss +~1 KB.
* Gallery: same decoder, one chunk per main-loop pass.

## 7. Results

Host (tools/dlog-test, 6000 rows per set, chunks of 1..1200 rows):

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

Emulator (native image, Renode, USB/IP): `LOG:CSV ON` for 60 s on a 1.234 V
input (154 readings, MEDIUM 2.5/s): LOG000.DLG 455 bytes (248 of them the
header), CSV 3108 bytes; `MMEM:DATA? "LOG000.CSV"` == tools/dlog.py of
`MMEM:DATA? "LOG000.DLG"` == LUN-1 LOG000.CSV read over MSC.

Status and the device checklist: TODO.md "Compact binary logs".
