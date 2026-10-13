import { packet, FrameDecoder } from './protocol.js';
const uuid = short => `0000${short}-0000-1000-8000-00805f9b34fb`;
export const PROFILES = [
  { service: uuid('fff0'), tx: uuid('fff3'), rx: uuid('fff1') },
  { service: uuid('fff0'), tx: uuid('fff1'), rx: uuid('fff2') },
  { service: uuid('ffff'), tx: uuid('ff01'), rx: uuid('ff02') },
];
export function bluetoothSupportError() {
  if (!globalThis.isSecureContext) return 'Откройте автономный dm40.html в Chrome/Edge либо используйте localhost или HTTPS.';
  if (navigator.bluetooth) return '';
  if (/Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent)) {
    return 'Web Bluetooth выключен или не поддерживается этим браузером. В Chrome/Chromium на Linux откройте chrome://flags/#enable-experimental-web-platform-features, выберите Enabled и нажмите Relaunch. Затем снова откройте dm40.html. Web Serial доступен отдельно, если DM40 представлен последовательным портом.';
  }
  return 'Web Bluetooth недоступен. Откройте файл в Chrome/Edge с поддержкой BLE. Для Bluetooth Classic используйте Web Serial, если ОС предоставляет порт DM40.';
}
export class Connection {
  constructor({ onFrame = () => {}, onLog = () => {}, onDisconnect = () => {}, timeout = 1800 } = {}) {
    Object.assign(this, { onFrame, onLog, onDisconnect, timeout });
    this.decoder = new FrameDecoder(); this.queue = Promise.resolve(); this.closed = false;
  }
  receive(chunk) {
    this.onLog('RX', chunk);
    for (const frame of this.decoder.push(chunk)) {
      if (this.pending?.command === frame[3]) {
        const p = this.pending; this.pending = null; clearTimeout(p.timer); p.resolve(frame);
      }
      this.onFrame(frame);
    }
  }
  request(command, data = [], discovery = false) {
    const task = this.queue.then(async () => {
      if (this.closed) throw new Error('Соединение закрыто');
      const bytes = packet(command, data, discovery);
      const response = new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          this.pending = null;
          const error = new Error('DM40 не ответил. Проверьте Bluetooth и откройте экран мультиметра.');
          reject(error); void this.close(error.message);
        }, this.timeout);
        this.pending = { command, resolve, reject, timer };
      });
      // Install rejection handler before an asynchronous write can time out.
      response.catch(() => {});
      try { this.onLog('TX', bytes); await this.write(bytes); }
      catch (error) { await this.close(error.message); throw error; }
      const frame = await response;
      if ([1, 3, 4, 5, 6, 7].includes(command) && (frame[4] !== 1 || frame[5] !== 0)) throw new Error('Прибор отклонил команду');
      return frame;
    });
    this.queue = task.catch(() => {}); return task;
  }
  async bluetooth() {
    const unsupported = bluetoothSupportError();
    if (unsupported) throw new Error(unsupported);
    this.device = await navigator.bluetooth.requestDevice({ acceptAllDevices: true, optionalServices: [...new Set(PROFILES.map(p => p.service))] });
    this.disconnected = () => void this.close('Bluetooth-соединение потеряно');
    this.device.addEventListener('gattserverdisconnected', this.disconnected);
    const server = await this.device.gatt.connect();
    for (const profile of PROFILES) {
      try {
        const service = await server.getPrimaryService(profile.service);
        const tx = await service.getCharacteristic(profile.tx), rx = await service.getCharacteristic(profile.rx);
        if (!(tx.properties.write || tx.properties.writeWithoutResponse) || !(rx.properties.notify || rx.properties.indicate)) continue;
        this.tx = tx; this.rx = rx;
        this.notification = event => {
          const v = event.target.value; this.receive(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
        };
        rx.addEventListener('characteristicvaluechanged', this.notification);
        try { await rx.startNotifications(); }
        catch (error) { rx.removeEventListener('characteristicvaluechanged', this.notification); throw error; }
        this.write = bytes => tx.properties.write ? tx.writeValueWithResponse(bytes) : tx.writeValueWithoutResponse(bytes);
        return this.device.name || 'DM40';
      } catch { /* Try the next documented profile. */ }
    }
    throw new Error('Совместимый BLE-профиль не найден. Для Bluetooth Classic выберите Web Serial.');
  }
  async serial(baudRate = 115200) {
    if (!navigator.serial) throw new Error('Web Serial недоступен в этом браузере. Используйте настольный Chrome/Edge.');
    this.port = await navigator.serial.requestPort();
    await this.port.open({ baudRate });
    this.reader = this.port.readable.getReader(); this.writer = this.port.writable.getWriter();
    this.write = bytes => this.writer.write(bytes);
    this.readTask = (async () => {
      try {
        while (!this.closed) { const { value, done } = await this.reader.read(); if (done) break; this.receive(value); }
      } catch (error) { this.readError = error.message; }
      finally { this.reader.releaseLock(); }
    })();
    this.readTask.then(() => { if (!this.closed) void this.close(this.readError || 'Последовательный порт отключён'); });
    return 'DM40 · Serial';
  }
  async close(reason = '') {
    if (this.closed) return;
    this.closed = true;
    if (this.pending) { clearTimeout(this.pending.timer); this.pending.reject(new Error(reason || 'Соединение закрыто')); this.pending = null; }
    this.device?.removeEventListener('gattserverdisconnected', this.disconnected);
    this.rx?.removeEventListener('characteristicvaluechanged', this.notification);
    this.device?.gatt?.disconnect();
    try { await this.reader?.cancel(); } catch { /* Device may already be gone. */ }
    await this.readTask;
    try { this.writer?.releaseLock(); await this.port?.close(); } catch { /* Already disconnected. */ }
    this.onDisconnect(reason);
  }
}
