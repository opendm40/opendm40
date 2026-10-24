# DM40 memrw kit — резервная копия прошивки и калибровки

Набор для владельцев мультиметра **Alientek DM40 (A/B/C)**. Позволяет
**сделать резервную копию внутренней флеш-памяти прибора** (в том числе записи
калибровки) по Bluetooth, прежде чем экспериментировать с прошивками.

Внутри:

1. **Пропатченная штатная прошивка** `firmware/dm40_v_2_6_memrw.bin` — официальная
   V2.6 плюс одна добавленная Bluetooth-команда чтения/записи памяти (0x0B). Из неё
   используется только **чтение** — это и есть способ снять дамп.
2. **Хост-утилита** `tools/dm40_memrw.py` — консольный клиент на Python 3
   (без сторонних зависимостей). По умолчанию **только чтение**.
3. **Образ для отката** `firmware/dm40_v_2_6.bin` — оригинальный файл обновления
   V2.6 от производителя (Alientek), ничем не изменён.

> Это **инструмент для бэкапа вашего собственного прибора**. Автор не несёт
> ответственности за последствия. Запись в память и вызов кода могут **убить
> калибровку или «окирпичить»** прибор — поэтому они спрятаны за флагом
> `--unsafe` и в инструкции ниже **не используются**.

---

## 1. Что это и зачем

У DM40 включена защита флеша от чтения (RDP Level 1): отладчик по SWD **не может**
прочитать прошивку и калибровку. Если что-то пойдёт не так при обновлении, восстановить
заводскую калибровку штатными средствами нельзя — её просто неоткуда взять.

Пропатченная прошивка добавляет в прибор маленькую Bluetooth-команду, которая умеет
читать любой адрес памяти. Утилита из этого набора пользуется только чтением и
сохраняет дамп на компьютер. **Так можно сохранить калибровку заранее.**

Что реально умеет прошивка (команда BT `0x0B`):

| Подкоманда | Действие | В этом наборе |
|---|---|---|
| 0 | **чтение** памяти (до 0xFF байт за запрос) | да, основной режим |
| 1 | запись памяти | только за `--unsafe`, опасно |
| 2 | вызов кода по адресу | только за `--unsafe`, **не работает** (вешает прибор до сброса по сторожевому таймеру) |

Больше в прошивке ничего не добавлено: нет «демо-версии 9.9», нет передачи
измерений по USB. Прибор по-прежнему сообщает свою версию как **V2.6** (патч не
меняет номер версии). Reset-вектор и структура файла — как у штатной V2.6.

---

## 2. Требования

- Мультиметр DM40 с **Bluetooth**.
- Компьютер с Bluetooth и **Python 3 с поддержкой Bluetooth-сокетов**
  (`socket.AF_BLUETOOTH`).
  - **Linux:** используйте **системный** интерпретатор `/usr/bin/python3`.
    Некоторые сборки Python (из дистрибутива для «минимального» окружения,
    из Homebrew и т. п.) собраны **без** поддержки Bluetooth и выдадут ошибку —
    см. FAQ.
  - **Windows:** CPython с python.org поддерживает `AF_BLUETOOTH`/RFCOMM —
    **но на Windows этот набор не проверялся** (untested).
- На Linux адрес прибора находится **автоматически** (нужен `bluetoothctl` из
  BlueZ). На других системах нужен MAC-адрес прибора (`--mac`, см. раздел 4).

Все команды ниже запускают Python с флагом `-I` (изолированный режим), чтобы
интерпретатор не подхватывал посторонние модули из текущей папки.

---

## 3. Установка пропатченной прошивки (пошагово)

Загрузчик прибора принимает файл обновления с именем вида `dm40*.bin`. Поэтому
пропатченный файл назван `dm40_v_2_6_memrw.bin` — префикс `dm40` обязателен.

1. На приборе откройте **USB-подключение** (на DM40 это пункт меню
   *Settings → Other Settings → «USB Connect»*). Прибор появится на компьютере
   как USB-диск.
