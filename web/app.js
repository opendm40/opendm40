import { GROUPS, hex, measurement, modeInfo, ranges } from './protocol.js';
import { Connection, bluetoothSupportError } from './transport.js';
const $ = id => document.getElementById(id);
let connection = null, demo = false, connecting = false, current = null, timer, lastUpdate = 0, busy = false;
let history = [], saved = [], logs = [], packets = 0, seriesKey = '', demoMode = 0x30, demoHold = false, demoRel = false, demoZero = 0;
const groupButtons = [...document.querySelectorAll('[data-group]')];
function message(text = '') { $('message').textContent = text; $('message').hidden = !text; }
function active() { return demo || (!!connection && !connection.closed && !connecting); }
function controls() {
  const enabled = active() && !!current && !busy;
  groupButtons.forEach(button => button.disabled = !enabled);
  for (const id of ['hold', 'rel', 'reading']) $(id).disabled = !enabled;
  $('range').disabled = !enabled || !ranges(current?.mode ?? 0).length;
  $('rel').disabled ||= !!current?.hold;
  $('connect').disabled = connecting || demo;
  $('connect').textContent = connection && !connection.closed && !connecting ? 'Отключить' : 'Подключить';
  $('transport').disabled = connecting || !!connection || demo;
  $('demo').disabled = connecting || !!connection;
  $('demo').textContent = demo ? 'Завершить демо' : 'Деморежим';
  $('demo-badge').hidden = !demo;
  $('export').disabled = !history.length;
}
function log(direction, bytes) {
  packets++; logs.push(`${new Date().toLocaleTimeString('ru-RU')}  ${direction}  ${hex(bytes)}`);
  if (logs.length > 150) logs.shift();
  $('log').textContent = logs.join('\n'); $('packet-count').textContent = `${packets} пакетов`;
}
function slots() {
  $('slots').replaceChildren(...Array.from({ length: 6 }, (_, index) => {
    const div = document.createElement('div'); div.className = `slot${saved[index] ? '' : ' empty'}`;
    div.textContent = saved[index] ? `${saved[index].text} ${saved[index].unit}` : `${String(index + 1).padStart(2, '0')}  —`;
    if (saved[index]) div.title = `${saved[index].mode} · ${saved[index].time} · ${saved[index].demo ? 'DEMO' : 'DM40'}`;
    return div;
  }));
}
function render(m) {
  current = m; lastUpdate = Date.now();
  const key = `${m.mode}:${m.rel}:${m.main.baseUnit}`;
  if (seriesKey !== key) { history = []; seriesKey = key; }
  $('reading').textContent = m.main.text; $('unit').textContent = m.main.unit;
  $('kind').textContent = m.group <= 1 ? ['DC', 'AC', 'AC+DC'][m.sub] ?? '' : m.group === 4 ? (m.sub === 1 ? ')))' : '→|') : '';
  $('mode-name').textContent = m.name;
  $('hold').textContent = m.hold ? 'HOLD' : 'RUN'; $('hold').classList.toggle('active', m.hold);
  $('rel').classList.toggle('active', m.rel); $('relative').textContent = m.rel ? 'REL Δ' : '';
  $('range').textContent = ranges(m.mode, $('model').value).find(r => r.mode === m.mode)?.label ?? 'AUTO';
  $('battery').textContent = `${m.charging ? 'ϟ ' : ''}▱ ${m.battery}`;
  $('battery').title = `Уровень батареи по данным прибора: ${m.battery}`;
  $('lock').textContent = m.locked ? '▣' : '';
  $('aux').replaceChildren(...(m.aux.length ? m.aux : [{ label: '', text: '—', unit: '' }, { label: '', text: '—', unit: '' }]).map(a => {
    const span = document.createElement('span'), label = document.createElement('small'); label.textContent = a.label;
    span.append(label, `${a.text} ${a.unit}`); return span;
  }));
  const baseLabels = ['V ⎓', 'A ⎓', 'Ω', '┤├', '→| · )))', 'Hz'];
  groupButtons.forEach((button, i) => {
    button.classList.toggle('active', i === m.group);
    button.querySelector('b').textContent = i === m.group ? (i <= 1 ? `${i === 0 ? 'V' : 'A'} ${['⎓', '~', '≂'][m.sub] ?? '?'}` : i === 2 ? (m.sub ? 'Ω ON' : 'Ω') : i === 4 ? (m.sub ? ')))' : '→|') : i === 5 ? (m.sub ? '°C' : 'Hz') : '┤├') : baseLabels[i];
  });
  if (!m.hold && m.main.value !== null) {
    history.push({ time: new Date().toISOString(), ...m.main, mode: m.name, demo, rel: m.rel, raw: m.raw });
    if (history.length > 10000) history.shift();
  }
  $('freshness').textContent = demo ? 'ИСКУССТВЕННЫЕ ДАННЫЕ' : 'ДАННЫЕ ПРИБОРА';
  draw(); controls();
}
function draw() {
  const canvas = $('chart'), rect = canvas.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(rect.width * dpr); canvas.height = Math.round(rect.height * dpr);
  const ctx = canvas.getContext('2d'); ctx.scale(dpr, dpr);
  const w = rect.width, h = rect.height, points = history.slice(-180), left = 64, right = w - 6, top = 16, bottom = h - 20;
  ctx.strokeStyle = '#2a3639'; ctx.lineWidth = 1;
  for (let i = 0; i < 4; i++) { const y = top + (bottom - top) * i / 3; ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke(); }
  $('plot-empty').hidden = points.length > 0;
  if (!points.length) { $('max').textContent = '—'; $('min').textContent = '—'; return; }
  const all = history.map(p => p.value), min = Math.min(...all), max = Math.max(...all), unit = points[0].baseUnit;
  const compact = n => `${Number(n.toPrecision(5))} ${unit}`;
  $('max').textContent = compact(max); $('min').textContent = compact(min);
  let lo = Math.min(...points.map(p => p.value)), hi = Math.max(...points.map(p => p.value));
  const pad = Math.max((hi - lo) * .15, Math.abs(hi) * .0005, 1e-12); lo -= pad; hi += pad;
  ctx.font = '10px monospace'; ctx.fillStyle = '#819399';
  for (let i = 0; i < 4; i++) ctx.fillText(Number((hi - (hi - lo) * i / 3).toPrecision(6)).toString(), 1, top + (bottom - top) * i / 3 - 4);
  ctx.fillText(unit, 1, h - 3); ctx.fillText(`${points.length} / 180`, Math.max(65, w - 65), h - 3);
  ctx.strokeStyle = '#ff9010'; ctx.lineWidth = 1.5; ctx.beginPath();
  points.forEach((p, i) => { const x = left + (right - left) * i / Math.max(1, points.length - 1), y = bottom - (p.value - lo) / (hi - lo) * (bottom - top); if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y); }); ctx.stroke();
}
function stopView(reason = '') {
  clearTimeout(timer); $('status').textContent = 'Не подключён'; $('device-name').textContent = 'Прямое подключение к прибору'; $('led').className = '';
  $('freshness').textContent = current ? 'СОЕДИНЕНИЕ ЗАКРЫТО · ПОСЛЕДНИЕ ДАННЫЕ' : 'Ожидание данных';
  if (reason) message(reason); controls();
}
function newSession() { history = []; saved = []; seriesKey = ''; current = null; slots(); draw(); message(); }
async function poll(link) {
  if (connection !== link || link.closed) return;
  try { await link.request(9); }
  catch (error) { message(error.message); await link.close(); return; }
  if (connection === link && !link.closed) timer = setTimeout(() => poll(link), Number($('interval').value));
}
$('connect').onclick = async () => {
  if (connection) { const old = connection; connection = null; await old.close(); stopView(); return; }
  if (demo) return;
  connecting = true; message(); controls(); $('status').textContent = 'Подключение…';
  const link = new Connection({ onLog: log, onFrame: frame => {
    if (connection !== link) return;
    if (frame[3] === 9 && frame[4] === 11) render(measurement(frame));
  }, onDisconnect: reason => { if (connection === link) { connection = null; connecting = false; stopView(reason); } } });
  connection = link;
  try {
    // The chooser is invoked directly from the click, before any unrelated await.
    const name = await ($('transport').value === 'ble' ? link.bluetooth() : link.serial(Number($('baud').value)));
    newSession();
    const discovery = await link.request(0, [], true);
    if (discovery[4] !== 3 || discovery[5] !== 5 || discovery[6] !== 3) throw new Error('Выбранное устройство не является DM40');
    const model = String.fromCharCode(discovery[7]); if ('ABC'.includes(model)) $('model').value = `DM40${model}`;
    connecting = false; $('status').textContent = 'Подключён'; $('device-name').textContent = `${name} · ${$('model').value}`; $('led').className = 'on';
    controls(); void poll(link);
  } catch (error) {
    await link.close(); connection = null; connecting = false; stopView();
    message(error.name === 'NotFoundError' ? 'Выбор устройства отменён или подходящее устройство не найдено.' : error.message);
  }
};
async function command(code, data) {
  if (!active() || busy) return;
  busy = true; controls(); message();
  try {
    if (demo) {
      if (code === 6) { demoMode = data[0]; demoHold = false; demoRel = false; demoZero = 0; }
      if (code === 4) demoHold = !!data[0];
      if (code === 5) { demoRel = !!data[0]; demoZero = demoRel ? demoRaw() : 0; }
      demoTick();
    } else {
      const link = connection; await link.request(code, data); const frame = await link.request(9);
      const m = measurement(frame);
      if ((code === 6 && m.mode !== data[0]) || (code === 4 && m.hold !== !!data[0]) || (code === 5 && m.rel !== !!data[0])) message('Прибор подтвердил команду, но состояние не изменилось. Откройте экран мультиметра на DM40.');
    }
  } catch (error) { message(error.message); }
  finally { busy = false; controls(); }
}
$('hold').onclick = () => command(4, [current.hold ? 0 : 1]);
$('rel').onclick = () => command(5, [current.rel ? 0 : 1]);
groupButtons.forEach((button, group) => button.onclick = () => {
  const options = GROUPS[group]; const next = current.group === group ? (current.sub + 1) % options.length : 0;
  void command(6, [options[next].mode]);
});
$('range').onclick = () => {
  $('range-options').replaceChildren(...ranges(current.mode, $('model').value).map(range => {
    const button = document.createElement('button'); button.textContent = range.label; button.classList.toggle('active', range.mode === current.mode);
    button.onclick = () => { $('range-dialog').close(); void command(6, [range.mode]); }; return button;
  })); $('range-dialog').showModal();
};
$('close-range').onclick = () => $('range-dialog').close();
$('settings').onclick = () => $('settings-dialog').showModal();
$('model').onchange = () => { if (current) $('range').textContent = ranges(current.mode, $('model').value).find(r => r.mode === current.mode)?.label ?? 'AUTO'; };
$('reading').onclick = () => {
  if (!current) return;
  saved.push({ text: current.main.text, unit: current.main.unit, mode: current.name, time: new Date().toLocaleTimeString('ru-RU'), demo });
  if (saved.length > 6) saved.shift(); slots();
};
$('clear').onclick = () => { history = []; saved = []; slots(); draw(); controls(); };
$('export').onclick = () => {
  const quote = value => `"${String(value ?? '').replaceAll('"', '""')}"`;
  const rows = [['time', 'mode', 'value_si', 'unit_si', 'display', 'display_unit', 'relative', 'source', 'raw'], ...history.map(p => [p.time, p.mode, p.value, p.baseUnit, p.text, p.unit, p.rel, p.demo ? 'DEMO' : 'DM40', p.raw])];
  const blob = new Blob(['\ufeff', rows.map(row => row.map(quote).join(',')).join('\r\n')], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob), a = document.createElement('a'); a.href = url; a.download = `dm40-${demo ? 'demo-' : ''}${new Date().toISOString().replaceAll(':', '-')}.csv`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
};
function demoRaw() { return 12345 + Math.round(Math.sin(Date.now() / 2800) * 38 + Math.sin(Date.now() / 480) * 9); }
function demoTick() {
  if (!demo) return;
  clearTimeout(timer);
  if (!demoHold || !current || current.mode !== demoMode) {
    const { group, sub } = modeInfo(demoMode);
    let raw = demoRaw() - demoZero, format = [0x16, 0x16, 0x16, 0x14, sub ? 0x04 : 0x18, sub ? 0x02 : 0x04][group];
    if (raw < 0) { format |= 1; raw = -raw; }
    const frame = [0xdf, 5, 3, 9, 11, demoMode, 4 | (demoRel ? 32 : 0) | (demoHold ? 128 : 0), 4, 2, format, 0x88, 0x13, 0xf4, 1, raw & 255, raw >> 8];
    frame.push((-frame.reduce((a, b) => a + b, 0)) & 255); render(measurement(Uint8Array.from(frame)));
  } else render({ ...current, hold: true, rel: demoRel });
  timer = setTimeout(demoTick, Number($('interval').value));
}
$('demo').onclick = () => {
  demo = !demo;
  if (demo) { newSession(); demoMode = 0x30; demoHold = false; demoRel = false; demoZero = 0; $('status').textContent = 'Деморежим'; $('device-name').textContent = 'Симуляция · без подключения к прибору'; $('led').className = 'demo'; demoTick(); }
  else stopView();
  controls();
};
setInterval(() => { if (!demo && active() && lastUpdate && Date.now() - lastUpdate > 3000) $('freshness').textContent = 'НЕТ СВЕЖИХ ДАННЫХ'; }, 1000);
new ResizeObserver(draw).observe($('chart'));
slots(); controls();
function checkSupport() {
  if ($('transport').value === 'ble') message(bluetoothSupportError());
  else if (!navigator.serial) message('Web Serial недоступен в этом браузере. Используйте настольный Chrome/Edge.');
  else message();
}
$('transport').onchange = checkSupport;
checkSupport();
