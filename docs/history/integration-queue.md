# Integration queue (hooks into shared files)

Empty. Everything queued here was integrated on 2026-10-13:

- touch.rs EXTI gating + DIAG bytes 20..59: dc50429
- SCPI over Bluetooth (two sessions, ScpiPort, DBG refusal; DIAG 60 / 64..67 = BT TX DMA ok /
  bytes dropped): eb3439b
- FR-05 rate / smoothing, UF-21, DMM rate chip, Settings page 2, Range page note: c67deb3
- Generator offset (p[39] W_GEN_OFFS), actual frequency, clamp note, 4-box page: 780a3fb
- csvlog wiring (swipe-down REC, LOG:CSV, MMEM:CAT?/DEL, finish on power-off / reset, M2 toast): 4b33f20
- hwtrig (H0..H8, DEFAULT_ENABLED = false): bf4fab4
- prose path fixes: abcb31b
- gallery (main-menu entry, View::Gallery): 16b6089

New hooks for shared files go below as before (exact code per entry; delete the entry once integrated).