2. Скопируйте на этот диск **ровно ОДИН** файл `dm40_v_2_6_memrw.bin`.
   **На диске не должно остаться других файлов `dm40*.bin`** — иначе загрузчик
   не знает, какой ставить.
3. Безопасно извлеките (eject) USB-диск.
4. **Выключите прибор и включите снова.** Установку делает загрузчик при
   «холодном» включении (при обычном программном сбросе установка не
   запускается). На экране будет прогресс вида `имя : NN.N%`, затем `update ok`.
5. После успешной установки штатное приложение **само удалит** файл с диска.

Проверить файл перед копированием (не обязательно):

```
sha256sum -c firmware/SHA256SUMS
```

Ожидаемый SHA-256 пропатченного файла:
`2e8bab1395dd16dfc85bfad2b0766ce315e3ac27a19c7a60761707f161606290`.

---

## 4. Поиск прибора по Bluetooth (автоматически)

Включите Bluetooth на компьютере и на приборе (*Settings*), на приборе откройте
экран **мультиметра**. Больше ничего делать не нужно: **на Linux утилита сама
находит прибор**, `--mac` указывать не надо.

Как это работает (через `bluetoothctl` из BlueZ, без сторонних пакетов):

1. `bluetoothctl devices` — берутся устройства, в имени которых есть `DM40`.
2. Для каждого `bluetoothctl info <адрес>` — подходит только то, у которого есть
   сервис **Serial Port** (`00001101-…`, RFCOMM). Прибор виден в списке **дважды**:
   - классическое устройство, например `XX:XX:XX:XX:XX:XX`
     «C-1-ATK-DM40-CALI» с Serial Port — **это нужный адрес**;
   - BLE-устройство для телефонного приложения, например `XX:XX:XX:XX:XX:XX`
     «DM40A» (первый байт на 1 меньше) — для утилиты **не подходит**.
3. Если прибор ещё неизвестен системе — 10-секундный поиск
   (`bluetoothctl --timeout 10 scan on`) и повтор шагов 1–2.
4. Найден один прибор — утилита пишет `Found DM40: <имя> <адрес>` и работает с
   ним. Найдено несколько — предлагает выбрать (если запуск не из терминала —
   просит указать `--mac`). Не найдено ни одного — объясняет, что проверить.
5. Найденный адрес запоминается в `~/.config/dm40-memrw/mac` и в следующий раз
   проверяется первым (`Using cached DM40: …`). Флаг `--rescan` — искать заново,
   не глядя в кеш (например, если у вас несколько приборов).

```
python3 -I tools/dm40_memrw.py read 0x1FFF7A10 12            # адрес найдётся сам
python3 -I tools/dm40_memrw.py --rescan read 0x1FFF7A10 12   # искать заново
```

**Указать адрес вручную (необязательно).** `--mac` или переменная `DM40_MAC`
всегда имеют приоритет над автопоиском; на Windows/macOS (где нет
`bluetoothctl`) они обязательны:

```
python3 -I tools/dm40_memrw.py --mac XX:XX:XX:XX:XX:XX read 0x1FFF7A10 12
export DM40_MAC=XX:XX:XX:XX:XX:XX        # или один раз на сеанс
```

Как узнать адрес самому:
- **Linux:** `bluetoothctl devices` → для каждого кандидата с `DM40` в имени
  `bluetoothctl info <адрес>` → нужен тот, где есть строка
  `UUID: Serial Port (00001101-…)`. Если прибора нет в списке — сначала
  `bluetoothctl --timeout 10 scan on`.
- **Windows:** *Параметры → Bluetooth и устройства* → прибор → свойства, или
  *Диспетчер устройств → Bluetooth* → прибор → *Сведения* → «Адрес Bluetooth-устройства».

**Нужна ли пара (pairing)?** Текущий клиент подключается к RFCOMM-каналу 1
**напрямую, без сопряжения** — обычно пары не требуется. Если ваша ОС всё же
требует сопряжение (запрос PIN и т. п.), спарьте один раз через
`bluetoothctl` (`pair <MAC>`, при необходимости `trust <MAC>`) и повторите.

---

## 5. Быстрый бэкап (рекомендуется всем)

