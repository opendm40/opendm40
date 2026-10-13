// Mode/range and display mappings cross-checked against Urobotos/DM40-Wireless.
// See THIRD_PARTY_NOTICES.md. Framing follows the parent repository's README.
export const hex = bytes => Array.from(bytes, x => x.toString(16).padStart(2, '0')).join(' ').toUpperCase();
export function packet(command, data = [], discovery = false) {
  const bytes = [0xaf, discovery ? 255 : 5, discovery ? 255 : 3, command, data.length, ...data];
  return Uint8Array.from([...bytes, (-bytes.reduce((a, b) => a + b, 0)) & 255]);
}
export function validFrame(bytes) {
  return bytes.length >= 6 && bytes[0] === 0xdf &&
    ((bytes[1] === 5 && bytes[2] === 3) || (bytes[1] === 255 && bytes[2] === 255)) &&
    bytes[4] <= 194 && bytes.length === bytes[4] + 6 && (bytes.reduce((a, b) => a + b, 0) & 255) === 0;
}
export class FrameDecoder {
  buffer = [];
  push(chunk) {
    this.buffer.push(...chunk);
    const frames = [];
    while (this.buffer.length) {
      if (this.buffer[0] !== 0xdf) { this.buffer.shift(); continue; }
      if (this.buffer.length < 5) break;
      if (!((this.buffer[1] === 5 && this.buffer[2] === 3) || (this.buffer[1] === 255 && this.buffer[2] === 255)) || this.buffer[4] > 194) {
        this.buffer.shift(); continue;
      }
      const length = this.buffer[4] + 6;
      if (this.buffer.length < length) break;
      const frame = Uint8Array.from(this.buffer.slice(0, length));
      if (!validFrame(frame)) { this.buffer.shift(); continue; }
      this.buffer.splice(0, length); frames.push(frame);
    }
    return frames;
  }
}
export const GROUPS = [
  [{ name: 'V DC', mode: 0x30 }, { name: 'V AC', mode: 0x70 }, { name: 'V AC+DC', mode: 0xb0 }],
  [{ name: 'A DC', mode: 0x39 }, { name: 'A AC', mode: 0x79 }, { name: 'A AC+DC', mode: 0xb9 }],
  [{ name: 'Ω', mode: 0x32 }, { name: 'Ω ONLINE', mode: 0x72 }],
  [{ name: 'CAP', mode: 3 }], [{ name: 'DIODE', mode: 4 }, { name: 'CONT', mode: 0x44 }],
  [{ name: 'Hz', mode: 5 }, { name: '°C', mode: 0x45 }],
];
export function modeInfo(mode) {
  const group = mode & 7, sub = mode >> 6;
  return { group, sub, range: (mode >> 3) & 7, name: GROUPS[group]?.[sub]?.name ?? `MODE 0x${mode.toString(16).toUpperCase()}` };
}
export function ranges(mode, model = 'DM40C') {
  const { group, sub } = modeInfo(mode), n = ({ DM40A: 4, DM40B: 5, DM40C: 6 })[model] ?? 6;
  const labels = [
    [`${n}00 mV`, `${n} V`, `${n}0 V`, `${n}00 V`, '1000 V', 'AUTO', 'AUTO+'],
    [`${n}00 µA`, `${n} mA`, `${n}0 mA`, `${n}00 mA`, `${n} A`, '10 A', 'AUTO', 'AUTO+'],
    [`${n}00 Ω`, `${n} kΩ`, `${n}0 kΩ`, `${n}00 kΩ`, `${n} MΩ`, `${n}0 MΩ`, 'AUTO'],
  ][group];
  return labels?.map((label, i) => ({ label, mode: group | (i << 3) | (sub << 6) })) ?? [];
}
function value(format, raw, type) {
  const decimals = (format >> 1) & 7, prefix = (format >> 4) & 3;
  const units = { voltage: ['mV', 'V', 'kV', 'MV'], current: ['µA', 'mA', 'A', 'kA'], resistance: ['Ω', 'kΩ', 'MΩ', 'GΩ'], capacitance: ['nF', 'µF', 'mF', 'F'], frequency: ['Hz', 'kHz', 'MHz', 'GHz'] };
  const offset = { voltage: -3, current: -6, resistance: 0, capacitance: -9, frequency: 0 }[type];
  let places = decimals, unit = units[type]?.[prefix] ?? '', exponent = prefix * 3 + (offset ?? 0);
  if (type === 'temperature' || type === 'fahrenheit' || type === 'internal') { places = 1; exponent = 0; unit = type === 'fahrenheit' ? '°F' : '°C'; }
  if (type === 'duty') { places = 1; exponent = 0; unit = '%'; }
  if (type === 'voltage' && (format & 0xfe) === 0x08) { unit = 'V'; exponent = 0; }
  const display = raw / 10 ** places * (format & 1 ? -1 : 1);
  return { raw, format, unit, text: raw === 65535 ? 'OL' : `${raw === 0 && (format & 1) ? '-' : ''}${display.toFixed(places)}`, value: raw === 65535 ? null : display * 10 ** exponent,
    baseUnit: { voltage: 'V', current: 'A', resistance: 'Ω', capacitance: 'F', frequency: 'Hz' }[type] ?? unit,
    special: !!(format & 0x80) };
}
export function measurement(frame) {
  if (!validFrame(frame) || frame[3] !== 9 || frame[4] !== 11 || frame[1] !== 5) throw new Error('Неверный пакет измерения');
  const info = modeInfo(frame[5]);
  const type = ['voltage', 'current', 'resistance', 'capacitance', info.sub === 1 ? 'resistance' : 'voltage', info.sub === 1 ? 'temperature' : 'frequency'][info.group];
  const slot = (fmt, pos, kind) => value(frame[fmt], frame[pos] | frame[pos + 1] << 8, kind);
  let aux = [];
  if (info.group <= 1 && info.sub === 1) aux = [{ ...slot(7, 10, 'frequency'), label: 'FREQ' }, { ...slot(8, 12, 'duty'), label: 'DUTY' }];
  if (info.group <= 1 && info.sub === 2) aux = [{ ...slot(7, 10, type), label: 'AC' }, { ...slot(8, 12, type), label: 'DC' }];
  if (info.group === 4 && info.sub === 0) aux = [{ ...slot(8, 12, 'resistance'), label: 'RES' }];
  if (info.group === 5) aux = info.sub === 1 ? [{ ...slot(7, 10, 'internal'), label: 'INT' }, { ...slot(8, 12, 'fahrenheit'), label: 'TEMP' }] : [{ ...slot(8, 12, 'duty'), label: 'DUTY' }];
  const main = slot(9, 14, type);
  if (!type || !GROUPS[info.group]?.[info.sub]) { main.text = '—'; main.value = null; main.unit = ''; }
  return { ...info, mode: frame[5], flags: frame[6], hold: !!(frame[6] & 128), rel: !!(frame[6] & 32), locked: !!(frame[6] & 64), battery: frame[6] & 7, charging: !!(frame[6] & 8), main, aux, raw: hex(frame) };
}
