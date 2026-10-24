# DMM log storage: standard formats and how far below DLG we can go

Owner questions, 2026-10-21: "why .dlg, a strange extension — is there really
nothing standard? Lots of software solved the same task." Then the refocus: "who
cares what we store it in if CSV is read directly — but do study whether there is
MORE EFFICIENT storage."

Context: docs/compact-log-format.md (DLG v1: delta + zigzag + adaptive Rice in
self-contained, CRC-protected chunks, one per csvlog commit; CSV generated on
read on the virtual LUN 1, by SCPI and by tools/dlog.py).

**Short answer.**

1. **Container: keep DLG.** No widely used standard format is appendable in
   small power-cut-safe commits from an MCU with a few KB of RAM *and* compresses
   noisy readings well. The closest one, ASAM MDF4 (unsorted, DZ deflate
   blocks), comes out at 4-5 B/reading even with desktop zlib, against DLG's
   1.2. CSV on LUN 1 already makes the format invisible to users.
2. **A better coder alone does not pass the 1.5x bar.** A range-coder version
   of DLG with better predictors ("DLG2", built and round-trip tested here) is
   **1.13-1.64x** smaller (median ~1.22x; 10 h run: 1.26x). With the current
   timestamps, the information-theoretic floor of the test data is only
   1.3-1.7x below DLG v1 (2.2x on the steady-ohm set).
3. **The largest share is in the timestamps.** About 3 of DLG's 9-12 bits per
   reading code the ±3 ms main-loop jitter of the time stamp, not the
   measurement. Stamps on the converter's grid save **1.37x with today's DLG v1
   and no format change**, and **2.0x** together with DLG2. First measure the
   real jitter on the device. Then decide whether the stamp may be snapped to
   the grid (this changes the CSV time column at the ms level).
4. **Scope records: a clear win.** They are stored uncompressed today (u16 per
   10-bit code). A fixed polynomial predictor + Rice (the coder fw-rs already
   has) brings them to 0.35-0.53 B/sample instead of 2.0 (4-6x). That is
   FLAC-level compression. Packing the codes into 10 bits gives 1.6x with almost
   no risk.

Everything below is measured by `tools/experiments/logcodec-lab/` on the tools/dlog-test
data sets (same generators, same chunk cuts). Every DLG2 number comes from a
real encode that was decoded back and compared byte for byte with the reference
CSV, chunk by chunk.

## 1. Where the bits go: the floor

The synthetic data is "a value + Gaussian noise of N counts in the 6th digit"
with timestamps `t += period + U(-3..+3) ms`. An encoder that knows the
generator still has to pay for the noise:

* time: uniform jitter over 7 values = log2 7 = **2.81 bits/row** (Hz set:
  ±5 ms, 3.46). DLG codes the delta-of-delta. That is the difference of two
  jitters (triangular, H = 3.50 bits measured), so it pays +0.7 bit/row by
  construction.
* value: quantized Gaussian of σ counts ≈ 0.5·log2(2πe·σ²). Noisy DC V
  (σ = 3 counts) 3.64 bits. Plain delta coding of i.i.d. noise doubles the
  variance (+0.5 bit: measured order-0 entropy 4.14 bits for "previous value"
  against 3.68 for a slow EMA). AC V (random walk + noise) needs the Kalman
  innovation, 6.64 bits.
* a Rice code costs at least 1 bit per symbol, so time + value can never go
  below 2 bits/row. A range coder has no such limit (steady signals).

| set | CSV B/row | DLG v1 | floor (time + value bits) | floor B/row | floor gain over DLG v1 |
|---|---|---|---|---|---|
| noisy DC V 10/s | 19.82 | 1.227 | 2.8 + 3.6 | 0.806 | 1.52x |
| steady ohm 2.5/s | 22.54 | 0.919 | 2.8 + 0.6 | 0.422 | 2.18x |
| ramp 2.5/s | 20.54 | 1.519 | 2.8 + 4.4 | 0.897 | 1.69x |
| AC V 230 10/s | 19.82 | 1.550 | 2.8 + 6.6 | 1.181 | 1.31x |
| Hz 3/s | 21.44 | 1.298 | 3.5 + 4.4 | 0.979 | 1.33x |
| ohm + OL bursts | 19.02 | 1.292 | 2.8 + 4.3 | 0.894 | 1.45x |
| MEDIUM + autorange + flags | 22.48 | 1.502 | 2.8 + 4.8 | 0.949 | 1.58x |