Снимает самое важное — UID, option bytes, OTP, загрузочные данные и **запись
калибровки**. Занимает меньше минуты.

Условия: прибор включён, показан экран **мультиметра**, Bluetooth свободен.

```
python3 -I tools/dm40_memrw.py dump --quick -o backup_quick
```

В каталоге `backup_quick/` появятся:

| Файл | Адрес | Размер | Что это |
|---|---|---:|---|
| `uid_1FFF7A10.bin` | 0x1FFF7A10 | 12 Б | 96-битный UID чипа |
| `option_bytes_1FFFC000.bin` | 0x1FFFC000 | 16 Б | option bytes флеша |
| `otp_1FFF7800.bin` | 0x1FFF7800 | 528 Б | область OTP |
| `bootdata_080A4000_16K.bin` | 0x080A4000 | 16 КиБ | загрузочные данные, привязанные к UID |
| `calibration_080C0000_32K.bin` | 0x080C0000 | 32 КиБ | настройки + **запись калибровки DMM/осциллографа** |
| `manifest.json` / `manifest.txt` | — | — | адреса, размеры, SHA-256, дата, MAC, версия ПО |
| `SHA256SUMS` | — | — | контрольные суммы файлов |

> Запись калибровки занимает первые ~800 байт блока `0x080C0000`; сохраняем 32 КиБ
> с запасом. Именно этот файл вы будете беречь.

---

## 6. Полный дамп (всё, на всякий случай)

Снимает малые внутренние области **и всю 1 МиБ внутреннего флеша**
(загрузчик + приложение + загрузчик обновлений + загрузочные данные + калибровка).
Каждый блок читается дважды и сверяется; при ошибках идёт переподключение и
повтор. **Занимает около 11 минут** на 1 МиБ.

```
python3 -I tools/dm40_memrw.py dump --full -o backup_full
```

Главный файл — `flash_08000000_1MB.bin` (полная копия внутренней флеш-памяти).

---

## 7. Проверка результата

```
cd backup_quick      # или backup_full
sha256sum -c SHA256SUMS
cat manifest.txt
```

Все строки должны быть `OK`. `manifest.txt` показывает адреса, размеры,
SHA-256, дату (UTC), MAC прибора и версию прошивки (если удалось прочитать).

Разовое чтение произвольного участка (например, заголовок калибровки):

```
python3 -I tools/dm40_memrw.py read 0x080C0000 0x100 -o cal_head.bin
python3 -I tools/dm40_memrw.py read 0x1FFF7A10 12      # UID в hex в консоль
```

---

## 8. Откат на штатную прошивку

В наборе лежит оригинальный файл производителя `firmware/dm40_v_2_6.bin`.
Установите его точно так же, как в разделе 3:

1. *USB Connect* → скопировать **только** `dm40_v_2_6.bin` (других `dm40*.bin`
   на диске быть не должно) → eject.
2. Выключить и включить прибор. Дождаться `update ok`.

`dm40_v_2_6.bin` — это **файл Alientek**, он ничем не изменён
(SHA-256 `5c2efe345369238604b8001af99ab089f82ff405fa91d5b8368c0086484038ac`).

---

## 9. Спасение прибора (rescue) через экран загрузчика

Если на диск попал **битый или неполный** `dm40*.bin`, загрузчик при включении
покажет `<имя> update error` или `open file error` и **на каждом включении будет
сам выставлять свой USB-диск**, пока не поставить годный образ. Это штатный
режим восстановления, не поломка. Действия:

1. Прибор при включении сам показывает свой USB-диск на компьютере.
2. На этом диске **удалите все файлы `dm40*.bin`**.
3. Скопируйте туда **один** заведомо годный образ — `firmware/dm40_v_2_6.bin`
   из этого набора (или заново `dm40_v_2_6_memrw.bin`).
4. На экране загрузчика нажмите **«Update»**.
5. Дождитесь `update ok` и перезагрузки.

> У прибора **нет** второго слота/резервного образа и загрузки в RAM: единственный
> путь восстановления — положить годный `dm40*.bin` и дать загрузчику его поставить.
> Поэтому важно сделать бэкап **до** экспериментов.

