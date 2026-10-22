# SCPI over Bluetooth serial

fw-rs answers SCPI on the meter's Bluetooth serial link, next to the stock
phone-app protocol, so PyVISA, sigrok and simple loggers can talk to the meter
without a cable. The same parser as USBTMC handles the commands
(`src/scpi.rs`, `Scpi::handle`); only the transport is different.

Code: `src/bt_scpi.rs` (dispatcher, command filter, TX ring; no hardware),
`src/bt.rs` (UART4 + DMA, `ScpiPort`), `src/main.rs` (one `Scpi` session and
one READ?/MEAS? wait per transport, `bt.pump()` every loop, replies through
`ScpiPort`, `Out::Dbg` refused when `!port.trusted()`). DBG:DIAG? bytes 60
(TX DMA path working, 0 = blocking fallback) and 64..67 (u32 bytes dropped on
a full TX ring). Host test: `tools/bt-scpi-test/run.sh`.
Client: `tools/dm40bt_scpi.py`.

## Wire format

One byte stream from the BT module (classic SPP and BLE share the UART), split
on the first byte of each request:

| First byte | Taken as | Ends at |
|---|---|---|
| `0xAF` | stock request frame `AF <05 03\|FF FF> cmd len data crc` | its length; bad id/length re-scans from the next byte, bad CRC drops it |
| letter, `*`, `:` | SCPI message (`;` separates commands) | `\n` or `\r` (CR LF = one message), max 256 bytes |
| anything else | ignored (control bytes, non-ASCII) or skipped up to the next `\n`/`\r` (other ASCII) | |

- Inside a frame every byte is frame data, so a payload may hold ASCII or `\n`.
- `0xAF` (or any non-ASCII byte) inside a text line drops that line; `0xAF`
  then starts a frame.
- A partial line is dropped after 2 s without a byte, a partial frame after
  0.5 s, and both are dropped when the link goes up or down (PC14).
- A line over 256 bytes is discarded; `SYST:ERR?` then reports "Input buffer
  overrun".
- Replies: the text plus `\n`. A definite-length block (`WAV:DATA?`,
  `LOG:DATA?`) is `#<n><len><data>` plus `\n`.
- Messages run one at a time: the next line is read only after the reply to
  the previous query has gone out, as on a serial instrument. Pipelined
  queries are answered in order, and none is cut short ("Query INTERRUPTED"
  only happens on USBTMC).
- USBTMC and Bluetooth have separate SCPI sessions (error queue, pending
  `READ?`), so one host does not see or consume the other's errors.
- The BT module's own answers (`OK`, `CBN+...`) arriving up to 0.5 s after an
  AT exchange, or 2 s after `AT+RESET`, are dropped rather than parsed.
  Anything else the module prints on its own (a connect message, if this
  module sends one) goes to the parser and only adds an "Undefined header" to
  `SYST:ERR?`; it gets no reply.
- SCPI over Bluetooth works only while Bluetooth is enabled in the meter's
  settings (`app.bt_on`), like the phone-app protocol.

## Security: DBG:* is refused

Bluetooth is an unauthenticated radio link: anyone in range can connect. The
Bluetooth transport is untrusted (`ScpiPort::trusted() == false`). Any message
that contains a `DBG` command (any case, any number of leading `:`, in any
`;`-separated part) is refused **as a whole before the parser sees it**. That
covers `DBG:UPDATE` / `DBG:RECOVER` (they zero the loader marker inside
`Scpi::handle` on native builds), `DBG:RESET`, `DBG:RELEASE`, input injection
(`DBG:KEY`, `DBG:SWIPE D,2` = power off), the debug reads, and any `DBG:*`
added later, including the ones the parser routes as `Out::Instr`
(`DBG:GRAB`, `DBG:GEST?`). No part of the message runs and no reply is sent;
`SYST:ERR?` then reports `-100,"Command protected"`. main.rs also refuses
`scpi::Out::Dbg` from an untrusted port, as a backstop.
The binary protocol has no memory read/write command in fw-rs (`Bt::serve`
handles cmd 00..09 only; the 0x0B memory command exists only in the patched
stock firmware of the memrw kit).

No other SCPI root is dangerous today (the roots are `*`-commands, MEAS/CONF/
READ/FETC/FUNC/SENS/RATE, the DMM function names, CALC, SYST, SOUR/OUTP, TIM/
CHAN/TRIG/RUN/STOP/SING/SCOP/WAV, LOG, DIAG, MMEM). `MMEM:DELete` (deletes a
screenshot / CSV file on the USB disk) is executed by main.rs, which refuses
it from an untrusted port (`Command protected`); `MMEM:CAT?` and `LOG:CSV`
work over Bluetooth. A new command that can write flash or reset the meter
must go under `DBG:` or be added to `bt_scpi::UNTRUSTED_DENY`.

## Throughput

The UART runs at 115200 8N1 = 11.5 kB/s (87 us per byte). TX goes out by DMA
(DMA0 channel 4) from an 8 KiB ring, so a long reply does not block the main
loop. RX goes to a 1 KiB circular DMA ring (89 ms of input).

| Query | Reply | On the UART | Round trip (expected) |
|---|---|---|---|
| `*IDN?` | ~30 B | 3 ms | ~20-60 ms (RFCOMM latency) |
| `READ?` | 15 B | 1.3 ms | 0.1-0.4 s, set by the DMM conversion, not the link |
| `MEAS:<f>?` after a function change | 15 B | 1.3 ms | ~3 s (settling, as on USBTMC) |
| `WAV:DATA?` | 406 B | 35 ms | ~50-100 ms |
| `LOG:DATA?` | <= 6145 B | 0.53 s | ~0.6 s (AD1 <= 640 S/s only, see below) |

