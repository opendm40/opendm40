import binascii
import os
import socket
import struct
import sys
from binascii import hexlify, unhexlify


def to_bytes(s):
    return unhexlify(s.replace(' ', ''))

def add_crc(data):
    crc = 256 - sum(data) % 256
    return data + struct.pack('B', crc)

def decode_value(fmt: int, raw: int):
    """
    Decode one 16-bit value using its format byte.

    Format byte: bit 0 - sign, bits 1-3 - decimal places,
    bits 4-5 - unit prefix step (x1000 each).
    """
    if raw == 0xFFFF:
        return None

    decimals = (fmt >> 1) & 0x07
    unit = (fmt >> 4) & 0x03
    exponent = unit * 3 - decimals

    value = raw * (10 ** exponent)
    if fmt & 0x01:
        value = -value
    return value


def decode_packet_bytes(packet: bytes) -> dict:
    """
    Decode a measurement response (17 bytes).

    Args:
        packet (bytes): A 17-byte binary message.

    Returns:
        dict: {
            'value': float,         # main value
            'value2': float,        # second value (bytes 10–11)
            'value3': float,        # third value (bytes 12–13)
            'mode': int,            # mode byte
            'hold': bool,
            'rel': bool
        }
    """
    if len(packet) < 17:
        raise ValueError("Packet must be at least 17 bytes long.")

    mode = packet[5]
    flags = packet[6]

    # Raw values (3 x 16-bit), little-endian
    raw2 = packet[10] + (packet[11] << 8)
    raw3 = packet[12] + (packet[13] << 8)
    mantissa = packet[14] + (packet[15] << 8)

    return {
        'value': decode_value(packet[9], mantissa),
        'value2': decode_value(packet[7], raw2),
        'value3': decode_value(packet[8], raw3),
        'mode': hex(mode),
        'group': mode & 0x07,
        'range': (mode >> 3) & 0x07,
        'submode': mode >> 6,
        'hold': bool(flags & 0x80),
        'rel': bool(flags & 0x20),
        'flags': hex(flags),
        'mantissa' : mantissa,
        'exp' : hex(packet[9]),
    }



if not hasattr(socket, 'AF_BLUETOOTH'):
    sys.exit("This Python has no Bluetooth socket support, run it with /usr/bin/python3")

# DM40 classic Bluetooth MAC address (XX:XX:XX:XX:XX:XX): argument or $DM40_MAC
mac = sys.argv[1] if len(sys.argv) > 1 else os.environ.get('DM40_MAC')
if not mac:
    sys.exit("usage: dm40.py XX:XX:XX:XX:XX:XX  (or set DM40_MAC)")

s = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM)
s.connect((mac, 1))

while 1:
    packet = add_crc(to_bytes('af 05 03 09 00'))
    s.send(packet)
    data = s.recv(1024)
    print(binascii.hexlify(packet))
    print(binascii.hexlify(data))
    print((data))
    print(decode_packet_bytes(data))