---

## 10. Временная смена модели (DM40A → B/C) в ОЗУ

> Экспериментальная функция. Меняет тип прибора **только в оперативной памяти**,
> флеш и калибровка **не трогаются**, после перезагрузки всё возвращается к родной
> модели. Проверено на DM40A с пропатченной V2.6.

Что это даёт: прошивка вычисляет пределы и режимы по «модели», записанной в
копии настроек в ОЗУ. Переключив байт модели на B или C, можно временно включить
то, что обычно доступно только старшим моделям: режим **AC+DC**, диапазоны на
**60000 отсчётов**, пункты меню B/C. Телефонное приложение начнёт показывать новую
модель.

```
# прибор должен показывать экран МУЛЬТИМЕТРА (Bluetooth работает только там)
python3 -I tools/dm40_memrw.py set-model C      # включить поведение DM40C
python3 -I tools/dm40_memrw.py set-model B      # DM40B
python3 -I tools/dm40_memrw.py set-model A      # DM40A
python3 -I tools/dm40_memrw.py set-model restore  # вернуть значение из флеша
```

Перед записью команда делает проверки и **откажется** работать, если что-то не так:
1. читает вектора приложения по `0x08004000` и сверяет с V2.6 (адреса привязаны к
   V2.6; если это не та прошивка — отказ);
2. читает 16 байт копии настроек в ОЗУ `0x20028E58` и сверяет с флешем
   `0x080C0000`, а также 8 байт `0x20028FC0` против `0x080C0168` — это доказывает,
   что байт модели действительно там, где мы думаем;
3. пишет один байт модели в ОЗУ `0x20028FC0` (и в дублирующую копию страницы
   мультиметра `0x20026B0C`) и читает его обратно.

Это запись **одного известного байта в ОЗУ**, поэтому флаг `--unsafe` не нужен и
произвольная запись не открывается.

**Чтобы изменение вступило в силу:** страница мультиметра пересобирается при входе
на неё. Переключитесь на другую страницу (осциллограф/меню) и вернитесь на
мультиметр — пределы и подписи пересчитаются. Часть элементов интерфейса может
обновиться только после повторного входа на страницу.

Изменение временное — до перезагрузки, только в ОЗУ. Пользовательские настройки
прибор хранит в `CONFIG.BIN` на SPI-диске, а не в записи `0x080C0000`.

---

## 11. FAQ / частые проблемы

- **«Bluetooth занят» / EBUSY / не подключается.** За раз допустимо **одно**
  соединение. Между попытками подключения выдержите **~4 секунды**. Закройте
  приложение-компаньон на телефоне и другие программы, держащие связь с прибором.
  Утилита сама делает паузы и повторы.
- **`no DM40 found over Bluetooth`.** Включите Bluetooth на приборе
  (*Settings*) и на компьютере (`bluetoothctl power on`), откройте на приборе
  экран мультиметра, закройте приложение на телефоне и повторите. Если не
  помогает — укажите адрес вручную `--mac` (раздел 4). Если автопоиск выбрал не
  тот прибор — `--rescan` или `--mac`.
- **Команда не отвечает / таймаут.** Команда памяти `0x0B` разбирается прибором
  **только когда на экране показан мультиметр (DMM)**. Выйдите из осциллографа/
  генератора/меню на экран мультиметра и повторите.
- **`This Python build has no Bluetooth socket support`.** У вашего Python нет
  `socket.AF_BLUETOOTH`. На Linux запускайте именно `/usr/bin/python3`
  (системный). Проверка: `/usr/bin/python3 -c "import socket; print(socket.AF_BLUETOOTH)"`.
- **Прибор завис после `--unsafe call`.** Путь вызова кода в этой прошивке
  **не работает** и вешает прибор до сброса по сторожевому таймеру. Подождите
  сброса / выключите-включите. Не пользуйтесь `call`.
- **Чтение идёт медленно.** Это нормально: ~11 минут на 1 МиБ при полном дампе
  (каждый блок читается дважды для надёжности).

---

## 12. Карта памяти: где калибровка

