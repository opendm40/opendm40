# Remote control: Bluetooth and the phone app

opendm40 can be read and controlled from a phone or a computer:

| Link | What you can use |
|---|---|
| Bluetooth, phone app | The Alientek phone app for the DM40. |
| Bluetooth, serial | SCPI commands from a computer, as over USB (debug commands excluded). |
| USB | SCPI over USBTMC, the standard link of lab instruments; plus the USB disk. |

Setting up a computer (USB or Bluetooth, Linux, Windows, macOS, PyVISA,
sigrok): [Connecting from a computer](connecting-from-a-computer.md). All
commands: [SCPI reference](scpi-reference.md).

## Bluetooth

Bluetooth is on by default; switch it off in **Settings > Bluetooth**. While
a phone or a computer is connected, the Bluetooth symbol shows in the status
bar.

The meter's Bluetooth module appears twice in a scan:

- **DM40A** (or B, C): the Bluetooth Low Energy device the phone app uses.
- **C-1-ATK-DM40-CALI**: the classic Bluetooth serial port (SPP) for SCPI
  from a computer, RFCOMM channel 1.

The module takes one connection at a time: close the phone app before you
connect from a computer.

## The phone app

opendm40 speaks the stock firmware's Bluetooth protocol, so the Alientek
phone app shows the readings and can switch functions, HOLD and REL. The
protocol is described in [docs/bt-protocol.md](../bt-protocol.md).

- Bluetooth stays on in every view, and the multimeter keeps measuring while
  a phone is connected, also on the scope and generator pages. (The stock
  firmware powers Bluetooth only on its multimeter page.)
- In the power meter, the app shows the power as the main value.

## SCPI over Bluetooth

A line of text that starts with a letter, `*` or `:` is taken as an SCPI
command; the phone-app protocol keeps working on the same link. Replies end
with a newline.

Anyone in range can connect, so commands that can change the firmware,
restart the meter or delete files work over USB only. The link is also
slower than USB. The details:
[SCPI reference, Bluetooth differences](scpi-reference.md#bluetooth-differences).

## Known limitations

- Untested: the Alientek phone app itself with opendm40.
- While a fast data stream runs, the meter does not answer the phone-app
  protocol.
