# fw-rs TODO

## Goal

> feature parity со стоковой прошивкой в режиме мультиметра, осциллографа, генератора, включая bluetooth. по usb - usbtmc. В отличие от стоковй прошивке не надо "переключать" режимы, рбаотать должно быть всё одновременно. Переключаться должен только режим отображения: каждый на своём экране (как в исходной) или какие-то комбинации одновременно (сам придумай как лучше). Меню тоже надо сделать.

Additional requirements:
- Main DMM value drawn in red.
- No Alientek logo in menus.
- Touch incl. side keys and gestures (swipes), with debounce and long-press filtering.
- DMM digits and chart redraw as fast as stock (~6 ms).
- Both analog rails on by default.
- USB composite = USBTMC + mass-storage disk, always present (like the stock disk mode).
- Debug commands live in USBTMC as `DBG:*` SCPI commands (no CDC).
- Data-logger mode (see FR-38 in `../DM40C_feature_request.md`).

## Remaining work

- [x] `tools/dm40dbg.py` ported from CDC/pyserial to `/dev/usbtmc` + `DBG:` commands (a3427d4).
  - Still verify that `tools/reload.sh` and `tools/flash.py` work with it (no serial/ttyACM references remain).
- [ ] On-device test of USBTMC (`*IDN?`, `DBG:PING?`/`DIAG?`/`LCD?` screenshot, `DBG:TAP`/`SWIPE`) and the mass-storage disk.
  - Disk is backed by the W25Q128 SPI flash (1f2bf30); README records `DBG:FLASH?` -> EF4018, but a full test (mount, write/read) is not logged.
- [ ] Faster redraw of the main DMM value and trend chart (target ~6 ms like stock: render to RAM, single window flush, chart column ring).
  - 4d2dc40 sped up digits and chart, but the ~6 ms target is not confirmed.
- [ ] Item-by-item parity check vs stock: DMM functions/ranges, scope, generator, Bluetooth phone-app protocol, menus, combined views.
  - DMM, scope and generator running simultaneously (fbfced3, 7c63aac); a systematic comparison is not done.
- [ ] Install fw-rs via the stock update file format instead of the Bluetooth RAM loader (user request; not designed yet).
- [ ] Data-logger mode (FR-38).
- [ ] Repo tidy-up (user request; not done yet).
