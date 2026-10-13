import test from 'node:test';
import assert from 'node:assert/strict';
import { packet, hex, validFrame, FrameDecoder, measurement, ranges, GROUPS } from '../protocol.js';
import { Connection } from '../transport.js';
if (!globalThis.navigator) Object.defineProperty(globalThis, 'navigator', { value: {}, configurable: true });

function response(command, data, discovery = false) {
  const bytes = [0xdf, discovery ? 255 : 5, discovery ? 255 : 3, command, data.length, ...data];
  return Uint8Array.from([...bytes, -bytes.reduce((a, b) => a + b, 0) & 255]);
}
function sample(mode = 0x30, format = 0x18, raw = 12345, flags = 4) {
  return response(9, [mode, flags, 4, 2, format, 0x88, 0x13, 0xf4, 1, raw & 255, raw >> 8]);
}
test('documented command bytes and zero checksum wraparound', () => {
  assert.equal(hex(packet(9)), 'AF 05 03 09 00 40');
  assert.equal(hex(packet(4, [1])), 'AF 05 03 04 01 01 43');
  assert.equal(hex(packet(0, [], true)), 'AF FF FF 00 00 53');
  assert.equal(packet(6, [66]).at(-1), 0);
  for (const group of GROUPS) for (const mode of group) assert.equal(packet(6, [mode.mode]).reduce((a, b) => a + b, 0) & 255, 0);
});
test('every possible split of a measurement and combined responses', () => {
  const bytes = sample();
  for (let split = 1; split < bytes.length; split++) {
    const decoder = new FrameDecoder();
    assert.deepEqual(decoder.push(bytes.slice(0, split)), []);
    assert.deepEqual(decoder.push(bytes.slice(split)), [bytes]);
  }
  assert.equal(new FrameDecoder().push([...bytes, ...bytes]).length, 2);
});
test('noise, bad checksum and invalid lengths do not become readings', () => {
  const bytes = sample(), bad = bytes.slice(); bad[14] ^= 1;
  const decoder = new FrameDecoder();
  assert.deepEqual(decoder.push([0, 9, 0xdf, 5, 3, 9, 255, ...bad, ...bytes]), [bytes]);
  assert.equal(validFrame(bad), false); assert.throws(() => measurement(bad));
  assert.throws(() => measurement(bytes.slice(0, 16)));
});
test('voltage decimals, sign, unit prefixes and SI values', () => {
  assert.equal(measurement(sample()).main.text, '1.2345');
  assert.equal(measurement(sample()).main.value, 1.2345);
  assert.equal(measurement(sample(0x30, 0x19)).main.text, '-1.2345');
  const mv = measurement(sample(0x30, 4)); assert.equal(mv.main.text, '123.45'); assert.equal(mv.main.unit, 'mV'); assert.ok(Math.abs(mv.main.value - .12345) < 1e-10);
  assert.equal(measurement(sample(0x30, 8)).main.value, 1.2345);
});
test('current, resistance, capacitance, frequency and temperature units', () => {
  for (const [mode, fmt, value, unit] of [[0x39, 0x28, 1.2345, 'A'], [0x32, 0x18, 1234.5, 'kΩ'], [3, 0x14, .00012345, 'µF'], [5, 4, 123.45, 'Hz'], [0x45, 2, 1234.5, '°C']]) {
    const m = measurement(sample(mode, fmt)); assert.ok(Math.abs(m.main.value - value) < 1e-10, unit); assert.equal(m.main.unit, unit);
  }
});
test('auxiliary ordering: frequency at bytes 10/11, duty at 12/13', () => {
  const m = measurement(sample(0x70));
  assert.deepEqual(m.aux.map(a => [a.label, a.text, a.unit]), [['FREQ', '50.00', 'Hz'], ['DUTY', '50.0', '%']]);
});
test('overload, negative zero, flags and unknown modes', () => {
  const m = measurement(sample(0x30, 0x18, 65535, 0xed));
  assert.equal(m.main.text, 'OL'); assert.equal(m.main.value, null);
  assert.equal(m.hold, true); assert.equal(m.rel, true); assert.equal(m.charging, true); assert.equal(m.locked, true); assert.equal(m.battery, 5);
  assert.equal(measurement(sample(7)).main.value, null);
  assert.equal(measurement(sample(0x30, 0x19, 0)).main.text, '-0.0000');
  assert.equal(measurement(sample(0x44, 0x84, 50)).main.text, '0.50');
});
test('model-specific ranges and preservation of submode', () => {
  assert.equal(ranges(0x70, 'DM40A')[0].label, '400 mV');
  assert.equal(ranges(0x70, 'DM40B')[1].label, '5 V');
  assert.equal(ranges(0x70, 'DM40C').at(-1).mode, 0x70);
  assert.equal(ranges(0x39).at(-1).mode, 0x39);
  assert.deepEqual(ranges(3), []);
});
test('command queue waits for matching response and validates ACK', async () => {
  const c = new Connection(), writes = [];
  c.write = async bytes => { writes.push(bytes[3]); };
  const first = c.request(4, [1]), second = c.request(6, [0x70]);
  await new Promise(resolve => setImmediate(resolve)); assert.deepEqual(writes, [4]);
  c.receive(sample()); assert.deepEqual(writes, [4]);
  c.receive(response(4, [0])); await first;
  await new Promise(resolve => setImmediate(resolve)); assert.deepEqual(writes, [4, 6]);
  c.receive(response(6, [1])); await assert.rejects(second, /отклонил/); await c.close();
});
test('timeout closes connection so late ACK cannot acknowledge the next command', async () => {
  const c = new Connection({ timeout: 15 }), writes = [];
  c.write = async bytes => writes.push(bytes);
  const first = c.request(4, [1]), second = c.request(4, [0]);
  await Promise.all([assert.rejects(first, /не ответил/), assert.rejects(second, /закрыто/)]);
  assert.equal(c.closed, true); assert.equal(writes.length, 1);
});
test('disconnect cancels outstanding request; rejected write cleans up', async () => {
  const c = new Connection(); c.write = async () => {};
  const p = c.request(9); await new Promise(resolve => setImmediate(resolve));
  await c.close(); await assert.rejects(p, /закрыто/);
  const broken = new Connection(); broken.write = async () => { throw new Error('write failed'); };
  await assert.rejects(broken.request(9), /write failed/); assert.equal(broken.closed, true);
});
test('Web Serial streams deliver frames and release all locks on disconnect', async () => {
  let input, closed = false, options;
  const port = {
    readable: new ReadableStream({ start(controller) { input = controller; } }),
    writable: new WritableStream({ write(bytes) { input.enqueue(bytes[3] === 9 ? sample() : response(bytes[3], [0])); } }),
    async open(value) { options = value; }, async close() { closed = true; },
  };
  Object.defineProperty(navigator, 'serial', { configurable: true, value: { requestPort: async () => port } });
  const received = [], c = new Connection({ onFrame: f => received.push(f) });
  try {
    await c.serial(9600); assert.deepEqual(options, { baudRate: 9600 });
    assert.deepEqual(await c.request(9), sample()); assert.equal(received.length, 1);
    await c.close(); assert.equal(closed, true);
    assert.equal(port.readable.locked, false); assert.equal(port.writable.locked, false);
  } finally { delete navigator.serial; }
});
test('Web Serial stream error cancels request and reports disconnection', async () => {
  let input, reason;
  const port = {
    readable: new ReadableStream({ start(controller) { input = controller; } }),
    writable: new WritableStream({ write() { input.error(new Error('cable unplugged')); } }),
    async open() {}, async close() {},
  };
  Object.defineProperty(navigator, 'serial', { configurable: true, value: { requestPort: async () => port } });
  const c = new Connection({ onDisconnect: r => { reason = r; } });
  try {
    await c.serial(); await assert.rejects(c.request(9), /cable unplugged/);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(reason, 'cable unplugged'); assert.equal(c.closed, true);
  } finally { delete navigator.serial; }
});
