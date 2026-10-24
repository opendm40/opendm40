# Integration queue (hooks into shared files)

Empty. Everything queued here was integrated on 2026-10-13:

- touch.rs EXTI gating + DIAG bytes 20..59: 043da5c
- SCPI over Bluetooth (two sessions, ScpiPort, DBG refusal; DIAG 60 / 64..67 = BT TX DMA ok /
  bytes dropped): 4c2f0a6
- FR-05 rate / smoothing, UF-21, DMM rate chip, Settings page 2, Range page note: 0cb58e8
- Generator offset (p[39] W_GEN_OFFS), actual frequency, clamp note, 4-box page: 8cf32aa
- csvlog wiring (swipe-down REC, LOG:CSV, MMEM:CAT?/DEL, finish on power-off / reset, M2 toast): a1a7e74
- hwtrig (H0..H8, DEFAULT_ENABLED = false): 4046e8f
- prose path fixes: 940d02d
- gallery (main-menu entry, View::Gallery): 2fcf080

New hooks for shared files go below as before (exact code per entry; delete the entry once integrated).
