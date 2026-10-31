"""Build the v2.05 121GW rows and write rows8 with them replacing the v1.02 rows.
usage: build121.py v2_05.bin v1_02.bin rows8.json out.json"""
import sys, json
img205, img102, rin, rout = sys.argv[1:5]
d = open(img205, 'rb').read()
d102 = open(img102, 'rb').read()
B = 0x08006000

def row(a):
    return bytearray(d[a - B:a - B + 20])

# (mode, label, table name, flash address of the row, row bytes override or None)
T = []
def add(mode, label, tbl, a, data=None, note=''):
    T.append((mode, label, tbl, a, data, note))

for i, l in enumerate(['DCV 5.0000V, DC power 5.0000V, Burden Voltage', 'DCV 50.000V, DC power 50.000V',
                       'DCV 500.00V', 'DCV 1000.0V (600.0V), LowZ in DC']):
    add('dcv', l, 'dcv', 0x080288a0 + 20 * i)
for i, l in enumerate(['DCmV 50.000mV, Temperature', 'DCmV 500.00mV']):
    add('dcmv', l, 'dcmv', 0x08028f1c + 20 * i)
for i, l in enumerate(['DCA 500.00mA, DCmA 5.0000mA, DCuA 50.000uA', 'DCA 5.0000A, DCmA 50.000mA, DCuA 500.00uA',
                       'DCA 10.000A']):
    add('dca', l, 'dc_amps', 0x08028d94 + 20 * i)
for i, l in enumerate(['DC power 500.00mA, 5.0000mA, 50.000uA', 'DC power 5.0000A, 50.000mA, 500.00uA',
                       'DC power 10.000A']):
    add('dca', l, 'dc_amps_power', 0x08028d58 + 20 * i)
for i, l in enumerate(['ACV 5.0000V, AC power 5.0000V, Burden Voltage', 'ACV 50.000V, AC power 50.000V',
                       'ACV 500.00V', 'ACV 1000.0V (600.0V)']):
    add('acv', l, 'acv', 0x080289c8 + 20 * i)
r = row(0x080289c8 + 60)
r[10] |= 0x20
r[14] = (r[14] & 0x7F) | 0x20
add('acv', 'LowZ AC (edited 1000.0V row)', 'acv', 0x080289c8 + 60, r, 'modified at run time: R2A OR 0x20, R2E AND 0x7F OR 0x20')
for i, l in enumerate(['ACV 5.0000V (1ms peak)', 'ACV 50.000V (1ms peak)', 'ACV 500.00V (1ms peak)',
                       'ACV 1000.0V (600.0V) (1ms peak)']):
    add('acv', l + ' [1 ms peak table]', 'acv_1ms_peak', 0x08028904 + 20 * i)
for i, l in enumerate(['ACmV 50.000mV', 'ACmV 500.00mV']):
    add('acmv', l, 'acmv', 0x08028f44 + 20 * i)
for i, l in enumerate(['ACmV 50.000mV (1ms peak)', 'ACmV 500.00mV (1ms peak)']):
    add('acmv', l + ' [1 ms peak table]', 'acmv_1ms_peak', 0x08028f6c + 20 * i)
for i, l in enumerate(['ACA 500.00mA, ACmA 5.0000mA, ACuA 50.000uA / AC power 500.00mA, 5.0000mA, 50.000uA',
                       'ACA 5.0000A, ACmA 50.000mA, ACuA 500.00uA / AC power 50.000mA, 500.00uA',
                       'ACA 10.000A, AC power 10.000A']):
    add('aca', l, 'ac_amps', 0x08028d1c + 20 * i)
add('freq', 'freq', 'freq', 0x08028fbc)
for i, l in enumerate(['50.000Ω', '500.00Ω', '5.0000kΩ', '50.000kΩ', '500.00kΩ', '5.0000MΩ', '50.000MΩ']):
    add('ohm', l, 'ohms', 0x080285bc + 20 * i)
r = row(0x080285bc + 120)
r[10] &= 0xF7
add('ohm', '50.000MΩ (PA0 P open)', 'ohms', 0x080285bc + 120, r, 'modified at run time: R2A AND 0xF7 while EEPROM byte 0x26 ≠ 0xA5')
add('cont', 'continuity', 'continuity', 0x08029150)
add('diode', 'diode', 'diode', 0x08028f94)
for i, l in enumerate(['10.00nF', '100.0nF', '1.000uF', '10.00uF', '100.0uF', '10.00mF']):
    add('cap', l, 'cap', 0x08028748 + 20 * i)
for i, l in enumerate(['10.00nF', '100.0nF', '1.000uF', '10.00uF', '100.0uF', '10.00mF']):
    a = 0x080286d0 + 20 * i
    if d[a - B:a - B + 20] != d[0x08028748 + 20 * i - B:0x08028748 + 20 * i - B + 20]:
        add('cap', l + ' (PF8 low)', 'cap_pf8', a)

rows = json.load(open(rin))
first = next(i for i, e in enumerate(rows) if e['src'] == '121gw')
rows = [e for e in rows if e['src'] != '121gw']
new = []
for mode, label, tbl, a, data, note in T:
    b = bytes(data) if data is not None else bytes(row(a))
    in102 = d102.find(b)
    o = f"v2.05 flash 0x{a:08x} (table {tbl})"
    if note:
        o += f", {note}"
    if data is None:
        o += "; same row in v1.02" if in102 >= 0 else "; not in v1.02"
    new.append({'src': '121gw', 'mode': mode, 'label': label,
                'regs': {'%02X' % (0x20 + i): v for i, v in enumerate(b)}, 'origin': o})
rows[first:first] = new
json.dump(rows, open(rout, 'w'), indent=0, ensure_ascii=False)
for e in new:
    print(e['mode'], '|', e['label'], '|', ' '.join('%02X' % v for v in e['regs'].values()), '|', e['origin'])