The floors leave out the 28-30 B chunk headers. Because of them, even a perfect
coder lands at about 1.3-1.6x. Only the steady signal leaves a large margin.

## 2. Coders measured

B/row including the chunk headers, with the dlog-test chunk cuts (1..1200 rows,
avg ~350).

| set | DLG v1 | Rice + new predictors | RC + DLG predictors | **DLG2** (RC + new predictors) | DLG2 / v1 | xz -9 of CSV | Gorilla (f32 XOR) |
|---|---|---|---|---|---|---|---|
| noisy DC V 10/s | 1.227 | 1.092 | 1.109 | **0.984** | 1.25x | 2.19 | 2.58 |
| steady ohm 2.5/s | 0.919 | 0.794 | 0.665 | **0.560** | 1.64x | 1.95 | 1.35 |
| ramp 2.5/s | 1.519 | 1.205 | 1.202 | **1.092** | 1.39x | 3.18 | 4.10 |
| AC V 230 10/s | 1.550 | 1.461 | 1.441 | **1.376** | 1.13x | 2.56 | 3.44 |
| Hz 3/s | 1.298 | 1.236 | 1.174 | **1.107** | 1.17x | 3.00 | 3.55 |
| ohm + OL bursts | 1.292 | 1.188 | 1.155 | **1.064** | 1.21x | 2.56 | 2.82 |
| MEDIUM + autorange + flags | 1.502 | 1.399 | 1.303 | **1.239** | 1.21x | 3.49 | 4.66 |
| decade sweep (stress) | 7.197 | 7.140 | 4.104 | 4.096 | 1.76x | 5.32 | 4.38 |
| special values (stress) | 6.480 | 6.539 | 3.306 | 3.712 | 1.75x | 2.36 | 4.42 |

10 h at 10/s (360 000 rows), chunk size against loss on a power cut:

| rows/chunk | DLG v1 | DLG2 | gain | lost on a power cut |
|---|---|---|---|---|
| 60 (6 s) | 1.871 | 1.575 | 1.19x | 6 s |
| **600 (1 min, today)** | **1.158** | **0.920** | **1.26x** | 60 s |
| 3000 (5 min) | 1.093 | 0.854 | 1.28x | 5 min |
| 6000 (10 min) | 1.085 | 0.845 | 1.28x | 10 min |

Larger chunks than today's 1 min gain only ~6 %. That is not worth losing more
data on a power cut.

Where DLG2 spends its bits (ideal bits/row): time 3.05-3.75 against a 2.81-3.46
floor; value 4.0 / 0.8 / 5.0 / 7.1 / 4.6 / 4.8 / 5.6 against 3.6 / 0.6 / 4.4 /
6.6 / 4.4 / 4.3 / 4.8. The remaining gap is ~0.25 bit for time and ~0.4 bit for
value per row. Most of it is adaptation cost after each chunk restart: the
single-chunk run (one 6000-row chunk, slow-EMA time predictor only; printed by lab.py) reaches 2.92 bits for time.

What each idea gives (ablation in the lab):

* **Range coder instead of Rice** (same predictors): 1.03-1.38x, the most on
  steady data and on the stress sets, where Rice's 1-bit-per-symbol floor and
  escape cost dominate.
* **Better predictors**: time = backward selection among {previous dt (DLG),
  slow EMA of dt, second-order grid loop}; value = selection among {previous
  (DLG), EMA 1/4, EMA 1/16, Holt level+trend}. The selection is backward (the
  decoder runs the same predictors), by the smaller running mean |residual|.
  They give +5..20 % on top of the range coder. One trap here: integer EMAs need
  rounded shifts. `x += (d >> s)` has a dead zone that biases the prediction by
  ~2^s units and cost 0.25 bit/row until it was fixed.
* No gain from: 4 activity classes of contexts (±0.5 %); priors computed from a
  two-sided geometric distribution and stored as a header byte (0 to −2 %);
  fixed-rate LZMA probabilities against counter-based rates (≤1 %).
* **Gorilla / Chimp-style XOR float coding** is 2-3x *worse* than DLG. Noisy
  decimal readings have no shared mantissa bits. It also cannot reproduce the
  printed 6-digit decimals exactly.
