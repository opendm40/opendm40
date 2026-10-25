# Security and safety reports

opendm40 runs on a multimeter that is used on mains voltages. A firmware bug
can be a safety problem, so some bugs should not be public before they are
fixed.

## What to report privately

- A reading, a range or an overload indication that is wrong in a way that
  can make a dangerous voltage or current look safe.
- A wrong-connection alert or the danger alarm that does not fire when it
  should ([Safety and alerts](docs/manual/README.md#safety-and-alerts) says
  what each alert covers, and
  [What cannot be detected](docs/manual/README.md#what-cannot-be-detected)
  what none of them can).
- An update file, a USB disk write, a SCPI command or a Bluetooth message
  that leaves the meter unable to start or to reach the update loader, or
  that changes the calibration.
- A way for a computer or a Bluetooth device to run its own code on the
  meter or to read data it should not.

Everything else: a normal [issue](https://github.com/wirenboard/opendm40/issues).

## How

Use **Report a vulnerability** on the repository's
[Security tab](https://github.com/wirenboard/opendm40/security). Say the
model, the opendm40 version (**Settings > More > More > Device**) and how to
reproduce it.

## Versions

Fixes go into the next release; only the latest
[release](https://github.com/wirenboard/opendm40/releases/latest) is
supported. To go back to the stock firmware meanwhile:
[Back to the stock firmware](docs/manual/README.md#back-to-the-stock-firmware).