| Адрес | Размер | Что | Бэкапится в |
|---|---|---|---|
| 0x08000000–0x08003FFF | 16 КиБ | ROM-загрузчик | полный дамп |
| 0x08004000– | ~480 КиБ | приложение | полный дамп |
| 0x08008000 | 12 Б | маркер установки (пишет сам загрузчик) | полный дамп |
| 0x080A0000–0x080BFFFF | 128 КиБ | загрузчик обновлений (FS-loader) | полный дамп |
| 0x080A4000 | 16 КиБ | загрузочные данные, привязанные к UID | быстрый + полный |
| **0x080C0000** | **32 КиБ** (исп. ~800 Б) | **настройки + КАЛИБРОВКА DMM/осциллографа** | **быстрый + полный** |
| 0x1FFF7A10 | 12 Б | 96-битный UID чипа | быстрый + полный |
| 0x1FFF7800 | 528 Б | OTP | быстрый + полный |
| 0x1FFFC000 | 16 Б | option bytes | быстрый + полный |

**Главное:** калибровку хранит блок **`0x080C0000`** — его сохраняет и быстрый,
и полный бэкап.

---

## 13. Управление измерением по Bluetooth (mode / hold / rel / measure)

Эти команды используют **штатный** Bluetooth-протокол прибора (команды 06/04/05/09)
— никакой записи в память и никакого `--unsafe` не требуется. Это обычные функции,
которыми пользуется и телефонное приложение.

> Bluetooth у прибора работает **только на экране мультиметра** — на других
> страницах штатная прошивка выключает BT-порт. Поэтому все команды ниже работают
> только когда на приборе открыт экран мультиметра. Инструмент **не** переключает
> страницы за вас.

```
# выбрать функцию измерения (cmd 06)
python3 -I tools/dm40_memrw.py mode dcv     # V= (авто)
python3 -I tools/dm40_memrw.py mode acv     # V~        acdcv  # AC+DC (B/C)
python3 -I tools/dm40_memrw.py mode dca     # A= (авто) ohm cont diode cap freq temp
#   доступные имена: dcv acv acdcv dcmv acmv dca aca acdca dcma acma ohm cap diode cont freq temp

# HOLD и REL (cmd 04 / 05)
python3 -I tools/dm40_memrw.py hold on
python3 -I tools/dm40_memrw.py rel on

# прочитать текущее показание (cmd 09) — удобно для логирования с ПК
python3 -I tools/dm40_memrw.py measure
python3 -I tools/dm40_memrw.py measure --json
```

`measure` расшифровывает 11-байтовый ответ: основное значение с единицей, два
дополнительных поля (частота/скважность, AC/DC-части, холодный спай и т. п. — в
зависимости от режима), флаги (HOLD, REL, уровень батареи) и сырые байты. Значение
`OL/--` означает «нет значения / перегрузка» (0xFFFF в кадре).

> Команды `mode`/`hold`/`rel` — штатный функционал прибора. `mode acdcv`/`acdca`
> включают режим AC+DC, который сток обычно показывает только у B/C (см. раздел 10).

---

## 14. Запись в память (`--unsafe write`) — только для опытных

> ⚠️ **Сначала сделайте полный дамп** (раздел 6). Неверная запись может повесить
> прибор или испортить настройки. Для смены модели пользуйтесь `set-model`
> (раздел 10) — он делает всё то же самое, но с проверками.

**Что делает.** Пишет байты в память прибора через Bluetooth-патч:

```
python3 -I tools/dm40_memrw.py --unsafe write <адрес> <hex-байты>
```

- Утилита спросит подтверждение (`--yes` пропускает вопрос).
- За один пакет пишется не больше 0x91 (145) байт; длинные данные утилита сама
  делит на пакеты.
- Как и чтение, работает только на экране мультиметра.

**Куда можно писать.** В оперативную память (0x20000000–0x2002FFFF,
0x10000000–0x1000FFFF) и в регистры периферии. **Во внутренний флеш так писать
нельзя:** флеш программируется только через контроллер флеша (стирание сектора +
программирование), обычная запись туда игнорируется или вешает прибор. Поэтому
калибровку во флеше эта команда не меняет, а изменения в ОЗУ пропадают после
перезагрузки.

