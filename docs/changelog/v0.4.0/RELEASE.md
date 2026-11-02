opendm40 v0.4.0, the replacement firmware for the Alientek DM40 (A / B / C).
This release reworks the scope page around channels: the multimeter and the
generator are now scope channels next to CH1, with a protocol decoder for
UART, Modbus RTU and DALI. On the multimeter side it adds a 600 MΩ range,
a SHORT band for continuity and σ in the statistics, and Settings now has
tabs.

## Highlights

**Scope: a block per channel.** The right panel has a block for CH1, DMM,
GEN and MATH, each with its V/div and zoom-in / zoom-out magnifiers; a tap
on the title opens the channel or switches it on and off. The graph reaches
the left edge of the screen.

![Before and after: the scope page](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/scope-panel.png)

**The multimeter and the generator on the scope.** CH2 (DMM) draws the
multimeter's input over CH1 on the same time axis; the GEN trace shows the
generator's output on its own V/div. **Source GEN** triggers on the
generator with no wire.

![All four channels: CH1, DMM, GEN and ABS math](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/channels.png)

**Protocol decoder.** UART (RS-232, RS-485, with Modbus RTU frames found by
their CRC) and DALI, decoded under the trace, also in the Wave view.

![The decoder band: UART and Modbus RTU](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/decoder.png)

**Decoder Mode Log** lists every frame of a running stream; a tap on a
frame shows its waveform.

![Mode Log and a frame's waveform](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/decode-log.png)

**Scope: 5 and 2 mV/div,** a digital zoom of 10 mV/div for small signals.

![2 mV/div](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/mv-div.png)

**Drag as on a Rigol DHO800/900.** Drag a marker to move it; anywhere else,
sideways moves the trigger position and up / down the selected trace.

![Drag zones on the scope page](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/drag.png)

**Extra digit at full size.** The reading starts further left, so the
other digits stay put when the sixth digit comes or goes.

![Before and after: the extra digit](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/extra-digit.png)

**Wave chip and σ.** In V and A the **Wave** chip on the trend chart opens
the scope on the multimeter inputs; the statistics column adds the standard
deviation.

![The Wave chip and the σ row](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/wave-sigma.png)

**Settings in tabs:** Display, Alerts, Power, Multimeter, Scope, Other.

![Before and after: Settings](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/settings.png)

**600 MΩ range,** experimental, on the range page only.

![The 600 MΩ range page and reading](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/ohm-600m.png)

**Continuity: a green SHORT band** and green digits while the beeper
sounds, also at volume 0 %.

![Continuity before and with a short](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/cont-short.png)

**Record button on the range pages:** MF, then **Record**.

![Before and after: the current range page](https://raw.githubusercontent.com/opendm40/opendm40/v0.4.0/docs/changelog/v0.4.0/record-button.png)

Also new: each function button remembers its sub-mode, **Mode Remember**
restores the multimeter and the scope after a power-off, a longer scope
capture (up to 65 520 samples, 262 144 in roll), **Record On change**, and
the README and the manual in Russian.

## Fixes

- A firmware update keeps the multimeter function and the scope settings.
- Capacitance: large capacitors (1 .. 47 mF) read in 1.5 to 5 s instead of
  9 to 20 s; AUTO picks the right range after a large capacitor.
- Scope STOP keeps a complete triggered record at every timebase.
- A-jack detection in more functions, and no false alarms from a fast
  signal on V-COM.
- The charging state, the USB and Bluetooth symbols and the recording dot
  in the status bar.
- Bluetooth and USB SCPI: no lost or misrouted replies; the documented
  error codes for out-of-range numbers.
- The Device page shows the bootloader and hardware versions the right way
  round.

## Install

Copy `dm40_opendm40_v0.4.0.bin` from the assets below onto the meter's USB
disk and switch the meter off and on; see
[Install](https://github.com/opendm40/opendm40/blob/v0.4.0/docs/manual/README.md#install)
in the manual. Back up `INFO/` from the meter's disk after the first start.
To try a change before the next release, each open pull request has a
**Test build** comment with its update file.

Full list of changes: [CHANGELOG.md](https://github.com/opendm40/opendm40/blob/v0.4.0/CHANGELOG.md).
