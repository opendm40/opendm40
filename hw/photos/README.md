# DM40 teardown photos

**Copyright.** All photos in this folder are © CTAHOK, the author of the
review below. They are kept here for reference only and are **not** covered
by this repository's MIT license. Ask the author before any further use.

- Review: «Комбинированный прибор 3-в-1: Мультиметр, Осциллограф, Генератор:
  Alientek DM40C» (3-in-1 instrument: multimeter, oscilloscope, generator:
  Alientek DM40C)
- Author: CTAHOK (mysku.club)
- Published: 2025-04-25
- Page: https://mysku.club/blog/aliexpress/103047.html
- Retrieved: 2026-10-24. The files are the original JPEGs from the page,
  renamed by content, except [debug-header-annotated.jpg](debug-header-annotated.jpg), a marked-up crop
  made here (same copyright).

| File | Original URL | What it shows |
|---|---|---|
| [opened-case-overview.jpg](opened-case-overview.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/612ba0.jpg | The case opened: main board with the metal shield plate over the DMM area (fuse visible top left), MCU area below; back cover with foil lining and the battery |
| [battery-ln425272-label.jpg](battery-ln425272-label.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/7b053d.jpg | Battery label: LN425272, 3.7 V, 2000 mAh, 7.4 Wh, 4.2 V charge limit, 2025-02-10, protection PCB on the cell edge |
| [main-board-mcu-gd32f427.jpg](main-board-mcu-gd32f427.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/46e50d.jpg | MCU area: GD32F427VGT6, SPI flash, 25.000 MHz crystal, Bluetooth chip with 24.000 MHz crystal and PCB antenna, CA-IS3741HW, LT61 transformers, battery connector, USB-C, debug pads "VCC RX TX GND" and "V D C G", silkscreen TOUCH / VCCA / DCV |
| [dmm-isolation-hy3131-shield-area.jpg](dmm-isolation-hy3131-shield-area.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/1d5451.jpg | Isolation strip: CA-IS3741HW, LT61-117 / LT61-113 transformers with VPS8504B drivers, EL3H7 and "174 F163H" optocouplers, HFD4/3-LS relay, buzzer, 4.9152 MHz crystal at the HY3131 shield can, scope area can, CHA / GEN coaxial jacks |
| [charger-tp4056-and-power.jpg](charger-tp4056-and-power.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/e44e68.jpg | Power section: TP4056 charger, VBAT connector, USB-C, small transistors / regulators (unidentified markings), 2R2 inductor, tantalum capacitors |
| [input-jacks-shunts-fuse-relay.jpg](input-jacks-shunts-fuse-relay.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/6d0d95.jpg | DMM input area (under the shield plate): HFD4/3-LS relay, input resistors and varistors, 0.5 ohm "R500" shunt, wire shunt between the jacks, precision resistor, fuse in clips, COM and A/mA/uA jacks, HY3131 shield can |
| [isolated-supply-vps8504b-lt61.jpg](isolated-supply-vps8504b-lt61.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/0bdf36.jpg | Close-up: two VPS8504B transformer drivers under the LT61-1xx transformers, silkscreen TOUCH |
| [debug-header-annotated.jpg](debug-header-annotated.jpg) | derived from 46e50d.jpg (crop, 1.5x, annotations added in this repo) | The debug header: TX = PA2, RX = PA3, GND, VCC ~3.1 V; SWD row V, D = PA13, C = PA14, G |
| [relay-hfd4-and-varistor-closeup.jpg](relay-hfd4-and-varistor-closeup.jpg) | https://art.mysku-st.net/uploads/arts/01/56/73/2025/04/24/e5b7e4.jpg | Close-up: HFD4/3-LS relay and a 07D681K varistor (680 V) in the input network |

How these photos are used: [../README.md](../README.md).