* General-purpose compressors on the CSV, even xz -9 with a desktop-size
  window, give 2-3.5 B/row.

## 3. The timestamp is the lever

`app.rs` stamps a reading with `clk.now_ms()` of the main-loop pass that
handles it. On the device the HY3131 converts at a fixed rate, so the real
stamps are probably "grid + loop latency", not the random walk of the test
generator. No real-device log exists in the repo yet. Two experiments:

**Grid + loop latency** (t_i = 100 i + U(0..6) ms), noisy DC V values:

| coder | B/row | time bits/row |
|---|---|---|
| DLG v1 (delta-of-delta) | 1.313 | - |
| DLG2 without the grid predictor | 1.062 | 3.67 |
| DLG2 (with the second-order grid loop) | 1.018 | 3.32 |
| floor | 0.806 | 2.81 |

**Stamps on the conversion grid** (t_i = 100 i exactly, same values):

| set | DLG v1 B/row | DLG2 B/row | DLG2 time bits/row | gain over DLG v1 with jittered stamps |
|---|---|---|---|---|
| noisy DC V 10/s | 0.893 | 0.609 | 0.05 | v1 1.37x, DLG2 2.0x |
| steady ohm 2.5/s | 0.597 | 0.181 | 0.04 | v1 1.54x, DLG2 5.1x |
| AC V 230 10/s | 1.211 | 0.996 | 0.04 | v1 1.28x, DLG2 1.56x |
| ohm + OL bursts | 0.952 | 0.685 | 0.04 | v1 1.36x, DLG2 1.89x |

With the DLG v1 coder unchanged, a grid stamp costs 1 bit/row: Rice with
k = 0 for dod = 0. The range coder costs ~0.05 bit/row.

The ±3 ms in today's CSV is when the main loop got around to the reading, not
when the converter took it. A stamp derived from the reading count and the
converter's period (with a re-sync and a GAP flag when the loop misses readings
or the rate changes) is arguably *more* truthful. It is still a visible change
of the time column, so it is the owner's call.

## 4. Scope record

26624 10-bit codes per record. Today the file holds u16 codes (2 B/sample,
53 KB). Synthetic records (`tools/experiments/logcodec-lab/scope.py`), B/sample:

| record | u16 (today) | 10-bit packed | zlib 1 KiB window | fixed LPC + Rice | fixed LPC + RC | FLAC -8 |
|---|---|---|---|---|---|---|
| DC + 1 LSB noise | 2.000 | 1.250 | 0.400 | 0.364 | 0.331 | 0.352 |
| sine 13 periods, 0.7 LSB noise | 2.000 | 1.250 | 1.059 | 0.347 | 0.278 | 0.340 |
| square 10 periods | 2.000 | 1.250 | 0.376 | 0.347 | 0.296 | 0.343 |
| mains + harmonics, 2 LSB noise | 2.000 | 1.250 | 1.263 | 0.533 | 0.469 | 0.466 |
| sine 400 periods (fast) | 2.000 | 1.250 | 1.453 | 0.524 | 0.482 | 0.373 |
| noise burst on DC | 2.000 | 1.250 | 0.541 | 0.374 | 0.348 | 0.362 |

"Fixed LPC" means FLAC's fixed polynomial predictors, order 0..3. The order is
chosen per 4096-sample block (forward choice is fine: the record is in RAM).
The residual goes to the adaptive Rice coder DLG already has. The result is
**4-6x smaller than today** and on par with FLAC. Real ADC noise is unknown:
the gain shrinks if the noise is wider than 1-2 LSB, but it stays above the
1.6x of 10-bit packing for any noise of a few LSB (not measured on the device).

Constraint (compact-log-format.md §3): the record goes through the screenshot
writer, which needs the file length up front. Options: allocate for the u16
size and truncate the cluster chain after writing (a FAT size update and a
freed chain), or code into 4 KiB output blocks with a size table at the end.
The CSV generator has to decode to seek, but each 4096-sample block can restart
the coder and store its byte offset. Then random access stays O(1 block).

## 5. Standard formats (brief)

Measured with the desktop writers (`tools/experiments/logcodec-lab/standards.py`).
Time u32 ms, value f32, flags u8. Whole files, B/reading:

