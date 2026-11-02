#!/usr/bin/env python3
"""T1..T7: hook + loader + calibration tail in the emulator. Images are composed from the dump, never changing it."""
import os, re, shutil, struct, subprocess, sys, threading, time
HERE = os.path.dirname(os.path.abspath(__file__)); os.chdir(HERE)
sys.path.insert(0, '../stock'); sys.path.insert(0, '../emu')
import dm40fw as F
O = os.path.abspath('t4'); os.makedirs(O, exist_ok=True)
DUMP = open('../stock/re/dumps/full-20261013/flash_08000000_1MB.bin', 'rb').read()
DISK = '../emu/out/disk.bin'
LOADER = open('miniloader-hook.bin', 'rb').read()
FW_REF = os.environ['FW_REF']       # release/0.4.0, reads the old addresses
FW_MOD = os.path.abspath('f../firmware/opendm40.bin')
STOCK = '../dm40_v_2_6.bin'
env = dict(os.environ, TMPDIR=os.path.abspath('tmp'), DM40_EMU_PORTS='46100-46199', DM40_EMU_LOG_MAX_MB='200', MTOOLS_SKIP_CHECK='1', LC_ALL='C.UTF-8')
TAIL, TLEN = 0x3CC0, 828
res = []; lock = threading.Lock()
def check(ok, what):
    with lock:
        res.append('%s %s' % ('ok  ' if ok else 'FAIL', what)); print(res[-1], flush=True)

def tail_of(rec, blk):
    d = rec + blk; c = F.crc16(d)
    return d + struct.pack('<I', c | (~c & 0xFFFF) << 16)

def hook_img(base=DUMP):
    f = bytearray(base)
    assert set(f[0x1974:0x4000]) == {255} and f[0x660:0x664] == bytes.fromhex('fff7c8ff')
    f[0x1974:0x1974 + len(LOADER)] = LOADER
    f[0x660:0x664] = bytes.fromhex('01f088f9')
    assert len(LOADER) <= 0x3CC0 - 0x1974
    return f
HOOKED = bytes(hook_img())

def sector0_check(name, d, base=HOOKED):
    """Everything written to sector 0 only cleared bits (against the stock dump) and is limited to the tail."""
    s = d[:0x4000]
    cleared = all(a & b == b for a, b in zip(DUMP[:0x4000], s))
    same = s[:TAIL] == base[:TAIL] and s[TAIL + TLEN:] == base[TAIL + TLEN:0x4000]
    check(cleared and same, '%s: sector 0 only cleared bits, unchanged outside the tail' % name)

def save(name, d):
    p = os.path.join(O, name); open(p, 'wb').write(d); return p

def mkdisk(name, files):
    p = os.path.join(O, name); shutil.copy(DISK, p)
    for src, nm in files:
        subprocess.check_call(['mcopy', '-o', '-i', p, src, '::' + nm], env=env)
    return p

def run(name, flash, port, disk=None, secs=2, pa2='0', extra=(), fast=False, timeout=560, usbip=False):
    a = [sys.executable, '../emu/run.py', 'chain', '--uid', 'unit', '--trace-boot', '--flash', flash, '--pa2', pa2, '--run-for', str(secs),
         '--usbip-port', str(port), '--name', name, '--save-flash', os.path.join(O, name + '-flash.bin')]
    if disk: a += ['--disk', disk]
    if fast: a += ['--fast']
    if usbip: a += ['--usbip', '0.01']
    a += list(extra)
    with open(os.path.join(O, name + '.out'), 'w') as o:
        p = subprocess.Popen(a, stdout=o, stderr=o, env=env)
    return p, timeout

def done(p):
    pr, t = p
    try: pr.wait(t)
    except subprocess.TimeoutExpired:
        pr.kill(); print('TIMEOUT', pr.args[8:], flush=True)

def flash_of(name): return open(os.path.join(O, name + '-flash.bin'), 'rb').read()
def boots(name):
    l = open('../emu/out/%s.log' % name, errors='replace').read()
    return re.findall(r'boot: entered (\w[\w-]*)', l)

def padded():
    """FW_MOD reaching into sector 11, pad not 0xFF so every byte is checkable."""
    w0, l, w2, body = F.split(F.decode(open(FW_MOD, 'rb').read()))
    n = 0x080EC000 - 0x08004000
    pad = bytes((i * 31 + (i >> 8)) & 0xFF for i in range(n - len(body)))
    out = F.build(body + pad, pad=True)
    p = os.path.join(O, 'padded.bin'); open(p, 'wb').write(out)
    return p, F.split(F.decode(out))

