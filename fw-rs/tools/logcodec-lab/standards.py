#!/usr/bin/env python3
"""Standard container formats on the same DMM logs (desktop writers, i.e. an
upper bound for what an MCU writer of the same format could reach).

Per reading: time (u32 ms), value (f32), flags (u8: OL = invalid).
Needs: asammdf, nptdms, pyarrow, h5py, zstandard (pip, see the doc).

    standards.py DATA_DIR OUT_DIR
"""
import gzip
import lzma
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import load, EMPTY, NEGZ   # noqa: E402

SETS = [('noisy DC V 10/s', 't0'), ('steady ohm 2.5/s', 't1'), ('ohm + OL bursts', 't5'),
        ('MEDIUM + autorange + flags', 't6'), ('10 h at 10/s', 'long')]


def arrays(rows):
    t = np.array([r[0] for r in rows], dtype=np.uint32)
    v = np.array([(-r[1][1] if r[1][0] else r[1][1]) * 10.0 ** (r[1][2] - 5)
                  if isinstance(r[1], tuple) else np.nan for r in rows], dtype=np.float32)
    f = np.array([r[2] for r in rows], dtype=np.uint8)
    return t, v, f


def mdf4(t, v, f, path, comp):
    from asammdf import MDF, Signal
    m = MDF(version='4.10')
    ts = t.astype(np.float64) / 1000
    inval = (f & 1).astype(bool)
    sig_v = Signal(v, ts, name='value', unit='V', invalidation_bits=inval)
    sig_f = Signal(f, ts, name='flags')
    m.append([sig_v, sig_f], comment='DM40 DMM log')
    m.save(path, overwrite=True, compression=comp)
    return os.path.getsize(path)


def check_mdf(path, n):
    from asammdf import MDF
    m = MDF(path)
    df = m.to_dataframe()
    assert len(df) == n, (len(df), n)
    csvp = path + '.export'
    m.export('csv', filename=csvp, escapechar='\\', quotechar='"')
    return True


def tdms(t, v, f, path):
    from nptdms import TdmsWriter, ChannelObject
    with TdmsWriter(path) as w:
        # Several segments, as a logger appending every minute would write.
        step = 600
        for i in range(0, len(t), step):
            w.write_segment([ChannelObject('log', 'time_ms', t[i:i + step]),
                             ChannelObject('log', 'value', v[i:i + step]),
                             ChannelObject('log', 'flags', f[i:i + step])])
    return os.path.getsize(path)


def parquet(t, v, f, path, comp):
    import pyarrow as pa
    import pyarrow.parquet as pq
    tb = pa.table({'time_ms': t, 'value': v, 'flags': f})
    pq.write_table(tb, path, compression=comp, row_group_size=600 if comp == 'zstd-small' else None) \
        if comp != 'zstd-small' else pq.write_table(tb, path, compression='zstd', row_group_size=600)
    return os.path.getsize(path)


def hdf5(t, v, f, path):
    import h5py
    with h5py.File(path, 'w') as h:
        for nm, a in (('time_ms', t), ('value', v), ('flags', f)):
            h.create_dataset(nm, data=a, chunks=(min(len(a), 600),), compression='gzip',
                             compression_opts=6, shuffle=True, maxshape=(None,))
    return os.path.getsize(path)


def main():
    D, O = sys.argv[1], sys.argv[2]
    os.makedirs(O, exist_ok=True)
    cols = ['CSV', 'DLG v1', 'MDF4 plain', 'MDF4 DZ deflate', 'MDF4 DZ transposed', 'TDMS (600-row segments)',
            'Parquet zstd', 'Parquet zstd, 600-row groups', 'HDF5 gzip+shuffle, 600 chunks', 'CSV.gz -9', 'CSV.xz -9']
    print('| set | ' + ' | '.join(cols) + ' |')
    print('|---' * (len(cols) + 1) + '|')
    for name, k in SETS:
        hdr, unit, rows, _ = load('%s/%s.csv' % (D, k))
        csvb = open('%s/%s.csv' % (D, k), 'rb').read()
        dl = os.path.getsize('%s/%s.dlg' % (D, k))
        n = len(rows)
        t, v, f = arrays(rows)
        cells = [len(csvb), dl]
        for comp in (0, 1, 2):
            p = '%s/%s_c%d.mf4' % (O, k, comp)
            cells.append(mdf4(t, v, f, p, comp))
            check_mdf(p, n)
        cells.append(tdms(t, v, f, '%s/%s.tdms' % (O, k)))
        cells.append(parquet(t, v, f, '%s/%s.parquet' % (O, k), 'zstd'))
        cells.append(parquet(t, v, f, '%s/%s_g.parquet' % (O, k), 'zstd-small'))
        cells.append(hdf5(t, v, f, '%s/%s.h5' % (O, k)))
        cells.append(len(gzip.compress(csvb, 9)))
        cells.append(len(lzma.compress(csvb, preset=9)))
        print('| %s | %s |' % (name, ' | '.join('%.2f' % (c / n) for c in cells)))
    print('\n(bytes per reading, whole files incl. headers; MDF4 files load in asammdf and export to CSV)')


if __name__ == '__main__':
    main()
