# Frequency against the MMB3.0 calibrator, 1-2-5 steps (2026-10-25)

The owner stepped the MMB3.0 calibrator's frequency output from 2 Hz to 10 kHz while the meter (DM40A,
opendm40 0.1.0, Hz function) recorded to CSV ([freq-steps-2026-10-25.csv](freq-steps-2026-10-25.csv)). 2 Hz and
5 Hz gave no reading (the 350 ms gate; a fix is in progress), so the log starts at 10 Hz. The calibrator's
frequency accuracy is not known here. Per step: the median of the steady readings (the first one dropped).

| Calibrator set | Meter reading (median) | Delta | Delta, ppm | Readings | Spread (max-min) |
|---|---|---|---|---|---|
| 10 Hz | 9.99973 Hz | -0.00027 Hz | -27 | 11 | 0.00024 Hz |
| 20 Hz | 20.0007 Hz | +0.0007 Hz | +35 | 15 | 0.0005 Hz |
| 50 Hz | 50.0001 Hz | +0.0001 Hz | +2 | 15 | 0.0041 Hz |
| 100 Hz | 100.003 Hz | +0.003 Hz | +30 | 21 | 0.001 Hz |
| 200 Hz | 200.001 Hz | +0.001 Hz | +5 | 94 | 0.005 Hz |
| 500 Hz | 499.964 Hz | -0.036 Hz | -72 | 26 | 0.017 Hz |
| 1 kHz | 1000.05 Hz | +0.05 Hz | +50 | 21 | 0.02 Hz |
| 2 kHz | 2000.1 Hz | +0.1 Hz | +50 | 24 | 0.08 Hz |
| 5 kHz | 5000.27 Hz | +0.27 Hz | +54 | 27 | 0.07 Hz |
| 10 kHz | 10000.5 Hz | +0.5 Hz | +50 | 18 | 0.1 Hz |

From 1 kHz up the meter reads a constant +50 ppm high; that is one of the two time bases (the meter's or the
calibrator's) off by 50 ppm, which one is unknown. Below 1 kHz the deltas scatter between −72 and +35 ppm, at the
level of the shown resolution (6 significant digits).
