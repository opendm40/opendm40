# Connecting from a computer

One USB cable gives the computer two things at once:

- a **USB disk** with your recordings, screenshots and backups
  ([USB disk](usb-disk.md)); it needs no driver on any system;
- a **measuring instrument** (USBTMC, the standard USB class of lab
  instruments) that takes SCPI commands ([SCPI reference](scpi-reference.md)).

Bluetooth carries the same SCPI commands without a cable
([below](#bluetooth)).

The examples send `*IDN?` and read a DC voltage. The `*IDN?` reply looks
like this:

```
Alientek,DM40A,0123456789ABCDEF01234567,opendm40 0.1
```

The fields: maker, model (DM40A, DM40B or DM40C), the meter's serial number
(24 hex digits, the chip's unique ID; also on the USB disk as
`INFO/UID.BIN`), the firmware and its version.

## What the meter looks like on USB

| | |
|---|---|
| USB ID | `1209:0001` |
| Product | `DM40 multimeter (opendm40)` |
| Serial number | 24 hex digits, the same as in `*IDN?` |
| Interface 0 | USBTMC / USB488 (class FE, subclass 03, protocol 01) |
| Interface 1 | USB mass storage, the disk **DM40** |

`1209:0001` is a test ID of the pid.codes project. The rules and driver
bindings below use it.

Every reply ends with a newline. Allow **5 s** for a reply: a reading takes
up to about 0.85 s, and the first reading after a function change up to
about 4 s.

## Linux

### Check the connection

```
$ lsusb -d 1209:0001
Bus 003 Device 012: ID 1209:0001 ... 
$ ls /dev/usbtmc*
/dev/usbtmc0
```

The kernel's `usbtmc` driver creates `/dev/usbtmcN` by itself. The disk
appears as a normal USB drive.

### Access without root

Create `/etc/udev/rules.d/70-dm40.rules`:

```
# Alientek DM40 with opendm40: USBTMC node and libusb access
SUBSYSTEM=="usbmisc", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="0001", TAG+="uaccess"
SUBSYSTEM=="usb", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="0001", TAG+="uaccess"
```

Then `sudo udevadm control --reload` and re-plug the meter. The first line
is for `/dev/usbtmcN`, the second for programs that use libusb (pyvisa-py,
sigrok). This rule is untested.

### The simplest way: dm40scpi.py

The repository's [`firmware/tools/dm40scpi.py`](../../firmware/tools/dm40scpi.py)
needs only Python 3. It finds the meter's `/dev/usbtmcN` by itself. Keep it
next to `dm40dbg.py` in the same folder.

```
$ python3 firmware/tools/dm40scpi.py scpi '*IDN?'
Alientek,DM40A,0123456789ABCDEF01234567,opendm40 0.1
$ python3 firmware/tools/dm40scpi.py scpi 'CONF:VOLT:DC' 'READ?'
$ python3 firmware/tools/dm40scpi.py read -n 5 --conf dcv
```

More commands of the tool:

| Command | What it does |
|---|---|
| `info` | Identity and state. |
| `conf dcv` (also `acv`, `acdcv`, `dci`, `aci`, `acdci`, `res`, `cont`, `diode`, `cap`, `freq`, `temp`) | Choose the function. |
| `read -n 10 -i 0.5` | Read 10 values, one every 0.5 s. |
| `log out.csv -i 1 -d 60` | Log one value per second for 60 s to a CSV file. |
| `get NAME -o FILE` | Copy a file from the meter's disk. |
| `scpi 'CMD' ...` | Send any SCPI commands. |

If you have more than one meter, choose one with `DM40_USBTMC=/dev/usbtmc1`.

### Without any tool

The usbtmc driver turns one write into one command and one read into one
reply:

```
$ echo '*IDN?' > /dev/usbtmc0
$ head -1 /dev/usbtmc0
Alientek,DM40A,0123456789ABCDEF01234567,opendm40 0.1
```

Untested in this form; the tools above work this way.

### Python with PyVISA

```
pip install pyvisa pyvisa-py pyusb
```

```python
import pyvisa
rm = pyvisa.ResourceManager('@py')
dmm = rm.open_resource(rm.list_resources('USB?*::0x1209::0x0001::?*')[0],
                       read_termination='\n', write_termination='\n', timeout=5000)
print(dmm.query('*IDN?'))
dmm.write('CONF:VOLT:DC')
print(float(dmm.query('READ?')))
```

The resource name is `USB0::0x1209::0x0001::<serial>::INSTR`. pyvisa-py
talks to the meter through libusb and takes the interface away from the
kernel driver: `/dev/usbtmcN` disappears until you re-plug the meter. Use
either PyVISA or the `/dev/usbtmcN` tools, not both at the same time.

The PyVISA example is untested as a whole.

## Windows

The USB disk works without a driver. For SCPI, Windows has no USBTMC driver
of its own: the device manager lists the instrument under **Other devices**
with code 28. Choose one:

- **NI-VISA** or **Keysight IO Libraries Suite**: both install a USBTMC
  driver. Then use `pyvisa.ResourceManager()` (without `'@py'`). Untested.
- **Zadig + pyvisa-py**: in [Zadig](https://zadig.akeo.ie/), **Options >
  List All Devices**, choose the DM40's USBTMC interface (**not** the mass
  storage one), choose **WinUSB**, **Install Driver**. Then:

  ```
  pip install pyvisa pyvisa-py pyusb libusb-package
  ```

  and use the Python example above with `pyvisa.ResourceManager('@py')`.
  Untested.

## macOS

The USB disk and pyvisa-py over USB are untested on macOS. pyvisa-py with
pyusb and libusb (`brew install libusb`) is the likely path.

## Bluetooth

Bluetooth works the same from every system that has a Bluetooth serial port
(SPP). The meter shows up twice in a scan; use **C-1-ATK-DM40-CALI**,
RFCOMM channel 1 (the other one, **DM40A**, is for the phone app). Close the
phone app first: the meter takes one connection at a time. Bluetooth must be
**ON** in the meter's settings.

On Linux, the simplest client is
[`firmware/tools/dm40bt_scpi.py`](../../firmware/tools/dm40bt_scpi.py):

```
$ /usr/bin/python3 firmware/tools/dm40bt_scpi.py scpi '*IDN?'
Alientek,DM40A,0123456789ABCDEF01234567,opendm40 0.1
$ /usr/bin/python3 firmware/tools/dm40bt_scpi.py shell
```

It finds the meter with `bluetoothctl`; `--mac AA:BB:CC:DD:EE:FF` sets the
address. It needs a Python with Bluetooth sockets: the system
`/usr/bin/python3`.

For other programs, bind a serial device:

```
bluetoothctl devices                      # find the address of C-1-ATK-DM40-CALI
sudo rfcomm bind 0 AA:BB:CC:DD:EE:FF 1    # creates /dev/rfcomm0
```

and open `/dev/rfcomm0` (any baud rate), or in PyVISA
`ASRL/dev/rfcomm0::INSTR` with the same terminations and timeout as above
(untested).

Over Bluetooth the debug commands and file deletion are refused, and the
link is slower (about 11 kB/s); see
[SCPI reference, Bluetooth differences](scpi-reference.md#bluetooth-differences).

The phone-app protocol of the stock firmware also works: see
[Remote control](remote-control.md) and
[docs/bt-protocol.md](../bt-protocol.md).

## sigrok (sigrok-cli, PulseView, SmuView)

opendm40 can answer as an HP 34401A, a meter every sigrok version knows:

```
python3 firmware/tools/dm40scpi.py scpi 'SYST:IDN:EMUL 34401A'
sigrok-cli -d scpi-dmm:conn=usbtmc/<bus>.<address> --samples 10
```

The setting is kept; `SYST:IDN:EMUL OFF` switches back. In this mode the
host sees only the 34401A functions (no capacitance, temperature or AC+DC)
and cannot set ranges. Untested on a real meter.

## Troubleshooting

| Problem | What to do |
|---|---|
| `Permission denied` on `/dev/usbtmc0` | Add the udev rule above, or run as root. |
| No `/dev/usbtmc0`, but the disk is there | A libusb program (pyvisa-py, sigrok, PulseView) took the interface. Close it and re-plug the meter. |
| `Device or resource busy` | Another program has the instrument open. Use one program at a time. |
| A reply times out | Set the timeout to 5 s. After a function change the first reading takes up to 4 s. |
| Replies seem to belong to the wrong command | Two programs share the instrument: their replies cross. Close one. Over USB, a new command while a `READ?` waits cancels that `READ?` (`SYST:ERR?` says `Query INTERRUPTED`). |
| A command does nothing | Ask `SYST:ERR?`: it tells why. |
| Windows: code 28 | Install a USBTMC driver (see [Windows](#windows)). |
| Bluetooth: `Host is down` | Bluetooth is OFF in the meter's settings, or the phone app holds the connection. |
| Bluetooth: `Command protected` | That command works only over USB. |

The USB and the Bluetooth connection can be used at the same time; each has
its own session, and they share the meter's settings.
