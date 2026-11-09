# Fonts: the stock bitmap fonts, their storage and cost

The icons and the large digits and units of the readings are drawn with the
stock DM40 V2.6 bitmap fonts, the texts with bitmaps rendered from Noto Sans
and X11 misc-fixed at build time (section 1a); all are stored compressed in
the image.
This page describes where they come from, the storage format, where they
live in flash, and why the storage is lossless.

## 1. stockfont

The stock look is the baseline of the UI, so the other fonts are the stock ones.
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

## 1a. Text fonts: Noto Sans and X11 6x13

The text fonts are rendered at build time from two fonts that are not in the
repo; [`tools/fetch-fonts.sh`](../tools/fetch-fonts.sh) downloads them into
`target/` and checks their SHA-256:

* [Noto Sans](https://github.com/notofonts/latin-greek-cyrillic) 2.015, the
  variable `NotoSans[wdth,wght].ttf` of google/fonts (SIL OFL 1.1,
  [`fonts/OFL-NotoSans.txt`](../fonts/OFL-NotoSans.txt));
* X11 misc-fixed `6x13.bdf` of xorg font-misc-misc 1.1.3 (public domain,
  [`fonts/X11-misc-fixed.txt`](../fonts/X11-misc-fixed.txt)).

| font | draws | from |
|---|---|---|
| TEXT22, ASCII26 | menus, settings, the generator page, dialogs (one set of glyphs) | Noto Sans Medium, caps 14 px |
| HIST | the DMM history row | Noto Sans Medium, 87.5 % width, caps 13 px |
| F14 | small labels, MAX / AVG / MIN, the scale | X11 6x13, 1 bpp, caps 9 px, 6 px cells |
| SC_F16, SC_F12 | the scope menus, popups, bars and cells (one set) | Noto Sans SemiBold, caps 10 px |
| SC_C | the scope status letters | Noto Sans Bold, caps 12 px |
| F21, K_FONT, LETTERS | the units and letters of the readings | Noto Sans Medium, caps 20 / 22 px |

The big digits (F78, UNIT) and the icons stay stock.

[`tools/mkttf.py`](../tools/mkttf.py) renders the Noto places: FreeType light
autohinting at the em where H is the cap height, 2 bpp gray (coverage 0 / 5 /
10 / 15), whole-pixel advances, one advance for the digits where live
numbers are shown; the line box grows when the face's ascenders or
descenders do not fit the stock one. A font after the first of a place draws
with the first one's glyphs (`font_base` in `stockfont.rs`). 2 bpp is enough
for these sizes and halves the data of 4 bpp.

[`tools/mkterm.py`](../tools/mkterm.py) places the 6x13 BDF glyphs of F14 at
their native size (1 bpp, sharp on the grey REL box, where a 2 bpp face
looked blurred), the ink on the stock baseline. Our change: the period's
3 x 3 cross one row higher, on the baseline (it reached a row below the
others). The stock Ω stays (its feet on the baseline).

Both generators are called by `mkstockfont.py` and `mklang_x.py` (the
language glyphs in the same faces).

## 2. The stock fonts

26 font objects (class A: 12-byte entries with yoff / w / h / adv; class B:
8-byte entries, h = line_h; class C: fixed 1 bpp; B+: merged), 1005 glyphs.
Bit depth 1 bpp (10 fonts) or 4 bpp anti-aliased (16 fonts); the 4 bpp fonts
use all 16 coverage levels, so a lower bpp is not lossless. The stock stores
one column-major bit stream per glyph box: 88 497 B of bitmaps and 12 444 B
of entries.

What can be dropped or packed without changing a pixel:

* **Unused fonts.** The stock scope page's 20 px ASCII (0x08054148, the
  panel buttons) is not embedded: our panel and the strip's units too
  wide for SC_DIGITS ("mWh") use SC_F16, the same face at 16 px. Of
  G_08059428 only the up arrow (the generator's wave drop-down) is kept.
* **Unused glyphs.** 232 glyphs are CJK / full-width (U+2E80 up): 216 in
  TEXT22, 16 in ASCII26, 16.2 KB of bitmaps. opendm40 is English only and
  no source string contains one. The ASCII fonts (TEXT22, F14, ASCII26,
  SC_F12 / F16) draw flowing text (menus, dialogs, file names,
  SCPI-set values), so they keep all of 0x20..0x7F.
* **Padding.** Class B glyphs are line_h tall: 9.1 KB of blank rows and
  columns. Blank columns are part of the layout (w is the text width), so
  only rows can be cropped.
* **Run structure.** 4 bpp glyphs are mostly 0 and 15 with 1-3 AA pixels at
  each edge, and stems repeat row after row: row-major runs compress them
  2.5-5x (F78 5.1x).
* **Duplicates** across the stock fonts: 5 glyphs, ~0.4 KB after
  compression, not worth an indirection per glyph.
* **Shared glyphs.** The fonts of one Noto place (section 1a) store one
  set of glyphs: SC_F12 draws with SC_F16's, ASCII26 with TEXT22's,
  K_FONT with F21's (`font_base` in `stockfont.rs`), each placed in its own
  line box.

## 2a. Glyphs of our own

UNIT, the stock unit font without a lowercase k, takes ours
([`tools/mkwglyph.py`](../tools/mkwglyph.py), `stockfont_w.rs`): "kW",
"kΩ", "kHz" instead of the stock "KW". The Noto places (section 1a) draw
their W, k, Ω, ℃, ℉ and σ from the face.

## 3. Storage

`mkstockfont.py` emits:

* no glyph from U+2E80 up (`CJK_FROM`);
* 4 bpp glyphs as **HUF**: row tokens, no token crosses a row end, a glyph
  starts at a byte boundary. Tokens: `L(v)` one pixel of coverage 1..14;
  `F0(n)` / `F15(n)` n pixels of 0 / 15; `C(E)` as a row's first token: the
  row above again. A run length is a class (1..6, 7-8, 9-12, 13-20, 21-36,
  37-68) plus 0-5 extra bits, or `E` = to the end of the row. Each token is
  a canonical Huffman code from the table of its context (the previous
  token's kind in the row: row start, L, F0, F15), then the extra bits, MSB
  first. The 4 tables are shared by all 4 bpp fonts (`HUF_CNT`: codes per
  length, `HUF_SYM`: symbols in code order; 182 B). The generator parses each
  row optimally for the code lengths and alternates parse and Huffman
  construction six times;
* 1 bpp glyphs cropped to their ink rows t .. t + ih (column-major, ih bits
  per column);
* 10-byte entries `E(code u16, off u16, w, h, adv, yoff, t, ih)`; the full
  box (w, h) and the placement are unchanged;
* each glyph decoded back and compared with the stock bitmap (an assert in
  the generator).

Drawing reads rows top to bottom, so a HUF glyph keeps a 6-byte cursor
(`Cur`: bit index of the next token, of the last literal row, next row)
instead of a row index table; `row_runs` hands a row's runs straight to the
LCD writes (`label`, `glyphs`), `row` decodes into a 64-byte row buffer. A
code of up to 6 bits (95 % of the tokens) is one lookup in a 512-byte table
in SRAM, built from `HUF_CNT` / `HUF_SYM` at the first glyph lookup; longer
codes are decoded bit by bit. 1 bpp glyphs keep the direct column walk.
Random access (`alpha`) remains for `Ink::at` / the scope markers (1 bpp
fonts); on a HUF glyph it decodes from the top (correct, slow, not used by
any 4 bpp drawing path).

Result (with the 20 px scope font, since dropped: -1 317 B of bitmaps,
-960 B of entries): 27 142 B of bitmaps (4 bpp 18 843 B, was 24 123 B as RLE4 and
90 897 B raw), 182 B of tables, 8 690 B of entries. Against RLE4: region A2
+2 148 B free, region B +2 296 B (4 bpp data -5 280 B, tables +182 B,
decoder code +624 B), SRAM -512 B, static worst stack path unchanged. Draw time
(Renode, below) is a few per cent longer than RLE4.

### Draw time

[`DBG:PROF?`](../src/prof.rs) in the emulator, same scenario on each build
(DMM page with a DC input whose every digit changes every 0.3 s, five DMM
page entries, the scope page entry), and a temporary bench hook (not in
the tree) over every 4 bpp glyph: each row through `row_runs` only, and
each glyph through `label`. Renode
runs one instruction per cycle at 200 MIPS, so these figures are
instruction counts: no flash wait states, no pipeline or branch costs.

| | RLE4 | HUF |
|---|---:|---:|
| bench: decode, instructions per glyph row | 282 | 408 (+44 %) |
| bench: `label` of all 261 glyphs, k instructions | 6 734 | 7 608 (+13 %) |
| DMM reading redraw (`d.read` avg / max), us | 2 346 / 3 480 | 2 531 / 3 776 (+8 %) |
| DMM page entry (`tick` max), us | 19 060 | 19 954 (+5 %) |
| scope page entry (`tick` max; its texts are in the 1 bpp SC_ fonts), us | 86 318 | 86 415 (+0.1 %) |

Three or four runs per build, alternating: the DMM and bench figures
repeat to within 0.1 %.

Without the table (codes decoded bit by bit) a row took 814 instructions
and the DMM reading redraw 3 045 us.

### Where the fonts live

Every blob is a named array with its own section, so `llvm-nm` shows each
with its size: the tables in `.rodata.fonts.<NAME>_E`, the blobs in
`.rodata.fonts.<NAME>_B`, which the `.text` output section takes into
region B (zero-wait flash). ASCII26, F14, LETTERS, UNIT and LOCK (`A2_FONTS` in
`mkstockfont.py`) go to `.rodata.a2.<NAME>`, which
[`link-native.x`](../link-native.x) places in region A2 (zero-wait flash
after the shim, below the loader marker; [native-boot.md, Region
A2](native-boot.md#region-a2-2026-10-22)). Moving another blob to A2 is one
name in `A2_FONTS`; the link fails if A2 overlaps the marker. No font may go
to region C (not zero-wait): labels are drawn while the AD2 stream ISR runs.

## 3a. Localized images

[i18n/](../i18n/README.md), [`src/lang.rs`](../src/lang.rs): one language per
image (features `zh`, `ru`, `it`, `fr`), resolved at build time (`tr!`); none in
the default image.
Chinese: 14 px 1 bpp glyphs in one set for every font of 15 px and up, each hanzi
stored as U+0400 + its index. Russian: letters shaped like a Latin one use the
Latin glyph (and one byte of text); the others are rendered in the font's
face (Noto Sans, or X11 6x13 in F14) by [`mklang_x.py`](../tools/mklang_x.py).
Italian, French: the font's letter plus an accent.

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
| HUF contexts by the pixel above (0 / 15 / 1..14 / none, 20 tables) + `C(n)` partial copies from the row above | 1.1 KB more than HUF (4 bpp data 16 282 B, tables 794 B, code +860 B) | decode 763 instructions per row (2.7x RLE4), DMM reading redraw +53 %; the cursor holds the previous row (+32 B per glyph: `label` frame +1.5 KB), table 2.5 KB of SRAM ([feat/font-huffman-b](https://github.com/opendm40/opendm40/tree/feat/font-huffman-b)) |

Every lossy option saves at most ~6 KB of the ~36 KB of font data and
changes the look.

## 5. Checks

* [`tools/font-matrix/run.sh`](../tools/font-matrix/run.sh): every glyph of
  every font (W / k included) through `label`, `glyphs`, `label_half`,
  `cells_diff` and `Ink::at` at 4 aligns x 3 clips x 2 spacings x 3 texts;
  the screen hashes must match the previous storage.
* [`tools/ui-render`](../tools/ui-render) (`UIR_HASH=<file>`: a hash of the
  screen per checked case) and `tools/ui-render/stock-diff.sh` (per-region
  scores against the stock replica scenes, [tools/stock-ui](../tools/stock-ui/README.md)).
