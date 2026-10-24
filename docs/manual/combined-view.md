# Combined view: multimeter and scope together

The combined view shows the multimeter reading in a strip above the live
oscilloscope. Both measure at the same time: the multimeter on its V / A
jacks, the scope on **CHA**. The generator can run too.

To open it: main menu, **Combined view: DMM + Scope** (the bottom row), or
swipe left from the multimeter page.

![Combined view](img/combined.png)

## The meter strip

From the left:

- the function (**DC V**, **AC V**, **AC+DC**, **AutoDC** / **AutoAC**,
  **DC A**, **FREQ**, **TEMP**, **POWER** ...);
- one state badge, the most important of: **HOLD** / **A-HOLD**, an alert
  (**A-JACK** and others), **HV** (30 V and more), **REC**, **REL**, the
  limit verdict (**PASS**, **HI**, **LO**), the range mode (**MANU**,
  **AUTO+**, **AUTO**). An alert alternates with HOLD every second;
- the reading and its unit;
- up to two small values: the AC and DC parts, frequency and duty cycle,
  the burden voltage (**Bd**), the 4-20 mA percentage, the cold junction,
  or the voltage and current of the power meter;
- **GEN** and the frequency while the generator runs.

From 100 V the strip gets a red frame.

## Controls

| Action | What it does |
|---|---|
| Tap the left column of the strip | REL on / off. |
| Tap elsewhere on the strip | Opens the function band: the seven function buttons of the multimeter over the bottom of the graph. Tap a function as on the multimeter page. The band closes 4 s after the last tap, or with a tap elsewhere or MENU. |
| Long press the strip | The full multimeter page. |
| **RUN** | Multimeter HOLD; hold 1 s for AutoHold. |
| **AUTO** | Multimeter autorange. |
| **MF** | The multimeter's range page (or Hold & Limits). |
| Tap the run state in the top bar | Scope run / stop. |
| **TRIG** or the trigger in the top bar | The scope menu. |
| Swipes left / right | Change the view, also on the graph. |

The graph, the right panel and the scope menus work as on the
[oscilloscope page](scope.md). Scope auto-set is not available here: AUTO
belongs to the multimeter.

## Analog power

The DM40 has separate supplies for the multimeter and the scope /
generator. **Settings > Analog power**:

- **Auto (per view)** (default): the multimeter page powers the
  multimeter side, the scope and generator pages the scope side, the
  combined view both. The multimeter side also stays on while a recording,
  the power meter or a Bluetooth / USB client needs it; the scope side
  while the generator runs.
- **Both always**: both sides stay on in every view. Uses more battery.

While the multimeter side is off, the multimeter shows its last reading.

## Known limitations

- While the scope side is powered (combined view, scope page, generator on,
  or **Both always**), a fast square wave on the multimeter input reads
  about 0.2 % high. DC readings are not affected noticeably.
- The long press on the strip has not been tried on a real meter yet.