**Пример** — то, что делает `set-model C` (байт модели в копии записи настроек в ОЗУ):

```
python3 -I tools/dm40_memrw.py read 0x20028FC0 1              # было 00 (DM40A)
python3 -I tools/dm40_memrw.py --unsafe write 0x20028FC0 02    # DM40C
python3 -I tools/dm40_memrw.py read 0x20028FC0 1              # проверка: 02
```

После этого уйдите с экрана мультиметра (например, на осциллограф) и вернитесь —
экран мультиметра пересоздаётся и берёт новую модель. Отмена — перезагрузка прибора.

**Как не навредить.**
- Всегда читайте обратно то, что записали.
- Между подключениями выдерживайте ~4 с (иначе «Device or resource busy»).
- Неверная запись может повесить прибор: сторожевой таймер перезапустит его
  примерно через 20 с.
- Сток может позже сохранить испорченные настройки (обычные настройки хранятся в
  `CONFIG.BIN` на диске прибора) — держите копию диска.
- Запись в регистры периферии может перевести измерительный тракт в
  непредсказуемое состояние — не делайте этого во время измерения опасных напряжений.

**`--unsafe call` не использовать.** Вызов функций прошивки из обработчика
Bluetooth вешает прибор (проверено: вызов функции сброса подвесил его до
срабатывания сторожевого таймера). Команда оставлена только для исследований.

---

## English (short)

This kit lets **Alientek DM40 (A/B/C)** owners **back up the meter's internal
flash — including the calibration record — over Bluetooth**, before flashing
experimental firmware. The meter's flash is read-out protected (RDP level 1), so
a debugger cannot read it; this patched stock firmware adds one Bluetooth memory
command and is the only way to dump it.

**Contents:**
- `firmware/dm40_v_2_6_memrw.bin` — official stock V2.6 + one added BT memory
  read/write command (0x0B). Only **read** is used. Reports version V2.6; no
  other behaviour changed.
- `firmware/dm40_v_2_6.bin` — the vendor's original V2.6 update file, for
  rollback (unmodified).
- `tools/dm40_memrw.py` — Python 3 CLI, no third-party deps, **read-only by
  default**.
- `firmware/SHA256SUMS` — checksums.

**Requirements:** Python 3 with Bluetooth sockets (`socket.AF_BLUETOOTH`). On
Linux use the system `/usr/bin/python3`. Windows CPython supports it but **is
untested here**.

**Install the patched firmware:** on the meter open *USB Connect*, copy exactly
**one** `dm40*.bin` (`dm40_v_2_6_memrw.bin`) to the USB disk, remove any other
`dm40*.bin`, eject, then **power the meter off and on** (a cold boot). The
loader shows `name : NN.N%` then `update ok`; the app deletes the file
afterwards.

**Finding the meter (automatic on Linux):** switch Bluetooth on (meter
*Settings*), keep the meter on the **multimeter** screen and just run the
commands — no `--mac` needed. The tool asks `bluetoothctl devices` for names
containing `DM40` and keeps the one whose `bluetoothctl info` lists the **Serial
Port** UUID (`00001101-…`). The meter appears twice: the classic SPP device
(e.g. `XX:XX:XX:XX:XX:XX` "C-1-ATK-DM40-CALI") is the right one; the BLE entry
for the phone app (e.g. `XX:XX:XX:XX:XX:XX` "DM40A", first byte one lower) is
not usable. If nothing is known yet it runs a 10 s `bluetoothctl --timeout 10
scan on` and retries. One meter → `Found DM40: <name> <addr>`; several → you
pick (or, without a terminal, pass `--mac`); none → a hint what to check. The
choice is cached in `~/.config/dm40-memrw/mac` (verified before reuse);
`--rescan` ignores the cache.

`--mac AA:BB:CC:DD:EE:FF` / `DM40_MAC` are optional overrides, and required
where `bluetoothctl` is unavailable (Windows: Bluetooth device properties or
Device Manager → the meter → Details → "Bluetooth device address"). Manual
lookup on Linux: `bluetoothctl devices`, then pick the one whose `bluetoothctl
info <addr>` shows "Serial Port". The client connects to RFCOMM channel 1
directly, usually **without pairing**; pair with `bluetoothctl` only if your OS
demands it.

