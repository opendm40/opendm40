# Alientek DM40 Bluetooth protocol

Bluetooth protocol of the Alientek DM40 multimeter as used by the stock
firmware and its phone app. The meter communicates via Bluetooth RFCOMM. The replacement firmware
[opendm40](../firmware/README.md) keeps the DM40 commands below and adds SCPI
([firmware/docs/bt-scpi.md](../firmware/docs/bt-scpi.md)).

Host tools for a meter running opendm40: `firmware/tools/dm40scpi.py --bt` (SCPI
over Bluetooth, and `btpoll` for these stock frames) and
[`firmware/tools/dm40bt_scpi.py`](../firmware/tools/dm40bt_scpi.py). They need a Python with Bluetooth sockets
(`/usr/bin/python3` on Linux; linuxbrew's has none).

## Connection Setup

1. Create Bluetooth RFCOMM socket
2. Connect to device MAC address on channel 1
3. Send commands and receive responses

## Packet Structure

### Common Elements

All packets use the following structure:
- **Header**: packet identifier (0xAF for requests to the DM40)
- **Command/Data**: Variable length payload
- **CRC**: Single byte checksum

### CRC Calculation

The CRC is calculated as: `256 - sum(all_data_bytes) % 256`

## DM40 Multimeter Protocol

### Connection
- **MAC Address**: the meter's classic Bluetooth address, `XX:XX:XX:XX:XX:XX`
- **Channel**: 1

### Packet Format

Reverse engineered from firmware V2.6 (packet parser at `0x0800d2fa`).

```
Request:   AF 05 03 [CMD] [LEN] [DATA...] [CRC]
Response:  DF 05 03 [CMD] [LEN] [DATA...] [CRC]
```

- `05 03` is the DM40 device id; a request may also use `FF FF`
- `LEN` is the number of data bytes (max 0xC2)
- CRC covers everything before it
- A request with a wrong header, id, length or CRC gets no response

### Commands

| CMD | Request data | Response data | Description |
|-----|--------------|---------------|-------------|
| 00  | -            | 3 bytes       | Discovery: `05 03` + model letter `'A'`/`'B'`/`'C'`. Response id is `FF FF` |
| 01  | -            | 1 byte        | No-op, returns status only |
| 02  | -            | 3 bytes       | Versions, each byte is `major * 10 + minor`. The bytes are the bootloader version, the firmware version (26 = V2.6) and the hardware version; the first and last come from bootloader data |
| 03  | 1 byte (ignored) | 1 byte    | Reset: clears REL and HOLD |
| 04  | 1 byte       | 1 byte        | HOLD: non-zero = on, 0 = off |
| 05  | 1 byte       | 1 byte        | REL: non-zero = on, 0 = off (ignored while HOLD is active) |
| 06  | 1 byte       | 1 byte        | Select measurement mode, see mode byte below |
| 07  | 20 bytes     | 1 byte        | Set Bluetooth name (null padded). The meter sends `AT+SBN<name>` to the BT module |
| 08  | -            | 20 bytes      | Get Bluetooth name (null padded) |
| 09  | -            | 11 bytes      | Read measurement |

The 1-byte response of commands 01, 03-07 is a status: 0 = OK, 1 = wrong
request length (for 07 also: BT module did not answer `OK`).

Commands 03-06 are executed by the multimeter page, so they only take effect
while the meter shows the multimeter screen.

Examples:
```
AF 05 03 09 00 40       read measurement
AF 05 03 04 01 01 43    HOLD on
AF 05 03 04 01 00 44    HOLD off
AF 05 03 00 00 49       discovery
```

Note: `AF 05 03 09 01 3F` also works, but only by accident: `3F` is taken as a data byte and the checksum then
happens to match a zero byte from the cleared receive buffer.

### Mode Byte

Used as the data byte of command 06 and as the first byte of the measurement.

```
Bits 0-2   Function group: 0 = voltage, 1 = current, 2 = resistance, 3-5 = other functions
Bits 3-5   Range (high values select auto range and mV / uA)
Bits 6-7   Sub-mode (e.g. DC / AC / AC+DC)
```

The exact meaning of groups 3-5 (continuity, diode, capacitance, frequency)
and of the range values is *inferred*: not checked on a device.

The protocol has no unit field (the unit follows from the group and the
format byte's prefix) and no group for a power function. opendm40's power meter
therefore reports the DC V byte (`0x28` auto, range field = the V channel's
range) with P (W) as the main value in the V group's units: the app shows
"230.02 V" for 230.02 W, mW as mV, a source negative; value 2 / 3 are
`FF FF`. Command 06 with that byte keeps the power meter, any other byte
leaves it. The full power readings are on BT SCPI (`FETC:POW:ALL?`); see
[firmware/docs/power-meter-and-misconnect.md](../firmware/docs/power-meter-and-misconnect.md) 3.13.

### Measurement Response (17 bytes)
```
Byte    Description
0-4     Header: DF 05 03 09 0B
5       Mode byte
6       Flags: bit 7 = HOLD, bit 5 = REL, low bits = other state
7       Format byte of value 2
8       Format byte of value 3
9       Format byte of main value
10-11   Value 2 (little-endian)
12-13   Value 3 (little-endian)
14-15   Main value (little-endian)
16      CRC
```

A value of `FF FF` means "no value".

### Format Byte
```
Bit 0      Sign: 1 = negative
Bits 1-3   Number of decimal places
Bits 4-5   Unit prefix step (x1000 each)
Bit 7      Set by the firmware in one mode when the main value exceeds 0xFFF0
           (the value is then sent minus 0xFFF0)
```

### Value Decoding Algorithm
1. Take the format byte and the 16-bit value
2. Decimal places: `(format >> 1) & 7`
3. Prefix: `(format >> 4) & 3`
4. Exponent: `prefix * 3 - decimals`
5. Final value: `value * 10^exponent`, negated if bit 0 of the format byte is set

## DM40 Firmware Image

Firmware update files (`dm40_v_X_Y.bin`) are obfuscated. [stock/dm40fw.py](../stock/dm40fw.py)
decodes and re-encodes them, see [stock/README.md](../stock/README.md) for
building modified images:

```
python3 stock/dm40fw.py decode dm40_v_2_6.bin dm40_v_2_6.dec.bin
```

### Obfuscation

The file is processed in 128-byte blocks. Two plaintext bytes of each block
are stored as is and act as keys for the rest of the block:

- byte at offset `0x1F`: additive key `a`
- byte at offset `a` (or `a >> 1` if `a >= 0x80`): XOR key `k`
- every other byte: `plain = ((cipher ^ k) - a) & 0xFF`

### Decoded Layout

```
Offset  Description
0-3     Header check: (~X << 16) | X, X = CRC-16 of bytes 4-11
4-7     Length of the image body (file size - 12)
8-11    0x00010000 | CRC-16 of the body without its 12 bytes at offset 0x4000
12-     Image body, loaded at 0x08004000 (Cortex-M vector table first)
```

CRC-16: polynomial 0x8005, init 0xFFFF, MSB first, no reflection, no final
XOR. The 12 bytes skipped by the body CRC (flash `0x08008000`) are a marker
that the bootloader manages. [`dm40fw.py`](../stock/dm40fw.py) computes both words.

The bootloader below `0x08004000` is not part of the update file.

## Usage Examples

### DM40 - Read Measurement
```
PSEUDOCODE:
1. Send measurement request: AF 05 03 09 00 + CRC
2. Receive 17-byte response
3. Decode using DM40 algorithm
4. Display measured value
```

## Protocol Notes

- All multi-byte values use little-endian byte order
- Float values are IEEE 754 single precision (4 bytes)
- Always wait for response before sending next command
- Device discovery triggers AT+CBN Bluetooth command on host
- String values are null-padded to fixed lengths
