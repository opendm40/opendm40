# Experimental 600 MΩ range

Ohm r7 (`OHM600` in [`src/dmm.rs`](../src/dmm.rs)). Manual only: the range
page's **600MΩ experimental** item and SCPI `RESistance:RANGe` above 60 MΩ
select it. The autorange, the AUTO key's short-press range steps, `RES:RANG
MAX` and a restored setting never do. The screen marks it with **EXP**
before the test current. There is no accuracy label.

## Method

The stock 60 MΩ row is a current source with an absolute reference (~20 nA,
*inferred* from the stock Rref constant 63 M and REFO 1.25 V). At 600 MΩ it
would need ~12 V; the source gives 3.2 V at most. The stock 6 MΩ row
(Rref 10 MΩ, ratiometric) cannot take a ratio of 60 between the voltage
across Rx and across Rref either.

r7 is the 6 MΩ row (`CFG_F10_P8CLR[5]`) with three bytes changed:

| byte | value | |
|---|---|---|
| R22 | 0x16 | AD1 200 ms (10 mains periods at 50 Hz) |
| R28 | 0x31 | AD1 reference PB6 − AGND (the MAX6190, absolute) |
| R31 SFUVR | 3 | OP3 on AGNDP9, ≈1.05 V above AGND |

AD1 runs OP1O − PB3 throughout. Two phases switch only R32, OP1's input:

| phase | R32 | OP1+ | AD1 reads |
|---|---|---|---|
| A | 0x3C | PB0 (the jack through 900 k) | Vx, the jack against COM |
| B | 0x39 | FB (OP3's side of Rref) | Vfb |

R = 10 MΩ · gain · Vx / (Vfb − Vx − off). Both phases go through the same
OP1, AD1 input and reference, so the reference's value and OP1's offset
cancel. Per phase `skip` (2) results are dropped after the switch and the
median of `keep` (3) is taken: one reading per ~2 s. The display averages the
last 4 readings and starts over on a step above 2 %. The first reading after
a row write or a step is grey. OL: Vfb − Vx − off ≤ 0, or R above 618 MΩ.
LIVE?: the jack below COM, or above FB.

Format 600.0 MΩ (6000 counts). Test current label: **EXP I 105 nA**, the
AGNDP9 short-circuit current through 10 MΩ
([short.csv](data/short.csv), `AGNDP9,R10M`).

## Why AGNDP9

V/Ω open, R10M path, `DBG:SRC` with the jack read through OP1O, current =
(FB − jack) / 10 MΩ ([leak-open-op1o.csv](data/ohm600m-2026-10-28/leak-open-op1o.csv)):

| OP3 tap | FB (V) | jack (V) | current into the jack |
|---|---|---|---|
| AGNDP9 | 1.04746 | 1.04754 | −8 pA (the method's limit, ±10 pA) |
| AGNDP0 | 0.09112 | 0.09617 | −0.50 nA |
| VDS1 | −0.97378 | −0.95787 | −1.59 nA |
| VDS15 | 2.198 | 2.483 | – (OP1O clips at ~2.48 V) |

*Inferred* from these points: about 1.3–1.9 GΩ from the jack to a node at
≈1.05 V. With OP3 at that level, the leak is a constant share of the current,
Rref / R_leak ≈ 0.7 %, a gain error. With the stock VDS15 (2.1 V) it would
be ~20 % at 600 MΩ and depend on Rx.

Read through PB0 directly (no OP1), the jack seems to sit at 1.05–1.2 V
whatever the source ([leak-open-pb0.csv](data/ohm600m-2026-10-28/leak-open-pb0.csv)):
PB0 itself is loaded when AD1 reads it without OP1.

## Why OP1 on FB

The first version read Vfb on AD1 directly (R24 FB − PB3). On the open jacks
Vfb − Vx was then +0.45 … +0.49 mV, 2.8 % of the 17 mV drop at 600 MΩ. On
5.57 MΩ, FB through OP1 and FB on AD1 directly differ by 5.7 µV
([phases-5m57.csv](data/ohm600m-2026-10-28/phases-5m57.csv), configs B2 and
B1), so the 0.47 mV was not OP1's offset. What it was is *unknown*: a leak of
~47 pA or the jack node not settled after the row write (10 MΩ behind it).

## Device checks (2026-10-28, one unit)

Prototype with `DBG:HY:CAP?` on the 6 MΩ row (the r7 bytes, the two phases),
nominal Rref, gain 1, no offset
([cap-blocks.txt](data/ohm600m-2026-10-28/cap-blocks.txt),
[phases-5m57.csv](data/ohm600m-2026-10-28/phases-5m57.csv)):

| resistor (another meter) | stock 6 MΩ | stock 60 MΩ | r7 method |
|---|---|---|---|
| 0.9925 MΩ | 0.9938 MΩ | – | 0.9945 MΩ |
| 5.5693 MΩ | 5.5756 MΩ | 5.5721 MΩ | 5.5701 MΩ |

Per conversion (200 ms): Vfb sd 1–7 µV, Vx 6–35 µV. The first result after
a switch can be off by up to 4 mV (the step from 0.37 V to 1.05 V on OP1's
input); later results drift by ~20 µV over 2 s.

Not checked: any resistor above 6 MΩ, the noise and settling at hundreds of
MΩ, the leak's stability (humidity, temperature).

## Tuning without a reflash

`DBG:OHM6 gain,off_uV,skip,keep` sets the RAM values (an empty field keeps
its value); `DBG:OHM6?` answers `gain,off_uV,skip,keep,Vx,Vfb` (V, the last
phase medians). Defaults 1, 0, 2, 3.

## Emulator

[`emu/hy3131/ohm600mtest.py`](../../emu/hy3131/ohm600mtest.py): the range
rules (explicit, MAX, by value, AUTO, the AUTO key). The HY3131 model reads
the ohm rows ratiometrically only (no absolute AD1 reference, SOP1P masked),
so the r7 value is not modelled.
