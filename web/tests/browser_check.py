"""UI and mocked BLE integration checks; does not access real Bluetooth hardware."""
import os
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

MOCK_BLE = r"""
(() => {
  let mode = 0x30, flags = 4;
  const device = new EventTarget();
  const rx = new EventTarget();
  rx.properties = {notify: true}; rx.startNotifications = async () => rx;
  const respond = (command, data, discovery = false) => {
    const frame = [223, discovery ? 255 : 5, discovery ? 255 : 3, command, data.length, ...data];
    frame.push((-frame.reduce((a,b) => a+b, 0)) & 255);
    // Simulate one GATT notification split in the middle of a frame.
    for (const part of [frame.slice(0,7), frame.slice(7)]) {
      rx.value = new DataView(Uint8Array.from(part).buffer);
      rx.dispatchEvent(new Event('characteristicvaluechanged'));
    }
  };
  const tx = {properties: {write: true}, writeValueWithResponse: async bytes => {
    window.sent.push(Array.from(bytes));
    if (bytes.reduce((a,b) => a+b,0) % 256) throw new Error('Bad outgoing CRC');
    const cmd = bytes[3];
    if (cmd === 0) respond(0, [5,3,66], true);
    else if (cmd === 9) respond(9, [mode,flags,4,2,24,136,19,244,1,57,48]);
    else {
      if(cmd === 4) flags = (flags & ~128) | (bytes[5] ? 128 : 0);
      if(cmd === 5) flags = (flags & ~32) | (bytes[5] ? 32 : 0);
      if(cmd === 6) mode = bytes[5];
      respond(cmd, [0]);
    }
  }};
  device.name = 'DM40B test';
  device.gatt = {connect: async () => ({getPrimaryService: async () => ({
    getCharacteristic: async id => id.includes('fff3') ? tx : rx
  })}), disconnect: () => {device.dispatchEvent(new Event('gattserverdisconnected'));}};
  window.sent = [];
  window.dropDM40 = () => device.gatt.disconnect();
  Object.defineProperty(navigator, 'bluetooth', {configurable:true,value:{requestDevice:async () => device}});
})();
"""

with sync_playwright() as p:
    browser = p.chromium.launch(executable_path=os.getenv('DM40_CHROME', '/usr/bin/google-chrome'), headless=True)
    page = browser.new_page(viewport={'width': 1280, 'height': 1080}, device_scale_factor=1)
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.goto(os.getenv('DM40_URL', 'http://127.0.0.1:8080'))
    expect(page.locator('#status')).to_have_text('Не подключён')
    expect(page.locator('#hold')).to_be_disabled()
    page.get_by_role('button', name='Деморежим', exact=True).click()
    expect(page.locator('#status')).to_have_text('Деморежим')
    expect(page.locator('#reading')).not_to_have_text('— — — —')
    page.locator('#reading').click()
    expect(page.locator('.slot').first).not_to_have_class('slot empty')
    page.locator('#hold').click()
    expect(page.locator('#hold')).to_have_text('HOLD')
    held = page.locator('#reading').inner_text()
    page.wait_for_timeout(650)
    assert page.locator('#reading').inner_text() == held
    expect(page.locator('#rel')).to_be_disabled()
    page.locator('#hold').click()
    page.locator('#rel').click()
    expect(page.locator('#relative')).to_have_text('REL Δ')
    page.locator('[data-group="0"]').click()
    expect(page.locator('#mode-name')).to_have_text('V AC')
    expect(page.locator('#aux')).to_contain_text('50.00 Hz')
    page.locator('#range').click()
    page.get_by_role('button', name='6 V', exact=True).click()
    expect(page.locator('#range')).to_have_text('6 V')
    page.locator('#settings').click()
    page.locator('#model').select_option('DM40A')
    page.get_by_role('button', name='Готово', exact=True).click()
    expect(page.locator('#range')).to_have_text('4 V')
    with page.expect_download() as download_info:
        page.locator('#export').click()
    content = Path(download_info.value.path()).read_text(encoding='utf-8-sig')
    assert 'DEMO' in content and 'value_si' in content
    for group, expected in [(1,'A DC'), (2,'Ω'), (3,'CAP'), (4,'DIODE'), (4,'CONT'), (5,'Hz'), (5,'°C')]:
        page.locator(f'[data-group="{group}"]').click()
        expect(page.locator('#mode-name')).to_have_text(expected)
    page.locator('[data-group="0"]').click()
    page.wait_for_timeout(1100)
    page.screenshot(path='/tmp/dm40-desktop.png', full_page=True)
    for width in [390, 320]:
        page.set_viewport_size({'width':width, 'height':844})
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth'), f'Overflow at {width}'
        settings = page.locator('#settings').bounding_box()
        meter = page.locator('.meter').bounding_box()
        assert settings['x'] + settings['width'] <= meter['x'] + meter['width'], 'Settings clipped'
        page.screenshot(path=f'/tmp/dm40-mobile-{width}.png', full_page=True)
    page.locator('#demo').click()
    expect(page.locator('#hold')).to_be_disabled()
    expect(page.locator('#freshness')).to_contain_text('СОЕДИНЕНИЕ ЗАКРЫТО')
    page.locator('#clear').click()
    expect(page.locator('#export')).to_be_disabled()
    page.set_viewport_size({'width':1280, 'height':1080})
    page.add_init_script(MOCK_BLE)
    page.reload()
    page.locator('#connect').click()
    expect(page.locator('#status')).to_have_text('Подключён')
    expect(page.locator('#reading')).to_have_text('1.2345')
    expect(page.locator('#device-name')).to_contain_text('DM40B')
    page.locator('#hold').click()
    expect(page.locator('#hold')).to_have_text('HOLD')
    page.locator('#hold').click()
    expect(page.locator('#hold')).to_have_text('RUN')
    page.locator('#rel').click()
    expect(page.locator('#relative')).to_have_text('REL Δ')
    page.locator('[data-group="0"]').click()
    expect(page.locator('#mode-name')).to_have_text('V AC')
    assert page.evaluate('sent.some(x => x[3] === 6 && x[5] === 0x70)')
    page.evaluate('dropDM40()')
    expect(page.locator('#status')).to_have_text('Не подключён')
    expect(page.locator('#hold')).to_be_disabled()
    expect(page.locator('#message')).to_contain_text('потеряно')
    page.locator('#connect').click()
    expect(page.locator('#status')).to_have_text('Подключён')
    page.locator('#connect').click()
    expect(page.locator('#status')).to_have_text('Не подключён')
    assert not errors, errors
    browser.close()
    print('PASS: demo, all mode groups, HOLD/REL, ranges, saved reading, CSV, 320/390px layouts, mocked BLE connect/control/disconnect/reconnect; no JS errors')
