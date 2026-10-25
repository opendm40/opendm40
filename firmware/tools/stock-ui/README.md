# tools/stock-ui: reference renderer of the STOCK DM40 V2.6 scope UI

Spec: `stock/re/docs/stock-scope-ui.md`; plan: `docs/history/stock-scope-port-plan.md`;
per-function RE notes: `stock/re/stock-ui-notes/` (spec and notes in the private analysis repo).

* `run.sh [out] [scene..]` builds `main.rs` (+ `gfx.rs`, `scope.rs`, `check.rs`,
  `../ui-render/host.rs`) for the host, maps `dm40_v_2_6.dec.bin` at 0x08004000
  so every stock font is read live, renders the scene list to `out/*.png` (+ `@2x`),
  runs the owner's layout rules on each scene (`out/check.txt`; the stock
  layout is expected to fail them, see the plan sec. 8).
* `diff.py ours.png ref.bmp [-o diff.png] [--regions regions.json] [--tol N]
  [--align | --dx DX --dy DY] [--json out.json]`: RGB565-space pixel diff with
  per-region scores; reads the device's RGB565 BMPs (BI_BITFIELDS), 24-bit BMPs,
  PNG, PPM; 2x renders are scaled down.
* `dump_glyphs.py [--all]`: per-glyph PNGs + contact sheets of the scope fonts in
  `assets/glyphs/` (names `font_<addr>_<code>.png`; which code is which element:
  spec sec. 0.1 / 3).
* `regions.json`: the screen regions (shared by diff.py and the renderer).
* `ref/S000..S003.png`: the vendor's 2020 disk images converted from
  `private/spi-disk-backup/*/S00x.BMP` (older layout, 16 colours; spec sec. 9).
