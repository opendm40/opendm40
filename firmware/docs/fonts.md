# Fonts: the stock bitmap fonts, their storage and cost

Every text and icon on the opendm40 screens is drawn with the stock DM40 V2.6
bitmap fonts, copied into the image at build time and stored compressed.
This page describes where they come from, the storage format, where they
live in flash, and why the storage is lossless.

## 1. stockfont

The stock look is the baseline of the UI, so the fonts are the stock ones.
They used to be read live from the stock app at 0x0805xxxx..0x0806xxxx; once
opendm40 is flashed over 0x08004000 that app is gone, so
[`tools/mkstockfont.py`](../tools/mkstockfont.py) copies them out of
`dm40_v_2_6.dec.bin` (the decoded stock update file in the repo root, not in
the repo) into [`src/stockfont_data.rs`](../src/stockfont_data.rs).

[`src/stockfont.rs`](../src/stockfont.rs) is the renderer: layout (stock
placement rules), anti-aliased blend through a 16-step colour ramp, the stock
threshold compositing of the scope page (`Ink`), the half-size +1 digit
(`label_half`) and opendm40's own W / k glyphs
([`src/stockfont_w.rs`](../src/stockfont_w.rs)) for the power meter.

## 2. The stock fonts

27 font objects (class A: 12-byte entries with yoff / w / h / adv; class B:
8-byte entries, h = line_h; class C: fixed 1 bpp; B+: merged), 1101 glyphs.
Bit depth 1 bpp (11 fonts) or 4 bpp anti-aliased (16 fonts); the 4 bpp fonts
use all 16 coverage levels, so a lower bpp is not lossless. The stock stores
one column-major bit stream per glyph box: 90 897 B of bitmaps and 13 212 B
of entries.

What can be dropped or packed without changing a pixel:

* **Unused glyphs.** 232 glyphs are CJK / full-width (U+2E80 up): 216 in
  TEXT22, 16 in ASCII26, 16.2 KB of bitmaps. opendm40 is English only and
  no source string contains one. The ASCII fonts (TEXT22, F14, ASCII26,
  SC_F12 / F16 / F20) draw flowing text (menus, dialogs, file names,
  SCPI-set values), so they keep all of 0x20..0x7F.
* **Padding.** Class B glyphs are line_h tall: 9.1 KB of blank rows and
  columns. Blank columns are part of the layout (w is the text width), so
  only rows can be cropped.
* **Run structure.** 4 bpp glyphs are mostly 0 and 15 with 1-3 AA pixels at
  each edge, and stems repeat row after row: row-major runs compress them
  2.5-5x (F78 5.1x).
* **Duplicates** across fonts: 5 glyphs, ~0.4 KB after compression, not
  worth an indirection per glyph.

## 3. Storage

`mkstockfont.py` emits:

* no glyph from U+2E80 up (`CJK_FROM`);
* 4 bpp glyphs as **RLE4**: row-major nibble tokens, no run crosses a row
  end, byte-aligned per glyph: `1..14` one pixel of that coverage; `0|15, n`
  n + 1 pixels of 0|15 (n <= 13); `0|15, 14, hi, lo` 15 + (hi lo) pixels;
  `0, 15, k` at a row start: the last literal row k + 1 more times;
* 1 bpp glyphs cropped to their ink rows t .. t + ih (column-major, ih bits
  per column);
* 10-byte entries `E(code u16, off u16, w, h, adv, yoff, t, ih)`; the full
  box (w, h) and the placement are unchanged;
* each glyph decoded back and compared with the stock bitmap (an assert in
  the generator).

Drawing reads rows top to bottom, so an RLE4 glyph keeps a 6-byte cursor
(`Cur`: next token, start of the last literal row, pending repeats, next
row) instead of a row index table; `row_emit` decodes a row straight into
the LCD writes (`label`, `glyphs`), `row` into a 64-byte row buffer. 1 bpp
glyphs keep the direct column walk. Random access (`alpha`) remains for
`Ink::at` / the scope markers (1 bpp fonts); on an RLE4 glyph it decodes
from the top (correct, slow, not used by any 4 bpp drawing path).

Result on af4be54: 32 422 B of bitmaps and 8 690 B of entries (was 90 897 +
13 212 B), ~12.4 KB of renderer code. The RLE decode is also faster than
the raw bitmaps (Renode, DBG:PROF?: DMM page full draw -8.7 %, DMM reading
redraw avg 646 -> 484 us): blank rows are skipped and a run fills several
pixels per token.

### Where the fonts live

Every blob is a named array with its own section, so `llvm-nm` shows each
with its size: the tables in `.rodata.fonts.<NAME>_E`, the blobs in
`.rodata.fonts.<NAME>_B`, which the `.text` output section takes into
region B (zero-wait flash). ASCII26, F14, LETTERS, UNIT and SC_PROBE (`A2_FONTS` in
`mkstockfont.py`) go to `.rodata.a2.<NAME>`, which
[`link-native.x`](../link-native.x) places in region A2 (zero-wait flash
after the shim, below the loader marker; [native-boot.md, Region
A2](native-boot.md#region-a2-2026-10-22)). Moving another blob to A2 is one
name in `A2_FONTS`; the link fails if A2 overlaps the marker. No font may go
to region C (not zero-wait): labels are drawn while the AD2 stream ISR runs.

## 4. Options not taken

| option | saves | why not |
|---|---:|---|
| subset the ASCII fonts to the strings in src/ | ~3-5 KB | a new string with a missing glyph draws a gap |
| crop columns too | ~1 KB | needs an x offset field |
| snap coverage 1-2 -> 0, 13-14 -> 15 | ~2.5 KB | lossy (edge AA) |
| 2 bpp in the same RLE | 3-6 KB | visibly coarser edges on F78 / UNIT |
| big digits scaled from a smaller master | ~4 KB | visibly softer |
| re-rasterize an outline font at build time | ~0 | not the stock face (the stock fonts are bitmaps of an unknown face) |
| runtime vector rasterizer | negative | outlines ~15-30 KB + rasterizer 5-8 KB, 10-50x the draw time per pixel |

Every lossy option saves at most ~6 KB of the ~41 KB of font data and
changes the look.

## 5. Checks

* [`tools/font-matrix/run.sh`](../tools/font-matrix/run.sh): every glyph of
  every font (W / k included) through `label`, `glyphs`, `label_half`,
  `cells_diff` and `Ink::at` at 4 aligns x 3 clips x 2 spacings x 3 texts;
  the screen hashes must match the previous storage.
* [`tools/ui-render`](../tools/ui-render) (`UIR_HASH=<file>`: a hash of the
  screen per checked case) and `tools/ui-render/stock-diff.sh` (per-region
  scores against the stock replica scenes, [tools/stock-ui](../tools/stock-ui/README.md)).