**Back up** (meter on the **multimeter** screen):
```
python3 -I tools/dm40_memrw.py dump --quick -o backup_quick   # fast, calibration
python3 -I tools/dm40_memrw.py dump --full  -o backup_full    # whole 1 MiB, ~11 min
```
Each run writes the region files plus `manifest.json`, `manifest.txt` and
`SHA256SUMS`. Verify with `sha256sum -c SHA256SUMS`. One-shot read:
`python3 -I tools/dm40_memrw.py read 0x080C0000 0x100 -o cal.bin`.

**Rollback:** install `firmware/dm40_v_2_6.bin` the same way (only that one
`dm40*.bin` on the disk, then power-cycle).

**Rescue:** a bad/partial `dm40*.bin` makes the loader show `update error` /
`open file error` and expose its own USB disk on every boot — delete all
`dm40*.bin` there, copy the good `dm40_v_2_6.bin`, press **Update** on the loader
screen. There is no backup slot, so **back up before experimenting**.

**Danger:** `write` and `call` need `--unsafe`; they can destroy calibration or
brick/hang the meter (the firmware's `call` path is known to hang the unit until
the watchdog resets it). Do not use them for a backup.

**Calibration lives at flash `0x080C0000`** (32 KiB region, ~800 bytes used),
saved by both the quick and the full backup.

**Temporary model switch (experimental):**
```
python3 -I tools/dm40_memrw.py set-model C        # behave as DM40C
python3 -I tools/dm40_memrw.py set-model restore  # back to the flash default
```
This writes the model byte **in RAM only** (the live settings copy at
`0x20028FC0`, plus the DMM page's secondary copy at `0x20026B0C`); flash and
calibration are untouched and it reverts on reboot. It first verifies the V2.6
image (vectors at `0x08004000`) and that the RAM settings copy (`0x20028E58`)
equals flash (`0x080C0000`), refusing otherwise; then it writes one byte and
reads it back. No `--unsafe` needed (single known byte, no general writes).
Switch to another page and back to the multimeter for the new ranges/labels to
take effect; the phone app will report the new model. RAM-only, reverts on
reboot; normal settings are stored in `CONFIG.BIN` on the SPI disk, not in the
`0x080C0000` record.

**Control the meter over Bluetooth (stock protocol, no `--unsafe`):** these use
the unmodified firmware's own commands (06/04/05/09), the same ones the phone
app uses. They only work while the meter shows the **multimeter** page (the
stock turns the BT UART off elsewhere); the tool does not switch pages.
```
python3 -I tools/dm40_memrw.py mode dcv      # set function: dcv acv acdcv dcmv
                                             #   acmv dca aca acdca dcma acma ohm
                                             #   cap diode cont freq temp
python3 -I tools/dm40_memrw.py hold on       # HOLD on/off
python3 -I tools/dm40_memrw.py rel  on       # REL on/off
python3 -I tools/dm40_memrw.py measure       # read one reading (add --json)
```
`measure` decodes the 11-byte reply: main value + unit, two secondary fields
(frequency/duty, AC/DC parts, cold junction, etc., per mode), flags (HOLD, REL,
battery level) and the raw bytes; `OL/--` means no value / over-range.

### Advanced: memory write (`--unsafe write`)
`python3 -I tools/dm40_memrw.py --unsafe write <addr> <hexbytes>` stores raw bytes via the
BT patch (max 0x91 bytes per packet, split automatically; multimeter screen only). It works for RAM
(0x2000xxxx, 0x1000xxxx) and peripheral registers only — it cannot program internal flash, so the
flash calibration is never changed and RAM edits revert on reboot. Example (= `set-model C`):
`--unsafe write 0x20028FC0 02`, then leave and re-enter the multimeter page. Always read back,
make a full dump first, never poke
peripherals while measuring hazardous voltages. `--unsafe call` hangs the meter — do not use it.
