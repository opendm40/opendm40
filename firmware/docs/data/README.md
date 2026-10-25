# Accuracy against calibrators (2026-10-25)

Measurements of one DM40A running opendm40 against two calibrators (an MMB3.0 and an MR9270), with the raw CSV files
in this directory. Each table gives the set value, the meter's median reading per step, the delta and, where the
calibrator's specification is known, its tolerance. A delta is meter minus calibrator, not the meter's error alone.

## Summary

| Function | Range | Result |
|---|---|---|
| DC V | 6 V .. 60 V | within ±0.015 % (inside the calibrator's tolerance) |
| DC V | 600 mV | gain −0.007 %, offset −5.5 µV (inside the calibrator's tolerance) |
| DC V | 60 mV | gain −0.073 %, offset +6 µV; from 30 mV up outside the calibrator's tolerance (1.3x .. 2.5x) |
| DC A | 1 mA .. 20 mA | within −0.009 % .. −0.034 % (inside the calibrator's tolerance) |
| DC A | 1 µA .. 500 µA | inside the calibrator's ±2 µA, which is too coarse to test these steps |
| Frequency | 10 Hz .. 10 kHz | +50 ppm from 1 kHz up; ~+43 ppm also against the MR9270, so the meter's time base (inferred) |
| Resistance | 0.01 Ω .. 4 kΩ | −0.02 .. −0.03 Ω offset up to 200 Ω; −0.006 % .. −0.012 % from 1 kΩ |

## Contents

- [DC V, 1-2-5 steps](#dc-v-against-a-source-1-2-5-steps-2026-10-25)
- [DC V, 60 mV and 600 mV ranges](#dc-v-on-the-60-mv-range-against-the-mmb30-calibrator-2026-10-25)
- [DC A](#dc-a-against-the-mmb30-calibrator-1-2-5-steps-2026-10-25)
- [Frequency](#frequency-against-the-mmb30-calibrator-1-2-5-steps-2026-10-25)
- [Resistance](#resistance-against-the-mmb30-calibrator-res-steps-2026-10-25csv)

## DC V against a source, 1-2-5 steps (2026-10-25)

The owner stepped an MMB3.0 calibrator through 1, 2, 5, 10 ... in each decade, from 1 mV to 10 V, while the meter
(DM40A, opendm40 0.1.0, DC V, manual range, rate MED) recorded to CSV on the Record page.
Raw logs: [dcv-steps-2026-10-25-a.csv](dcv-steps-2026-10-25-a.csv) (1 mV .. 500 mV) and
[dcv-steps-2026-10-25-b.csv](dcv-steps-2026-10-25-b.csv) (1 V .. 10 V).

Per step: the median of the steady readings (the first two of each step and the transition readings
dropped; a step is a run of readings within 0.02 % or 3 uV of each other). The calibrator's specification
(23 ± 5 °C, 1 year, ±(% of reading + offset)): 100 mV range (−10 .. 110 mV) ±(0.01 % + 10 µV);
1 V range (0 .. 1.2 V) ±(0.01 % + 0.1 mV); 10 V range (0 .. 12 V) ±(0.01 % + 2 mV). Its current source (for
later current checks): 20 mA range (0 .. 24 mA, 1 µA resolution) ±(0.02 % + 2 µA), compliance 19 V. The delta is the meter
minus the set value; "Within" says whether it lies inside the calibrator's tolerance alone (the meter's own
specification is not applied). Which calibrator range served each step is inferred from the set value.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | Readings | Spread (max-min) |
|---|---|---|---|---|---|---|---|
| 1 mV | 1.00434 mV | +0.0043 mV | +0.434 % | ±10.1 µV | yes | 7 | 0.11 uV |
| 2 mV | 2.00358 mV | +0.0036 mV | +0.179 % | ±10.2 µV | yes | 4 | 0.13 uV |
| 5 mV | 5.00037 mV | +0.0004 mV | +0.007 % | ±10.5 µV | yes | 18 | 0.42 uV |
| 10 mV | 9.99588 mV | -0.0041 mV | -0.041 % | ±11.0 µV | yes | 12 | 0.35 uV |
| 20 mV | 19.98870 mV | -0.0113 mV | -0.056 % | ±12.0 µV | yes | 10 | 0.20 uV |
| 50 mV | 49.96400 mV | -0.0360 mV | -0.072 % | ±15.0 µV | **no** (2.4x) | 35 | 0.50 uV |
| 100 mV | 99.98070 mV | -0.0193 mV | -0.019 % | ±20.0 µV | yes | 20 | 4.00 uV |
| 100 mV | 99.99470 mV | -0.0053 mV | -0.005 % | ±20.0 µV | yes | 21 | 9.80 uV |
| 200 mV | 200.14600 mV | +0.1460 mV | +0.073 % | ±120.0 µV | **no** (1.2x) | 29 | 18.00 uV |
| 500 mV | 499.61300 mV | -0.3870 mV | -0.077 % | ±150.0 µV | **no** (2.6x) | 39 | 48.00 uV |
| 1 V | 1.00014 V | +0.14 mV | +0.014 % | ±200.0 µV | yes | 12 | 10.00 uV |
| 2 V | 1.99991 V | -0.09 mV | -0.004 % | ±2.20 mV | yes | 17 | 10.00 uV |
| 5 V | 4.99969 V | -0.31 mV | -0.006 % | ±2.50 mV | yes | 25 | 30.00 uV |
| 10 V | 9.99883 V | -1.17 mV | -0.012 % | ±3.00 mV | yes | 28 | 670.00 uV |

Notes:

- 100 mV shows two steady steps (0.0999807 V for 8 s, then 0.0999948 V for 9 s) with no gap between them; which
  setting changed (the range, the source) is not known; both are listed.
- After 500 mV the recording ends in OL (the manual range); the 1 V .. 10 V steps are the second recording.

### From 10 µV ([dcv-steps-2026-10-25-c.csv](dcv-steps-2026-10-25-c.csv))

The same procedure from 10 µV to 50 mV (a third recording). Below 1 mV the readings sit about +5 .. +8 µV above the
set value, a constant offset rather than a gain error; it is inside the calibrator's ±10 µV offset term. The set value of
each step is inferred from the order (10 µV, 20 µV, 50 µV, ...); the two "20 µV?" rows between 20 µV and 50 µV
are steady levels whose setting is unknown. Two levels at 1 mV and at 10 mV, as at 100 mV in the first table.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | Readings | Spread (max-min) |
|---|---|---|---|---|---|---|---|
| 10 µV | 16.673 µV | +6.67 µV | +66.73 % | ±10.0 µV | yes | 14 | 0.26 µV |
| 20 µV | 26.000 µV | +6.00 µV | +30.00 % | ±10.0 µV | yes | 24 | 0.43 µV |
| 20 µV? | 28.188 µV | +8.19 µV | +40.94 % | ±10.0 µV | yes | 10 | 0.57 µV |
| 20 µV? | 25.963 µV | +5.96 µV | +29.81 % | ±10.0 µV | yes | 6 | 0.25 µV |
| 50 µV | 56.360 µV | +6.36 µV | +12.72 % | ±10.0 µV | yes | 24 | 0.29 µV |
| 100 µV | 106.008 µV | +6.01 µV | +6.01 % | ±10.0 µV | yes | 26 | 0.23 µV |
| 200 µV | 204.893 µV | +4.89 µV | +2.45 % | ±10.0 µV | yes | 32 | 1.24 µV |
| 500 µV | 507.790 µV | +7.79 µV | +1.56 % | ±10.1 µV | yes | 97 | 2.31 µV |
| 1 mV | 1.00471 mV | +4.71 µV | +0.47 % | ±10.1 µV | yes | 22 | 0.14 µV |
| 1 mV | 1.00674 mV | +6.74 µV | +0.67 % | ±10.1 µV | yes | 20 | 0.24 µV |
| 2 mV | 2.00482 mV | +4.82 µV | +0.24 % | ±10.2 µV | yes | 23 | 0.24 µV |
| 5 mV | 5.00200 mV | +2.00 µV | +0.04 % | ±10.5 µV | yes | 26 | 0.20 µV |
| 10 mV | 9.99776 mV | -2.24 µV | -0.02 % | ±11.0 µV | yes | 22 | 0.36 µV |
| 10 mV | 10.00010 mV | +0.10 µV | +0.00 % | ±11.0 µV | yes | 49 | 0.27 µV |
| 20 mV | 19.99120 mV | -8.80 µV | -0.04 % | ±12.0 µV | yes | 40 | 2.20 µV |
| 50 mV | 49.96700 mV | -33.00 µV | -0.07 % | ±15.0 µV | **no** (2.2x) | 38 | 2.40 µV |

### 100 mV .. 10 V again ([dcv-steps-2026-10-25-d.csv](dcv-steps-2026-10-25-d.csv))

A fourth recording, same calibrator settings, the meter on one manual range for the whole run (which range is not
in the log; the readings' resolution suggests 6 V, inferred). Against the first table, 200 mV and 500 mV now agree
with the calibrator within its tolerance; the first recording's 200 mV / 500 mV deltas (+0.073 % / −0.077 %) were
then on another meter range, presumably 600 mV (inferred). So those deltas point at the meter's 600 mV range, not at
the calibrator.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | First recording | Readings | Spread (max-min) |
|---|---|---|---|---|---|---|---|---|
| 100 mV | 100.0250 mV | +25.0 µV | +0.0250 % | ±20.0 µV | **no** (1.2x) | 99.9807 mV / 99.9948 mV | 11 | 7.0 µV |
| 200 mV | 200.0190 mV | +19.0 µV | +0.0095 % | ±120.0 µV | yes | 200.1460 mV | 14 | 6.0 µV |
| 500 mV | 499.9970 mV | -3.0 µV | -0.0006 % | ±150.0 µV | yes | 499.6130 mV | 15 | 13.0 µV |
| 1 V | 1.00000 V | +0.0 µV | +0.0000 % | ±200.0 µV | yes | — | 10 | 10.0 µV |
| 2 V | 1.99996 V | -40.0 µV | -0.0020 % | ±2.20 mV | yes | — | 23 | 10.0 µV |
| 5 V | 4.99976 V | -240.0 µV | -0.0048 % | ±2.50 mV | yes | — | 18 | 30.0 µV |
| 10 V | 9.99885 V | -1150.0 µV | -0.0115 % | ±3.00 mV | yes | — | 33 | 540.0 µV |

## DC V on the 60 mV range against the MMB3.0 calibrator (2026-10-25)

The owner stepped the MMB3.0 calibrator (its 100 mV range: ±(0.01 % + 10 µV)) through 0.01 .. 0.10 mV, 0.1 .. 2.0 mV
in 0.1 mV steps, 2 .. 20 mV in 1 mV steps and 20, 30, 40, 50, 60, 61 mV, with the meter (DM40A, opendm40 0.2.0,
DC V, the manual 60 mV range) recording to CSV ([dcv-60mv-2026-10-25.csv](dcv-60mv-2026-10-25.csv)). Per step: the
median of the steady readings (the first two dropped); the set value of each step is inferred from the readings and
the order. One steady block at 1.0068 mV between the 0.09 mV and 0.10 mV steps is left out (its setting is not
known); 16 mV gave no steady block.

A least-squares line through all points: reading = +0.0060 mV + 0.999267 × set, i.e. an offset of +6.0 µV and a gain
error of -0.073 %. "Residual" is the reading minus that line; it shows the non-linearity after offset and gain.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | Residual from the line |
|---|---|---|---|---|---|---|
| 0.02 mV | 0.0189 mV | -1.1 µV | -5.329 % | ±10.0 µV | yes | -7.0 µV |
| 0.03 mV | 0.0280 mV | -2.0 µV | -6.529 % | ±10.0 µV | yes | -7.9 µV |
| 0.04 mV | 0.0378 mV | -2.2 µV | -5.597 % | ±10.0 µV | yes | -8.2 µV |
| 0.05 mV | 0.0474 mV | -2.6 µV | -5.289 % | ±10.0 µV | yes | -8.6 µV |
| 0.06 mV | 0.0587 mV | -1.3 µV | -2.220 % | ±10.0 µV | yes | -7.3 µV |
| 0.07 mV | 0.0683 mV | -1.7 µV | -2.490 % | ±10.0 µV | yes | -7.7 µV |
| 0.08 mV | 0.0777 mV | -2.3 µV | -2.915 % | ±10.0 µV | yes | -8.3 µV |
| 0.09 mV | 0.0873 mV | -2.7 µV | -2.945 % | ±10.0 µV | yes | -8.6 µV |
| 0.1 mV | 0.1080 mV | +8.0 µV | +7.975 % | ±10.0 µV | yes | +2.1 µV |
| 0.2 mV | 0.2070 mV | +7.0 µV | +3.493 % | ±10.0 µV | yes | +1.2 µV |
| 0.3 mV | 0.3080 mV | +8.0 µV | +2.663 % | ±10.0 µV | yes | +2.2 µV |
| 0.4 mV | 0.4070 mV | +7.0 µV | +1.762 % | ±10.0 µV | yes | +1.4 µV |
| 0.5 mV | 0.5082 mV | +8.2 µV | +1.632 % | ±10.1 µV | yes | +2.5 µV |
| 0.6 mV | 0.6070 mV | +7.0 µV | +1.174 % | ±10.1 µV | yes | +1.5 µV |
| 0.7 mV | 0.7079 mV | +7.9 µV | +1.133 % | ±10.1 µV | yes | +2.5 µV |
| 0.8 mV | 0.8069 mV | +6.9 µV | +0.862 % | ±10.1 µV | yes | +1.5 µV |
| 0.9 mV | 0.9077 mV | +7.7 µV | +0.858 % | ±10.1 µV | yes | +2.4 µV |
| 1 mV | 1.0070 mV | +7.0 µV | +0.700 % | ±10.1 µV | yes | +1.8 µV |
| 1.1 mV | 1.1082 mV | +8.2 µV | +0.744 % | ±10.1 µV | yes | +3.0 µV |
| 1.2 mV | 1.2068 mV | +6.8 µV | +0.570 % | ±10.1 µV | yes | +1.7 µV |
| 1.3 mV | 1.3080 mV | +8.0 µV | +0.612 % | ±10.1 µV | yes | +2.9 µV |
| 1.4 mV | 1.4070 mV | +7.1 µV | +0.504 % | ±10.1 µV | yes | +2.1 µV |
| 1.5 mV | 1.5079 mV | +7.9 µV | +0.529 % | ±10.2 µV | yes | +3.0 µV |
| 1.6 mV | 1.6069 mV | +6.9 µV | +0.429 % | ±10.2 µV | yes | +2.1 µV |
| 1.7 mV | 1.7077 mV | +7.7 µV | +0.455 % | ±10.2 µV | yes | +3.0 µV |
| 1.8 mV | 1.8066 mV | +6.6 µV | +0.368 % | ±10.2 µV | yes | +2.0 µV |
| 1.9 mV | 1.9078 mV | +7.8 µV | +0.408 % | ±10.2 µV | yes | +3.2 µV |
| 2 mV | 2.0069 mV | +6.9 µV | +0.346 % | ±10.2 µV | yes | +2.4 µV |
| 3 mV | 3.0059 mV | +5.9 µV | +0.197 % | ±10.3 µV | yes | +2.1 µV |
| 4 mV | 4.0061 mV | +6.1 µV | +0.151 % | ±10.4 µV | yes | +3.0 µV |
| 5 mV | 5.0039 mV | +3.9 µV | +0.078 % | ±10.5 µV | yes | +1.6 µV |
| 6 mV | 6.0031 mV | +3.1 µV | +0.052 % | ±10.6 µV | yes | +1.5 µV |
| 7 mV | 7.0032 mV | +3.2 µV | +0.046 % | ±10.7 µV | yes | +2.4 µV |
| 8 mV | 8.0025 mV | +2.5 µV | +0.032 % | ±10.8 µV | yes | +2.4 µV |
| 9 mV | 9.0005 mV | +0.5 µV | +0.006 % | ±10.9 µV | yes | +1.2 µV |
| 10 mV | 9.9999 mV | -0.1 µV | -0.001 % | ±11.0 µV | yes | +1.2 µV |
| 11 mV | 10.9995 mV | -0.5 µV | -0.004 % | ±11.1 µV | yes | +1.6 µV |
| 12 mV | 11.9992 mV | -0.8 µV | -0.007 % | ±11.2 µV | yes | +2.0 µV |
| 13 mV | 12.9968 mV | -3.2 µV | -0.025 % | ±11.3 µV | yes | +0.3 µV |
| 14 mV | 13.9968 mV | -3.2 µV | -0.023 % | ±11.4 µV | yes | +1.1 µV |
| 15 mV | 14.9964 mV | -3.6 µV | -0.024 % | ±11.5 µV | yes | +1.4 µV |
| 17 mV | 16.9935 mV | -6.5 µV | -0.038 % | ±11.7 µV | yes | -0.0 µV |
| 18 mV | 17.9930 mV | -7.0 µV | -0.039 % | ±11.8 µV | yes | +0.2 µV |
| 19 mV | 18.9928 mV | -7.2 µV | -0.038 % | ±11.9 µV | yes | +0.7 µV |
| 20 mV | 19.9929 mV | -7.1 µV | -0.035 % | ±12.0 µV | yes | +1.6 µV |
| 30 mV | 29.9833 mV | -16.7 µV | -0.056 % | ±13.0 µV | **no** (1.3x) | -0.7 µV |
| 40 mV | 39.9756 mV | -24.4 µV | -0.061 % | ±14.0 µV | **no** (1.7x) | -1.1 µV |
| 50 mV | 49.9688 mV | -31.2 µV | -0.062 % | ±15.0 µV | **no** (2.1x) | -0.5 µV |
| 60 mV | 59.9605 mV | -39.5 µV | -0.066 % | ±16.0 µV | **no** (2.5x) | -1.5 µV |
| 61 mV | 60.9598 mV | -40.2 µV | -0.066 % | ±16.1 µV | **no** (2.5x) | -1.5 µV |

### The 600 mV range ([dcv-600mv-2026-10-25.csv](dcv-600mv-2026-10-25.csv))

The same calibrator at 100 .. 600 mV in 100 mV steps, the meter on the manual 600 mV range (opendm40 0.2.0). The
calibrator's 100 mV range serves 100 mV (±(0.01 % + 10 µV)), its 1 V range the rest (±(0.01 % + 0.1 mV)); which
calibrator range served 100 mV is inferred from its specification limits.

A least-squares line: reading = -0.0055 mV + 0.999933 × set (offset -5.5 µV, gain -0.0067 %). So the 600 mV range's gain
is about ten times closer than the 60 mV range's (−0.073 %), and every step is inside the calibrator's tolerance.
The first DC V recording's +0.073 % at 200 mV and −0.077 % at 500 mV ([the first table](#dc-v-against-a-source-1-2-5-steps-2026-10-25))
do not repeat here; which meter range and calibrator setting they came from is unknown.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | Residual from the line |
|---|---|---|---|---|---|---|
| 100 mV | 99.9875 mV | -12.5 µV | -0.0125 % | ±20.0 µV | yes | -0.2 µV |
| 200 mV | 199.9830 mV | -17.0 µV | -0.0085 % | ±120.0 µV | yes | +2.0 µV |
| 300 mV | 299.9720 mV | -28.0 µV | -0.0093 % | ±130.0 µV | yes | -2.3 µV |
| 400 mV | 399.9680 mV | -32.0 µV | -0.0080 % | ±140.0 µV | yes | +0.4 µV |
| 500 mV | 499.9600 mV | -40.0 µV | -0.0080 % | ±150.0 µV | yes | -0.8 µV |
| 600 mV | 599.9550 mV | -45.0 µV | -0.0075 % | ±160.0 µV | yes | +0.9 µV |

## DC A against the MMB3.0 calibrator, 1-2-5 steps (2026-10-25)

The owner stepped the MMB3.0 calibrator's current source through 1, 2, 5, 10 ... from 1 µA to 20 mA, while the
meter (DM40A, opendm40 0.1.0, DC A, rate MED) recorded to CSV on the Record page: 1 µA .. 5 mA with AUTO
([dca-steps-2026-10-25-a.csv](dca-steps-2026-10-25-a.csv)), 10 mA and 20 mA on a manual range
([dca-steps-2026-10-25-b.csv](dca-steps-2026-10-25-b.csv)).

Per step: the median of the steady readings (the first two of each step and the transition readings dropped).
Calibrator specification (23 ± 5 °C, 1 year): 20 mA range (0 .. 24 mA, 1 µA resolution) ±(0.02 % + 2 µA). With a
2 µA offset term, the calibrator alone cannot test the µA steps: every delta below about 10 µA is inside its tolerance
by construction. The delta is the meter minus the set value; "Within" compares it with the calibrator's tolerance only.
The set value of each step is inferred from the order of the steps.

| Calibrator set | Meter reading (median) | Delta | Delta, % | Calibrator tolerance | Within | Readings | Spread (max-min) |
|---|---|---|---|---|---|---|---|
| 1 µA | 0.8712 µA | -0.1288 µA | -12.881 % | ±2.000 µA | yes | 28 | 0.0418 µA |
| 2 µA | 2.0205 µA | +0.0205 µA | +1.027 % | ±2.000 µA | yes | 29 | 0.0061 µA |
| 5 µA | 5.0798 µA | +0.0798 µA | +1.595 % | ±2.001 µA | yes | 27 | 0.0045 µA |
| 10 µA | 10.0454 µA | +0.0454 µA | +0.454 % | ±2.002 µA | yes | 18 | 0.0035 µA |
| 20 µA | 19.9695 µA | -0.0305 µA | -0.153 % | ±2.004 µA | yes | 19 | 0.0046 µA |
| 50 µA | 50.1314 µA | +0.1314 µA | +0.263 % | ±2.010 µA | yes | 18 | 0.0058 µA |
| 100 µA | 100.0810 µA | +0.0810 µA | +0.081 % | ±2.020 µA | yes | 25 | 0.0060 µA |
| 200 µA | 200.0900 µA | +0.0900 µA | +0.045 % | ±2.040 µA | yes | 15 | 0.0030 µA |
| 500 µA | 499.9100 µA | -0.0900 µA | -0.018 % | ±2.100 µA | yes | 19 | 0.0050 µA |
| 1 mA | 0.99966 mA | -0.3400 µA | -0.034 % | ±2.200 µA | yes | 50 | 0.0310 µA |
| 2 mA | 1.99955 mA | -0.4500 µA | -0.022 % | ±2.400 µA | yes | 32 | 0.0300 µA |
| 5 mA | 4.99884 mA | -1.1600 µA | -0.023 % | ±3.000 µA | yes | 43 | 0.0200 µA |
| 10 mA | 9.99905 mA | -0.9500 µA | -0.009 % | ±4.000 µA | yes | 24 | 0.1800 µA |
| 20 mA | 19.99810 mA | -1.9000 µA | -0.009 % | ±6.000 µA | yes | 20 | 0.1000 µA |

## Frequency against the MMB3.0 calibrator, 1-2-5 steps (2026-10-25)

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

## Resistance against the MMB3.0 calibrator ([res-steps-2026-10-25.csv](res-steps-2026-10-25.csv))

The calibrator's resistance output stepped from 0.01 Ω to 4 kΩ (4 kΩ instead of 5 kΩ at the end), the meter (opendm40
0.1.0, Resistance, manual range, changed by the owner during the run) recording to CSV. The calibrator's resistance
specification is not known here. Readings 0 Ω are the meter's clamp of negative results (R < 0 shows 0). The 500 Ω
step read OL for 32 s on the 600 Ω range; that is analysed in [PR #7](https://github.com/wirenboard/opendm40/pull/7)
(the firmware holds no OL; the calibrator at 500 Ω and 0.90 mA is the likely cause, inferred), so it is left out.

| Calibrator set | Meter reading (median) | Delta |
|---|---|---|
| 0.01 Ω | 0 Ω | -0.0100 Ω |
| 0.02 Ω | 0 Ω | -0.0200 Ω |
| 0.05 Ω | 0.0296 Ω | -0.0204 Ω |
| 0.1 Ω | 0.0817 Ω | -0.0183 Ω |
| 0.2 Ω | 0.1787 Ω | -0.0213 Ω |
| 0.5 Ω | 0.4738 Ω | -0.0262 Ω |
| 1 Ω | 0.9714 Ω | -0.0286 Ω |
| 2 Ω | 1.9744 Ω | -0.0256 Ω |
| 5 Ω | 4.9797 Ω | -0.0203 Ω |
| 10 Ω | 9.9727 Ω | -0.0273 Ω |
| 20 Ω | 19.9773 Ω | -0.0227 Ω |
| 50 Ω | 49.9785 Ω | -0.0215 Ω |
| 100 Ω | 99.9738 Ω | -0.0262 Ω |
| 200 Ω | 199.968 Ω | -0.0320 Ω |
| 1000 Ω | 999.951 Ω | -0.0490 Ω |
| 2000 Ω | 1999.88 Ω | -0.1200 Ω |
| 4000 Ω | 3999.76 Ω | -0.2400 Ω |

Up to 200 Ω the meter reads 0.02 .. 0.03 Ω low, a constant offset (zero or lead resistance difference, which one is
unknown). From 1 kΩ up the deltas are −0.006 % .. −0.012 %.
