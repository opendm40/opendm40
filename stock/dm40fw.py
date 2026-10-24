#!/usr/bin/env python3
"""Alientek DM40 firmware update file tool.

  dm40fw.py info   <fw.bin>
  dm40fw.py decode <fw.bin> <out.dec.bin>     whole file, header included
  dm40fw.py encode <in.dec.bin> <fw.bin>
  dm40fw.py unpack <fw.bin> <body.bin>        prints header words
  dm40fw.py pack   <body.bin> <fw.bin> [--pad] [--like <orig_fw.bin>]

The file is obfuscated in 128-byte blocks from file offset 0 (header
included). The vendor FS-loader mis-decodes a SHORT final block (0x1F < len
< 128) whose key position is outside it (see tail_hazard()). build() / pack
refuse to write such a file; `--pad` (build(pad=True), used for opendm40 and
patched images) pads the body with 0xFF so the whole file (12-byte header +
body) is a multiple of 128 and the hazard cannot occur. Without --pad the
body is packed as is, so pack(unpack(vendor file)) == vendor file. `info` /
`unpack` warn about a hazardous input.
"""
import argparse
import struct
import sys

BLOCK = 128
ADD_POS = 0x1f
HEADER_SIZE = 12
LOAD_ADDR = 0x08004000


def key_pos(a):
    return a if a < BLOCK else a >> 1


def _keys(b):
    """Return (a, key_pos) or None if the block is stored as is."""
    if len(b) <= ADD_POS:
        return None
    a = b[ADD_POS]
    kp = key_pos(a)
    if kp >= len(b):
        return None
    return a, kp


def decode_block(b):
    keys = _keys(b)
    if keys is None:
        return bytes(b)
    a, kp = keys
    k = b[kp]
    out = bytearray(((c ^ k) - a) & 0xff for c in b)
    out[ADD_POS] = a
    out[kp] = k
    return bytes(out)


def encode_block(p):
    keys = _keys(p)
    if keys is None:
        return bytes(p)
    a, kp = keys
    k = p[kp]
    out = bytearray(((c + a) & 0xff) ^ k for c in p)
    out[ADD_POS] = a
    out[kp] = k
    return bytes(out)


def decode(data):
    return b''.join(decode_block(data[i:i + BLOCK]) for i in range(0, len(data), BLOCK))


def encode(data):
    return b''.join(encode_block(data[i:i + BLOCK]) for i in range(0, len(data), BLOCK))


def tail_hazard(data):
    """The FS-loader hazard of an update file (encoded or decoded: byte 0x1F of
    a block is the same in both): None when it is safe, else (last_len, a, kp)
    of its last block.

    Hazard: the final 128-byte block is short with 0x1F < last_len < 128 and
    its key position kp (a = block[0x1F], kp = a if a < 128 else a >> 1) is
    >= last_len. _keys / encode_block store such a block as is; the real
    FS-loader does not: it takes the key from its 128-byte buffer, i.e. a
    stale byte, and corrupts the tail (body CRC mismatch -> "update error",
    the app is erased; reproduced on the real FS-loader in the Renode
    emulator). Safe: whole blocks; a final block of <= 0x1F bytes (stored as
    is by both sides); a short block with kp < last_len (the vendor's
    dm40_v_2_6.bin: 36 bytes, kp 8). Same rule in firmware/src/calbackup.rs
    (FwFile::tail_hazard) and firmware/tools/dm40dbg.py (`image`); equivalent to
    alientek-dm40-tools fw/dm40fw.py final_block_unsafe()."""
    n = len(data) % BLOCK
    if not ADD_POS < n < BLOCK:
        return None
    a = data[len(data) - n + ADD_POS]
    kp = key_pos(a)
    return (n, a, kp) if kp >= n else None


def tail_warning(data):
    """A warning line for tail_hazard(data), or None."""
    h = tail_hazard(data)
    if h is None:
        return None
    n, a, kp = h
    return ('warning: the last 128-byte block is short (%d bytes) and its key position '
            '0x%02x ([0x1F] = 0x%02x) is outside it: the vendor FS-loader decodes it with '
            'a stale key, the body CRC fails and the app is erased ("update error"); '
            'repack the body with `dm40fw.py pack --pad`' % (n, kp, a))


def pad_len(body_len):
    """0xFF bytes to append to a body so that header + body is whole blocks."""
    return -(HEADER_SIZE + body_len) % BLOCK


def split(dec):
    """Decoded file -> (word0, length, word2, body)."""
    w0, length, w2 = struct.unpack_from('<3I', dec, 0)
    body = dec[HEADER_SIZE:]
    if length != len(body):
        raise ValueError('length field 0x%x != body size 0x%x' % (length, len(body)))
    return w0, length, w2, body


def crc16(data, crc=0xffff):
    """CRC-16, poly 0x8005, not reflected, no final XOR (vendor routine)."""
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x8005) & 0xffff if crc & 0x8000 else (crc << 1) & 0xffff
    return crc


