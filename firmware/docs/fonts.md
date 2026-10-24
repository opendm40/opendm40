# Fonts: what stockfont is, what it costs, how it is stored (2026-10-22)

## 1. Why `stockfont` at all

Every text and icon on the fw-rs screens is drawn with the **stock DM40 V2.6
bitmap fonts**: the "stock UI is the baseline" rule (replicate the stock look,
pixel-diff against stock, tools/stock-ui and tools/ui-render/stock-diff.sh).
They used to be read live from the stock app at 0x0805xxxx..0x0806xxxx;
once fw-rs is flashed over 0x08004000 that app is gone, so
tools/mkstockfont.py copies them out of `dm40_v_2_6.dec.bin` (the decoded
stock update file in the repo root) into src/stockfont_data.rs. An earlier
own font (src/fonts.rs + Roboto) was dropped for this (TODO.md).
`stockfont.rs` is the renderer: layout (stock placement rules), AA blend
through a 16-step colour ramp, the stock threshold compositing of the scope
page (`Ink`), the half-size +1 digit (`label_half`) and our own W / k glyphs
(stockfont_w.rs) for the power meter.

Without it we would need another font of the same look; section 4 compares
that against keeping the stock bitmaps.

## 2. Analysis (before this change)

Source: the stock font objects (class A: 12-byte entries with yoff / w / h /
adv; class B: 8-byte entries, h = line_h; class C: fixed 1 bpp; B+: merged),
27 fonts, **1101 glyphs**. Bit depth: 1 bpp (11 fonts) or 4 bpp anti-aliased
(16 fonts). The 4 bpp fonts use **all 16 coverage levels** (AUX_TAG,
BATTERY, G_08059428 miss 1-3 of them), so a lower bpp is not lossless.
Stored as one column-major bit stream per glyph box, 12-byte entries.

Measured in the image (llvm-nm, the blobs located in the body):

| part | bytes | where |
|---|---:|---|
| glyph bitmaps (27 `&[u8]` blobs, anonymous rodata) | 90,897 | region B 0x0805F48C..0x0807751D, interleaved with the tables |
| entry tables (1101 x 12 B, named `stockfont::data::*_E`) | 13,212 | region B |
| stockfont code (label, layout, Ink, ...) | ~10,600 | region B |
| **total** | **~114,700** | |

(The "~143 KB" in the request could not be reproduced: the fonts are 104 KB of
data + ~11 KB of code; the rest of the anonymous rodata is other tables /
strings.)

Per font (raw = the stock bitmaps as embedded; new = this change):