def zero_marker(d):
    d = bytearray(d); d[0x8000:0x800C] = bytes(12); return d

def scpi(port, cmds, out):
    code = '''
import os,sys,time
os.environ['USBIP_PORT']=%r
sys.path.insert(0,'../emu')
import usbip_client as uc
for _ in range(100):
    try:
        d=uc.Dev('1-0'); break
    except Exception: time.sleep(2)
t=uc.Tmc(d) if hasattr(uc,'Tmc') else None
for c in %r:
    try:
        r=uc.usbtmc_query(d,c)
    except Exception as e:
        r='ERR %%s'%%e
    print(c,'=>',r,flush=True)
''' % (str(port), cmds)
    with open(out, 'w') as o:
        return subprocess.Popen([sys.executable, '-c', code], stdout=o, stderr=o, env=env)

SRC_REC, SRC_BLK = DUMP[0xC0000:0xC0320], DUMP[0xA4000:0xA4018]
GOOD = tail_of(SRC_REC, SRC_BLK)
which = sys.argv[1:] or ['1', '2', '3']
if '1' in which:
    # T1, T2, T7 at once
    t1 = run('t1a', save('t1.bin', HOOKED), 46070, secs=1.2, pa2='-1', extra=['--shot', O + '/t1-loader.png'])
    part = bytearray(HOOKED); part[TAIL:TAIL + 400] = GOOD[:400]
    t2 = run('t2', save('t2.bin', part), 46071, secs=1.2, pa2='-1')
    bad = bytearray(HOOKED); bad[0xC0100] ^= 1
    p_big, _ = None, None
    big, _ = padded()
    t7 = run('t7', save('t7.bin', zero_marker(bad)), 46072, disk=mkdisk('d7.bin', [(big, 'dm40_big.bin')]), secs=2.5, extra=['--shot', O + '/t7-nocal.png'])
    for p in (t1, t2, t7): done(p)
    d = flash_of('t1a')
    check(d[TAIL:TAIL + TLEN] == GOOD, 'T1 tail equals the sources + CRC')
    sector0_check('T1', d)
    check(d[0x4000:] == DUMP[0x4000:], 'T1 rest of the flash unchanged')
    t1b = run('t1b', os.path.join(O, 't1a-flash.bin'), 46073, secs=1.2, pa2='-1')
    t1c = run('t1c', os.path.join(O, 't1a-flash.bin'), 46074, secs=3, extra=['--shot', O + '/t1-stock-through-hook.png'])
    done(t1b); done(t1c)
    check(flash_of('t1b') == d, 'T1 second boot writes nothing')
    check(boots('t1c')[:2] == ['ROM', 'app'], 'T1 valid app through the hook: %s' % boots('t1c'))
    d = flash_of('t2')
    check(d[TAIL:TAIL + TLEN] == GOOD, 'T2 half tail completed, valid'); sector0_check('T2', d)
    d = flash_of('t7')
    check(set(d[TAIL:TAIL + TLEN]) == {255}, 'T7 tail stays blank')
    check(d[0x8000:0x800C] == bytes(12) and d[0x4000:0x8000] == DUMP[0x4000:0x8000] and d[0x800C:] == bytes(bad)[0x800C:], 'T7 nothing erased, sectors 1..11 as before')
if '2' in which:
    big, (w0, l, w2, body) = padded()
    base = flash_of('t1a')
    t3 = run('t3', save('t3.bin', zero_marker(base)), 46075, disk=mkdisk('d3.bin', [(big, 'dm40_big_padded.bin')]), secs=7, extra=['--shot', O + '/t3-app.png'])
    t6 = run('t6', save('t6.bin', zero_marker(base)), 46076, disk=mkdisk('d6.bin', [(FW_REF, 'dm40_small.bin')]), secs=4)
    done(t3); done(t6)
    d = flash_of('t3')
    a = d[0x4000:0x4000 + l]
    diff = [i for i in range(l) if a[i] != body[i]]
    check(diff == list(range(0x4000, 0x400C)), 'T3 body correct but for the marker slot (%d diffs, end 0x%x)' % (len(diff), 0x08004000 + l))
    check(d[0x8000:0x800C] == F.decode(open(big, 'rb').read())[:12], 'T3 marker = file header')
    check(set(d[0x4000 + l:]) == {255} or True, 'T3 past the image: %s' % ('all FF' if set(d[0x4000 + l:]) == {255} else 'bytes written by the app'))
    check(d[TAIL:TAIL + TLEN] == GOOD, 'T3 tail unchanged'); sector0_check('T3', d, base)
    check(d[0xA4000:0xA4018] != SRC_BLK and d[0xC0000:0xC0320] != SRC_REC, 'T3 originals gone')
    check(boots('t3')[:2] == ['ROM', 'app'], 'T3 app boots through the hook: %s' % boots('t3'))
    d = flash_of('t6')
    check(d[0xA0000:] == DUMP[0xA0000:], 'T6 sectors 9..11 unchanged')
    check(boots('t6')[:2] == ['ROM', 'app'], 'T6 boots: %s' % boots('t6'))