| set | CSV | DLG v1 | MDF4 plain | MDF4 DZ deflate | MDF4 DZ transposed | TDMS (600-row segments) | Parquet zstd | Parquet zstd, 600-row groups | HDF5 gzip+shuffle | CSV.gz -9 | CSV.xz -9 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| noisy DC V 10/s | 19.85 | 1.27 | 14.23 | 5.08 | 6.70 | 9.27 | 5.38 | 5.84 | 4.57 | 4.32 | 2.18 |
| steady ohm 2.5/s | 22.57 | 0.96 | 14.23 | 4.51 | 5.69 | 9.27 | 5.15 | 5.46 | 3.59 | 3.57 | 2.11 |
| ohm + OL bursts | 19.05 | 1.33 | 14.23 | 5.59 | 6.54 | 9.27 | 5.79 | 6.68 | 4.57 | 4.47 | 2.51 |
| MEDIUM + autorange | 22.52 | 1.54 | 14.23 | 5.78 | 7.10 | 9.27 | 6.34 | 6.53 | 4.76 | 5.09 | 3.49 |
| 10 h at 10/s | 21.69 | 1.16 | 14.00 | 4.84 | 4.05 | 9.27 | 5.32 | 5.79 | 3.42 | 4.25 | 2.27 |

All MDF4 files load in asammdf 8.8 (`MDF(...).to_dataframe()`, OL as
invalidation bits) and export to CSV. asammdf writes the time as f64. A
hand-written MDF4 with u32-ms master and int32 value channels plus linear
conversion blocks would drop the plain size to ~9 B. Even then deflate on an
MCU leaves it at several B/row.