| font | bpp | line_h | glyphs (CJK) | raw blob B | CJK B | new glyphs | new blob B | ratio |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| F78 (main readout) | 4 | 78 | 16 (0) | 25298 | 0 | 16 | 4974 | 0.20 |
| UNIT | 4 | 31 | 14 (0) | 4408 | 0 | 14 | 1441 | 0.33 |
| LETTERS | 4 | 30 | 30 (0) | 4937 | 0 | 30 | 2288 | 0.46 |
| F21 (status bar) | 4 | 21 | 35 (0) | 4292 | 0 | 35 | 2295 | 0.53 |
| F14 | 1 | 14 | 98 (0) | 638 | 0 | 98 | 637 | 1.00 |
| HIST | 1 | 15 | 26 (0) | 484 | 0 | 26 | 382 | 0.79 |
| ASCII26 (dialogs, gallery) | 4 | 26 | 112 (16) | 11546 | 3016 | 96 | 6279 | 0.74 |
| AUX_TAG | 4 | 21 | 3 (0) | 492 | 0 | 3 | 183 | 0.37 |
| MODE | 4 | 32 | 5 (0) | 1603 | 0 | 5 | 672 | 0.42 |
| BTN_GLYPH | 4 | 24 | 9 (0) | 2188 | 0 | 9 | 952 | 0.44 |
| BATTERY | 4 | 16 | 6 (0) | 1920 | 0 | 6 | 771 | 0.40 |
| LOCK | 4 | 20 | 2 (0) | 420 | 0 | 2 | 255 | 0.61 |
| SAVE_BT | 4 | 21 | 2 (0) | 370 | 0 | 2 | 246 | 0.66 |
| ON_TAG | 4 | 12 | 1 (0) | 126 | 0 | 1 | 125 | 0.99 |
| LIGHTNING | 4 | 59 | 1 (0) | 944 | 0 | 1 | 276 | 0.29 |
| TEXT22 (menus) | 1 | 22 | 313 (216) | 16213 | 13176 | 97 | 1813 | 0.60 |
| G_08059428 | 4 | 12 | 2 (0) | 258 | 0 | 2 | 150 | 0.58 |
| K_FONT / SC_DIGITS | 4 | 20 | 34 (0) | 4405 | 0 | 34 | 2080 | 0.47 |
| ICONS | 4 | 30 | 5 (0) | 2250 | 0 | 5 | 1136 | 0.50 |
| SC_C | 1 | 16 | 13 (0) | 248 | 0 | 13 | 182 | 0.73 |
| SC_F12 | 1 | 12 | 96 (0) | 1077 | 0 | 96 | 715 | 0.66 |
| SC_F20 | 1 | 20 | 96 (0) | 2400 | 0 | 96 | 1317 | 0.55 |
| SC_PROBE | 1 | 17 | 36 (0) | 1620 | 0 | 36 | 1242 | 0.77 |
| SC_EDGE | 1 | 15 | 2 (0) | 30 | 0 | 2 | 26 | 0.87 |
| SC_MARK | 1 | 12 | 42 (0) | 714 | 0 | 42 | 714 | 1.00 |
| SC_BATT | 1 | 16 | 6 (0) | 480 | 0 | 6 | 480 | 1.00 |
| SC_F16 | 1 | 16 | 96 (0) | 1536 | 0 | 96 | 791 | 0.51 |
| **total** | | | 1101 (232) | 90897 | 16192 | 869 | 32422 | 0.43 |

Findings:

* **Unused glyphs.** 232 glyphs are CJK / full-width (U+4E00.., U+FF01,
  U+FF1A): 216 in TEXT22, 16 in ASCII26, 16.2 KB of bitmaps + 2.8 KB of
  entries. No source string contains one (`grep` for U+2E80..U+FFEF in src/:
  only a comment), runtime text is numbers, units and 8.3 file names (ASCII,
  `from_utf8` of the name bytes). Every one of the 27 fonts is drawn
  somewhere; the fixed-charset fonts (F78 digits + "OL" / "+" / "-",
  UNIT / LETTERS / F21 / HIST units, MODE / BTN_GLYPH / BATTERY icons, ...)
  are already exactly their charset; the ASCII fonts (TEXT22, F14, ASCII26,
  SC_F12 / F16 / F20) draw flowing text (menus, dialogs, file names, SCPI-set
  values), so they keep all of 0x20..0x7F.
* **Padding.** Class B glyphs are line_h tall: 9.1 KB of blank box rows /
  columns (4.6 KB of it CJK in TEXT22; SC_F20 1.5 KB, SC_F16 0.9 KB, SC_PROBE
  0.5 KB, ICONS 0.5 KB). Horizontal blank columns are part of the layout
  (w is used for the text width), so only rows can be cropped losslessly.
* **Duplicates.** 5 glyphs are identical across fonts (F21 / K_FONT space,
  AUX_TAG / BTN_GLYPH 0x04 0x05, MODE / BTN_GLYPH 0x07 0x08): 827 B raw,
  ~400 B compressed. Sizes of one face (F78 / LETTERS / F21 / HIST digits)
  are separately hinted bitmaps, not scaled copies: no sharing.
* **Run structure.** 4 bpp glyphs are mostly 0 and 15 with 1-3 AA pixels at
  each edge, and stems repeat row after row: row-major runs compress them
  2.5-5x (F78 5.1x).

## 3. Options