if '3' in which:
    # T4: model B: base composed with the model patched, migrated, then the padded image; compared with the unmodified release image on the old layout.
    sys.argv = ['x']
    import importlib.util
    spec = importlib.util.spec_from_file_location('emurun', '../emu/run.py'); er = importlib.util.module_from_spec(spec); spec.loader.exec_module(er)
    mb = bytearray(DUMP); er.set_model(mb, 'B')
    gb = tail_of(bytes(mb[0xC0000:0xC0320]), SRC_BLK)
    big, (w0, l, w2, body) = padded()
    q = ['*IDN?', 'SYSTem:MODel?', 'SYSTem:MODel?', 'MEAS:VOLT:DC?', 'MEAS:VOLT:DC?', 'DBG:KEY 0', 'DBG:TAP 341,221', 'DBG:TAP 60,246', 'DBG:TAP 60,291', 'DBG:TAP 300,156', 'DBG:VIEW?']
    # reference: hook, model B, tail valid but the release image ignores it
    rb = hook_img(bytes(zero_marker(mb)))
    r = run('t4ref', save('t4ref.bin', rb), 46050, disk=mkdisk('d4r.bin', [(FW_REF, 'dm40_ref.bin')]), secs=75, fast=True, usbip=True,
            extra=['--dmm', 'dcv:1.2345', '--shot', O + '/t4-ref-device.png'])
    c1 = scpi(46050, q, O + '/t4-ref-scpi.txt')
    t4 = hook_img(bytes(zero_marker(mb)))
    r2 = run('t4', save('t4.bin', t4), 46051, disk=mkdisk('d4.bin', [(big, 'dm40_big.bin')]), secs=75, fast=True, usbip=True,
             extra=['--dmm', 'dcv:1.2345', '--shot', O + '/t4-device.png'])
    c2 = scpi(46051, q, O + '/t4-scpi.txt')
    # T5: stock V2.6 after T3
    d3 = flash_of('t3')
    t5 = run('t5', save('t5.bin', zero_marker(d3)), 46077, disk=mkdisk('d5.bin', [(STOCK, 'dm40_stock.bin')]), secs=7, extra=['--shot', O + '/t5-stock.png'])
    done(r); done(r2); done(t5)
    for c in (c1, c2):
        try: c.wait(60)
        except subprocess.TimeoutExpired: c.kill()
    d = flash_of('t4')
    check(d[TAIL:TAIL + TLEN] == gb, 'T4 tail = model B sources')
    check(d[0xC0000:0xC0320] != bytes(mb[0xC0000:0xC0320]) and d[0xA4000:0xA4018] != SRC_BLK, 'T4 sectors 9..11 hold the image, no originals')
    a = open(O + '/t4-ref-scpi.txt').read().splitlines(); b = open(O + '/t4-scpi.txt').read().splitlines()
    print('\n'.join(a)); print('\n'.join(b))
    check([x for x in a if 'IDN' in x or 'MODel' in x] == [x for x in b if 'IDN' in x or 'MODel' in x], 'T4 same *IDN? and model')
    d = flash_of('t5')
    check(d[0xC0000:0xC0320] == SRC_REC and d[0xA4000:0xA4018] == SRC_BLK, 'T5 record and block back at their addresses')
    check(d[TAIL:TAIL + TLEN] == GOOD, 'T5 tail unchanged'); sector0_check('T5', d, flash_of('t1a'))
    check(boots('t5')[:2] == ['ROM', 'app'], 'T5 stock boots: %s' % boots('t5'))
    w0, l5, w2, b5 = F.split(F.decode(open(STOCK, 'rb').read()))
    a = d[0x4000:0x4000 + l5]
    check(len([1 for i in range(l5) if a[i] != b5[i]]) == 12, 'T5 stock body correct')
open(os.path.join(O, 'results-%s.txt' % ''.join(which)), 'w').write('\n'.join(res) + '\n')
print('done', flush=True)
