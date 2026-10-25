# Integration queue (hooks into shared files)

Empty. Everything queued here was integrated on 2026-10-13:

- touch.rs EXTI gating + DIAG bytes 20..59: 00f9373
- SCPI over Bluetooth (two sessions, ScpiPort, DBG refusal; DIAG 60 / 64..67 = BT TX DMA ok /
  bytes dropped): d06b423
- FR-05 rate / smoothing, UF-21, DMM rate chip, Settings page 2, Range page note: c7ec2c1
- Generator offset (p[39] W_GEN_OFFS), actual frequency, clamp note, 4-box page: 38d9983
- csvlog wiring (swipe-down REC, LOG:CSV, MMEM:CAT?/DEL, finish on power-off / reset, M2 toast): 7b1e0c0
- hwtrig (H0..H8, DEFAULT_ENABLED = false): a870070
- prose path fixes: 2be6f09
- gallery (main-menu entry, View::Gallery): 18ae461

New hooks for shared files go below as before (exact code per entry; delete the entry once integrated).