Draw cost: every label redraw reads its glyphs, also while the AD2 ISR
streams; the data must stay in region B (zero-wait flash; region C costs
~20 us bus stalls, link-native.x).

| option | saves | pixels | draw cost |
|---|---:|---|---|
| (a) subset: drop CJK / full-width | 19.0 KB (16.2 bitmaps + 2.8 entries) | identical (never drawn) | none |
| (a') subset ASCII fonts to the strings in src/ | ~3-5 KB more | identical only for the strings known today | none; a new string with a missing glyph draws a gap: rejected |
| (b) crop 1 bpp glyphs to the ink rows | 3.3 KB (after a) | identical (t / ih in the entry) | blank rows skipped: cheaper |
| (b') crop columns too | ~1 KB | needs an x offset field | small; not done |
| (c) 2 bpp / 3 bpp for 4 bpp fonts | lossless: 0 (all 16 levels used) | lossy, see 4 | - |
| (d) RLE of 4 bpp, row-major nibble runs + row repeat | 41.3 KB (65.5 -> 24.1 KB, CJK excluded) | identical | decode streams into the LCD writes: faster than raw (below) |
| (e) dedupe across fonts | ~0.4 KB after (d) | identical | an indirection per glyph: not worth it |
| entries 12 -> 10 B (u16 code / offset, + t / ih) | 4.5 KB | - | none |
| (f) big digits from a smaller master | ~4 KB (F78 after RLE) | not pixel-exact, visibly softer | a scaler per draw: rejected |

Named section: every blob is now a named array with its own section,
the tables in `.rodata.fonts.<NAME>_E` (llvm-nm shows each with its size).
ASCII26 and F14 (tools/mkstockfont.py `A2_FONTS`) go to `.rodata.a2.<NAME>`,
which link-native.x places in region A2 (zero-wait flash after the shim,
below the loader marker; docs/native-boot.md "Region A2"); the other blobs
are `.rodata.fonts.<NAME>_B` and match `*(.rodata .rodata.*)` of the
`.text` output section: region B, no link-native.x change. Moving more blobs
to A2 is one name in `A2_FONTS` (A2 now has 7.9 KB free: e.g. F78, 5.0 KB).
None may go to region C: labels are drawn while the AD2 stream runs.

## 4. With the pixel-exact rule relaxed (owner, 2026-10-22)

The lossless result is 41.1 KB of font data (24.1 KB RLE4, 8.3 KB 1 bpp,
8.7 KB entries). What is left to gain beyond it:

| option | font data | look | draw cost / risk |
|---|---:|---|---|
| lossless (this change) | 41.1 KB | stock, identical | faster than before |
| + snap coverage 1-2 -> 0, 13-14 -> 15 | -2.5 KB | edge AA 0.46/15 mean error, barely visible | same |
| + 4 levels (2 bpp) in the same RLE | -3.0 KB (-6 KB with 2-bit literal packing) | visibly coarser edges on F78 / UNIT | same |
| re-rasterize an outline font at build time | ~same as lossless for the same glyph set and AA | not the stock face: every screen changes, stock-diff lost | same |
| runtime vector rasterizer (outlines + AA scanline) | outlines of 4-5 faces ~15-30 KB + rasterizer 5-8 KB | close to stock only if the stock face's outlines exist (they do not: the stock fonts are bitmaps of an unknown, licence-unclear face) | per pixel 10-50x the RLE decode; an F78 readout every reading; a glyph cache does not fit the 10.8 KB SRAM headroom; more bus time while the AD2 ISR streams |
| another family (e.g. a DejaVu / Roboto subset) rasterized at build time | similar to lossless | different look | same as now |

Every lossy option saves at most ~6 KB of the remaining 41 KB and changes
the look; the vector rasterizer would cost more flash than it saves and
much more draw time. Recommendation: keep the lossless storage, no lossy
step (nothing to show the owner as a before / after sheet: the screens are
unchanged).

## 5. What was done (pixel-identical)

tools/mkstockfont.py (same source, same parse) now emits:

* no glyph from U+2E80 up;
* 4 bpp glyphs as **RLE4**: row-major nibble tokens, no run crosses a row
  end, byte-aligned per glyph: `1..14` one pixel of that coverage; `0|15, n`
  n + 1 pixels of 0|15 (n <= 13); `0|15, 14, hi, lo` 15 + (hi lo) pixels;
  `0, 15, k` at a row start: the last literal row k + 1 more times;
* 1 bpp glyphs cropped to their ink rows t .. t + ih (column-major, ih bits
  per column); entries `E(code u16, off u16, w, h, adv, yoff, t, ih)`,
  10 B; the full box (w, h) and the placement are unchanged;
* each glyph decoded back and compared with the stock bitmap in the
  generator (assert).

src/stockfont.rs: `Glyph` stays 16 bytes (`Ink` holds 24). Drawing reads
rows top to bottom, so an RLE4 glyph keeps a 6-byte cursor (`Cur`: next
token, start of the last literal row, pending repeats, next row) instead of
a row index table; `row_emit` decodes a row straight into the LCD writes
(`label`, `glyphs`), `row` into a 64-byte row buffer (twin W, `label_half`
two rows). 1 bpp / raw 4 bpp glyphs keep the direct column walk. Random
access (`alpha`) remains for `Ink::at` / the scope markers (all 1 bpp fonts);
on an RLE4 glyph it decodes from the top (correct, slow, not used by any
4 bpp drawing path).

### Size (link-native, build.sh; on 5f9b5a0)

| | before | after |
|---|---:|---:|
| font bitmaps | 90,897 | 32,422 |
| entry tables | 13,212 | 8,690 |
| stockfont code | ~10,600 | ~12,400 |
| **region B free** | **26,360** | **82,152 (+55.8 KB)** |
| region A2 free (ASCII26 + F14 blobs there) | 2,652 | 7,920 (+5.3 KB) |
| region C free | 18,220 | 18,228 (layout unchanged) |
| stack worst path | 24,276 | 24,284 |

### Draw time (Renode, DBG:PROF?, 200 MIPS; a 5 s window from the page switch; on 5f9b5a0)

| | before | after |
|---|---:|---:|
| DMM page full draw (power -> DMM, `tick` max) | 17,145 us | 15,653 us (-8.7 %) |
| power page full draw (DMM -> power, `tick` max) | 11,257 us | 10,477 us (-6.9 %) |
| DMM reading redraw `d.read` avg / max (steady) | 646 / 4,891 us | 484 / 3,814 us |
| `d.read` avg / max right after the switch | 1,633 / 5,256 us | 1,506 / 4,553 us |
| DMM chart `d.chart` max (DMM / power page) | 1,001 / 3,627 us | 978 / 3,501 us |
| DMM page `tick` max, steady | 1,005 us | 982 us |

(Repeat runs of one image give the same maxima to the microsecond; Renode
counts instructions and does not model flash wait states, which are the same
before and after: the data stays in region B.) Why faster: 1 bpp glyphs skip
their blank rows, RLE4 runs fill several pixels per token, and `glyphs` now
walks a glyph row instead of computing every pixel's bit position.

### Pixel identity

* tools/font-matrix/run.sh: every glyph of every font (codes below U+2E80,
  W / k included) through label, glyphs, label_half, cells_diff and Ink::at
  at 4 aligns x 3 clips (inside, clipped at the top-left screen edge, at the
  bottom-right) x 2 spacings x 3 texts: 178,360 screen hashes, identical to
  the previous storage (5f9b5a0).
* tools/ui-render (UIR_HASH=<file>: a hash of the screen per checked case):
  24,309 case hashes identical, 155 renders identical, 50,700 cases,
  0 violations.
* tools/ui-render/stock-diff.sh: same per-region scores before and after,
  the 45 replica scenes pixel-identical.
* touch-test and every other tools/*/run.sh pass; emu/hy3131/wavetest.py and
  ad2streamtest.py (labels drawn while the AD2 ISR streams) PASS.
