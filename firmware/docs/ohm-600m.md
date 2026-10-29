# Experimental 600 MΩ range (ohm r7)

A resistance range above the stock 60 MΩ, selected only explicitly. Code:
[`src/ohm600m.rs`](../src/ohm600m.rs) (row, phase plan, maths, `DBG:OHMX`
parser; host test [`tools/ohm600m-test`](../tools/ohm600m-test/main.rs)),
[`src/ohm600m_dmm.rs`](../src/ohm600m_dmm.rs) (the poll and `DBG:OHMX`),
the range rules in [`src/dmm.rs`](../src/dmm.rs) (`OHM_EXP`). The gain is
**not calibrated**: the [calibration plan](#device-calibration-plan) below is
for the owner's session on the device.

## Range rules

| Path | What it does on 600 MΩ |
|---|---|
| MF range page | **600MΩ EXP**, a ninth item below the grid, after AUTO (`ui::range_items`) |
| AUTO key, short press | never steps to it (the cycle is the items before AUTO, `ui::range_key_next`); from 600 MΩ it goes to 60 Ω |
| Autorange | never: AUTO chosen on 600 MΩ (AUTO key held, the page's AUTO, `RES:RANG:AUTO ON`, BT) continues on 60 MΩ (`dmm::ohm_auto_range`) |
| REL / HOLD released | stays on 600 MΩ, manual (`Dmm::hold_range`) |
| SCPI | `RES:RANG <x>` / `CONF:RES <x>` with 60 MΩ < x ≤ 600 MΩ; `MAX` is 60 MΩ (`scpi::range_max_item`); `RES:RANG?` 6E8, `RES:RES?` 1E5 |
| Bluetooth | reported as 60 MΩ (the stock protocol has no field for it); the byte echoed back keeps 600 MΩ |
| Settings | not stored: the settings keep the function, not the range, so ohm starts in AUTO after power-up (as every range; nothing to restore) |
| Screen | status bar **600MΩ**, **EXP** (orange) beside the test current label **I 105 nA**; readout 600.0 MΩ; no accuracy label (the DM40 manual has no row) |
| Rate (FAST / MED / SLOW) | ignored: the range has its own conversion plan; the rate row shows "n/a here" |

## Why not the stock rows

- 600 Ω .. 6 MΩ are ratiometric: OP3 drives a PA-pin reference resistor Rref
  in series with Rx; AD1 input OP1O − PB3 (OP1 buffers PB0), reference
  FB − RLU. 6 MΩ is stock row `CFG_F10_P8CLR[5]`
  ([`src/hy3131_tables.rs`](../src/hy3131_tables.rs)): Rref 10 MΩ, SFT1
  pre-filter 100 k, OP3 tap SFUVR = 2 (VDS15, ~2.1 V).
- 60 MΩ is a current-source row with the REFO absolute reference (stock Rref
  constant 63 M, ~20 nA, *inferred*). 600 MΩ at that current needs ~12 V;
  the source's compliance is 3.2 V.
- 6 MΩ's row at 600 MΩ: the drop on Rref is ~1/60 of the jack voltage,
  beyond AD1's ratiometric range.

## Method

The 6 MΩ row with OP3's tap at SFUVR = 3 (AGNDP9, ≈ 1.05 V vs AGND). At
600 MΩ: I ≈ 1.7 nA, drop on Rref ≈ 17 mV, V_jack ≈ 1.03 V. Both voltages are
converted against the absolute reference N = PB6 − AGND (1.25 V MAX6190, as
the `DBG:SRC` phases of [`src/dbgsrc.rs`](../src/dbgsrc.rs)) in alternate
conversions:

    R = gain_cal · GAIN · RREF · (V_jack − V_com) / (V_FB − V_jack − OFFS)

- `gain_cal`: the meter's 6 MΩ calibration gain (record slot 0x124 + 4·5),
  the same Rref; a starting point (*to be calibrated*, `GAIN`).
- Row changes against the stock row: R31 SFUVR, R23 AD1 reference buffers
  off (as `DBG:SRC`), R22 = 0x16 (OSR 16384 chopped, 200 ms), R24 / R28 per
  phase. Relay, PA / DS / FS switches, OP1, SFT1 stay the stock row's.
- Phases (`PLAN` bits; each switch: ENAD1 off, R24 / R28, ENAD1 on, `SKIP`
  conversions dropped, one kept):

  | Bit | Phase | AD1 input / reference | Use |
  |---|---|---|---|
  | 1 | X | OP1O − AGND / N | the jack (through OP1) |
  | 2 | F | FB − AGND / N | OP3's side of Rref |
  | 4 | C | PB3 − AGND / N | COM; without it V_com = 0 |
  | 8 | Q | OP1O − AGND / FB − AGND | jack / FB in one conversion: no X-to-F offset; R from q, V_FB from F or nominal 1.0476 V |
  | 16 | D | FB − RLU / N | the stock pair, diagnostic only (not used for R) |

  Default `PLAN 3` (X, F): 4 conversions per cycle with `SKIP 1`, ~0.8 s.
  SAD1FN cannot select OP1O (datasheet §12.3, the AD1FN mux: SENSE, RLU,
  VSSA, AGND, PB2..PB5), so FB − OP1O is not available as one input pair;
  Q is the one-conversion alternative (*untested*).
- Readings: one per complete cycle, the mean jack / drop of the last `NAVG`
  cycles (default 4, ~3.2 s), R from the ratio of the means. Provisional
  (grey) until two consecutive cycles agree within `VET` · R + 0.5 MΩ and
  `SETTLE` ms (default 3000) have passed since the row write; a cycle that
  disagrees with the one before restarts the window. OL above 618 MΩ (DM40C
  over-range 1.03) or with no positive drop (open leads).
- Sentinels: as the other ohm rows (`sentinel::row_for` takes mode 10 the
  same for every range): A-JACK on the PB1 comparator, the jack check, and
  LIVE? when V_jack − V_com < −3.3 mV (the `LIVE_CODE` 0.24 % of AD1's
  1.389 V, *inferred* equivalent). [power-meter-and-misconnect.md](power-meter-and-misconnect.md) §4.
- Test current label: 105 nA, the AGNDP9 short-circuit current on the 10 MΩ
  path in [`data/short.csv`](data/short.csv) (V_drop / Rref, one unit).

## Device data (one unit, 2026-10-28)

Leakage at the V/Ω node, jacks open, R10M path (`DBG:SRC`, jack read through
OP1O, current = (FB − jack) / 10 MΩ):

| OP3 tap | FB (V) | jack (V) | leak |
|---|---|---|---|
| AGNDP9 | 1.04746 | 1.04754 | −8 pA (method limit ±10 pA) |
| AGNDP0 | 0.09112 | 0.09617 | −0.50 nA |
| VDS1 | −0.97378 | −0.95787 | −1.59 nA |
| VDS15 | 2.198 | 2.483 (OP1O clips ~2.48 V: invalid) | – |

- *Inferred* model: ~1.3–1.9 GΩ from the jack to a node at ≈ 1.05 V. With
  the source at AGNDP9 the leak is a near-constant fraction ≈ Rref / R_L ≈
  0.7 % of the current, a gain error the calibration absorbs (the host test
  models 1.5 GΩ: 100 .. 600 MΩ within 0.2 % of each other); with VDS15 it
  would be ~20 % and depend on Rx. Hence AGNDP9.
- PB0 read directly (no OP1) is wrong here: PB0 is pulled to ~1.05–1.2 V by a
  load of a few MΩ (1.0486 V on PB0 with a 0.09 V source). The method reads
  OP1O.
- In the R10M row FB − RLU read 0.86 V with the jacks open at AGNDP9 while
  FB ≈ jack: RLU is not simply the jack side of Rref there. The method uses
  FB − AGND and OP1O − AGND (and PB3) as separate absolute conversions.
- Between two separate conversions offsets up to ~0.1 mV were seen: 0.6 % of
  a 17 mV drop. `OFFS` takes a constant one out; Q avoids it.
- Ohm-row input-referred noise ≈ 6 µV rms per 200 ms conversion (shorted
  jacks, 600 Ω and 60 kΩ rows): ≈ 0.035 % ≈ 0.2 MΩ rms at 600 MΩ per
  conversion. Hence 600.0 MΩ (0.1 MΩ step) and the 4-cycle window; a
  60000-count format is not realistic.

## Tuning on the device (DBG:OHMX)

USB only, RAM only (defaults after a restart; `DBG:OHMX DEF` restores them).
A change on 600 MΩ writes the row again (the window and the settle restart).

| Knob | Default | Range | Meaning |
|---|---|---|---|
| `TAP` | 3 (AGNDP9) | 0..15 | OP3 SFUVR code (`dbgsrc::VREF_NAMES`) |
| `PLAN` | 3 | 1..31, Q or X + F | phase bits (table above) |
| `SKIP` | 1 | 0..4 | conversions dropped after a phase switch |
| `OSR` | 0x16 | byte | R22 |
| `NAVG` | 4 | 1..8 | cycles in the window |
| `SETTLE` | 3000 | 0..60000 ms | provisional after a row write |
| `VET` | 0.01 | 0..1 | relative agreement of two cycles (+ 0.5 MΩ) |
| `GAIN` | 1.0 | 0.5..2 | on top of the 6 MΩ calibration gain |
| `OFFS` | 0 | ±0.05 V | subtracted from the drop |
| `RREF` | 1E7 | 1E6..1E8 Ω | nominal Rref |
| `ROW,<reg>,<val>` | – | R20..R35, 4 slots | row byte override (R24 / R28 stay the plan's); `ROW,CLR` |

`DBG:OHMX?`: `tap,plan,skip,osr,navg,settle,vet,gain,offs,rref,cal;`
`on,cycles,ok,V_x,V_fb,V_com,V_drop,V_d,R_cycle,R_window;` the last cycle's
five AD1 codes (X, F, C, Q, D; 0 = not in the plan); the row (R20..R35 hex);
the overrides.

## Device calibration plan

Needs: 100 MΩ, ~300 MΩ and 1 GΩ resistors (a 1 GΩ one checks OL), each
measured on another meter that has a range there; short leads, guarded or
held in air (a hand or the bench nearby changes a 600 MΩ reading).

1. `RES:RANG 600E6`; check the status bar **600MΩ**, **EXP**, I 105 nA;
   `DBG:OHMX?` → `on` 1, `cycles` counting, `V_fb` ≈ 1.05 V.
2. Jacks open, `OFFS 0`: `V_drop` is the X-to-F offset plus the leak
   (−8 pA ≈ −0.08 mV at AGNDP9): set `OFFS` to the mean `V_drop` over ~20
   cycles; the reading is OL.
3. Leads shorted: R ≈ 0 (`V_x` ≈ V_com).
4. 100 MΩ then 300 MΩ (or 470 MΩ): `GAIN` = reference / reading × the
   present `GAIN`; check the other resistor, and that the ratio does not
   depend on R (if it does, the leak is not a constant fraction: try `TAP`,
   or measure the leak again with `DBG:SRC`).
5. Settling: from open to the resistor, count the provisional cycles; set
   `SETTLE` / `VET` so the first black reading is within the noise.
6. Try `PLAN 10` (Q + F) and compare its scatter and offset with `PLAN 3`.
7. Put the found `GAIN` / `OFFS` (and any plan change) into the defaults
   (`Knobs::DEF`) in a follow-up; a per-unit gain belongs in the calibration
   record, which has no 600 MΩ slot (open question).

## Open questions

- Settling time: *unknown*. *Inferred*: the jack node sees Rref ∥ Rx ≈ 10 MΩ,
  so τ ≈ 10 MΩ × the node capacitance (100 pF → 1 ms; the 26 nF pre-filter
  capacitor is behind the 100 k from OP1O, not on the jack:
  [hy3131-frontend-2026-10-26.md](hy3131-frontend-2026-10-26.md) §1). The
  generous `SETTLE` stays until a device check.
- Whether the X-to-F offset is constant (temperature, time) or must be
  measured per reading (an internal zero phase).
- Where the gain should live per unit (no slot in the calibration record).
- Whether BT apps need a 600 MΩ range field (the stock protocol has none).
