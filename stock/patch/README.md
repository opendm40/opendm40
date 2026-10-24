# DM40 patch images

Two separate update images are built from this directory. Build artefacts and
addresses were verified statically against the stock V2.6 image; see
`AGENTS.md` for the low-level patch mechanics (hook format, 64-byte pad rule,
firmware addresses).

| Image | Build | What it adds | Always-executed new code |
|-------|-------|--------------|---------------------------|
| `build-memrw/dm40_patched.bin` | `make BUILD=build-memrw SRCS="src/startup.c src/memrw.c"` | BT cmd 0x0B sub 0/1/2 (mem read/write/call) | none (one `bl` hook) |
| `build-dbg/dm40_dbg.bin` | `make dbg` | memrw **plus** LCD screenshot read-back (sub 3/4) | none (same one `bl` hook) |
| `build-dbg/dm40_dbgscope.bin` | `make dbg-scope` | dm40_dbg **plus** a scope-page hook: screenshots + touch/key injection on the scope page (armed over BT) | one scope-page hook, **flag-guarded and bit-identical to stock when disarmed** |

The plain memrw image is the proven-safe fallback and is kept **byte-identical**
(sha256 `2e8bab1395dd16dfc85bfad2b0766ce315e3ac27a19c7a60761707f161606290`). The
debug image is a separate file; building it does not touch the memrw build.

```
make BUILD=build-memrw SRCS="src/startup.c src/memrw.c" ORIG=../../dm40_v_2_6.bin
make dbg ORIG=../../dm40_v_2_6.bin
```

## What the debug image adds (all command-triggered, no new resident code)

`dm40_dbg.bin` is `src/stockdbg.c`, a strict superset of `src/memrw.c`. BT
command 0x0B keeps sub 0/1/2 byte-for-byte, and adds:

- **sub 3 — LCD raw read-back**: read up to 125 pixels of a driver row and
  return them as RGB565 (`AF 05 03 0C 2n <RGB565 LE> CRC`).
- **sub 4 — LCD RLE read-back**: same, but the device run-length-encodes the
  RGB565 stream and returns how many pixels it consumed, so the host tiles the
  row with no gaps. Flat UI areas compress strongly; a non-compressible row
  simply falls back to ~1 pixel per 3 bytes (use `--raw` there).

Both reuse the stock's own per-pixel GRAM read (`0x0800a6aa`, the function
behind the stock screenshot path), so the coordinate and colour conventions
match the stock exactly. The reads run from the Bluetooth command handler,
which the stock pumps **only from the multimeter-page loop** (the UART4 ISR
`0x0800d566` only buffers bytes; the one parser call is at `0x0803cec0` inside
the DMM page update). Consequences:

- Every BT command — memrw and screenshots — is answered only while the
  **multimeter page** is on screen, so `shot` captures the DMM page.
- The handler runs cooperatively in the main superloop, never in an ISR, so it
  cannot interleave with a GUI flush: a screenshot read cannot corrupt the
  render (worst case a transient panel-window glitch the next flush repaints).

### Safety (bench-only)

The debug image inherits memrw's **unauthenticated** read/write/call over
Bluetooth. Anyone in BT range who speaks the protocol can read or write any
address. This is a **bench/lab tool**; do not leave a meter running it in an
untrusted RF environment. The new sub-commands (3/4) are read-only.

The debug image's **only new always-executed code** is the same single `bl`
hook memrw uses (`0x0803cec0`, the parser call); there is no resident timer,
ISR, or background task. The modified-ranges list below shows the boot path
(vectors, reset handler, clock/flash init, loader marker) is untouched, exactly
as in the proven memrw image.

## Modified address ranges

Decoded-body diff (`stock/dm40fw.py`), flash addresses:

**stock V2.6 → dm40_dbg**
- `0x080041b4` (3 B) — startup ctor-init redirect **(identical to memrw)**.
- `0x0803cec0` (4 B) — parser-call `bl` → patch handler **(identical to memrw)**.
- `0x0807ce18 … 0x0807d247` — appended patch code, in free flash above the
  stock image end (`0x0807ce18`) and far below the FS-loader (`0x080a0000`),
  settings/calibration (`0x080c0000`) and boot-flag.
- Boot path verified UNTOUCHED: vector table (`0x08004000…0x080041b0`), reset
  and early init (`0x08004200…0x08009000`), loader marker (`0x08008000`).

**memrw → dm40_dbg**: differ **only** in the appended patch blob
(`0x0807d034 …`); the two hook sites are byte-identical, so the two images
share the exact same modifications to the stock code.

## Installing a debug image from the current state

The meter currently runs the plain memrw stock app. To install
`dm40_dbg.bin` or `dm40_dbgscope.bin`:

1. On the meter: Settings → Other Settings → **USB Connect** (it appears as a
   USB disk).
2. Copy **exactly one** `dm40*.bin` (the chosen debug image) to that disk; make
   sure no other `dm40*.bin` is present.