The data logger over Bluetooth (`LOG:STATe ON`, `LOG:DATA?`; 2026-10-22):
12-byte records at 11.5 kB/s are 960 records/s with the link 100 % busy;
each block also waits for the query's round trip (RFCOMM ~20-60 ms per
511-record block of 0.53 s), so ~870 records/s is the realistic ceiling.
fw-rs therefore refuses `LOG:STATe ON` from Bluetooth when the session's
nominal rate is above 800 S/s or the source is AD2 (whose slowest rung is
1200 S/s): `SYST:ERR?` = "Rate too high for Bluetooth"; every AD1 setting
(LOG:OSR / LOG:CHOP, 2.5 .. 640 S/s) works (640 S/s = 7.7 kB/s, 67 % of
the link; the 2048-record ring covers 3.2 s of host stalls). `LOG:DATA?`
from Bluetooth while a faster session runs (started over USB) answers an
empty block and the same error, so it never steals the USB host's
records. `LOG:DEST DISK` sessions are USB only ("Command protected", as
`LOG:CSV ON`). For AD2 rates use USBTMC or the disk (LOG:DEST DISK,
docs/compact-log-format.md "Fast stream"). Host: `tools/dm40scpi.py --bt
log --source ad1 --osr 0` (it warns about the ceiling). Emulator check:
emu/fastlogtest.py (the refusals, an AD1 session drained over BT).

The module has no RTS/CTS wiring. Classic SPP keeps up with 115200. Over BLE
(20-byte notifications at the connection interval) the module may not keep up
with a burst like `LOG:DATA?`. That is not measured yet. Standard software
uses classic SPP anyway.

## Host side

### dm40bt_scpi.py (RFCOMM socket, no binding needed)

```
/usr/bin/python3 tools/dm40bt_scpi.py scpi '*IDN?'
/usr/bin/python3 tools/dm40bt_scpi.py scpi 'CONF:VOLT:DC' 'READ?'
/usr/bin/python3 tools/dm40bt_scpi.py scpi 'WAV:DATA?' -o wave.bin
/usr/bin/python3 tools/dm40bt_scpi.py shell      # interactive
/usr/bin/python3 tools/dm40bt_scpi.py bench      # READ? / WAV:DATA? timing
```

**Which address.** The module is dual-mode, so BlueZ lists the meter twice:
the classic device with the Serial Port UUID, stock name
"C-1-ATK-DM40-CALI" (this unit: XX:XX:XX:XX:XX:XX; "CALI" is the module's
factory name, not a calibrator), and the BLE device "DM40A" / "B" / "C" for
the phone app, whose address has the first byte one lower. RFCOMM channel 1
of the classic address is the one to use. "Host is down" from it means the
module does not answer pages at all: Bluetooth OFF in Settings, or a phone
holding the module's one connection, or (fw-rs before 2026-10-22) the module
left unpowered (PC0 high).

The address is found like the memrw kit finds it (`bluetoothctl`, the classic
device with the Serial Port UUID, cached in `~/.config/dm40-memrw/mac`).
`--mac` or `DM40_MAC` set it directly, and `--dev /dev/rfcomm0` uses a bound
tty. Close the phone app first, because the module takes one connection at a
time.

### Bind /dev/rfcomm0

```
bluetoothctl devices                      # the DM40 classic address (Serial Port)
sudo rfcomm bind 0 AA:BB:CC:DD:EE:FF 1    # /dev/rfcomm0, channel 1; connects on open
sudo usermod -aG dialout $USER            # or chmod the node; re-login
sudo rfcomm release 0                     # when done
```

`rfcomm` is in BlueZ (on some distributions in a "deprecated tools"
package). The baud rate set on `/dev/rfcomm0` does not matter.

### PyVISA (ASRL)

```python
import pyvisa
rm = pyvisa.ResourceManager('@py')            # pyvisa-py + pyserial
dmm = rm.open_resource('ASRL/dev/rfcomm0::INSTR',
                       read_termination='\n', write_termination='\n',
                       timeout=5000)          # MEAS after a function change ~3 s
print(dmm.query('*IDN?'))
dmm.write('CONF:VOLT:DC')
print(float(dmm.query('READ?')))
wave = dmm.query_binary_values('WAV:DATA?', datatype='B', container=bytes)
```

### sigrok

```
sigrok-cli -d scpi-dmm:conn=/dev/rfcomm0:serialcomm=115200/8n1 --scan
sigrok-cli -d scpi-dmm:conn=/dev/rfcomm0:serialcomm=115200/8n1 --samples 10
```

libsigrok's serial SCPI transport ends commands with CR LF, which works here
because CR ends the line and the LF is ignored. scpi-dmm needs the DM40
profile from the libsigrok branch in TODO.md to recognise the `*IDN?` string.

A function changed on the meter during an acquisition does not end it: the
`READ?` waiting at that moment, and every `READ?` until the new function has
a reading, answers within 0.85 s with `+9.91E37` (SCPI NaN, "Data stale";
`+9.9E37` stays the overload). The DM40 profile skips those samples and
labels the next ones from `FUNC?`; other hosts see +inf for them. A function
changed over Bluetooth while sigrok reads over USBTMC counts as a change on
the meter (device check 2026-10-22: 16 changes, 880 samples, no stop). `FUNC?`
names of the fw-rs extras: `"VOLT:AUTO"` / `"CURR:AUTO"` (the profile's
`voltage` / `current` without a coupling flag; each sample flagged DC or AC as
`VOLT:COUP?` / `CURR:COUP?` say) and `"POW"` (read as W, entered on the meter
or with `CONF:POW`; not selectable from sigrok: its range hunt takes up to
15 s). Temperatures follow `UNIT:TEMP` (C or F).