def header_word0(length, word2):
    """Word 0: CRC-16 of header words 1 and 2, with its complement in the high half."""
    c = crc16(struct.pack('<2I', length, word2))
    return ((~c & 0xffff) << 16) | c


# The bootloader manages a 12-byte marker at flash 0x08008000 (body offset
# MARKER_OFFSET) and excludes it from the body checksum.
MARKER_OFFSET = 0x08008000 - LOAD_ADDR
MARKER_LEN = 12


def body_checksum(body):
    """Low half of word 2: CRC-16/0x8005 (init 0xFFFF, no reflection, no final
    XOR) over the flashed body with the 12-byte bootloader marker at offset
    0x4000 excluded."""
    data = body[:MARKER_OFFSET] + body[MARKER_OFFSET + MARKER_LEN:]
    return crc16(data)


def build(body, like=None, pad=False):
    """Body -> encoded update file.

    pad: first pad the body with 0xFF (pad_len) so the file is a whole number
    of 128-byte blocks; word 1 (length) and the word 2 body CRC then cover the
    padding (the loader flashes it like any other body byte). Without pad the
    body is packed as is (vendor round trip). Either way a file with a
    hazardous last block (tail_hazard) is refused with ValueError.

    Word 2 is 0x10000 | body checksum; while the checksum algorithm is unknown,
    word 2 is copied from the decoded original file `like`. Word 0 is always
    computed.
    """
    body = bytes(body)
    if pad:
        body += b'\xff' * pad_len(len(body))
    check = body_checksum(body)
    if check is None:
        if like is None:
            raise ValueError('body checksum algorithm unknown, pass the original file')
        _, _, w2, _ = split(decode(like))
        print('warning: word 2 copied from the original file, '
              'its body checksum does not match the modified body', file=sys.stderr)
    else:
        w2 = 0x10000 | check
    w0 = header_word0(len(body), w2)
    dec = struct.pack('<3I', w0, len(body), w2) + body
    h = tail_hazard(dec)
    if h:
        raise ValueError('refusing to pack: the last 128-byte block would be %d bytes with '
                         'its key position 0x%02x outside it; the FS-loader would corrupt it '
                         '("update error", app erased). Pad the body (pack --pad / '
                         'build(pad=True)) so 12 + body is a multiple of 128' % (h[0], h[2]))
    enc = encode(dec)
    assert not pad or len(enc) % BLOCK == 0
    return enc


def _warn_tail(data):
    w = tail_warning(data)
    if w:
        print(w, file=sys.stderr)


def cmd_info(a):
    data = open(a.fw, 'rb').read()
    w0, length, w2, body = split(decode(data))
    sp, reset = struct.unpack_from('<2I', body, 0)
    print('word0  0x%08x' % w0)
    print('length 0x%08x (end 0x%08x)' % (length, LOAD_ADDR + length))
    print('word2  0x%08x' % w2)
    print('SP     0x%08x' % sp)
    print('Reset  0x%08x' % reset)
    n = len(data) % BLOCK
    print('blocks %d x 128%s' % (len(data) // BLOCK, ' + %d (short last block)' % n if n else ''))
    _warn_tail(data)


def cmd_decode(a):
    open(a.out, 'wb').write(decode(open(a.fw, 'rb').read()))


def cmd_encode(a):
    open(a.out, 'wb').write(encode(open(a.dec, 'rb').read()))


def cmd_unpack(a):
    data = open(a.fw, 'rb').read()
    _warn_tail(data)
    w0, length, w2, body = split(decode(data))
    open(a.body, 'wb').write(body)
    print('word0 0x%08x length 0x%08x word2 0x%08x' % (w0, length, w2))


def cmd_pack(a):
    body = open(a.body, 'rb').read()
    like = open(a.like, 'rb').read() if a.like else None
    try:
        out = build(body, like, pad=a.pad)
    except ValueError as e:
        sys.exit('dm40fw.py pack: %s' % e)
    open(a.out, 'wb').write(out)
    n = pad_len(len(body)) if a.pad else 0
    if n:
        print('body padded with %d x 0xFF to whole 128-byte blocks (%d bytes)' % (n, len(out)),
              file=sys.stderr)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('info'); s.add_argument('fw'); s.set_defaults(f=cmd_info)
    s = sub.add_parser('decode'); s.add_argument('fw'); s.add_argument('out'); s.set_defaults(f=cmd_decode)
    s = sub.add_parser('encode'); s.add_argument('dec'); s.add_argument('out'); s.set_defaults(f=cmd_encode)
    s = sub.add_parser('unpack'); s.add_argument('fw'); s.add_argument('body'); s.set_defaults(f=cmd_unpack)
    s = sub.add_parser('pack'); s.add_argument('body'); s.add_argument('out')
    s.add_argument('--like', help='original update file to take word 2 from')
    s.add_argument('--pad', action='store_true',
                   help='pad the body with 0xFF so the file is whole 128-byte blocks '
                        '(required for new images: opendm40 build.sh, patcher)')
    s.set_defaults(f=cmd_pack)
    a = p.parse_args()
    a.f(a)


if __name__ == '__main__':
    main()