3. **Exit USB Connect** on the meter. The stock app zeroes the loader marker at
   `0x08008000` and resets; the FS-loader then installs the staged file
   (`update ok`) and the app deletes it. (A cold power-cycle also works.)

Keep a calibration backup first: `dm40_memrw.py dump --full`.

## Recovery (all images)

Neither image touches the primary bootloader (`0x08000000`) or the FS-loader
(`0x080a0000`), so a bad main-app flash is recoverable the normal way:

1. Put a single known-good `dm40*.bin` on the meter's SPI-flash FAT disk.
2. Power-cycle; the FS-loader installs it before the (broken) app runs.

If the app is too broken to expose "USB Connect" (so you cannot write the FAT
from the app), the fallback used so far is shorting the SPI flash to force the
FS-loader's own USB disk, or SWD — but SWD is RDP level 1, so a debugger
connect mass-erases and loses the loader + calibration. Keep a calibration
backup (`dm40_memrw.py dump --full`) before experimenting.

Because the debug image adds no resident code, if a bug ever manifested it
would only be while a BT command is in flight, and a reset returns the unit to
stock-equivalent behaviour until the next command.

## Expected screenshot time over Bluetooth

Full driver frame = 320×480 = 153 600 px = 300 KiB RGB565 raw. Each request is
one RFCOMM round-trip. At a measured RTT of 12–30 ms:

| Mode | Requests | ~Time |
|------|----------|-------|
| raw (sub 3), 125 px/req | 1440 | 17–43 s |
| RLE (sub 4), typical UI  | ~960 | 12–29 s |
| RLE, mostly-flat screen  | ~480 | 6–14 s |

`shot` prints the actual elapsed time. Use RLE (default) for UI screens and
`--raw` only for noisy regions where RLE would expand.

## Host tool

`kit/tools/dm40_stockdbg.py` (Python 3, no third-party deps; reuses
`dm40_memrw.py` for connection and Linux auto-discovery):

```
python3 -I kit/tools/dm40_stockdbg.py shot dmm.png           # DMM page PNG
python3 -I kit/tools/dm40_stockdbg.py shot btn.png --rect 400 0 80 320
python3 -I kit/tools/dm40_stockdbg.py read 0x20027C74 11      # measurement snapshot
python3 -I kit/tools/dm40_stockdbg.py hexdump 0x20025A20 64
python3 -I kit/tools/dm40_stockdbg.py peek-struct names       # named RAM symbols
python3 -I kit/tools/dm40_stockdbg.py peek-struct meas
```

Screenshots are read in the panel's native 320×480 driver space and transposed
to the 480×320 landscape view. If the orientation is wrong on your unit, pass
`--driver` to see the raw driver image and `--rot/--flipx/--flipy` to correct
it once.

On `dm40_dbgscope.bin`, scope-page commands:

```
python3 -I kit/tools/dm40_stockdbg.py enable-scope     # on the DMM page
# switch the meter to the scope page, then:
python3 -I kit/tools/dm40_stockdbg.py shot scope.png   # ~1-2 min (poll-limited)
python3 -I kit/tools/dm40_stockdbg.py tap 240 160
python3 -I kit/tools/dm40_stockdbg.py drag 100 160 380 160
python3 -I kit/tools/dm40_stockdbg.py key 1            # RUN
python3 -I kit/tools/dm40_stockdbg.py disable-scope
```

Tests (fake device, no hardware):
`python3 -I kit/tools/tests/test_dm40_stockdbg.py`.

## Static verification done here

- Both images build; `dm40_patched.bin` (memrw) is byte-identical to the
  recorded sha256; `dm40_dbg.bin` differs from it only in the appended blob.
- `tools/emu/emu_stockdbg.py` runs the **compiled** `patch_parse()` under
  Unicorn with the stock parser/UART/LCD-read entry points stubbed, and checks
  sub 3/sub 4 framing, checksum, raw pixels, RLE round-trip and the
  `consumed`-count tiling over flat, patterned and noisy rows.
- `tools/emu/emu_scopedbg.py` runs the **compiled** scope hook and checks the
  disarmed bit-identity, armed injection, parser-pump and boot disarm (above).

## Scope-page remote debug — `dm40_dbgscope.bin` (`make dbg-scope`)

The BT parser is pumped only from the DMM page, so on the scope page no BT
command is serviced and no injection happens. This image adds ONE hook in the
scope page's own code path so screenshots, memory reads and touch/key injection
work while the scope page is shown.

**Hook site** — the scope 100 ms timer dispatch thunk `0x080282a0`
(`ldr r0,[pc,#0x354]; ldr r0,[r0]`, bytes `d5 48 00 68`), replaced by
`b.w patch_scopepoll` (`src/scopetramp.S`). The scope page is built by
`0x080282ae`, called only from two menu-navigation sites (`0x08024148`,
`0x080242e6`); it is **not** reached from boot (`0x0800bbe0`) or the main loop
(`0x0801914e`) — re-verified statically — so the hook runs only while the scope
page is open. Boot, main menu and Settings (incl. the "USB Connect" recovery
path) run 100 % stock.

