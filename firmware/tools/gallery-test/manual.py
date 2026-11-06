#!/usr/bin/python3 -I
"""Files for the manual's gallery pictures (tools/gallery-test/manual.sh):
screenshots made from the manual's own screen renders (tools/ui-render) and
two recordings, as the meter would store them.

  manual.py <manual img dir> <out dir>

Writes S000..S007.BMP (480 x 320, 16 bpp 565 bitfields, bottom-up, as
screenshot.rs) and LOG000.CSV / LOG001.CSV (csvlog format; manual.sh
re-codes them as DLG). Needs PIL.
"""
import math
import os
import struct
import sys

from PIL import Image

SRC, OUT = sys.argv[1:3]
os.makedirs(OUT, exist_ok=True)

# Oldest first: the gallery shows the newest (S007) top left.
SHOTS = ['dmm-temp', 'dmm-cap', 'settings-display', 'generator', 'scope', 'combined', 'dmm-ohm', 'dmm']

for i, n in enumerate(SHOTS):
    im = Image.open(os.path.join(SRC, n + '.png')).convert('RGB')
    assert im.size == (480, 320), (n, im.size)
    px = im.load()
    rows = []
    for y in reversed(range(320)):
        rows.append(struct.pack('<480H', *(((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
                                           for r, g, b in (px[x, y] for x in range(480)))))
    pix = b''.join(rows)
    info = struct.pack('<IiiHHIIiiII', 40, 480, 320, 1, 16, 3, len(pix), 0, 0, 0, 0)
    masks = struct.pack('<III', 0xF800, 0x07E0, 0x001F)
    off = 14 + len(info) + len(masks)
    with open(os.path.join(OUT, 'S%03d.BMP' % i), 'wb') as f:
        f.write(b'BM' + struct.pack('<IHHI', off + len(pix), 0, 0, off) + info + masks + pix)


def csv(name, func, unit, rng, n, dt_ms, fn):
    lines = ['# Alientek DM40A, opendm40, function %s, range %s, started uptime 12.345 s' % (func, rng),
             '# time_s = seconds since the start of this recording (no real-time clock); empty value = overflow',
             'time_s,value,unit,flags']
    for i in range(n):
        t = i * dt_ms
        lines.append('%d.%03d,%.6g,%s,' % (t // 1000, t % 1000, fn(i), unit))
    open(os.path.join(OUT, name + '.CSV'), 'wb').write(('\r\n'.join(lines) + '\r\n').encode())


# A battery discharging under load, then a heater warming up.
csv('LOG000', 'DC V', 'V', 'AUTO', 1800, 1000, lambda i: 4.18 - 0.4 * i / 1800 - 0.05 * (i / 1800) ** 4)
csv('LOG001', 'Temperature', 'degC', 'AUTO', 900, 1000, lambda i: 24.6 + 60 * (1 - math.exp(-i / 250)))
