# Alientek DM40 / EL15 Bluetooth protocol

Bluetooth protocol of two Alientek devices, as used by the stock firmware, with
Python clients in [tools/](../tools/):

- **DM40 multimeter** (`tools/dm40.py`)
- **EL15 electronic load** (`tools/el15.py`)

Both devices communicate via Bluetooth RFCOMM. The replacement firmware
[fw-rs](../fw-rs/README.md) keeps the DM40 commands below and adds its own
(see `fw-rs/README.md`).

Usage: `/usr/bin/python3 tools/dm40.py <MAC>`, `/usr/bin/python3 tools/el15.py <MAC>`
(the MAC can also come from `DM40_MAC` / `EL15_MAC`). A Python build without
Bluetooth sockets (e.g. linuxbrew) cannot run them.

## Connection Setup

1. Create Bluetooth RFCOMM socket
2. Connect to device MAC address on channel 1
3. Send commands and receive responses

## Packet Structure

### Common Elements

All packets use the following structure:
- **Header**: Device-specific identifier
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
| 02  | -            | 3 bytes       | Versions, each byte is `major * 10 + minor`. The middle byte is the firmware version (26 = V2.6); the other two come from bootloader data (hardware / bootloader version) |
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

Note: `AF 05 03 09 01 3F` (used by earlier versions of `tools/dm40.py`) also works,
but only by accident: `3F` is taken as a data byte and the checksum then
happens to match a zero byte from the cleared receive buffer.

### Mode Byte

Used as the data byte of command 06 and as the first byte of the measurement.

```
Bits 0-2   Function group: 0 = voltage, 1 = current, 2 = resistance, 3-5 = other functions
Bits 3-5   Range (high values select auto range and mV / uA)
Bits 6-7   Sub-mode (e.g. DC / AC / AC+DC)
```

The exact meaning of groups 3-5 (continuity, diode, capacitance, frequency)
and of the range values was not verified on a device.

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

Firmware update files (`dm40_v_X_Y.bin`) are obfuscated. [firmware/dm40fw.py](../firmware/dm40fw.py)
decodes and re-encodes them, see [firmware/README.md](../firmware/README.md) for
building modified images:

```
python3 firmware/dm40fw.py decode dm40_v_2_6.bin dm40_v_2_6.dec.bin
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
that the bootloader manages. `dm40fw.py` computes both words.

The bootloader below `0x08004000` is not part of the update file.

## EL15 Electronic Load Protocol

### Connection
- **MAC Address**: the load's Bluetooth address, `XX:XX:XX:XX:XX:XX`
- **Channel**: 1

### Command Format
```
AF 07 03 [CMD] [DATA...] [CRC]
```

### Commands

#### Query Status
- Command: `AF 07 03 08 00 [CRC]`
- Returns current device status (28 bytes)

#### Lock/Unlock Control
- Unlock: `AF 07 03 09 01 00 [CRC]`
- Lock: `AF 07 03 09 01 01 [CRC]`

#### Load Control
- Load Off: `AF 07 03 09 01 00 [CRC]`
- Load On: `AF 07 03 09 01 04 [CRC]`

#### Set Current
- Command: `AF 07 03 04 04 [4-byte float] [CRC]`
- Float is IEEE 754 single precision, little-endian
- Example: 1.0A = `00 00 80 3F`
- Example: 1.234A = `B6 F3 9D 3F`

#### Set Operating Mode
- CC (Constant Current): `AF 07 03 03 01 01 [CRC]`
- Battery CAP: `AF 07 03 03 01 02 [CRC]`
- CV (Constant Voltage): `AF 07 03 03 01 09 [CRC]`
- Battery DCR: `AF 07 03 03 01 0A [CRC]`
- CR (Constant Resistance): `AF 07 03 03 01 11 [CRC]`
- CP (Constant Power): `AF 07 03 03 01 19 [CRC]`

#### Device Management
- Discovery: `AF FF FF 00 00 [CRC]`
  - Response: `DF FF FF 00 02 07 03 [CRC]`
- Get Device Name: `AF 07 03 07 00 [CRC]`
  - Response: `DF 07 03 07 0A "EL15" + 6 null bytes [CRC]`
- Set Device Name: `AF 07 03 06 0A [10-byte name] [CRC]`
  - Name is padded with null bytes to 10 bytes total

### Response Format (28 bytes)

```
Example response:
DF 07 03 08 16 41 02 B8 4A 66 41 2A 15 9E 3F 6B 00 00 00 88 80 23 42 7B 14 9E 3F AD
0  1  2  3  4  5  6  7  8  9  10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27
<-header->           <--volt --> <-current-> <--time---> <-temp--->   <---setp-> CRC

Byte    Description
0-6     Header (DF 07 03 08 unk1 mode/fan run)
7-10    Voltage (float, little-endian)
11-14   Current (float, little-endian)  
15-18   Runtime (long, little-endian)
19-22   Temperature (float, little-endian)
23-26   Setpoint (float, little-endian)
27      CRC
```

#### Status Byte Details
- **Byte 4**: Unknown parameter
- **Byte 5**: Lower 4 bits = Mode, Upper 4 bits = Fan status
- **Byte 6**: Run status

### Value Decoding Algorithm
1. Extract voltage from bytes 7-10 (IEEE 754 float, little-endian)
2. Extract current from bytes 11-14 (IEEE 754 float, little-endian)
3. Extract runtime from bytes 15-18 (32-bit signed integer, little-endian)
4. Extract temperature from bytes 19-22 (IEEE 754 float, little-endian)
5. Extract setpoint from bytes 23-26 (IEEE 754 float, little-endian)
6. Parse status bytes for mode, fan, and run state

## Usage Examples

### DM40 - Read Measurement
```
PSEUDOCODE:
1. Send measurement request: AF 05 03 09 00 + CRC
2. Receive 17-byte response
3. Decode using DM40 algorithm
4. Display measured value
```

### EL15 - Complete Control Sequence
```
PSEUDOCODE:
1. Connect to device via Bluetooth
2. Send unlock command: AF 07 03 09 01 00 + CRC (??)
3. Set mode to CC: AF 07 03 03 01 01 + CRC
4. Set current setpoint: AF 07 03 04 04 + float_bytes + CRC
5. Enable load: AF 07 03 09 01 04 + CRC
6. Query status: AF 07 03 08 00 + CRC
7. Decode 28-byte response for current readings
8. Display voltage, current, mode, and runtime
```

## Mode Values

| Mode | Value | Description |
|------|--------|-------------|
| CC   | 0x01   | Constant Current |
| CAP  | 0x02   | Battery Capacity Test |
| CV   | 0x09   | Constant Voltage |
| DCR  | 0x0A   | Battery DCR Test |
| CR   | 0x11   | Constant Resistance |
| CP   | 0x19   | Constant Power |

## Protocol Notes

- All multi-byte values use little-endian byte order
- Float values are IEEE 754 single precision (4 bytes)
- CRC is calculated as `256 - sum(data) % 256`
- Always wait for response before sending next command
- Device discovery triggers AT+CBN Bluetooth command on host
- String values are null-padded to fixed lengths
- Runtime is measured in seconds as a 32-bit signed integer
