# Remote control: Bluetooth, apps and software

opendm40 can be controlled and read from a phone or a computer:

| Link | What you can use |
|---|---|
| Bluetooth, phone app | The Alientek phone app for the DM40, as with the stock firmware. |
| Bluetooth, serial | SCPI commands, the same as over USB (debug commands excluded). |
| USB | SCPI over USBTMC, the standard link of lab instruments; plus the USB disk. |

Step-by-step setup for each computer system: [Connecting from a
computer](connecting-from-a-computer.md). All commands: [SCPI
reference](scpi-reference.md).

## Bluetooth

Bluetooth is on by default. To switch it off: **Settings > Bluetooth**.
While a phone or a computer is connected, the Bluetooth symbol shows in the
status bar.

The meter's Bluetooth module appears twice in a scan:

- **DM40A** (or B, C): the Bluetooth Low Energy device the phone app uses.
- **C-1-ATK-DM40-CALI**: the classic Bluetooth serial port (SPP) for SCPI
  from a computer. Use RFCOMM channel 1.

The module takes one connection at a time: close the phone app before you
connect from a computer.

### The phone app

opendm40 speaks the same Bluetooth protocol as the stock firmware, so the
Alientek phone app shows the readings and can switch functions, HOLD and
REL. The protocol is described in [docs/bt-protocol.md](../bt-protocol.md).

Differences from the stock firmware:

- The stock firmware powers Bluetooth only on its multimeter page. opendm40
  keeps it on in every view, and the multimeter keeps measuring while a
  phone is connected, also on the scope and generator pages.
- In the power meter, the app shows the power as the main value.

### SCPI over Bluetooth

Any line of text that starts with a letter, `*` or `:` is taken as an SCPI
command; the phone-app protocol keeps working on the same link. Replies end
with a newline.

Limits of the Bluetooth link:

- The link runs at 115 200 baud, about 11 kB/s. A reading takes 0.1 to
  0.4 s; a screen-sized scope waveform about 0.1 s. The full scope record
  is refused (too much data for the link).
- Anyone in range can connect, so the commands that can change the firmware,
  restart the meter or delete files are refused over Bluetooth:
  all `DBG:` commands and `MMEM:DELete`. `SYST:ERR?` then says
  `-100,"Command protected"`.
- Streaming with the `LOG:` commands works up to 800 samples/s; faster
  rates need USB.

The simplest client is
[`firmware/tools/dm40bt_scpi.py`](../../firmware/tools/dm40bt_scpi.py)
(Linux, no setup):

```
/usr/bin/python3 firmware/tools/dm40bt_scpi.py scpi '*IDN?'
/usr/bin/python3 firmware/tools/dm40bt_scpi.py scpi 'CONF:VOLT:DC' 'READ?'
/usr/bin/python3 firmware/tools/dm40bt_scpi.py shell
```

It finds the meter with `bluetoothctl`; `--mac` sets the address directly.
It needs a Python with Bluetooth sockets (the system `/usr/bin/python3` on
Linux).

More: binding `/dev/rfcomm0` and using PyVISA over Bluetooth are in
[Connecting from a computer](connecting-from-a-computer.md#bluetooth).

## Software for the computer

| Software | How |
|---|---|
| Python | PyVISA, or the tools in [`firmware/tools/`](../../firmware/tools/). See [Connecting from a computer](connecting-from-a-computer.md). |
| sigrok (sigrok-cli, PulseView, SmuView) | See [Connecting from a computer, sigrok](connecting-from-a-computer.md#sigrok-sigrok-cli-pulseview-smuview). |

## Known limitations

The Alientek phone app itself has not been tried with opendm40 yet; its
protocol has been checked with a computer. While a fast data stream runs,
the meter does not answer the phone-app protocol.