| format | spec / tools | MCU writer: append + power cut | compression on the MCU | verdict |
|---|---|---|---|---|
| **ASAM MDF4** (.mf4) | ASAM standard (paid spec, documented); asammdf (Python + GUI), MATLAB, Vector, CSS tools | unsorted/unfinalized files are legal and used by embedded CAN loggers: append data blocks, fix up on read. Power-cut friendly | DZ = zlib deflate (+ optional transposition) per block; needs a deflate encoder and the whole block before its header | closest fit; 4-5 B/row even with desktop zlib, ~6 B with an MCU-sized deflate. The CANedge itself compresses into its own .MFC files that need the vendor converter ([CSS docs](https://canlogger.csselectronics.com/tools-docs/converters_mf4/converters/), [log file](https://canlogger.csselectronics.com/canedge-docs/ce2/filesystem/log-file.html)) |
| NI TDMS | open spec; npTDMS, LabVIEW, DIAdem | segments append well; a torn last segment is detectable | none | 9.3 B/row |
| HDF5 | open; h5py, HDFView | needs a B-tree / metadata update per append; a torn file is unreadable without SWMR care; library far too big | gzip/szip filters | no |
| Parquet / Arrow IPC | open; pandas, DuckDB | footer at the end (Parquet: not appendable); Arrow IPC streaming appends, but no compression an MCU can afford | zstd/snappy/lz4 | no |
| EDF+/BDF+ | open; EDFbrowser, pyedflib | fixed records of int16/int24 per channel at a fixed rate; EDF+D allows gaps | none | no per-sample time or OL; 2-3 B/row |
| sigrok srzip (.sr) | open; PulseView | ZIP, central directory at the end; fixed samplerate, f32 | deflate | export target at most |
| WAV/FLAC + metadata | open; audio tools | FLAC frames append, header patch at the end | excellent (LPC + Rice) | fixed rate, no per-sample time, no OL; fits the scope record, not the DMM log |
| CSV.gz / .zst / .lz4 | universal | multi-member gzip / zstd frames append | MCU deflate with 1 KiB window ≈ 5.9 B/row (compact-log-format.md §2) | 5x worse than DLG |
| InfluxDB line protocol, CBOR, MessagePack | open; many libs | appendable streams | none (CBOR/MsgPack: ~6-9 B/row) | no |
| Vendor formats (Fluke, Keysight BenchVue, UNI-T, Owon, PicoScope .psdata, Saleae .sal) | proprietary; the PC apps export CSV | — | — | not standards; from memory, not verified: the handheld-meter apps log as CSV/XLS on the PC side |

MCU deflate cost (for MDF4 DZ or .gz): uzlib-class greedy LZ77 + fixed Huffman.
About 1.5-2 KB code; RAM = 1 KiB window + 0.5-1 KiB hash heads (≈2 KB); plus a
4 KiB staging buffer for MDF4 DZ, because its header holds both sizes. ~50-100
cycles per input byte, so ~1-2 ms per 4 KiB block at 200 MHz. CPU is not the
issue. The issue is the ratio (3-3.5x on CSV, ~1.5x on binary f32 records)
and the RAM.

## 6. DLG2 on the MCU (estimate)

* Range coder (LZMA-style, 12-bit probabilities, counter-based rate): ~0.4 KB
  Thumb-2 for encoder + decoder. Models (Exp-Golomb binarization with a 4-level
  bit tree, 3 time + 4 value predictors, selection): ~1.5 KB. In total
  **+2-3 KB flash** against today's Rice code.
* RAM per coder instance: 2 streams × 80 contexts × 3 B (u16 probability + u8
  count) ≈ 0.5 KB, plus ~60 B of predictor state. Today's DLG state is ~40 B.
  There are three instances (csvlog encoder, LUN-1 `vread` cursor, gallery
  decoder): **~1.5 KB .bss**, which is a real cost against ~5 KB of free SRAM.
  Trimming (12 unary contexts, 2-level tree: ~50 contexts) gives ~0.3 KB per
  instance at an estimated ≤1 % size cost.
* CPU: ~15-20 binary decisions per row at ~25 cycles + ~100 cycles of
  predictors ≈ 0.5 k cycles/row. Encoding at 10 rows/s is negligible. Decoding
  for LUN 1 runs at ~2.5 µs/row ≈ 400k rows/s, i.e. ~8 MB/s of CSV. That is
  faster than USB FS.
* Format: a new chunk magic/version and +2 B header (dt hint). The coder flush
  is ≤4 B per chunk. The chunk independence, CRC, CSV-offset skip list and the
  power-cut behaviour stay exactly as in v1. tools/dlog.py needs the decoder
  too (~150 lines).

## 7. Recommendation

| option | gain (DMM logs) | effort | risk |
|---|---|---|---|
| **A. Keep DLG v1 as is** | — | 0 | none |
| B. Switch the container to MDF4 (+deflate) | 0.25x (4x *larger*) | large | RAM, deflate code; no user benefit over the LUN-1 CSV |
| C. Keep DLG, also export `.mf4` on LUN 1 | interoperability only | ~1-2 days (MDF4 writer of fixed blocks from the chunk decoder) | low; only useful if someone asks for asammdf/MATLAB |
| D. DLG2 range coder + predictors | 1.13-1.64x, ~1.25x typical | 2-3 days (Rust port, dlog.py, dlog-test, fatren-test) | +1.5 KB RAM, a second format version to read forever |
| **E. Grid timestamps (measure first)** | 1.28-1.54x with v1 unchanged | 0.5-1 day + a device measurement | changes the CSV time column at the ms level; rate changes and missed readings must re-sync (GAP) |
| D + E | 1.56-5.1x (2.0x noisy DC) | D + E | as D + E |
| **F. Compressed scope records** | 4-6x (LPC + Rice), 1.6x (10-bit pack) | 1-2 days (pack: hours) | the up-front file length (truncate or block table); real noise unknown |

Do now: **F** (the scope record; the clear win, reusing the Rice coder) and the
**E measurement**: record 10 min of a stable DC input on the device and run
`tools/experiments/logcodec-lab` on its CSV to see the real stamp jitter. Then decide on E.
Defer **D**: alone it misses the 1.5x bar for the RAM it costs. Reconsider it
together with E, where it doubles the gain. **B is rejected.** **C** is optional
and only worth doing if someone asks for asammdf or MATLAB. The answer to "why
.dlg": no standard does this job within the MCU limits, and users never see the
format, because LUN 1 serves CSV.

## Reproduce

```
# data: a copy of tools/dlog-test/main.rs that also writes long.{dlg,csv}
#   (path of dlog.rs made absolute; after the long-run encode:
#    std::fs::write(dir/long.dlg / long.csv)); run it with an output dir D
python3 tools/experiments/logcodec-lab/lab.py D          # sections 1-3 (stdlib only, ~40 s)
python3 tools/experiments/logcodec-lab/scope.py          # section 4 (soundfile optional, for FLAC)
python3 tools/experiments/logcodec-lab/standards.py D OUT   # section 5: pip install asammdf nptdms pyarrow h5py
```