**No-brick guard** — a MAGIC arm word (`0x5C09EDB6`) in verified-mapped SRAM0+1
(`0x2000f100`), cleared to 0 by `patch_init()` at boot (same boot point as the
memrw init; writes only to SRAM0+1, which is mapped and free that early). So
after **every reset** the hook is disarmed and is a bit-identical passthrough of
the stock thunk. The parser pump and injection run only when armed by a BT
command (`enable-scope`, sent on the DMM page where the parser runs). A bug in
the armed path costs at most a reboot.

- **When disarmed** the trampoline reproduces the two overwritten instructions
  (`r0 = *(*0x080285f8)`) and continues at `0x080282a4`; emulator-proven
  bit-identical to the stock thunk (same r0..r3/sp/lr, same scope poll method,
  **no memory writes**).
- **When armed**, each 100 ms poll: (1) if a BT frame is ready
  (`0x0800d5f6`, the same gate the DMM page uses) parse it into a dedicated
  scratch frame and run the 0x0B handler — so `read`/`shot` work on the scope
  page; (2) play one queued touch entry into the stock touch queue
  (`0x2002561c`, 22-byte entries, see `stock/re/analysis/touch_v2_6.h`):
  tap = press then release, drag = a short path, key = key-report bit + code.
- The parser runs cooperatively (the DMM-page caller cannot run while the scope
  page is shown — single superloop), so there is no concurrent parser.

**Emulator verification** (`tools/emu/emu_scopedbg.py`, runs the compiled
image): (A) disarmed == stock thunk, bit-identical, no writes; (B) armed tap
lands the correct two 22-byte queue entries and the real poll still runs;
(C) the parser-pump serves a sub-3 screenshot reply on the scope page;
(D) `patch_init` disarms at boot.

**Modified ranges vs stock** — `0x080041b4` (startup, memrw-identical),
`0x0803cec0` (DMM parser hook, memrw-identical), **`0x080282a0`** (the new
scope hook, 4 B), and the appended blob `0x0807ce18…0x0807d3b7`. Boot path
(vectors, reset/early init, loader marker) UNTOUCHED. vs memrw: the only new
pre-existing-code change is the scope hook `0x080282a0`.

**Workflow**: on the DMM page `enable-scope`; switch to the scope page; then
`shot`, `read`, `tap/drag/key/swipe` operate on the scope page; a power-cycle
disarms it.

**Two residual items that need a one-time on-device check** (neither is a brick
risk — boot path untouched, disarmed after reset, reset recovers):
1. The arm word / injection state sits at `0x2000f100` in SRAM0+1. That address
   is verified mapped (memrw used it) but its *freeness while the scope
   acquisition buffers are allocated* was not verifiable statically. The MAGIC
   arm word makes a spurious arm essentially impossible even if the region were
   reused, but confirm on-device by reading `0x2000f100` while scoping. If it
   is reused, relocate the struct (one `#define`).
2. Whether the stock scope GUI accepts the synthetic touch-queue entries as a
   tap/drag/key is unverified (the queue byte layout is emulator-checked;
   efficacy is not). Validate with a visible action (e.g. `key 1` = RUN).

**Screenshot rate on the scope page** is poll-limited (~10 commands/s, one per
100 ms tick), so a full scope frame takes ~1–2 min; grab regions
(`shot --rect`) for quick checks. The DMM page (via `dm40_dbg.bin`) is not
poll-limited.

## USB as a transport — evaluation

- The stock app brings USB up **only** in "USB Connect" disk mode, where it
  enumerates as MSC; in every other mode USB is **down** (no device present).
- An experimental patch already brings USB up outside that mode: it hooks the
  multimeter-page update tail, calls a watchdog-fed `usb_enable_fast`, and
  installs its own **vendor-class** descriptors (made-up PID `0x02F0`, interface
  class `0xFF`) so the host does not mount a disk; it answers a vendor control-IN
  request with the measurement snapshot. The stock USB device library is usable
  for bulk/CDC (ep_init `0x08017744`, ep_send `0x080178c4`, ep_recv `0x0801782a`,
  ctl_send `0x080179e6`; per-EP FIFO table at `0x2002030a`). See
  `stock/re/ANALYSIS.md` §5.
- **Feasibility**: a vendor-specific bulk interface carrying these debug
  commands is feasible, but it requires enabling USB from a resident hook (the
  meter-page tail), i.e. **new always-executed code** — the opposite of the
  no-brick constraint here, and the v1 of that patch boot-looped until a
  watchdog auto-rollback was added. USB is also still tied to the meter page
  for the same reason the BT parser is.
- **Recommendation**: **Bluetooth first** (done here, zero resident code). USB
  vendor-bulk debug is a worthwhile but separate, resident-code change; keep it
  as a documented follow-up gated behind the same flag/auto-rollback safety and
  on-device validation.
