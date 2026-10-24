# What's new compared with the stock firmware

opendm40 replaces the stock firmware V2.6. It keeps its look, its touch
control and its Bluetooth phone-app protocol, and it leaves the bootloader,
the update loader and the calibration untouched. This page lists what is
different. Details are on the pages linked.

## Everything at once

| Stock | opendm40 |
|---|---|
| One instrument at a time; only the generator keeps running with the scope. | The multimeter, the scope and the generator run at the same time; the screen only chooses what you see. |
| — | [Combined view](combined-view.md): the multimeter reading above the live scope. |
| — | Swipe left / right between the multimeter, the combined view, the scope and the generator. |

## Multimeter

| | Stock | opendm40 |
|---|---|---|
| Counts | By model: 40000 (DM40A), 50000 (B), 60000 (C) | 60000 on every model; ranges 600 mV, 6 V ... |
| AC+DC | DM40B and C only; the first value can be computed from the DC part alone | Every model; shown only when both parts are measured, with the AC and DC parts |
| AUTO coupling | — | Shows the DC or the AC part, whichever the signal is |
| Volt autorange | Starts on mV | **AUTO+** by default: never steps down to mV |
| Speed and resolution | Fixed | FAST / MED / SLOW; an extra digit (600000 counts) in MED and SLOW |
| Smoothing | — | Optional |
| AutoHold, limits (OK / HI / LO), AVG, statistics session | — | Yes |
| Bargraph | — | 50 segments, by default |
| Temperature | K thermocouple, no offset | Also PT100, PT1000, three NTCs and a user NTC; offset per sensor; °F factor corrected |
| 4-20 mA % | Always shown, also negative | Labelled, shown only in the useful range |
| 60 Ω range | — | Manual range with milliohm resolution |
| Burden voltage | — | Optional |
| Function change | The old value or 0.0000 under the new unit | Grey dashes at once, grey until the value is final |
| History while HOLD | Cannot store the held value | Stores it |

See [Multimeter](multimeter.md).

## New instruments

- [Power meter](power-meter.md): P, S, Q, PF, frequency, Wh, VAh; DC, AC or
  AC+DC; the DC and AC parts.
- [Inrush current](power-meter.md#inrush-current): the RMS of the first
  100 ms and the peak of a switch-on current, saved to the disk.
- [Wave view](scope.md#wave-view-the-scope-on-the-multimeter-inputs): the
  oscilloscope on the isolated multimeter inputs.

## Safety

| Stock | opendm40 |
|---|---|
| Only the high-voltage warning (red from 30 V, lightning from 100 V) | Also alerts for wrong connections (A-JACK, A-PROT, V-JACK?, LIVE?, hints), protection of the small current shunt, a loud danger alarm, the "Leads in A jack?" reminder, a fuse check |

See [Safety and alerts](safety-and-alerts.md).

## Oscilloscope

| | Stock | opendm40 |
|---|---|---|
| Top bar | Not touchable | Tap the run state, timebase, V/div, trigger |
| Timebase by touch | Tap the left or right half | Corner zones with labels (or the stock halves), a list, pinch |
| V/div buttons | The top "+" made the trace smaller | Magnifier icons: the top one zooms in |
| Trigger position | Always the centre | Drag it |
| Triggers | Edge | Edge, pulse width, runt |
| Auto-set | Switches AC coupling to DC | Keeps AC coupling |
| Cursors | Through the Measure tab | One tap on **CUR** |
| FFT | No scale | Scale, centre frequency and peak readout; Flat-top window |
| Capture length | 65 520 samples | 26 624 samples |

See [Oscilloscope](scope.md).

## Generator

| | Stock | opendm40 |
|---|---|---|
| Sine / triangle / ramp above 50 kHz | Output stops without a message | Runs at 50 kHz, with a note |
| Shown frequency | The typed value | The real output frequency |
| Ramp | Falls, against its icon | Rises |
| Amplitude, offset | 0.5 to 3 Vpp, fixed offset | 0 to 3 Vpp, adjustable offset, DC output |
| Runs | With the scope page | On every page |

See [Signal generator](generator.md).

## Data, USB and Bluetooth

| Stock | opendm40 |
|---|---|
| No logging | The [Record page](logging.md) (every reading, or means per 1 s to 1 min, for hours) and a fast recorder |
| USB disk only in the **USB Connect** mode | The disk and a SCPI instrument at the same time, with CSV files, screenshots and backups |
| — | SCPI over USB (USBTMC) and Bluetooth: [Connecting from a computer](connecting-from-a-computer.md), [SCPI reference](scpi-reference.md) |
| Phone app over Bluetooth on the multimeter page | Phone app in every view, plus SCPI on the same link |
| — | A [gallery](usb-disk.md#screenshots-and-the-gallery) of screenshots and recordings on the meter |

## Other

- Touch lock, gestures (3-finger screenshot, 2-finger power-off) and the
  uptime counter as on the stock firmware.
- A warning 30 s before the automatic power-off.
- On-screen help (**Settings > More > Help**).
- The Device page shows the opendm40 version and the board temperature.

## What the stock firmware has and opendm40 does not

- Chinese menus: opendm40 is English only.
- The longer scope capture (65 520 samples).
