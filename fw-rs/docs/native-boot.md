# Native-format firmware with a recovery shim

Goal: stop RAM-loading fw-rs. Build it as a **full firmware in the stock update
format**, flashed exactly like a stock update at `0x08004000`, with a tiny shim
at our reset vector that keeps the device unbrickable and lets the user toggle
stock <-> fw-rs easily.

This design is **recovery-first**. The single most important fact is that the
stock **FS-loader** cannot be overwritten by a normal firmware update and runs
before the application on every boot, so it is a permanent escape hatch.

---

## 1. Established facts vs. unknowns

### Boot chain (ESTABLISHED — `firmware/re/ANALYSIS.md` §1, §3)

```
0x08000000  ROM bootloader  16 KB   SP 0x20021820  reset 0x080001c1   (sector 0)
0x08004000  application    ~480 KB  SP 0x2002f7f8  reset 0x080041c9   (sectors 1..8)
0x080a0000  FS-loader       64 KB   SP 0x20022de8  reset 0x080a01c5   (sector 9)
0x080a4000  UID-bound bootloader data  (inside sector 9)  -- NEVER erase
0x080c0000  settings / calibration     (sector 10)        -- NEVER write
0x080ff000  "one-word boot flag"       (sector 11)         UNVERIFIED use
```

- **ROM bootloader** verifies and launches the FS-loader; each stage launches
  the next with `msr msp` + jump through the next stage's vector table. No A/B
  slot, no RAM execution, no jump to the GD32 system (DFU) bootloader. SWD is
  read-locked (option bytes dumped). (`ANALYSIS.md` §1)
- The ROM bootloader polls **PA2** at boot: low for the whole poll sends the
  boot to the FS-loader's install state. PA2 is the "TX" pad of the internal
  debug header; shorting it to GND is the hardware recovery entry (owner,
  2026-10-24; procedure in [hw/README.md, Hardware recovery](../../hw/README.md#hardware-recovery)).
- **FS-loader** (`0x080a0000`) has **its own USB mass-storage stack + FatFs**,
  runs **before the app on every boot**, calls
  `f_findfirst("/","dm40*.bin")` (`0x080a26c4`) and installs the **first**
  match (directory order, not newest; one file per boot; no version compare).
  It verifies the header CRC and body CRC before accepting, writes the body to
  `0x08004000`, and the writer is **bounded to `0x08004000`..`0x080a0000`**. On
  success ("update ok") the FS-loader itself deletes the file (`f_unlink` at
  `0x080a27ae`, then a FAT sync and reset); on any failure the file stays. The
  stock app never deletes or renames it (code-verified 2026-10-13,
  fsloader-install-trigger.md "Who deletes the installed dm40*.bin").
  (`ANALYSIS.md` §3). Verified here: the FS-loader feeds the free watchdog (it
  references `FWDGT` `0x40003000` at `0x080a7438`) and references the app base
  `0x08004000` at `0x080a18cc`.
- **Recovery** therefore reduces to one requirement: *get a valid `dm40*.bin`
  onto the SPI-flash FAT and reboot.* The FS-loader does the rest, independent
  of whatever is (or is not) at `0x08004000`.

### Update-file format (ESTABLISHED — `ANALYSIS.md` §2, `firmware/dm40fw.py`)

Decoded file = 12-byte LE header `word0 word1 word2` + body (flashed at
`0x08004000`). `firmware/dm40fw.py` implements the whole thing and reproduces
all 11 official files byte-for-byte:

- `word1` = body length (`dm40fw.py:build`).
- `word0` = `(~X<<16)|X`, `X` = CRC-16 (poly `0x8005`, init `0xFFFF`, MSB-first,
  no reflection, no final XOR) over the 8 bytes `word1 ++ word2`
  (`dm40fw.py:header_word0`, `crc16`).
- `word2` = `0x00010000 | L2`; `L2` = the same CRC-16 over the body **with the
  12 bytes at body offset `0x4000` (flash `0x08008000`) excluded** — the
  bootloader-managed marker (`dm40fw.py:body_checksum`, `MARKER_OFFSET=0x4000`,
  `MARKER_LEN=12`).

**Obfuscation and the short-last-block rule (2026-10-14).** The whole file
(header included) is obfuscated in 128-byte blocks: `a = block[0x1F]`, key
position `kp = a if a < 128 else a >> 1`, `k = block[kp]`, plain = `(c ^ k) - a`
except at `0x1F` and `kp`. When the file length is not a multiple of 128 the
last block is short; if it is **longer than `0x1F` bytes and `kp` >= its
length**, `dm40fw.py` stores it as is, but the real FS-loader does not: it takes
the key from a stale byte of its 128-byte buffer and corrupts the tail (seen in
the Renode emulator on the real loader: `bf->41`, `ff->81`), so the body CRC
(`L2`) fails -> "update error", and the app is already erased (recovery only
through the loader's USB disk + "update"). Safe: whole blocks, a last block of
<= `0x1F` bytes (stored as is by both), or `kp` < its length (the vendor
`dm40_v_2_6.bin`: 36 bytes, `kp` 8). Rule: **every update file we generate
is padded with 0xFF so 12 + body is a multiple of 128** (`word1` and `L2`
include the padding; the loader flashes it like any body byte):
- `dm40fw.py pack --pad` / `build(pad=True)` pads; `pack` without `--pad`
  packs as is (vendor round trip) but refuses a hazardous body;
  `info` / `unpack` warn about a hazardous input (`tail_hazard`);
- `build.sh` packs with `--pad` and fails unless the file is a multiple
  of 128; `firmware/patch/patcher.py` builds with `pad=True`;
- `INFO/FIRMWARE.BIN` on the USB volume (`calbackup::FwFile`) is 12 +
  `word1` bytes, so it is whole blocks for every image built that way; for an
  installed image whose file would end in a hazardous block,
  `FwFile::from_flash` returns None (no firmware file);
- `tools/dm40dbg.py image` prints the last-block length and rejects a
  hazardous file; `tools/reload.sh` refuses to stage it;
- host test: `tools/fatren-test/run.sh` ("short last 128-byte block").

So `dm40fw.py pack --pad body.bin out.bin` produces a correct update file for
**any** body length, with no `--like` needed. **`firmware/README.md` is stale**
where it says words 0/2 are not understood — that was true earlier; the code now
computes both.

### Unknowns (and how to settle them live)

| Unknown | Impact | How to verify |
|---|---|---|
| Exact trigger that makes the FS-loader enter **its own** USB-disk mode at boot (not isolated — `ANALYSIS.md` §3). | Would let the shim bring up stock MSC with ~zero code (Option A below). Not required for safety. | Disassemble FS-loader around its USB init; or set candidate flags via the BT `memrw` patch (`firmware/patch/kit/tools/dm40_memrw.py`) and reboot, watch for USB disk enumerating. |
| Purpose of the `0x080ff000` "one-word boot flag" (`ANALYSIS.md` §1). A whole-image word scan found the value `0x080ff000` only inside an `ldm/stm` data-copy in the app (`0x08044844`), i.e. a false positive — **no confirmed pointer use**. | None if unused. If it is the FS-loader's disk-mode trigger, it enables Option A. | Read it over `memrw`; correlate with entering "USB Connect". |
| Whether the FS-loader leaves HCLK at 200 MHz / correct FMC wait states when it jumps to the app. | fw-rs currently **assumes** 200 MHz (`src/clock.rs:3`); XIP from flash needs correct wait states. | Read `RCU_CFG0`/`FMC_WS` in the shim over `DBG:` once Stage 1 runs; or disassemble the FS-loader clock init (`0x080a0ac1`, confirmed to set CPACR+VTOR+RCU). |
| Which flash sectors the FS-loader erases for an app update. | Confirms our hole/layout assumptions. | Disassemble the FS-loader erase path; or observe before/after dumps. |

Self-hold / power-latch: **no dedicated CPU self-hold GPIO is evident in
`fw-rs/src/power.rs` or the RE headers.** The power **key is PD6** (`dmm_v2_6.h`
"Power key PD6, 1.5 s hold -> power off"; the stock TIM7 1 kHz ISR polls it,
`lcd_buzzer_v2_6.h:52`). `PE2`/`PC4` are the DMM/scope **analog rails**, not a
CPU latch. The current RAM-loaded fw-rs runs fine without asserting any latch
because stock already set the pin state. Treat the self-hold mechanism as
**UNVERIFIED** and have the shim **leave GPIO as the FS-loader left it** and only
*read* PD6. (If a self-hold pin is later found, the shim must assert it before
its wait loop.) Later state (PE2 used as the power hold by `src/boot.rs`, still
open which rail holds the power):
[hw/README.md, Power and battery](../../hw/README.md#power-and-battery) and
[Unknowns](../../hw/README.md#unknowns) item 2.

---

## 2. Flash layout (native image)

```
0x08000000  ROM bootloader        sector 0    never touched by updates
0x08004000  OUR image starts      sector 1    = update body offset 0
0x08008000  12-byte marker hole   sector 2    leave 0xFF, excluded from L2
0x08004000..~0x0802c000           our ~160 KB code/rodata (current ~107 KB)
0x080a0000  FS-loader             sector 9    recovery path, never overwritten
0x080a4000  UID-bound data        sector 9    never erase
0x080c0000  settings/calibration  sector 10   read-only (as today)
0x08080000  (old sector-8 image)              becomes dead; retire
```

GD32F427 sector map (16/16/16/16/64/128.. KB): the app region spans **sectors
1..8** (`0x08004000`..`0x080a0000`), matching the FS-loader writer bound. Our
image occupies the low end of that. **Reserve 12 bytes at `0x08008000`** (body
offset `0x4000`) in `link.x`, filled `0xFF`, so `L2` matches what the bootloader
leaves there. Never place our code at `0x080a0000`+ (FS-loader, UID data,
settings).

---

## 3. XIP impact (fw-rs runs from flash now, not SRAM)

`0x08004000` is inside the zero-wait first 512 KiB, so the reason for the
SRAM-copy (`src/startup.rs` `.boot` copies `.text`+`.rodata` into SRAM0 because
sector 8 at `0x08080000` is slow) **goes away**:

- `link.x`: `FLASH ORIGIN` 0x08080000 -> **0x08004000**; drop the `CODE` region
  and the `.text AT > FLASH` LMA copy; `.text`/`.rodata` stay **in flash**
  (XIP). Keep `.data` copied to TCM, `.bss` in TCM, `.dmabuf` in SRAM1+2
  (`0x2001C000`). Stack top stays `0x10010000` (TCM). Reserve the `0x08008000`
  hole.
- `src/startup.rs`: remove the `.text`/`.rodata` SRAM copy loop; keep VTOR ->
  `0x08004000`, `.data` copy, `.bss` zero, FPU/TCM clock enable. Vectors live at
  the image start in flash.
- **`src/clock.rs` must stop assuming 200 MHz.** As the native app we own the
  clock tree: configure the PLL (M=25 N=400 P=2 from the 25 MHz HXTAL, as stock)
  **and set FMC wait states + prefetch + I/D cache before raising the clock**
  (raising HCLK with too-few wait states while executing XIP is a hard fault).
  The stock app does this at reset; replicate it. If live inspection shows the
  FS-loader already leaves a correct 200 MHz + wait-state config, we may keep
  the assumption, but configuring it ourselves is the robust choice and removes
  a hidden dependency.
- SRAM0 (`0x20000000`) is fully ours (no stock buffers to avoid), simplifying
  the scope record / DMA buffer placement that the RAM scheme had to tiptoe
  around.

---

## 4. The shim (minimal, dependency-free, first thing at 0x08004000)

The shim is the first code the FS-loader jumps to. Keep it tiny: no C runtime,
no `.data`, no peripherals beyond a GPIO read and a reset. It decides in a few
hundred bytes, then either falls through to fw-rs `main` or triggers recovery.

```
shim_reset (image word[1]):
  cpsid i
  set MSP = image word[0]            (already set by the FS-loader jump; redo cheaply)
  read reason = RCU_RSTSCK (0x40023874)   ; FWDGTRSTF = bit 29
  read key    = PD6 (configure GPIOD in, read IDR bit 6)   ; pressed level TBD
  read magic  = SRAM word @ RECOVERY_MAGIC (survives WDT reset)
  read tries  = SRAM word @ BOOT_TRIES

  recovery := (reason has FWDGTRSTF)            ; last boot was a watchdog reset
           or (key held >= ~2 s at power-on)     ; explicit user request
           or (magic == RECOVER_REQUEST)         ; app/DBG asked for recovery
           or (tries >= MAX_TRIES)               ; crash-loop guard

  clear RSTSCK.RSTFC (bit 24) and the magic
  if recovery:  -> enter_recovery()              ; see Option A / B
  else:
     tries += 1 ; store BOOT_TRIES
     VTOR = 0x08004000
     branch to fw_main_reset   (the real fw-rs startup)
```

- **Power-key hold:** if PD6 reads pressed, loop ~2 s **feeding FWDGT**
  (`0xAAAA -> 0x40003000`, harmless if the watchdog is off); if the key is
  released early, treat as a *normal* boot (short-circuit, no recovery) so a
  brief tap never strands the device. Only a sustained hold selects recovery.
- **Touch nothing else.** Inherit the FS-loader's GPIO/clock state. The only
  writes are: GPIOD mode for PD6 (restore after), `RSTSCK.RSTFC`, the SRAM magic
  and counter, and `VTOR`.
- **Boot-attempt counter** (`BOOT_TRIES` in a fixed SRAM word): the shim
  increments it; fw-rs `main` clears it once it has reached a known-good point
  (e.g. first full UI frame drawn). `MAX_TRIES` (e.g. 3) catches crash loops
  that are **not** watchdog resets (a hard fault that resets via a default
  handler, or a hang the watchdog later catches). Place `BOOT_TRIES` and
  `RECOVERY_MAGIC` in a reserved, never-zeroed SRAM slot (TCM top, outside
  `.bss`).
- **Indication in recovery:** cheapest is none, but a one-line LCD message or a
  status square is worth the few hundred bytes so the user knows they are in
  recovery. The LCD is already up from the FS-loader only if the FS-loader drew
  to it — do not assume; the recovery body (not the shim) can init the LCD.

Estimated shim size: well under 1 KB of code, no data.

---

## 5. Reset-cause and the recovery decision

- `RCU_RSTSCK` at **`0x40023874`**, `FWDGTRSTF` = **bit 29**, cleared by
  `RSTFC` = **bit 24** (precedent: the stock-app USB patch reads exactly these,
  `ANALYSIS.md` §5, clear routine `0x080069d8`). The patch runs in the **app**
  and successfully reads `FWDGTRSTF`, which **confirms the earlier boot stages
  do not clear `RSTSCK`** before reaching `0x08004000`. So the shim can rely on
  it. (Bit 29 is the standard GD32F4 position; still mark UNVERIFIED until read
  live — a wrong bit only weakens the watchdog path, the key-hold and SRAM-magic
  paths still work.)
- **SW reset** (`SYSRESETREQ` via `AIRCR` `0xE000ED0C`) sets `SFTRSTF`, **not**
  `FWDGTRSTF`, so normal/app-requested restarts are treated as **normal boots**
  by the shim. The app asks for *recovery* explicitly by writing
  `RECOVERY_MAGIC` then doing `SYSRESETREQ`, not by the reset kind.
- A single transient watchdog reset lands in recovery, but recovery **also
  offers a normal boot** (short key press, or a timeout that falls through to
  fw-rs) so one glitch does not strand the device.

---

## 6. New watchdog / tooling semantics

Today (RAM PoC): `dm40dbg reset`/`release`, a panic, and a keepalive timeout all
mean **"fall back to stock"** (stop feeding FWDGT -> reset -> stock fw). Under
the native scheme the meaning changes:

| Event | Old meaning | New meaning |
|---|---|---|
| fw-rs feeds FWDGT normally | bounded then release | **feed continuously while healthy** (we are the firmware now) |
| panic handler | stop feeding -> stock | stop feeding -> WDT reset -> shim sees `FWDGTRSTF` -> **recovery** |
| `DBG:` keepalive timeout | stop feeding -> stock | optional: keep it as a dev safety on debug builds; in production fw-rs feeds unconditionally |
| `dm40dbg reset` | reset to stock | `SYSRESETREQ` -> **normal reboot of fw-rs** |
| new `dm40dbg recover` | — | set `RECOVERY_MAGIC` + `SYSRESETREQ` -> **recovery/updater** |

fw-rs's app FWDGT policy: feed from the main loop once running; the shim feeds
only during its key-hold wait. The "never feed unconditionally" rule
(`docs/history/custom-fw-handover.txt`) still holds in spirit — the deliberate exits are now
(a) the panic/hang -> recovery path and (b) the explicit `recover` command.

---

## 7. Failure-mode / brick table

| Failure | What happens | What saves us | Residual risk |
|---|---|---|---|
| Our packed image has a bad header/body CRC | FS-loader rejects it ("update error"), keeps the current app | FS-loader verifies before writing | none (never half-flashed) |
| Our image hangs **before** the shim decision | WDT resets; next boot the shim sees `FWDGTRSTF` -> recovery | shim runs first, recovery brings up MSC | shim itself must be correct (keep it tiny) |
| Shim bug (wrong decision / hang) | could loop | FS-loader is untouched; a stock `dm40*.bin` already on the FAT is installed on the next boot regardless of the shim | shim must not erase/FAT-corrupt; it only reads |
| Power loss **during** FS-loader flashing | app region partly written, bad CRC | FS-loader re-runs next boot, re-installs the file (still on FAT: the loader deletes it only after a successful install) | needs the file still present + power |
| Corrupted SPI-disk / FAT | FS-loader may not mount | ROM bootloader + FS-loader still in flash; reflash SPI over the hardware fallback | hard case — see HW backstops |
| Stock `dm40*.bin` missing from FAT when app is dead | FS-loader has nothing to install, app won't run | **recovery mode's own MSC** (Option B) lets the user copy one | depends on the shim's recovery MSC working |
| Wrong-model update file | installs, but model byte mismatches | model is read from calibration (`0x080C0000` +0x168), labels/ranges only | cosmetic; not a brick |
| Dead battery mid-update | reset | FS-loader resumes when powered + file present | charge first |

**Hardware backstops.** Found 2026-10-24 (owner, on the device): shorting the
debug header's TX pad (PA2) to GND at power-on enters the FS-loader's install
state with its own USB disk, whatever the app does. Procedure:
[hw/README.md, Hardware recovery](../../hw/README.md#hardware-recovery). Still open: whether the SWD pads allow an
emergency reflash despite read-protection (after a mass-erase), and whether
`BOOT0` is exposed. The earlier fallback was **shorting the SPI flash**.

---

## 8. Staged rollout (Stage 0 first — nothing is risked)

**Stage 0 — prove recovery from TODAY's RAM-loaded fw-rs. No native image yet.**
Add a `DBG:RECOVER` SCPI command to the *current* sector-8 fw-rs that:
1. writes a known-good stock `dm40_v_2_6.bin` to the SPI-flash FAT (fw-rs already
   exposes this FAT over its own MSC and has `spiflash.rs`), named `dm40*.bin`;
2. does `SYSRESETREQ`.
Expected: the ROM bootloader + FS-loader install stock with **no stock app
involved**, and the device comes up stock. If this works, the entire recovery
concept is proven and reversible. If it fails, we have lost nothing (fw-rs is
still RAM-loaded; a battery pull returns to stock). **Do not proceed past Stage
0 until it passes and is logged in the README.**

**Stage 1 — native image = shim + current fw-rs app.**
- Build the body at `0x08004000`, reserve the `0x08008000` hole, `dm40fw.py
  pack` it into an update file.
- Install it the stock way: copy the file onto the FAT (via stock "USB Connect"
  while stock is running, or via fw-rs's own MSC), reboot, FS-loader flashes it.
- Toggle **stock <-> fw-rs several times**: fw-rs -> stock by copying
  `dm40_v_2_6.bin` + reboot; stock -> fw-rs by copying our file + reboot.
- Exercise every recovery trigger: power-hold, injected panic (WDT path),
  `DBG:RECOVER`, and crash-loop counter.

**Stage 2 — retire the RAM scheme.**
Remove `stub/`, `tools/legacy/flash.py`, the BT RAM loader path and the sector-8
`link.x`; update `README.md`, `docs/history/custom-fw-handover.txt`, `TODO.md`.

---

## 9. Tooling changes

- **`firmware/dm40fw.py`** — already complete; `pack body.bin out.bin` computes
  both header words for our body length. The dead `like`/`check is None` branch
  and the stale docstring can be cleaned up but are not blocking.
- **File naming for the FS-loader scan:** it installs the first `dm40*.bin` in
  FAT order. Keep **exactly one** `dm40*.bin` on the FAT when flashing (two give
  an order-dependent, possibly wrong install — `ANALYSIS.md` §3). Our file and
  the stock file must not coexist on the disk; the toggle procedure removes one
  before adding the other.
- **`fw-rs/build.sh`** — in addition to `dm40-fw.bin` (now the raw body), emit
  `dm40-fw.update.bin` = `dm40fw.py pack dm40-fw.bin`. Name the *on-disk* copy
  `dm40*.bin` only at flash time (per the stock app's deletion caveat in
  `docs/history/custom-fw-handover.txt`: the stock app scans for `dm40*.bin` on USB unplug;
  that is the intended path for installing us, but keep only one present).
- **`tools/reload.sh` / `tools/legacy/flash.py`** — the BT RAM loader becomes obsolete
  after Stage 2. The new "load fw-rs" flow is: copy the update file to the FAT
  (host file copy over whichever MSC is up) + reboot.
- **`tools/dm40dbg.py reset`** — now a normal `SYSRESETREQ` reboot of fw-rs; add
  `recover` (set magic + reset) and keep `shot`/`scpi`/`ping`.
- **Switch to stock from the host:** copy `dm40_v_2_6.bin` onto the FAT over
  fw-rs's MSC, then `dm40dbg reset` (or power-cycle). The FS-loader flashes
  stock and deletes the file itself after the successful install.

---

## 10. fw-rs impact list

Files to change:

- **`link.x`** — `FLASH ORIGIN 0x08004000`; drop `CODE`/SRAM-copy; XIP
  `.text`/`.rodata`; reserve 12 bytes at `0x08008000`; keep `.data`->TCM,
  `.bss`, `.dmabuf`; reserved never-zeroed SRAM slot for `RECOVERY_MAGIC` +
  `BOOT_TRIES`.
- **`src/startup.rs`** — remove the SRAM copy loop; add the **shim** as the
  reset entry (or a new `src/shim.rs` linked first) that runs the §4 decision
  before `rust_start`.
- **`src/clock.rs`** — real PLL + FMC wait-states/prefetch/cache init before
  raising HCLK (stop assuming 200 MHz).
- **`src/power.rs`** — add a PD6 read helper for the shim and for the app's own
  power-key handling; do not add a self-hold unless one is found.
- **`src/wdt.rs`** — feed continuously while healthy; panic handler stops
  feeding (-> recovery) instead of "-> stock".
- **`src/scpi.rs`** — `DBG:RECOVER` (magic + `SYSRESETREQ`); `DBG:` reset now a
  normal reboot. Add it to current fw-rs first for Stage 0.
- **`src/main.rs`** — clear `BOOT_TRIES` after the first healthy frame.
- **`build.sh` / `Cargo.toml`** — emit the packed update file; no new crates.

Behaviour that currently depends on stock having run first (must now be done by
fw-rs or inherited from the FS-loader, verify each): the **clock tree** (see
clock.rs), initial **GPIO state**, the **power self-hold** (inherited from the
FS-loader — do not disturb), **backup/SRAM init**, and the **SPI-flash disk**
state. The FS-loader runs immediately before us, so most peripheral state is a
known starting point; the risky one is the clock/FMC, addressed in §3.

---

## Summary

- The **FS-loader at `0x080a0000`** is the key: it has its own USB-MSC + FatFs,
  runs before the app on every boot, and installs any `dm40*.bin` on the FAT
  into `0x08004000` with CRC checks, writer bounded below `0x080a0000`. It is
  never overwritten by an app update, so the device is inherently hard to brick.
- **Recovery = get a valid `dm40*.bin` onto the FAT + reboot.** fw-rs already
  exposes that FAT over its own MSC, so toggling stock <-> fw-rs is a file copy
  plus a reset.
- The **shim** is tiny: read `RCU_RSTSCK` (`0x40023874`, `FWDGTRSTF` bit 29),
  read **PD6** (power key), check an SRAM magic and a boot-attempt counter; on
  watchdog reset / long key-hold / explicit request / crash-loop -> recovery,
  else branch to fw-rs. It touches almost no hardware.
- **Update format is solved** in `firmware/dm40fw.py` (`pack` computes both
  header words for any body length); `firmware/README.md` is stale on this.
- **Biggest real change:** fw-rs now runs **XIP from flash at `0x08004000`**, so
  `clock.rs` must configure the PLL **and FMC wait states/cache** itself instead
  of assuming 200 MHz, and `link.x`/`startup.rs` drop the SRAM copy and reserve
  the `0x08008000` marker hole.
- **Do Stage 0 first:** a `DBG:RECOVER` in the *current* RAM fw-rs that stages a
  stock file and resets, proving the FS-loader reflashes stock end-to-end before
  we ever replace the stock app.

### Open questions for the user
1. Physical backstop: answered 2026-10-24, the TX (PA2) to GND short on the
   debug header ([hw/README.md, Hardware recovery](../../hw/README.md#hardware-recovery)). SWD / `BOOT0` still open.
2. Is Option A worth chasing — i.e. finding the FS-loader's own USB-disk trigger
   so the shim needs no MSC at all — or is fw-rs's existing MSC in recovery mode
   (Option B) good enough? Option B is the safe default.
3. OK to run the Stage 0 reflash-to-stock test on the device (it reflashes stock
   over BT-loaded fw-rs; recoverable by a battery pull if it misbehaves)?


---

# Implementation (2026-10-13, build only — not yet run on hardware)

This section supersedes §2–§10 where they differ. By user decision:
- **Recovery** hands over to the vendor loader's own update mode by **zeroing
  the 12-byte app marker at `0x08008000`**, then resetting (SYSRESETREQ). There
  is no SPI, FAT, USB or LCD in recovery. Earlier variants are kept behind
  non-default features.
- **The installed `dm40*.bin` is renamed** to `_DM40PRV.BIN` after a cold
  normal boot. It is not deleted. (Observed on T3: the loader itself deletes
  the installed file, so fatren usually finds nothing; the running firmware's
  update file is `INFO/FIRMWARE.BIN` on the read-only USB volume, see
  "Unit data".)
- **Device-authenticity check** (`src/genuine.rs`, docs/genuine-check.md):
  informational only (`DBG:GENUINE?`). By user decision it is NOT in the boot
  path: the shim never reads it and nothing changes behaviour on its result.

## Files

| File | Role |
|---|---|
| `link-native.x` | native layout (below); the only linker script since the RAM build was removed (2026-10-22) |
| `build.sh` | builds the native image (`native` accepted as a no-op argument) |
| `src/shim.rs` | native vector table, `reset` (asm), `shim_main`; all in region A |
| `src/boot.rs` | shim decision `shim()`, reset flags, power key, power hold on/off, backup-SRAM words, loader marker check + `zero_marker()` (FMC), `DBG:BOOT?` text |
| `src/native.rs` | after the decision: `.data`/`.bss` init, then recovery, or board init + fatren + `main` |
| `src/board.rs` | native board bring-up (everything the stock used to do for us) |
| `src/recovery.rs` | recovery: zero the loader marker + SYSRESETREQ. Features `recovery-file` (`DM40RCVR.BIN` + power off) and `recovery-usb` (our own USB-MSC) keep the earlier variants for tests |
| `src/fatren.rs` | FAT16 root-directory edits: rename the installed image, remove a leftover `DM40RCVR.BIN`; with `recovery-file`, create `DM40RCVR.BIN` |
| `src/clock.rs` | `init_core(Src)`: measured stock values, bounded waits, IRC16M fallback, no FLASH_ACR write |
| `src/wdt.rs` | `start::fwdgt()` (shim); `powered_off()` (end of the app's power-off) |
| `src/msc.rs` | `STOP_UNIT` flag on SCSI START STOP UNIT |
| `src/scpi.rs` | `DBG:BOOT?`; `DBG:UPDATE` = `DBG:RECOVER`: zero the marker + reset |
| `src/main.rs`, `src/app.rs` | hooks |
| `tools/fatren-test/run.sh` | host test of fatren on mkfs.fat images (fsck.fat + mtools) |

Cargo features (2026-10-22, after the RAM build was removed; the stock fonts
are always embedded, the calibration backup always runs; every non-default
set is built, with all of build.sh's checks, by `tools/build-variants/run.sh`
into `target/variants/`, never over `dm40-fw-native.bin`):
- `vdisk` (default: the generated read-only USB volume);
- `fatren-dry-run`;
- `recovery-file`, `recovery-usb`;
- `keep-screen`;
- `ad2-isr` (the AD2 stream timer ISR, off until its device acceptance;
  docs/dmm-input-scope-feasibility.md 9).

Until 2026-10-22 there was also a RAM build (`./build.sh` with no mode: the
image in flash sector 8, copied to SRAM, loaded over Bluetooth into the
running stock firmware, `link.x`) and the T1/T2 RAM test images
(`test-initcore`, `test-recovery`, `link-test.x`). The native work was kept
byte-neutral for that build while it existed; see git history before that
date for its layout and traps.

## Native memory map and the loader marker

```
0x08004000  .vectors: word0 0x2002F7F8, word1 reset                region A, 16 KiB
            .shim:    reset, default_handler, boot::shim,
                      clock::init_core, wdt::start::fwdgt, shim_main
0x0800460c  .a2: hot named rodata (.rodata.a2.*: ASCII26, F14 blobs) region A2 (zero-wait)
            0xFF padding (objcopy --gap-fill=0xff) up to the marker
0x08008000  12-byte loader/ROM marker, 0xFF in our image             GAP
0x0800800C  4 bytes 0xFF padding
0x08008010  .text + .rodata (XIP): everything hot, then the .data   region B (zero-wait)
            load image (read once by the reset copy)
0x08080000  .cold (rarely run code)                                 region C (NOT zero-wait)
            end of image below 0x080A0000 (FS-loader above)
RAM: TCM  0x10000000 .data/.bss, then the stack down from 0x10010000;
     SRAM 0x20000000 .dmabuf (DMA buffers, the scope record, the disk
          scratch arena, the logger ring, ...), then .sram_bss (CPU-only, zeroed)
```

### Memory budget (2026-10-22 rework)

Numbers from `build.sh` (stackdepth.py and the link map) on the ad2-isr
build (default since b81abf2) / without the feature
(`--no-default-features --features vdisk`):

| | before (f5f8e96) | after |
|---|---|---|
| static worst stack path | 31 040 / 30 676 B | 21 760 / 21 396 B |
| TCM stack (64 KiB - .data/.bss) | 47 860 / 47 912 B | 46 620 / 46 688 B |
| stack headroom | 16 820 / 17 236 B | 24 860 / 25 292 B |
| emulator stack high-water (DBG:BOOT? STACK, scope + FFT + screenshot) | 30 700 B | 21 420 B |
| SRAM free (above .sram_bss) | 1 840 / 2 024 B | 12 856 / 13 040 B |
| region B free (zero-wait flash) | 348 / 8 188 B | 24 508 / 31 884 B |

Where it came from (no change in function; the one timing difference is
the FFT / screenshot sharing below):

- **main's frame** (20.5 KB): `App::new` was inlined into `main`, and its
  ~11 KB of temporaries (the sub-structs before they move into the App) sat
  under every main-loop call. `main` now builds the App in a static (`APP`,
  TCM .bss) through `new_app` (never inlined, `.cold`) and hands it to `run`
  (the loop); the start-up frame is popped first. The large SCPI reply
  builders (MMEM:CAT?, MMEM:DATA?, LOG:DATA?) run as closures through
  `ScpiPort::respond_with` instead of inlined into `run`. `run`'s frame is
  ~12.3 KB (the Usbtmc with its 6 KiB reply buffer is the largest local).
- **BT reply buffer** (6 KiB `.dmabuf` BLK): only ever live inside one SCPI
  dispatch (built, then copied into the TX ring at once), so it is now a
  local of `Bt::respond_with` (never inlined): that path, run -> respond_with
  -> builder (~21.4 KB), is shallower than the drawing paths that set the
  worst case. stackdepth.py follows the dyn calls (its `INDIRECT` list).
- **FFT work buffer** (superseded the same day, docs/ram-budget.md: the
  screenshot capture buffer CAP is gone, the screenshot streams from the
  panel, and the FFT work buffer lives in the disk scratch arena
  `calbackup::SCRATCH` with an ownership check; SRAM free 10.9 -> 60.7 KB.
  As it was before that:) (8 KiB of the 12 KB `scope_page::FFT` in TCM): an
  arena with an explicit owner, the last 8 KiB of the screenshot capture
  buffer (`screenshot::CAP`, 48 KiB). The FFT renews a lease every time it
  uses the buffer (a few main-loop passes); a screenshot started during a
  lease captures into CAP below the tail only (40 KiB; rows past it are read
  from the GRAM while writing, as for a full CAP); one started without a
  lease takes the whole CAP until its Data phase ends (the FFT job in flight,
  if any, is dropped and the frame asked again after), but gives the tail
  back as soon as its rows fit below it (~0.1 s). Effect: a screenshot of a
  screen over 40 KiB of RLE (the AC+DC and power views) taken while the
  scope FFT runs keeps the panel frozen longer; a spectrum is not updated
  while a screenshot of such a screen holds the tail. The FFT result (`db`,
  4 KB) stays in TCM. The host renders of the scope FFT views code to
  36-38 KiB.
- **Ink / Glyph**: `Glyph` keeps its blob slice from the glyph's first byte
  (no separate offset): 16 bytes instead of 20, ~1 KB off the drawing
  frames (every `Ink` holds 24 glyphs).
- **EP_MEMORY** 320 -> 64 words: USBFS has 4 endpoints with at most 64-byte
  packets, so 64 words is the most synopsys-usb-otg's OUT buffers can take.
- **screenshot `Job::off`** u32 -> u16 (word offsets into CAP): 642 B.
- Rebalanced: the touch log (1.9 KB) and the USB EP / MSC buffers (1.3 KB)
  moved from `.sram_bss` to TCM .bss (SRAM is the tighter one now).
- **Region B**: start-up and settings / page / dialog code and the debug /
  file replies moved to `.cold` (link-native.x lists them; see there), with
  `#[inline(never)]` on the helpers that were inlined into hot callers
  (`run`, `Scpi::handle`). The per-pass entry points of the `.cold`
  screenshot module (`poll`, `busy`, `frozen`, `fft_work`, `fft_free`) are
  `inline(always)` so a pass without a shot never reaches region C.

**Region B audit #2 (2026-10-22, after the fast logger)**: 9 304 / 16 568 B
free (ad2-isr / without) -> 30 368 / 37 840 B, no change in function. A map
of region B by inlined callee (addr2line -i over every address of `.text`
below 0x08080000) showed ~20 KB of `.cold` modules' code inlined into hot
callers, where the module patterns cannot reach it: `Gallery::poll` (with
the DLG decoders) in `App::tick`, `Gallery::touch` / `open` / `back`,
`VDisk::write_block` (host MSC writes) in `run`, `boot::put_info`
(DBG:BOOT?) in `Scpi::handle`, `upload::end`, `ui::help_page` and the
temperature page (`App::with_temp_page`) in `draw_static` / `gesture`,
the scope's Save CSV (`App::sp_save_csv` with `scope_csv::set`) in
`sp_gesture`, and `csvlog::delete_on` / `catalog_scpi_on` (patterns
without the `_on`): now `#[inline(never)]` (and a pattern where the module
is hot). `Dmm::poll_cap`'s two `sort_unstable_by` (5 and <= 35 values)
instantiated the core quicksort (~3.3 KB): an insertion sort (same order).
Stack and SRAM unchanged (headroom 22 328 B, SRAM free 11 872 B); region C
(`.cold`) grew by 16.7 KB, 31 700 B are left below 0x080A0000. Emulator
hooks on every function of C (from t = 3-6 s, per-second hit counts): DMM
idle, swipe-down CSV recording, combined view, an AD2 stream at 38400 S/s
(DMA frames, DRAW 1, ad2streamtest) and fast-logger disk sessions
(fastlogtest typ): C ran only at a recording / session start or stop and
for the DBG:HYSPI self-test after the stream, except LOG:DISK? (`fast_scpi`,
`.cold` since 5d5dba0), which a host polls while a disk session streams:
its reply (`fast_status`) is in region B again.

**Region B audit #3 (2026-10-22, on c58ae48)**: 11 928 / 20 672 B free
(ad2-isr / without) -> 26 160 / 32 808 B; region C (`.cold`) 25 328 /
25 768 -> 10 256 / 12 352 B left below 0x080A0000 (C is now the tighter
one: the next reclaim from B needs room there or a cut). Stack / SRAM
unchanged (headroom 21 132 -> 21 452 B, SRAM free 10 888 B). Same method
(addr2line -i over region B, then `#[inline(never)]` + a link-native.x
pattern per function, an nm diff of C against the old image: only the
intended symbols moved). To C:
- event code inlined into `App::gesture`: `App::tap` (5.3 KB with
  `pw_tap`, `kp_apply`, `resolution_cycle`, the range hits), `App::key`;
  `App::show` (view change), `App::wave_open` (`tap`, `key`, `show`: back
  in B since region A2, below);
- one-shot starts: `App::rec_start` (with `csvlog::start`); the outlined
  `Logger::start` / `logger::isr::start` (the Wave view's `start_wave`; the
  copies LTO inlined into `run` for LOG:STATe stay in B);
- page / dialog draws on a view change: `ui::pw::settings_page` (not
  `settings_status`: per tick on that page), `ui::device_page`,
  `dialog::fuse_check`; the menu / list / keypad helpers (`menu_item`,
  `main_menu_hit`, `list_page`, `page_title`, `set_stepper`,
  `keypad_input` / `_key` / `_hit`, `App::kp_labels` / `lim_unit`) and the
  generator page's boxes and 1 s status: no stream runs on those pages
  (leaving the Wave view or the inrush page stops theirs; touch and drawing
  are off while the logger runs);
- DBG / calibration replies: `Dmm::acdc_diag_text`, `Sentinel::diag_text`,
  `Scope::hw_calibrate` + `HwTrig::calibrate`;
- start-up, inlined into `run`: `Touch::new`, `Bt::new`, `SpiFlash::new`,
  `usb::init_hw` (not `Bt::init_module`: called every pass, returns early);
- vwrite's temp purge and post-write steps (`purge_tmp`, `park`, `check`,
  `finalize`, `commit`: after a host write session; MSC writes are refused
  while a logger session runs).
To B (the device check of f3aea2f, docs/device-check-2026-10-21.md 6a / 6c):
- the DMM-log and scope CSV generators of the USB volume (`Gen::open` /
  `advance`, `RowDec::*`, `SDec::*`, `chunk_at`, `parse_file`,
  `from_scale`, `s_pred`, `dlog::open` / `find` / `geometry` / `dir_start`
  / `dir_of` / `dlg_prefix`) in `.text.vdisk_rd`; `each_entry` is generic
  and inline(always), so no caller's closure gets a symbol of its own in C
  (the `near` bug of 0ec2093); `dlog::data` (MMEM:DATA?),
  `vdisk::readme` with its text writer `W` (sized on every root directory
  read), `vdisk::geo`, `vdisk::mem_read`,
  `calbackup::crc32_upd` (LTO merged dlog's identical CRC-32 into it) and
  `boot::bkp_enable` (`mark_healthy` at 60 s of uptime);
- the Save CSV tap's block plan (`scope_csv::set`, `dlog::scope_plan` /
  `scope_order`, `SEnc`; `scope_block` generic and inline(always)) in
  `.text.scope_plan`: it codes every block once (~1 ms each) in the tap,
  before the job starts and the stream pauses (wavetest with the emulator's
  SPI byte timing of c2709aa caught `scope_block` in C there).
Two Wave view changes (main.rs): the volume refresh waits while the stream
ISR owns the bus (`hy3131::streaming`; a host sees a save made in the Wave
view once the view closes), and the Wave view's stream pauses while a disk
job (Save CSV, a screenshot: screenshot.rs, region C every pass) runs and
restarts after it (the stopped record is kept, as on a range step): the
emulator showed the DLG write itself in C under the stream, not only the
refresh the device's lat_pc named.
Left in B on purpose: `scpi::parse_pow` / `App::pw_scpi` (FETC:POW:ALL? is
polled), the scope page's gestures (the Wave view's taps and drags, per
touch report while it streams), the logger panel, `persist`,
`autoset_step`. Taps: in the Wave view the scope page takes them
(`sp_gesture`, B), so `App::tap` meets a stream only on the inrush page
(whose `inrush_tap` was already in C): one tap = a burst of slow fetches,
each <= ~20 us of ISR entry latency (11.4), within the 20 us spike ad2pll
tolerates.
**Fonts stay in B** (except ASCII26 and F14, in region A2 since
2026-10-22, below). The glyph blobs are anonymous rodata (`.Lanon.*` after
the small named `*_E` descriptors: F78 25.3 KB, TEXT22 16.2 KB, ASCII26
11.5 KB, ...; 90.9 KB in all), not routable by a pattern without named
statics, and they
are read per draw on the screens a stream shows (the logger panel, the
DMM view, the Wave view). The emulator has no flash wait-state model (200
MIPS, no slow region), so it cannot measure the draw cost; the device did
(feasibility 11.4: ASCII26 / TEXT22 above 0x08080000 gave max_lat
18.8-21.8 us vs 4.6 us). The other ~55 KB of anonymous rodata (merged
strings, constants, panic locations) cannot be routed either.
**Check**: `emu/hy3131/wavetest.py` hooks every function of C and fails on
one entered while `hy3131::HANDED` is set (first call logged, then the hook
removed: a Python hook per call slows a busy function to a crawl): STOP +
Save CSV in the Wave view, then a host reading SCOPE/SCOP000.CSV and
LOGS/LOG000.CSV off the volume plus MMEM:DATA? of their DLGs while it
streams. a60f8ce and 4692aa9 fail it (`Gen::open`, `vdisk::refresh`, `scan_spi`'s
closure at the save; `SDec::get`, `dlog::data`, `vdisk::readme` at the
read); now only the Save CSV tap's own `sp_save_csv` runs. The same hooks
(a scratch script, `--extra-script`) in ad2streamtest (4800 DRAW 2, 9600
DRAW 1, 1200 DRAW 2; 38400 DMA DRAW 1 and 2), fastlogtest typ with
`--vol-read` and inrushtest: no region C call with the bus handed over,
except in inrushtest `App::inrush_live` once at the capture's start and
`inrush_scpi` for the test's own INR: setters / FETCh? while armed (by
design, app_inrush_live.rs: the polled INR:STATe? / CONDition? are in B).
Still in C on the volume's read path: the firmware image file (`FwFile`),
read only when a host opens it (its data is the installed image, region C
included, so its code in B would gain little). BACKUP.TXT (`describe`) is
in B since region A2 (`.text.vdisk_rd`).

### Region A2 (2026-10-22)

The zero-wait flash between the shim and the loader marker,
`0x0800460c..0x08008000` (14 836 B; "Flash outside the image" below), is
region A2: an output section `.a2` in `FLASH_A` right after `.vectors`
(link-native.x), claimed before `.cold` / `.text` (first matching rule
wins), taking `.rodata.a2.*` and `.text.a2.*`. It is part of the update
body (flash sector 1, with the vectors and the shim), so the FS-loader
writes it on every install like any other body byte; the body CRC `L2`
covers it; nothing in the install path reads that range but the marker
(`dm40fw.py` MARKER_OFFSET, reload.sh's 12-byte read at `0x08008000`,
emu/run.py `--zero-marker`, `boot::zero_marker`), which stays 0xFF in
the image. Only hot, region-B-class content belongs there; nothing the
shim or recovery needs (they stay in `.vectors` / `.shim`).

**What is in A2**: the ASCII26 (11 546 B) and F14 (638 B) font blobs,
named statics `#[link_section = ".rodata.a2.<NAME>"] pub static <NAME>_B:
[u8; N]` that `tools/mkstockfont.py` emits for the fonts in its
`A2_FONTS` (the others stay anonymous byte strings in B; the regenerated
`stockfont_data.rs` differs from the old one only in those two blobs'
declarations). Data, not code: a fixed size, no inlining that a pattern
could miss, and ASCII26 is the font with device evidence (the logger
panel, read per draw while the AD2 stream runs; feasibility 11.4). A2 has
2 652 B left; ASCII26 + F14 + HIST (484 B) would leave 2 168 B, just
above the warning, so HIST stays in B to leave the shim room to grow.

**What came back from C to B** with the 12.2 KB B gained: `App::tap`
(5.8 KB), `App::key`, `App::show` (their audit #3 patterns removed: taps
meet a stream on the inrush page, the slowest event code to have in C)
and `vdisk::describe` (`.text.vdisk_rd`). An nm diff of C against
e5dd09b: only these four symbols left it.

| (ad2-isr / without) | before (e5dd09b) | after |
|---|---|---|
| region A2 free | 14 836 B (unused) | 2 652 / 2 652 B |
| region B free | 26 136 / 33 320 B | 30 572 / 37 836 B |
| region C free | 10 120 / 12 240 B | 17 840 / 19 880 B |
| stack headroom (static worst path) | 21 436 / 21 888 B | unchanged |
| SRAM free | 10 856 / 11 080 B | unchanged |

Rebased onto 98bb57f (the power meter's AC+DC parts and region C audit
#1 (1/3) landed meanwhile), the landed tree 737cdcf: A2 2 652 B, B 21 996
/ 28 660 -> 26 360 / 33 112 B, C 10 436 / 12 572 -> 18 220 / 20 276 B,
stack headroom 21 236 / 21 672 B and SRAM free 10 848 / 11 072 B
unchanged.

**Budget**: the link fails when `.a2` reaches the marker (`FLASH_A` is
`0x08004000..0x08008000`, plus an assert with the message naming
`A2_FONTS`); `build.sh` prints `region A2 free` and ends with a banner
below 2 KiB. Its 0xFF check of the gap now runs from the end of A2 to
`0x08008010`. The shim grows into A2's room: a shim change that fails the
A2 assert takes a font out of `A2_FONTS`.

**Install-format check** (the image of this change): `dm40fw.py info` /
`unpack` (the unpacked body equals the objcopy body + 0xFF padding, the
12 bytes at body offset 0x4000 are 0xFF), `tools/dm40dbg.py image` (OK,
whole 128-byte blocks), and the emulator's full chain (`emu/run.py chain
--app dump --zero-marker --disk-add dm40-fw-native.bin::DM40_FW.BIN`):
the real FS-loader installs the file, the flash at `0x08004000` equals
the body, the marker holds the update header, and fw-rs boots from it.

### Region C audit #1 (2026-10-22)

Asked for: at least 12 KB more free in region C without losing a
feature, the gallery (17.6 KB of C) the first suspect. Measured with nm
sizes per symbol in `0x08080000..0x080A0000`, a DWARF walk of every
inlined copy in C (per abstract origin: copies, bytes, and whether an
out-of-line copy exists too) and inline trees of the largest functions.

What the image does NOT have (the usual suspects):
- **no `core::fmt` machinery** in the image (no `Formatter`, `write_str`,
  `pad_integral`; only `panic_fmt` (8 B) and two `Debug::fmt` stubs of 22
  / 40 B, all in B; panics are `panic=abort`, C calls the
  `panic_bounds_check` / `slice_index_fail` / `div_by_zero` stubs in B);
- **no duplicated BMP decode**: one `Bmp::parse` (692 B, inlined once),
  one `Bmp::color` (208 B), one `Job::pix_block` (992 B). The gallery is
  dense single-copy code: `Gallery::poll` 6.6 KB = `poll_on` -> `work` ->
  `step` (4.4 KB: BMP head / pixel blocks, and `step_dlg` 1.9 KB = the
  DLG / FAST decoders) + catalog / page scan / delete; `begin_b` 1.5 KB
  (the log viewer's texts); its formats (16 / 24 / 32 / 4 / 1 bpp, RLE4,
  DLG and FAST logs) are features;
- **no costly inlining**: inlined copies in C next to an out-of-line copy
  total < 1 KB (mostly 10-40 B leaves: `mmio::*`, `stockfont::rect`,
  `copy_from_slice`, clamps, where a call costs as much as the body);
- **one monomorphization**: `App::with_temp_page(impl FnOnce)`, four
  copies of ~330 B (fixed below).

Done (e054fc8 9 936 B free; ec22925 7 984 B after E7 grew C):
| change | region C | commit |
|---|---|---|
| `gallery::G` (672 B .data image) -> `.bss`, set on the first `get` | +592 | 53f7be2 |
| `scope_csv::SNAP` (548 B): `rec: Option<&[u16]>`, all zero -> `.bss` | +580 | 53f7be2 |
| `csvlog::JOB` (512 B): zero sentinels until `start` sets JOB0, `dlog::ENC_ZERO` -> `.bss` | +512 | 53f7be2 |
| `App::with_temp_page`: a key argument instead of a closure (4 copies -> 1) | +944 | 53f7be2 |
| `gallery::Tb::c` `#[inline(never)]` (9 inlined copies next to its symbol) | +76 | 36e038a |
| 23 of 24 dead-code warnings (LTO had dropped it all) | 0 (noise) | c663746 |
| the `.data` load image (1 056 B) -> region B after `.text` | +1 056 | d8eaa7e |
| relocation: `ad2exp` (DBG:AD2:TORN? / NOISE? / DCV? / N7?, E7's `dc_enter`) and `dbg_dcv` -> region B | +5 208 | 263fcab |

Meanwhile upstream: region A2 / `App::tap` / `key` / `show` /
`vdisk::describe` back to B (6a0eb31, +7.7 KB of C) and the font rework
(d0ed7e4: B 26.4 -> 82.2 KB free). On 0fd0379 (ad2-isr / without):
region C 17 420 / 19 484 -> 23 804 / 25 800 B free, B 78 192 / 85 016 ->
71 980 / 78 900 B, A2 7 920 B, stack headroom 21 228 / 21 656 -> 21 236 /
21 664 B, SRAM unchanged. From the task's start (e054fc8, 9 936 B): +13.9
KB, of which this audit's shrinking 2.7 KB (53f7be2 + Tb::c), its
relocation (the .data image and the DBG experiments, B had room) 6.3 KB,
the rest upstream (A2 move +7.7 KB, net of E7 and later growth). The relocated code is on-demand DBG / experiment
code (zero-wait flash only makes it cheaper); `git revert 263fcab` puts
it back in C if B gets tight.

Checks: ui-render and gallery-test renders pixel-identical to the base, 0
violations; tools/font-matrix identical; every `tools/*/run.sh`;
`emu/gallerytest.py` (new: open, page swipes, the viewer = the BMP bytes,
Delete -> Cancel / Delete, back out) PASS on base and change, screenshots
identical; wavetest (hooks every function of C while the stream owns the
bus: none called), ad2streamtest, inrushtest, dcvtest, couptest, powtest,
senttest, leadstest, burdentest, btwaketest, the HY3131 model unit tests.
Two wall-clock checks failed once under a load average of ~60 and passed
on a rerun (ad2streamtest "every captured record drained": 861 < 1000
records at 1200 S/s; powtest acdc-floor); modetest's fw-rs scenarios get
no BT reply on the base image as well (0 readings, pre-existing).

Candidates left (measured or estimated, not done):
- DBG-only code still in C, if B keeps its room: `hyspi` (DBG:HYSPI:SELF?
  / WR?, 2.3 KB; ad2streamtest uses both), `boot::put_info` (DBG:BOOT?,
  1.2 KB), `prof::report` (DBG:PROF?, 1.0 KB; usbtmc_stress, device
  checks), `touch_diag` (DBG:DIAG?, 0.9 KB), `Dmm::acdc_diag_text`
  (DBG:ACDC?, 0.7 KB), `HwTrig::calibrate` (0.7 KB): ~6.8 KB by relocation.
- Gating the AD2 experiments behind a non-default feature: 2 580 B on
  ec22925 (trial build), but the open TODO (B) device session still uses
  `ad2 noise` (DBG:AD2:NOISE?), so not done.
- One `fatren::fat_get` for fatdir / fastlog prep / csvlog / gallery
  (four inlined one-block-cache copies, ~0.2 KB; csvlog's runs during a
  recording, so the shared one must live in B); screenshot.rs and
  calbackup.rs have two more (the RAM-overlay change owns those files).
- `.data` statics of hot modules (scope_ui SHOWN 308 B, scope WAVE 224 B,
  inrush SES 200 B, STRIP_SHOWN 128 B): now B, not C, since d8eaa7e.
- The four text builders (`gallery::Tb`, `dlog::Txt`, `csvlog::W`,
  `vdisk::W`): different rules (Tb maps non-ASCII to '?', Txt has u64 /
  ms / Val), ~0.3 KB at most; panic sites in C (64 calls, ~0.5 KB with their checks).

**Link-time asserts** (`link-native.x`):
- region A ends at or below `0x08008000`;
- region A2 starts right after region A and ends at or below `0x08008000`
  (`build.sh` ends with a warning banner on stderr below 2 KiB free);
- `.text` starts at or above `0x0800800C`;
- the image ends at or below `0x080A0000`, with >= 4 KiB of region C free
  below it (`build.sh` ends with a warning banner on stderr below 8 KiB);
- region B (`.text`, hot) fits below `0x08080000` and leaves >= 8 KiB free
  (`build.sh` ends with a warning banner on stderr below 16 KiB);
- TCM stack (64 KiB - .data/.bss) >= 42 KiB;
- SRAM >= 8 KiB free above `.sram_bss`.

`build.sh` also requires >= 20 KiB between the TCM stack and the static
worst call path (`tools/stack-depth/stackdepth.py --check 20480`, incl. the
AD2 ISR and its exception frame), and prints region B / region C / SRAM
free.

Region C thresholds: C grows by whole functions moved out of B (0.3..5 KB
each, audit #3); 4 KiB still takes one more small move
after the 8 KiB warning. Relief for C is a cut, or `.cold` code moved back
to B while B's own budget allows: nothing above `0x080A0000` is reachable
(next section).

### Flash outside the image (2026-10-22, from the 2026-10-13 dump; not tried)

The GD32F427 here has 1 MiB (flash density 0x0400 at `0x1FFF7A22`, as
emu/run.py models it): sectors 0-3 16 KiB, 4 64 KiB, 5-11 128 KiB, erase
by whole sector. Only the low 512 KiB are zero-wait. The FS-loader writes
an update body only to `0x08004000..0x080A0000`, so anything outside would
have to be programmed by fw-rs itself (from a companion file on the SPI disk
or a compressed copy in the body), not by the update file.

| range | sector | 0xFF in the dump | usable? |
|---|---|---|---|
| `0x08000000..0x08004000` | 0 (ROM bootloader) | ~9.7 KB, scattered | no: the boot ROM's sector |
| `0x0800460c..0x08008000` | 1 (region A tail) | 14.8 KB | inside the image, zero-wait: used since 2026-10-22 as region A2 (the `.a2` output section, "Region A2" above) |
| `0x080AE700..0x080C0000` | 9 (FS-loader + boot data at `0x080A4000`) | ~70 KB | write-once: it can be programmed but never erased (the erase takes the FS-loader and the UID-bound data with it), so never updated |
| `0x080C0400..0x080E0000` | 10 (calibration / settings) | ~127 KB | write-once likewise (an erase loses the calibration); whether the stock app rewrites this sector on a settings save is not known: a return to stock could wipe it |
| `0x080E0000..0x080FF000` | 11 (`0x080FF000` boot word) | ~124 KB | erasable, but the FS-loader erases sector 11 at the start of every install (fsloader-install-trigger.md): fw-rs would re-program it after each update |

Sector 11 is the only candidate. Its risks: a version mismatch between the
image and the overlay (needs a CRC / version check and a "not available"
path, never a jump into stale code); ~124 KB programmed at boot under the
watchdog; power loss mid-program (the check must catch a partial overlay);
a wrong sector code in the erase (SN = code >> 3: 0x58 = 11, 0x50 = 10 =
calibration, 0x48 = 9 = FS-loader) bricks the recovery path or loses the
calibration; slow flash, so `.cold`-class code only; and the
`0x080FF000` word (stock app's boot flag) shares the sector.

**`build.sh`** checks the binary again:
- the 16 bytes at `0x08008000`, and all padding from the end of region A2 to
  `0x08008010`, are 0xFF;
- region A2 starts at the end of region A and ends at or below the marker;
- the end is below `0x080A0000`;
- word0 = `0x2002F7F8`;
- the reset vector is a Thumb address in region A.

It then runs `../firmware/dm40fw.py pack --pad`, which pads the body with 0xFF
to whole 128-byte file blocks ("Update-file format" above) and computes both
header words itself (no `--like`):
- word1 = body length (incl. the padding);
- word2 = `0x10000 | CRC16(body without the 12 marker bytes)`;
- word0 = CRC16 of words 1–2, with its complement in the high half.

Finally it runs `dm40fw.py info`. Current `fw-rs/dm40-fw-native.bin`:

```
word0  0xa83657c9   length 0x000384c4 (end 0x0803c4c4)   word2  0x0001e49e
SP     0x2002f7f8   Reset  0x08004041
```

`dm40fw.py unpack` of the packed file gives back the body byte-for-byte plus
the 0xFF padding (< 128 bytes). `build.sh` fails if the packed file is not a
multiple of 128 bytes or the padded body reaches `0x080A0000`.

**Word0 is the stock app's own initial SP** (`0x2002F7F8`). The FS-loader
launches the app with `msr msp, [0x08004000]; ldr r0, [0x08004004]; bx r0`
(`0x080a18f4`). The routine that decides whether to launch was not found, so
any SP plausibility check it may make passes exactly as it does for stock.
`reset` moves SP to the TCM top before anything uses the stack.

## Shim (reset path)

*Revised after the safety review `../docs/history/native-review-2026-10-13.md` (B2, H2,
H3, M1, M4, M5, L1, L3, L6).*

`reset` (asm):
1. `cpsid i`.
2. `CONTROL = 0` (MSP, privileged; L3).
3. TCM clock on.
4. FPU on (CPACR).
5. `SP = 0x10010000`.
6. `VTOR = 0x08004000`.
7. Call `shim_main` → `boot::shim(false)`. This uses only the stack, no
   `.data`/`.bss`:

1. **Watchdog:** `wdt::start::fwdgt()` writes `0xCCCC`, `0x5555`, waits
   (bounded) for PUD/RUD, sets PSC = 6 (/256) and RLD = 2500 (~16–20 s), then
   `0xAAAA`. From here on any hang resets into the shim. Every wait below
   feeds the watchdog.
2. **Power hold, then light.**
   - **Power:** PE2 = 1 (push-pull output, level set before the mode).
     - The ROM already raises PE2 and PC4 first thing on every reset
       (`0x08000574`). The stock app keeps PE2; this re-asserts it.
     - PE2 is the DMM analog supply; the stock power-off drops PE2 and PC4,
       so one of them holds the power.
   - **Light:** the backlight goes full on right away, as plain GPIO PA6 high
     (H2). A normal power-on press therefore lights the screen within
     milliseconds and the user lets go.
   - `board::init` later hands PA6 to TIM13 with the duty at 0, until
     `App::new` sets the level.
3. **Reset flags:** read `RCU_RSTSCK` (`0x40023874`), write RSTFC (bit 24),
   read it back.
   - Flag bits: 25 BOR, 26 PIN, 27 POR, 28 SW, 29 FWDGT, 30 WWDGT, 31 LP.
   - The stock never clears these flags; a running stock showed `0x3E000002`.
4. **PD6 input with pull-down** (B2), as the stock key init does
   (`app_decompile.c:6482`: input, PUPD = 2). Then about 10 µs for the pull to
   settle.
   - The ROM and loader leave PD6 floating, so without this a floating 1
     would read as a held key.
   - Pressed = 1 is confirmed in the stock code.
5. **Clock:** try `init_core(Hxtal)`, then `init_core(Irc16m)`. If both fail,
   SYSCLK stays on the 16 MHz RC (`clock` = FAILED).
   - The `Irc16m` path runs the PLL from the internal RC with M = 16. The PLL
     input is the same 1 MHz, so SYSCLK is 200 MHz and PLLSAI gives 48 MHz,
     only with RC accuracy.
   - `init_core`:
     - IRC16M on, AHB /2, SYSCLK → IRC16M, PLL and HXTAL off, PLL reset value;
     - HXTAL on (Hxtal path only);
     - LDOVS = 0b11, APB2 /2, APB1 /4;
     - **RCU_PLL = `0x29406419`** (measured stock value), PLL on;
     - over-drive handshake (ODEN, ODSWEN);
     - SYSCLK → PLL, TIMPRE.
   - **FLASH_ACR is not written.**
   - Every wait is a bounded poll.
   - The ROM and the loader spin forever on a dead HXTAL. So a failure here
     can only be transient (M4).
6. **DWT cycle counter on.**
7. **Backup SRAM:** enable access, then:
   - read and **consume** the request word;
   - read the previous-mode word;
   - read the crash counter.
8. **Key gesture** (H2). It applies only if PD6 reads pressed:
   - Wait while the key stays pressed for up to **4 s** (`HOLD_MS`). A release
     counts after 30 ms (bounce). A cycle-counter-independent iteration cap
     also bounds the wait (L1).
   - **The cap fails safe** (review N1): reaching it means the cycle counter
     does not advance, so it is treated as a **release**, never as "held". It
     is checked before the time limit. A stopped counter therefore cannot
     confirm the gesture or zero the marker: the first window ends in a normal
     boot reported as `KEY_HOLD_ABORTED` with `hold_ms` = 4000 and no cue (an
     abort with no beep heard means a stuck DWT counter); in the confirm
     window it is an abort. The cap is `(limit + 100) × cyc_per_ms / 8` passes;
     a pass is about 20 cycles in the native build, so with a running counter
     the time limit always comes first. With a stopped counter the first
     window ends after about 10 s of holding (watchdog fed).
   - The cue's per-half-period pass cap is `half / 2`, so even a counter that
     stops during the cue ends it after about 1 s. The cue decides nothing.
   - Released earlier → normal boot.
   - At 4 s comes the **cue**: the backlight goes off, a ~0.3 s beep sounds
     (buzzer PA7 toggled as GPIO at ~2.7 kHz), then the backlight comes back.
   - The key must then **still be held for 0.5 s** (`CONFIRM_MS`):
     - still held → recovery;
     - released in that window → **aborted** → normal boot (`KEY_HOLD_ABORTED`).
   - **Final gesture:** hold the power key from power-on until the beep, about
     4 s, and keep holding at least 0.5 s after it.
9. **Decide.** The first matching row wins.

   A watchdog reset is **genuine** (one of our own hangs) when FWDGTRSTF is set,
   neither SWRSTF nor PORRSTF is set, and `BKP_LAST` = NRML (H3). The shim
   clears the flags on every native boot, so a hang during a native session
   shows FWDGT alone. A loader install adds SW, and a power-on adds POR. A
   stale FWDGT from a stock session therefore never counts.

| Condition | Mode | `reason` |
|---|---|---|
| `BKP_REQ` = `0x52454356` (consumed) | recovery | `REQUESTED` |
| key gesture confirmed | recovery | `KEY_HOLD` |
| key held to the cue, released within 0.5 s | normal | `KEY_HOLD_ABORTED` |
| clock failed or genuine watchdog, crash counter +1 reaches `CRASH_MAX` (2) | recovery | `CLOCK_FAIL` / `WATCHDOG` |
| clock failed, counter below 2 | **SYSRESETREQ**, retry the boot | (`CLOCK_FAIL`) |
| genuine watchdog, counter below 2 | **normal** | `WDT_RETRY` |
| FWDGTRSTF, previous mode = recovery | normal | `WDT_AFTER_RECOVERY` |
| FWDGTRSTF otherwise (stale / with SW or POR / unknown previous mode) | normal | `WDT_STALE` |
| otherwise | normal | `NONE` |

10. **Hand over:** the previous-mode word and the crash counter are updated
    as follows:
    - Write the chosen mode to `BKP_LAST`.
    - On recovery, clear the crash counter: a fresh start after the
      reinstall.
    - Call `native::start`.

**Crash counter** (M1, M4):
- **Single hang or panic:** the first genuine watchdog reset only reboots
  normally (`WDT_RETRY`). The counter shows in `DBG:BOOT?`. A panic stops
  feeding, so it ends in the same watchdog reset.
- **Second crash:** a second one without 60 s of healthy main loop in between
  takes recovery.
- **Healthy clear:** `boot::mark_healthy(now_ms)` clears the counter once after
  60 s of main loop. **One-line call site for `main.rs`** (owned by another
  agent), inside the main loop right after `let now = clk.now_ms();`:
  ```rust
  #[cfg(feature = "native")] boot::mark_healthy(now);
  ```
  It is in place (`main.rs`, `boot::mark_healthy(now)` at the top of the
  loop). `now` counts from the start of `main` (after `pre_main`), so the
  window is **60 s of running main loop since the last boot**, not 60 s
  between two resets: a watchdog reset comes ~20 s after the hang, so two
  resets up to ~80 s apart (minus the boot time) still take recovery, and
  with DBG:RELEASE (the loop keeps running, unfed) the clear can also land
  inside the 20 s. Emulator-checked by `emu/recoverytest.py` (wdt-two,
  wdt-two-70, wdt-healthy).
- **Clock failure:** handled the same way. The first one retries the boot with
  a SYSRESETREQ, which redoes the ROM/loader clock set-up. Only a repeat takes
  recovery.

**Backup-SRAM words** (4 KiB at `0x40024000`; persist.rs uses `0x40024000..0x4002404B`):

| Address | Use |
|---|---|
| `0x40024FF0` `BKP_CRASH` | crash counter `0x435200nn` |
| `0x40024FF4` `BKP_TRIG` | last recovery result: `0x54520010` marker zeroed, `0x54520011` failed |
| `0x40024FF8` `BKP_REQ` | recovery request word (`0x52454356`), consumed by the shim; currently set by nobody |
| `0x40024FFC` `BKP_LAST` | mode of the previous boot: `0x4C4D524E` normal / `0x52564352` recovery; **cleared by `DBG:UPDATE`/`DBG:RECOVER`** (H3) |

**No reset loop takes control away from the user:**
- **A broken recovery does not repeat itself.** A hang in recovery
  (previous mode = recovery) is followed by a normal boot.
- **Hangs in normal mode retry first.** They reboot normally once, then
  recovery takes over.
- **Recovery itself is three flash word writes plus a reset.** After it, the
  vendor loader owns the unit until a valid install.

**First native boot:**
- After a loader install (SW reset) or a power-on, FWDGT is never "genuine",
  so the boot is normal.
- A stale stock FWDGT flag shows up as `WDT_STALE`, or the reason is `NONE`
  when the flags were clear.

## Recovery mode (`src/recovery.rs`)

Recovery uses the code analysis of the FS-loader full dump
(`docs/fsloader-install-trigger.md`, section "Update-loader install & error
paths").

**Boot gates.**
- **ROM:** at boot it polls **PA2** (`0x080005f4`, 1000 polls, pull-up). If PA2
  reads high at any poll, the ROM itself checks the 12-byte marker at
  `0x08008000` plus the body CRC, and launches the app; the loader never runs.
- **Loader:** runs only if PA2 stays low, or if the app/marker is invalid. It
  repeats the PA2 test; otherwise it enters its install state.
- **Install state:** with no file, or a failed one, it shows its screen and
  brings up a **working USB disk** (`0x080a53cc`) on every boot until an
  install succeeds.
- **On any `dm40*.bin`:** the loader first programs **12 zero bytes over the
  marker** (no erase). That is why a bad file left the app unbootable.

**Why the `DM40RCVR.BIN` + power-off variant is fragile.** If the user releases
the power key quickly at the next power-on, PA2 reads high and the ROM launches
our (possibly hung) app directly. The loader never sees the file.

**Recovery action (default):**
1. Keep the power hold (PE2) and feed the FWDGT.
2. `boot::zero_marker()` programs three words of `0x00000000` at
   `0x08008000..0x0800800B` with the FMC, exactly like the loader's
   `0x080a2564`/`0x080a7aec`:
   - Unlock if `LK` is set: KEY `0x45670123` then `0xCDEF89AB`.
   - Bounded wait for BUSY (the FWDGT is fed while waiting).
   - Clear STAT with `0xD3`.
   - CTL: PSZ = 32-bit (`0x200`), then PG.
   - For each word: write `0`, `dsb`, bounded BUSY wait, then check the error
     bits (`0x1F2`).
   - Clear PG, set LK.
   - Verify that all three words read back as 0.

   Details:
   - There is no erase: programming 0-bits over existing bits is what the
     loader itself does.
   - Words already 0 are skipped.
   - The address is a constant; **no other flash address is ever written.**
3. On success: `BKP_TRIG = 0x54520010`, then SYSRESETREQ.
   - The ROM rejects the marker whatever PA2 reads, so the loader runs.
   - The loader shows "open file error" and its **working USB disk** appears.
   - This works from a **warm** reset (verified in code). No file is needed.
4. On failure: the three words do not read back as 0. The read-back is the
   only success criterion. FMC error flags and a timeout are recorded
   separately as `fmc_last` (M5); the status clear is now `0x1F3`.
   - `BKP_TRIG = 0x54520011`, and the FWDGT stops being fed.
   - The watchdog resets the unit; the shim sees that the previous boot was
     recovery and boots normally.
   - The next normal boot shows `last_trigger` = `0x54520011` in `DBG:BOOT?`.
     Only that backup-SRAM word survives the reset: `fmc_last` is a `.bss`
     static, zeroed by `init_ram`, so the FMC error bits / timeout of the
     failed attempt are **not** visible after the reset (`fmc_last` then reads
     0). `fmc_last` is only meaningful after a `DBG:UPDATE` that answered
     "Flash error" (no reset follows then).

This is a **one-way door into the vendor updater.** The app bytes stay intact,
but nothing can make that installed copy bootable again except a vendor install
(SWD is read-locked).

**Recovery code size:** about 490 bytes, measured with `llvm-nm`:
- `recovery::run` 0x38, `zero_marker` 0xE4, `fmc_wait` 0x38;
- `sysreset` 0x34, `record_trigger` 0x18, `latch_on` 0x40.

The complete `test-recovery` RAM image is **1712 bytes**: shim, `init_core`,
FWDGT start, recovery and vectors (`.text` 1544).

**User procedure:**
1. The meter restarts by itself into the loader's update screen ("open file
   error"), and a USB disk (28e9:028f) appears.
2. Copy a firmware `dm40*.bin` onto it: the native fw-rs image as e.g.
   `dm40_fwrs.bin`, or `dm40_v_2_6.bin`. Nothing has to be deleted.
3. Press "update" on the screen, or power-cycle. The image is installed.

**Other entries** to the same state from a running native fw-rs:
- `DBG:UPDATE`, or its alias `DBG:RECOVER`:
  - Clear `BKP_LAST` (H3), zero the marker, then the deferred SYSRESETREQ.
  - "Flash error" with no reset only if the marker did not read back as zero.
  - **Sync and eject** a just-copied file first (L5).
- **Developer flow:** copy a new `dm40*.bin` onto fw-rs's own disk, then send
  `DBG:UPDATE`. The loader finds the file and installs it.
- The shim still honours the backup-SRAM request word (`BKP_REQ`), but nothing
  sets it now. It is kept for a later in-app "enter recovery".

**Non-default variants (tests only):**
- **`recovery-file`:** writes one 0-byte `DM40RCVR.BIN` root entry on the SPI
  FAT, only the root block, FAT untouched (`fatren::make_trigger`), then powers
  off (PE2 = PC4 = 0).
  - The update screen appears only on a cold boot with PA2 low long enough.
  - The `test-recovery` image is 9632 bytes.
- **`recovery-usb`:** our own minimal USB-MSC disk on the W25Q128 (msc.rs path,
  no USBTMC; serial = the chip UID as in normal mode, `REC1` until 2026-10-23).
  - Any SCSI START STOP UNIT, or a key hold of 2 s or more, powers off
    (an eject also makes the disk report no medium for that last second).
  - A short key press reboots.
  - The image is 25264 bytes.

## Normal native mode

`native::start` → `pre_main` (`board::init()` → fatren → calbackup) → the unchanged
fw-rs `main()`.

**What the stock had been initialising, which fw-rs relied on silently.** This
comes from an audit of all modules; `board.rs` now does each item, with values
from the stock boot FUN_0800af34:

- **GPIO port clocks A..E.**
  - The first `AHB1EN |= 0x1F` used to be in `Scope::new`.
  - `lcd`, `Dmm::new` (`hy3131::init_pins`, run once), `dmm.select` (relays)
    and `buzzer_init` (PA7 AF) all ran earlier. On a cold boot their GPIO
    writes would have been lost.
- **DMA0 clock (AHB1EN bit 21).** fw-rs never enabled it. The BT UART RX DMA
  and the generator DAC DMA only worked because stock had done it. DMA1
  (bit 22) is enabled too.
- **Rails as at stock boot:** PC0 = 1 (BT module), PE2 = 1 (DMM supply / power
  hold), 10 ms, PC4 = 0. PE2 used to be driven first by `show()`/`power_both`,
  after the HY3131 had already been configured.
- **PD6** as input.
- **FSMC bank 1 NE1:**
  - AHB3EN clock is enabled first, then AF12 with pull-up and very high speed
    on PD0/1/4/5/7/8/9/10/11/14/15 and PE7..15.
  - BCR1 = `(old & 0xFFF70081 & ~1) | 0x5011`, then MBKEN.
  - BTR1 = `0x00000F04` (read: ADDSET 4, DATAST 15).
  - BWTR1 = `0x00000201` (write: ADDSET 1, DATAST 2).
  - Decoded from the stock FUN_0800506c call (`app_decompile.c:5783`) and its
    mask literal `0xFFF70081`.
- **ILI9488:**
  - the stock FUN_0800a0e4 sequence (ID read 0xD3, then C0 … E1);
  - 0x11, wait 120 ms, 0x29;
  - MADCTL 0xE8 and a black screen.
- **Backlight TIM13:**
  - APB1EN bit 7; PSC 99, ARR 99; CCMR1 0x68 (PWM1 + preload); CCER CC1E.
  - CCR1 = 0, dark until `App::new` sets the level.
  - CR1 = ARPE | CEN; PA6 AF9.
  - fw-rs used to write CCR1 only.
- **Clock, TIMPRE and the PLLSAI input:** set by the shim's `init_core`.
  PLLSAI itself stays in `usb.rs`.
- **FWDGT:** started by the shim. fw-rs used to only feed it.
- **Fonts:** `native` implies `embedded-fonts`.
- **Unchanged:**
  - spiflash, usb and persist were already self-contained.
  - touch and power configure their own pins, but needed the port clocks.
  - Calibration (`0x080C0000`) and boot data (`0x080A4000`) are only read.

**Watchdog policy (native):**
- **Main loop feeds while it runs:** `INITIAL_HOLD_MS` = `u64::MAX/4`, so the
  loop feeds the FWDGT for as long as it runs.
- **Hang or panic → retry, then recovery:** a panic shows the magenta bar and
  stops feeding; a hang stops feeding too.
  - The first FWDGT reset reboots normally (`WDT_RETRY`, crash counter 1).
  - A second one without 60 s of healthy main loop in between goes to recovery.
- **`DBG:RELEASE`:** stop feeding, so a WDT reset follows after about 20 s.
  - It counts as a crash, the same as a hang.
  - Twice in a row it zeroes the marker.
- **`DBG:RESET`:** SYSRESETREQ → normal boot.
- **`DBG:UPDATE` / `DBG:RECOVER`:** `boot::enter_loader_update`: clear the
  previous-mode word `BKP_LAST` (H3) **and reset the crash counter**
  `BKP_CRASH` to `0x43520000` (N2: a count of 1 left from this session must not
  survive the stock interlude and turn the first hang after the reinstall into
  recovery), zero the loader marker, then SYSRESETREQ → the vendor update
  screen with its USB disk (see Recovery).
- **`DBG:BOOT?`** returns, in order:
  - `rstsck`, `rstsck_after`, `NORMAL|RECOVERY`, `reason`,
    `HXTAL|IRC16M|FAILED`, `last`, `hold_ms`;
  - `bcr1`, `btr1`, `bwtr1`, `lcd_id`: the FSMC values found before
    `board::init` rewrote them (in T1 these are the live stock values);
  - `fatren_status`, `found`, `renamed_8.3`, `old_backup`, `old_deleted`,
    `freed_clusters`, `others`, `rcvr_deleted`, `last_trigger`;
  - `marker_w0`, `marker_w1`, `marker_w2`, `VALID|INVALID`;
  - `crash_word` (`0x435200nn`), `HEALTHY|-`, `fmc_last` (0 = clean);
  - `PD6`, `moder`, `pupd`, `idr`: the power-key pin, read live at query time:
    `MODER[13:12]` (0 = input), `PUPD[13:12]` (**2 = pull-down**, B2),
    `IDR` bit 6 (1 = pressed). In native and T2 the pull-down is the shim's
    `pd6_input`; in T1 (`test_initcore_start` does not run the shim) it is the
    one `board::init` sets (`board.rs`, PD6 input + pull-down), on top of the
    stock's own. Expected `PD6,0,2,0` with the key released;
  - `STAGED`, `n`, `CALBK`, `status`, `written`, `conflict`, `differs`,
    `STACK`, `used`, `size`, `CALMS`, `ms` (`native::put_extra`, see "Stack
    depth" and "Unit data": calibration safety copy).
  
  `last_trigger` codes: `0x54520010` = marker zeroed, `0x54520011` = zeroing
  failed. With `recovery-file`, `0x5452000n`, n = 0 Created, 1 AlreadyThere,
  2 NoMedium, 3 BadBpb, 4 NoSpace, 5 Error.
- **`DBG:PING?`:** its `keepalive_left` field means nothing in native (it is a
  truncated `u64::MAX/4`).
- **Power-off in native** (key held 1.5 s, or idle timeout): if the unit stays
  powered (USB), `wdt::powered_off()` turns the backlight off and keeps
  feeding. The next key press after a release reboots normally. Without this,
  a charging meter would land in recovery 20 s after the idle shutdown. The RAM
  build is unchanged.

## fatren (`src/fatren.rs`)

The FS-loader installs **any** `dm40*.bin` in the FAT root on every boot it
runs on, re-installing identical images too, and never removes it. The stock
app used to delete it.

**Which file (review 2026-10-13 H1).** A cold power-on does **not** mean the
loader ran: when PA2 reads high the ROM launches a valid app directly. The
earlier PORRSTF rule would therefore have renamed a freshly staged, never
installed file (and deleted the old backup). fatren now renames only a
`dm40*.bin` whose **decoded 12-byte update header equals the 12-byte record at
`0x08008000`** — the loader writes exactly that header there when it installs
an image. The first 128-byte block of the file is decoded as in
`firmware/dm40fw.py` `decode_block` (`fatren::header_of`). Every other match is
left alone and counted as `staged` (the loader installs it on its next run).

**When:**
- Native normal mode, from `native::pre_main` before `main()`. USB does not
  exist yet, so the host cannot have the volume mounted.
- Every normal boot, whatever the reset cause (no PORRSTF gate any more: a
  file equal to the marker is installed in any case; this also covers
  `DBG:UPDATE`, after which the loader resets with SYSRESETREQ).
- Only when the `0x08008000` marker passes the ROM/loader header test (type
  0x0001, complement, CRC16 of word1‖word2; `boot::marker_valid`); otherwise
  nothing is renamed (`marker = None`).
- Never in recovery.

**Deviation from the request:** "after a confirmed healthy start (main loop
running a few seconds)" and "before USB is brought up" contradict each other
without restructuring `main()`, which brings USB up first. fatren therefore
runs once:
- the shim has decided normal;
- the clock is up;
- the SPI chip is identified;
- the BPB has passed its checks.

**Checks** (any failure → volume untouched, status `BADBPB`):
- jump byte and the 0x55AA signature;
- 4096 bytes/sector, equal to the device block;
- sectors/cluster a power of 2;
- 1–2 FATs;
- the root a whole number of sectors;
- total sectors ≤ device;
- FAT16 by cluster count (4085..65524), with the FAT covering every cluster.

**Steps.** The order means an interruption leaves at worst lost clusters,
never two live entries with the same name or a live entry on freed clusters.
1. **Scan** the root, read-only, in directory order.
   - A match is an 8.3 `DM40*` + `BIN`, or a long name `dm40*.bin`,
     case-insensitive.
   - Labels and directories are skipped.
   - A file's LFN entries are the checksum-matching run just before its 8.3
     entry.
   - `DM40RCVR.BIN` is handled separately.
2. **Leftover `DM40RCVR.BIN`:** mark it 0xE5. Its cluster chain is freed if it
   has one; the 0-byte trigger has none.
3. **Old backup:** if `_DM40PRV.BIN` exists, mark its 8.3 + LFN entries 0xE5
   and free its cluster chain to 0x0000 in **every** FAT copy. Only one backup
   is kept.
4. **Rename** the first match **whose header equals the marker** to
   `_DM40PRV` `BIN`, with the case flags cleared, and mark its LFN entries
   0xE5. No new LFN entries are written. Every other match is left alone:
   `staged` = header differs from the marker, `others` = all matches left
   untouched. (Step 3 only runs when there is such a file to rename.)
5. **Write and verify.** Every changed 4 KiB block goes through `spiflash.rs`
   `write_block`:
   - unchanged blocks are skipped;
   - a block that only clears bits is programmed without an erase;
   - any other block is erased (4 KiB) and programmed;
   - the write is read back.
   
   fatren then reads the block again and compares it. A final re-scan must find
   one match fewer, with the backup in place of the renamed entry. The status
   is `RENAMED`, otherwise `ERROR`.
6. **Restore:** remove the leading underscore on the disk.

`fatren-dry-run` only reports.

**Host test:** `tools/fatren-test/run.sh` passes. It uses mkfs.fat images in
the DM40 geometry: 4096 B sectors, 2 FATs, 512 root entries, 4087 clusters.
- **Rename** (update files are real packed images, `dm40fw.py pack`; the
  marker passed to the test is the decoded header of the installed one):
  - dry run writes nothing;
  - with an old backup, an LFN `dm40_v_2_6.bin` (installed), `DM40FW.BIN`
    (staged), a directory `dm40dir.bin` and other files present: the
    installed file is renamed, the old backup is deleted, and `DM40FW.BIN` is
    left (`staged=1`, `others=1`);
  - fsck.fat is clean and contents are unchanged;
  - the same marker again: `CLEAN`, nothing written; no marker: nothing
    renamed; the marker of `DM40FW.BIN`: it is renamed, replacing the backup;
    then `CLEAN`, nothing written;
  - H1 scenario: a staged stock file with fw-rs installed is not renamed and
    the backup survives; once the marker is that file's header it is renamed;
  - a long name with leading spaces is handled.
- **Recovery trigger:**
  - it reuses a 0xE5 slot;
  - only root block 5 is written, and the FAT is byte-identical;
  - the file is 0 bytes, and fsck.fat is clean;
  - a second call returns `AlreadyThere`;
  - nothing is written when a `dm40*.bin` is already staged;
  - a following normal run removes `DM40RCVR.BIN` and renames the image.
- **Not exercised:** a long name that matches while its 8.3 name does not.
  mtools derives the 8.3 name from the long one, so this case cannot be built.

## Unit data: the USB volume (`src/vdisk.rs`), the SPI disk (`src/fatdir.rs`) and the calibration safety copy (`src/calbackup.rs`)

USB storage redesign (owner 2026-10-21, replaces the two-LUN layout of the
same day). The SPI flash is **internal storage only**; USB exports **one
volume generated on the fly** (DAPLink-style virtual FAT; writable since
2026-10-22 only for deletes and a firmware file, "Host writes" below): the
unit's irreplaceable data (UF-20), the recordings / screenshots / scope
records converted to PC formats, and a raw image of the SPI flash. No host
can write the SPI disk or keep a stale copy of its FAT any more, so the
"Disk in use by PC - eject first" refusals are gone (screenshot, scope
record, recording, gallery / MMEM delete), and the on-device writers never
wait for an eject.

The **calibration safety copy** stays: ONE write-once copy of `CALIB.BIN`,
`BOOTDATA.BIN` and `UID.BIN` in the SPI disk's root (below).

Earlier builds (up to d341570) wrote nine files to the SPI disk at every
boot; **files those builds left are not touched** (they stay in the SPI root
and show only in `DEBUG/SPIFLASH.BIN`). fatren still renames an installed
`dm40*.bin` to `_DM40PRV.BIN` (loader re-install protection).

### The SPI disk (internal storage, `src/fatdir.rs`)

FAT16 as the stock firmware made it (the vendor FS-loader needs `dm40*.bin`
in its root for updates and recovery). Root: update files, `_DM40PRV.BIN`,
the safety copy. Our files live in subdirectories:

| Directory | Files | Writer |
|---|---|---|
| `LOGS/` | `LOGnnn.DLG` DMM recordings (docs/compact-log-format.md) | csvlog.rs |
| `SHOTS/` | `Snnn.BMP` screenshots | screenshot.rs |
| `SCOPE/` | `SCOPnnn.DLG` scope records | screenshot.rs `start_csv` |

`fatdir::boot` (native `pre_main`, after calbackup) creates the three with
NEED = 1002 slots (8 clusters of 4 KiB: 1000 numbered files + `.` + `..`, all
zeroed), so a writer never grows one; a writer that finds its directory
missing creates it itself (`fatdir::ensure`, ~1 s once), a shorter one (made
by a PC on the loader's disk, mtools' one cluster) is grown once. Order:
create = clusters zeroed (`.` / `..` in the first block) → chain in every
FAT copy → root entry (attribute 0x10, size 0); grow = new clusters zeroed →
their chain → the old last cluster linked. A power cut anywhere = at worst
lost clusters (host-tested with a cut before every block write).
`DBG:BOOT?` appends `FATDIR,<ok>,<created>,<failed>` (bit i = LOGS, SHOTS,
SCOPE).

**Files of ours from earlier builds stay in the root** (no boot-time move:
a move is two directory writes with a window in which either two live
entries share a chain or the file is lost). Every reader lists the root
too: the writers pick the lowest number free in the root AND the directory
(names never clash); the gallery shows root + LOGS + SHOTS; csvlog's list /
delete / `MMEM:CAT?` and dlog's `MMEM:DATA?` look in the file's directory
(fatdir::dir_of: the name decides it), then the root; the USB volume shows
both in its `LOGS/` / `SHOTS/` / `SCOPE/`.

### The USB volume (`src/vdisk.rs`)

One LUN (`Msc::new(.., 1)`, feature `vdisk`, implied by `native`; builds
without it still export the writable SPI disk; recovery-usb keeps its own).
INQUIRY product `DM40 files`. **Read-only by default** ("Files live",
decisions-2026-10-22 #6): the WP bit set, every WRITE fails DATA PROTECT
07/27/00, a change goes out as a UNIT ATTENTION at once. Settings 3/3
"USB disk" = "PC can edit" (or `DBG:VDISK:RO 0`) asks for the writable
volume: volatile (read-only again at every boot) and in force from the next
USB bus reset (a re-plug; a host reads the WP bit when it mounts, so the
mode never changes under a host); then a WRITE is judged sector by sector
(`src/vwrite.rs`, "Host writes" below), anything not accepted fails DATA
PROTECT 07/27/00. Volume label `DM40`, serial = CRC-32 of the UID. Geometry: 512-byte
sectors, 64 per cluster (32 KiB), 1 reserved sector, 2 FATs of 254 sectors,
512 root entries, 65000 clusters (FAT16), 4160541 sectors = 1.98 GiB (room
for SPIFLASH.BIN, the CSV expansion of a full SPI disk of logs, ~10-20x, and
the append-only allocation below).

| Path | Content |
|---|---|
| `README.TXT` | What is where; the two modes (Files live: new files while mounted, refresh; PC can edit + re-plug: delete / update from the PC); delete on the meter / MMEM:DEL; firmware update (copy dm40*.bin here in PC can edit, reload.sh / DBG:UPLOAD in any mode, or the loader's own disk). |
| `INFO/CALIB.BIN` … `LOADER.BIN` | Raw MCU regions, as the old LUN 1 (table below). |
| `INFO/BACKUP.TXT` | Addresses, sizes, CRC-32s (zlib), UID, firmware version, loader record, restore notes. |
| `INFO/FIRMWARE.BIN` | The running firmware as its vendor update file (`calbackup::FwFile`); only with a valid loader record and no hazardous last block. Reinstall: `tools/reload.sh --image FIRMWARE.BIN`, or copy it as `dm40fw.bin` to the loader's own disk. |
| `DEBUG/SPIFLASH.BIN` | The raw 16 MiB SPI flash, read on access (`fsck.fat -n` / mtools on it show the internal FAT16). A host may keep it in its page cache: copy it right after mounting for a current image. |
| `LOGS/LOGnnn.CSV` | The CSV generated from each `LOGnnn.DLG` (dlog.rs `Gen`; the exact text of the old CSV writer). |
| `SCOPE/SCOPnnn.CSV` | From `SCOPnnn.DLG`. |
| `SHOTS/Snnn.BMP` | The screenshot, streamed from its SPI file (FAT chain cursor). |

INFO sources:

| File | Source | Bytes |
|---|---|---|
| `CALIB.BIN` | `0x080C0000` | 4096 |
| `BOOTDATA.BIN` | `0x080A4000` | 16384 |
| `UID.BIN` | `0x1FFF7A10` | 12 |
| `OPTBYTES.BIN` | `0x1FFFC000` | 16 |
| `OTP.BIN` | `0x1FFF7800` | 528 |
| `ROMBOOT.BIN` | `0x08000000` | 16384 |
| `LOADER.BIN` | `0x080A0000` | 131072 |

Layout: clusters 2..6 = `INFO`, `DEBUG`, `LOGS`, `SCOPE`, `SHOTS` (one
cluster each: 1024 slots), 7 = `README.TXT`, then the INFO files, then
`SPIFLASH.BIN` (512 clusters), then the dynamic area. The INFO CRC-32s are
computed once in `pre_main` (`vdisk::init`); the first table of LOGS / SCOPE
/ SHOTS too (`vdisk::boot_refresh`: a DLG's CSV size costs ~10 ms per 100 KB
of logs; no media change for it).

**The dynamic files** (`TAB`, MAX_VF = 400 entries of 16 bytes in SRAM:
kind + number, directory slot, first cluster here, SPI start cluster + size,
size here). Numbered names only (`LOGnnn.DLG`, `SCOPnnn.DLG`, `Snnn.BMP`,
from the root or the directories); the file being recorded is left out
until it ends. `vdisk::refresh` (main loop: `dlog::stale()` after a
recording ended / a shot or scope record / a delete / an upload, run once
`dlog::gen()` has not moved for 1 s and no writer holds the disk) scans the
SPI directories and:

- keeps every file that is still there with the same SPI start + size **at
  the same directory slot and the same clusters** (a host's cached
  directory, FAT and inode stay right for it);
- appends a new file at the next slot of its directory and clusters from the
  high-water mark (a freed slot reads as a deleted entry, 0xE5; freed
  clusters stay unused);
- drops a deleted / changed file; `vdisk::forget` (csvlog `delete_on`) does
  that at once, so a new file of the same name never inherits its place;
- compacts (renumbers every file from the start) only when a file did not
  fit (slots or clusters exhausted) or after a USB bus reset (re-plug), and
  only while no host holds the medium locked (PREVENT = mounted on Linux);
  otherwise the new file waits for the Allow (unmount: `vdisk::on_unlock`).

Any change raises a one-shot UNIT ATTENTION 06/28/00, sent at once, also
while a host holds the volume **read-only** (the default, "Files live": the
2026-10-21 experiment: Linux then re-reads the mounted read-only volume and
shows new files at once, a file manager after F5; emulator + kernel attach
2026-10-22: a `LOG:CSV ON` / `OFF` while mounted, `LOGS/LOG000.CSV` listed
by `ls` within 2 s). With the writable volume ("PC can edit") no change is
made while a host has it mounted (`vwrite::frozen`, "Host writes"): the
change and its UNIT ATTENTION wait for the eject / re-plug. Emulator (read-only, 2026-10-21): a
screenshot taken while a PREVENT was active gave the UNIT ATTENTION 4 s
later, the new `SHOTS/S003.BMP` appended at a new slot and cluster, the
other files unchanged.

Sector generation (`VDisk::read_block`): boot sector from constants; FAT
entries from the fixed layout and, past it, a bisection of `TAB` (sorted by
first cluster: allocation is append-only, a compaction keeps the order); a
subdirectory sector = `.` / `..` + the files of that directory by slot; data
→ the owning file (MCU regions by byte loads, `BACKUP.TXT` / `README.TXT`
through a windowed text writer, `FIRMWARE.BIN` encoded per 128-byte block,
SPIFLASH by raw SPI reads, a CSV through the cached `dlog::Gen`, a BMP
through a cached FAT chain cursor). `DBG:VDISK:STAT?` → `<files>,<next free
cluster>,<files left out>`; `DBG:VDISK:RD?` → `<read_block calls>,<their
total ms>,<max us>,<CSV generator opens>,<SPI flash reads>,<their bytes>,<kind-3
seeks not from the cursor>,<chunk headers read>,<rows decoded>`
(cumulative since boot; diff two readings around a copy); `DBG:VDISK:EJECT n` = "no medium" for n commands,
then a UNIT ATTENTION (the earlier ADD / FORCE experiment hooks are gone).

### SCSI (`src/msc.rs`)

Per LUN as before (INQUIRY name from the device, removable; READ CAPACITY
/ FORMAT CAPACITIES; MODE SENSE WP; WRITE to a read-only device takes its
data phase and fails 07/27/00; the generated volume answers each block
itself (`gates_writes`: no global write inhibit; a refused block fails the
command with the device's sense, 07/27/00 refused / 02/04/01 busy, retried
by the host / 03/0C/00, after the rest of the data phase is drained);
sense per LUN; PREVENT / ALLOW tracked per
LUN, `msc::host_mounted()` = LUN 0's lock). UNIT ATTENTION: a device with
`media_change_immediate` (the generated volume) gets it at the next command
(not INQUIRY / REQUEST SENSE, never mid-transfer) whatever the lock; the
writable SPI disk of the builds without `vdisk` keeps the old rule (held
back while locked). A bus reset or unconfigure clears the locks and asks the
volume for a compaction at its next refresh. `MAX_BLOCK` = 512 in the native
build (the volume's sector), 4096 where the SPI disk is exported (its
buffer `MSC_BLK` then in SRAM `.sram_bss`, not TCM: the 4 KiB cost the TCM
stack its 42 KiB). INQUIRY with EVPD: page 0x00 (supported: 00, 80) and
0x80 (unit serial number = the USB serial, below); any other page fails
ILLEGAL REQUEST, INVALID FIELD IN CDB (05/24/00). Until 2026-10-23 an EVPD
request got the standard INQUIRY data. Linux usb-storage never asks for VPD
pages (`skip_vpd_pages`); Windows and macOS do.

**Eject.** START STOP UNIT with LoEj = 1, Start = 0 (Linux `eject` / `gio
mount -e` / a file manager's Eject; Windows Explorer's Eject and "Safely
remove") ejects the LUN: every command but INQUIRY, REQUEST SENSE, START
STOP UNIT and PREVENT ALLOW then fails NOT READY, MEDIUM NOT PRESENT
(02/3A/00) until a bus reset / unconfigure (a re-plug) or a host load (LoEj
= 1, Start = 1, e.g. `eject -t`), which raises the one-shot UNIT ATTENTION
06/28/00. Before this (up to b6909d6) the medium stayed present and Windows
mounted E: again at the next access. A plain STOP / START (LoEj = 0: Linux
sd around a suspend, `udisksctl power-off` before it removes the USB
device) and a power-condition request change nothing. Pending media-change
UNIT ATTENTIONs are kept while ejected. The CDB byte is read from the CBW
in the transport buffer as for PREVENT (usbd-storage hides it); a CBW not
found exactly once is ignored. Any 0x1B still sets `STOP_UNIT` (recovery-usb
powers off on it). "PC can edit": the session ends at the Allow (Linux sends
it before the eject) or after the quiet time (Windows); the firmware file
is committed by `vwrite::poll` once its data has settled, whether the
medium is ejected or not, and an Allow that comes before that check (`cp`
+ eject at once: the install used to be lost, also before the eject
change) waits for it, at most 1.5 s (`END_WAIT_MS`, frozen meanwhile). `DBG:VDISK:EJECT?` →
`<LUN 0 ejected>,<START STOP UNITs since boot>,<CB[4] of the last, 256 =
ignored>`. Tests: emu/mscresettest.py `stop-start`, `eject-load`,
`eject-port` (raw SCSI over USB/IP), emu/ejecttest.py (the kernel's
usb-storage + udisks: eject, power-off, a "PC can edit" install ejected at
once, re-plug).

### USB serial number (`src/uid.rs`)

Every unit used to report the USB serial `0001` (and `0` in `*IDN?`).
udisks merges disks with the same vendor / model / serial into one
(multipath) drive, so two DM40s, or the emulator next to a real meter, on
one Linux PC shared a udisks drive, and a drive-level Eject or Power Off
could act on the other unit. Since 2026-10-23 the serial is the GD32's
96-bit unique ID (`0x1FFF7A10`, 12 bytes) as 24 uppercase hex digits, bytes
in memory order (word 0 low byte first): the digits of `xxd -p -u
INFO/UID.BIN` and of the UID field of `DBG:GENUINE?`. The same string is:

- the USB iSerial (normal mode and `recovery-usb`);
- the SCSI unit serial number, INQUIRY VPD page 0x80 (msc.rs);
- the third field of `*IDN?`: `Alientek,DM40A,<serial>,dm40-fw 0.1`.
  libsigrok's scpi-dmm DM40 profile (`sr_scpi_get_hw_id`: four
  comma-separated fields, the serial is only stored) shows it as
  `[S/N: ...]`; ngscopeclient (`SCPIDevice`: `%127[^,]`) as the serial.
  Neither compares it. `SYST:IDN:EMUL 34401A` keeps the HP identity with
  serial `0`, as before.

The UID is in system memory, not in the slow flash bank (`0x08080000..`),
and the serial paths do not read it at request time: `uid::init` (region C,
`.cold`) reads it once in `native::start`, before the recovery / normal
decision is acted on, into a 24-byte TCM static; the USB descriptors, the
VPD page and `*IDN?` use that copy. (INFO/UID.BIN, `vdisk::mem_read`, and
`DBG:GENUINE?` still read 0x1FFF7A10 directly, as before.) The VID:PID (1209:0001) is unchanged, so udev rules keyed on it
still match; `/dev/disk/by-id/usb-ALIENTEK_DM40_files_...` names and the
Windows device instance path change once (a new device instance: Windows
installs it again, a WinUSB binding made with Zadig must be made again).

Privacy. The serial identifies the unit (as most USB disks' serials do):
any USB host reads it without mounting (`lsusb -v`), any Bluetooth peer
that may send SCPI reads it with `*IDN?`. It was already readable over USB:
`INFO/UID.BIN` on the volume and the FAT volume ID (CRC-32 of the UID). A
hash of the UID would be just as stable a tracking identifier, so it would
not hide more; the only thing the raw UID adds is the input of the stock
genuine check (docs/genuine-check.md), whose stored block is on the volume
too (`INFO/BOOTDATA.BIN`). There is no setting to hide it.

Emulator: `emu/run.py --uid` (default `emu_uid`: this unit's UID with bytes
4 and 9 = the USB/IP port, so every instance has its own serial and the
stock genuine check, which never reads those two bytes, still passes).
`emu/ejecttest.py` no longer rewrites the serial in its proxy; its safety
check requires the emulator's udisks drive to carry the emulator's serial,
not the unit's, and no other disk on it. Test: emu/mscresettest.py `serial`
(USB iSerial, VPD 0x80 / 0x00 / a missing page, `*IDN?`).

### The slow flash bank while an AD2 stream runs (2026-10-23)

Internal flash from `0x08080000` to the end of the 1 MiB is not zero-wait
(region C, the FS-loader, its UID data, the settings / calibration record).
One CPU load from it stalls the bus ~21.8 us, and the stream ISR's entry
with it, whether the load is an instruction fetch (region C code, the old
rule) or a DATA read. Device b6909d6 (device-check-2026-10-21.md "b6909d6"
item 3): a host copying INFO/CALIB.BIN, BOOTDATA.BIN, LOADER.BIN or the
FIRMWARE.BIN tail during a disk session lost 10 k samples at 38400 S/s and
289 k at 76800 (ROMBOOT.BIN and the FIRMWARE.BIN head, zero-wait: 280 ns).

Rule: **nothing reads the slow bank while `hy3131::streaming()`** (the ISR
owns the HY3131 bus: a fast-log session, the Wave view, the inrush page,
the AD1 stream ISR).

- **USB volume, the mirror** (`vdisk.rs` `mirror_boot` / `mir_read`,
  2026-10-24): every INFO/ byte from the slow bank lies in one range,
  `0x08080000..0x080C1000` (the FIRMWARE.BIN tail, LOADER.BIN with
  BOOTDATA.BIN inside it, CALIB.BIN), 260 KiB. It is kept as the raw file
  `_INFOMIR.BIN` in the SPI disk's root, and while `streaming()` the volume
  reads those bytes from there (`mem_src`; FIRMWARE.BIN through
  `FwFile::fill_with` + `fw_body`, which splits the one 128-byte block that
  straddles 0x08080000 between the flash and the mirror). So a copy of any
  INFO/ file runs at full speed during a session or in the Wave view, with
  no wait and no host timeout. Outside a stream the volume reads the flash
  as before.
  - Consistency: at every boot (`native.rs pre_main`, before USB, no
    stream) `calbackup::write_file` compares the file with the flash byte
    for byte; equal = nothing written (PRESENT), different or missing = a
    new chain written, read back, the entry switched, the old chain freed
    (WRITTEN, copy-on-write: a cut leaves the old file or lost clusters).
    The mirror is served only after this boot's PRESENT / WRITTEN (and its
    chain resolved, `calbackup::chain_of`): a stale file left by a failed
    rewrite is never used. Nothing else writes those clusters (on-device
    deletes take numbered names only; the host staging and the fast logger
    use free clusters). So it is the running image and calibration, byte
    for byte; a new image or a calibration changed by the stock firmware
    is picked up at the next boot.
  - Cost: 260 KiB = 65 clusters of the 16 MiB SPI disk (1.6 %; 130 while a
    rewrite holds both chains). Wear: only a changed mirror is written (65
    erases + 2 FAT and 1 root block per new image); an unchanged boot
    reads it (65 blocks). RAM: 144 bytes SRAM (the cluster list); region
    B ~0.5 KB (`mir_read`, `fw_body`, `body_copy`). Boot time: DBG:VDISK:
    MIR? field 4 (emulator: PRESENT 219 ms every boot; WRITTEN ~1.0 s, 4.3 s
    with the W25Q128 model's typ erase / program times; device: retest).
  - DBG:VDISK:MIR? = served (1/0), the writer's status (0 NOTRUN 1
    NOMEDIUM 2 BADBPB 3 PRESENT 4 WRITTEN 5 NOSPACE 6 ERROR), rewritten at
    this boot (1/0), ms.
  - MMEM:CAT? (csvlog `catalog_scpi`, its cache): lists gallery files only
    (S*.BMP, LOG*.DLG, SCOP*.DLG), so never `_INFOMIR.BIN`; its <used>
    count includes the mirror's 260 KiB. The cache keys on
    `msc::FLASH_WRITES`; the mirror is written only at boot, before any
    catalog, and its reads during a stream write nothing, so the cache
    stays valid while a host copies INFO/.
  - Privacy: CALIB / BOOTDATA bytes were already on the SPI disk (the
    write-once safety copy, calbackup.rs) and leave the meter only through
    the USB volume as before (INFO/, DEBUG/SPIFLASH.BIN). Nothing logs them.
  - Pacing (2026-10-24, branch mirror-bw; device 612c45c M2: a 76800 disk
    session lost ~21 700 samples per host copy of LOADER.BIN). A mirror
    sector costs the main loop its whole SPI read (512 polled bytes, ~0.97
    us each, plus a wait for a page program in the chip): ~1.1 ms of wall
    time with the stream ISR at ~54 %, taken from the fast logger's coder.
    So while `streaming()` a mirror sector is read only when the pass grants
    it (`vdisk::mirror_pass`, main.rs once per pass before the MSC poll, the
    level from `fastlog::mirror_level`; `mir_hold` via msc.rs
    `BlockDevice::read_wait`): at most one per pass, with the chip idle
    (one RDSR, `spiflash::chip_busy`), and only when the logger has caught
    up (ring <= 384 records, FIFO <= 1 KiB at the pass's start). Behind:
    after 10 ms of waiting; pressed (ring over half full, the writer
    trial): after 50 ms. Otherwise the READ waits in its data phase (the
    host's IN tokens NAK; no NOT READY, no retry, no 180 s exposure); a
    240-sector READ needs <= 12 s against its 30 s timeout even at the
    50 ms bound. Without a fast-log session (Wave view, inrush, AD1 / host
    sessions) the level is always free: one sector per pass. Memory-served
    sectors are not paced (they cost ~nothing; emulator: 7.7 MB of
    ROMBOOT.BIN during a 76800 session, lost 0). Region B
    (`.text.vdisk_rd`), checked with emu/regionc.py. DBG:VDISK:MIR? fields
    5-7: sectors read from the mirror while streaming, holds (one per poll),
    the longest hold (ms). The fast logger's own CPU share: LOG:DISK?
    field 14.
  - A failed SPI read of the mirror (`mir_read`) switches it off for the
    rest of the boot; msc.rs then answers that READ NOT READY (read_busy)
    instead of MEDIUM ERROR, and the fallback below applies.
- **USB volume, fallback without a mirror** (no SPI disk, NOSPACE, ERROR;
  `vdisk.rs` `read_busy`, `msc.rs`): a READ(10) touching a
  sector served from the slow bank fails CHECK CONDITION, NOT READY,
  LOGICAL UNIT IS IN PROCESS OF BECOMING READY (02/04/01) before its data
  phase (the CSW goes out at once; Linux takes it as "device skipped data
  phase"); a transfer in progress fails at the first such block. The
  sectors: CALIB.BIN, BOOTDATA.BIN and LOADER.BIN whole, FIRMWARE.BIN from
  the sector holding file offset 12 + 0x7C000 (body 0x08080000; with the
  12-byte header and the 128-byte encoding blocks: file offset 0x7C000).
  Every other sector (directories, README, BACKUP.TXT, ROMBOOT, UID, OTP,
  OPTBYTES, the FIRMWARE.BIN head, logs, shots) reads as before. The
  answer is per command: a READ(10) is refused whole when any of its
  sectors is slow, so a host request (readahead, a 120 KB bio) that spans
  FIRMWARE.BIN offset 0x7C000 waits too.
- **The settings / calibration record** (`dmm.rs` `load_record` /
  `rec_u32`): read once at start-up into a 1 KiB SRAM copy; `Dmm::ad2_scale`
  (every Wave view frame, LOG:SCALe?), `dmm::cal`, the scope's gain /
  offset pairs and hwtrig's trigger map read the copy. fw-rs never writes
  the record.
- **Boot-data version bytes** (0x080A4010..12, BT command 02, Device page):
  copied by `Bt::new` (`bt::boot_ver`).
- `Logger::start` / `logger::isr::start` are region B again (their tails
  ran after `stream::start`), and so is LOG:SCALe? (`main.rs log_scale`,
  asked while a session streams).

Why NOT READY (decision 2026-10-23, of three):
- (a) NOT READY 02/04/01, the host retries: no RAM, every file, exact (only
  the slow sectors), the precedent is the busy write (`write_error` 2).
  Linux (drivers/scsi, v7.0): `scsi_check_sense` retries 04/01 at once
  (NEEDS_RETRY), then `scsi_io_completion_action` ACTION_DELAYED_RETRY
  (requeue, device busy, ~3 ms) until `scsi_cmd_runtime_exceeced`:
  (retries + 1) x timeout from the request = (5 + 1) x 30 s = **180 s**
  (`/sys/block/sdX/device/timeout` scales it). Chosen.
- (b) a RAM cache filled before the stream: CALIB 4 KiB + BOOTDATA 16 KiB
  would fit SRAM (52 KiB free) but LOADER (128 KiB) and the FIRMWARE.BIN
  tail (~104 KiB) cannot, and the kernel's 240 KB readahead pulls the tail
  in on any FIRMWARE.BIN read: a partial cache leaves the usual copy
  stalling. Rejected.
- (c) hold the reply until the stream stops: a SCSI command timeout (Linux
  30 s, then abort + BOT reset) against sessions of minutes or an open-ended
  Wave view. Rejected.
- (d, 2026-10-24) the SPI mirror above: answers at once for any stream
  length, so (a) is now only the fallback. Not taken: a DMA copy of the
  slow pages during the stream (its loads go through the same flash
  interface the ISR's fetches need; whether that stalls the ISR is not
  measured and the emulator cannot show it), and RAM stays too small (b).

Verified over the real Linux path (emu/kvmtest/run.sh: a KVM guest with the
Ubuntu 7.0.0-38 kernel attaching the emulator over vhci-hcd, usb-storage,
sd, vfat; the NOT READY change, then the final image cafab0a):
- 6 s session (52-67 s of wall time in the loaded emulator): `cat` of the
  four files waited until the session ended, exit 0, byte-identical to
  copies made before, no kernel message (cafab0a: 51.7 s).
- 20-25 s sessions (> 180 s wall): the kernel logs "timing out command,
  waited 180s", "Not Ready / Logical unit is in process of becoming ready",
  "I/O error, dev sda, sector 925" (CALIB.BIN); an O_DIRECT read (`dd
  iflag=direct`) fails EIO; a buffered `cat` still completed with the
  right bytes (the page cache re-reads a failed readahead with a new
  request, a new 180 s). So: a copy made while the meter streams for more
  than ~3 minutes (an open Wave view, a long fast-log session) can fail;
  close the view / end the session and copy again.
- Windows (classpnp) is expected to retry 02/04/01 too; how long it keeps
  trying is not known here: not verified (device retest).

Pacing (branch mirror-bw, emulator --mips 110 --isr-stall-ns 700 with the
device's polled SPI byte, 24547cf): a host reading LOADER.BIN back to back
from 3 s into a 76800 run lost 119 853 of 768 000 samples on e3e81d4, 0
with the pacing (rung kept; a copy inside the writer trial no longer
makes it fall back); every INFO/ file + DBG:PROF? polls at 38400 / 76800
`--region-c`: lost 0, no region C, no NOT READY. Device: retest (TODO
"Device retest of mirror-bw").

Verified with the mirror (2026-10-24, 12a1d9c): fastlogtest
`--region-c --host-read --dur 20` at 38400 / 76800 S/s (typ chip timing):
every slow-bank sector of the four files read during the session (37 / 24
MB of INFO/ READs in all), no NOT READY, the same bytes as after the
session, no region C call or slow-bank load, lost 0 at both rates (76800
kept its rung). `emu/mirrortest.py`: fresh disk WRITTEN; the same image
PRESENT with the saved SPI image unchanged; another image WRITTEN; one
byte of the calibration sector changed WRITTEN, then PRESENT; each time
`_INFOMIR.BIN` equals flash 0x08080000..0x080C1000. kvmtest (Linux
usb-storage): `cat` of the four files and an O_DIRECT CALIB.BIN during a
38400 session finish while it streams (10.7 s), byte-identical; with
`dm40wait=240` (started 240 s of wall time into the stream, past the old
180 s limit) 7.4 s, exit 0, identical, no kernel I/O error.

Emulator check: `emu/regionc.py` arms `peripherals/FlashWatch.cs` (tlib's
memory-access hook): every CPU data load from 0x08080000..0x08100000 while
`hy3131::HANDED` is set, and every slow-bank PC the stream ISR interrupts,
is reported next to the region C function hooks; fastlogtest
`--region-c --host-read` (and wavetest, which reads INFO/ in the Wave view)
fail on any. Before the fix (4a734d1): memcpy from 0x08080000.. (the
FIRMWARE.BIN tail), `vdisk::mem_read` from 0x080C0000, `Logger::start`'s
literal pool, `dmm::cal` (Wave view), `scope::cal_pair` /
`hwtrig::rec_pair` (`--poll "SCOP:MEAS:VPP?;DBG:HWTRIG?"` in a disk
session), `main.rs log_scale` (LOG:SCALe?, region C code). On cafab0a:
fastlogtest `--region-c --host-read` at 640 (AD1) / 4800 / 38400 / 76800,
the same at 9600 with `--poll` of SCOP:MEAS:VPP? / DBG:HWTRIG? / WAV:PRE?
/ CHAN1:SCAL? / SYST:MOD? / *IDN? / LOG:SCALe? / LOG:SRATe? / LOG:SOURce? /
CONF?, wavetest (INFO/ in the Wave view, the polls with LOG:DEST? /
LOG:DURation? / *IDN? / SYST:MOD?), ad2streamtest, inrushtest: PASS, no
region C call and no slow-bank load while streaming (the Save CSV tap's own
handler, allowed as before, aside).

UI and host paths added 2026-10-23 (3f32bc3, emulator): `fastlogtest
--ui-nav` walks the menu, Settings 1..3, Device and the MCX scope page by
DBG:KEY / DBG:TAP while a disk session streams (real touch stops a session
at the first press; this audits the page code) and starts each session on
the Scope page: no region C / slow-bank read after `ui::main_menu_hit` moved
to B and `press_track` is skipped while streaming. wavetest checks MMEM:CAT?
(now from a cache, csvlog.rs `Cat`: a scan in one pass at 76800 dropped
~90 ms of the Wave view's ring), the V/div step (repaint before the restart)
and the badge exit per step.

### Host writes (`src/vwrite.rs`): delete from the PC, drag-and-drop update

The volume is writable but stores nothing a host writes as sectors: each
512-byte WRITE is compared with the sector the volume generates and accepted
only as one of the changes below, kept as small records (~1.4 KiB SRAM,
.sram_bss) from which the volume regenerates **exactly what the host
wrote** (a host that re-reads a sector after evicting it from its cache
sees its own data); anything else fails DATA PROTECT 07/27/00.

| Write | Accepted as |
|---|---|
| boot sector byte 0x25 | Linux's dirty flag on a rw mount (written at mount and umount) |
| FAT[1] bits 15 / 14 | the Windows / macOS clean / error flags |
| dates / times of an existing entry | access date (relatime), the parent's mtime: accepted and dropped (a re-read shows the old date) |
| a LOGS / SCOPE / SHOTS entry → 0xE5 (rest unchanged) | `T_DIR` on its table entry: shown 0xE5 + the rest |
| its whole chain → 0 in a FAT sector | `T_FAT`: shown free, data zeros |
| FAT entries of free clusters | the host's allocations (`pair`, 40) |
| free root slots: 0xE5 / long-name entries; an 8.3 `DM40*.BIN` file (≤ upload::MAX); a directory / file of the hosts' own (`.Trash*`, `._*`, `.DS_Store`, `.fseventsd`, `.Spotlight-V100`, `.TemporaryItems`, `.metadata_never_index*`, `.com.apple.*`, `System Volume Information`, `$RECYCLE.BIN`, by long name) | raw 32-byte records (10) |
| data in free clusters | staged in free SPI clusters (`stage`, 28 volume clusters) |

Refused: writing into README.TXT, INFO/, DEBUG/ or our files, an INFO
delete, entries created in our directories, a rename, a truncation, other
names / directories in the root, the volume label, past the capacities.
On Linux a refused write-back shows as one `Buffer I/O error ... lost async
page write` (the mount stays rw; the buffer is re-read later); the host's
in-memory view of that item stays inconsistent until the re-plug (a copied
`notes.txt` seems to be there until then). That is why the hosts' own
housekeeping is **accepted** for the session: when the trash directory's
root entry was refused (first capture), vfat kept the directory's inode
keyed by the slot position and a later `cp ... dm40fw.bin` into that slot
failed with "Is a directory".

**Delete.** When both marks of a file are seen (any order; Linux `rm`:
FAT 1, FAT 2, then the directory sector, at once), the main loop
(`vwrite::poll`, under the gallery lock) deletes the SPI file
(`csvlog::delete_on`) if its SPI entry is still the same file (start +
size); a delete on the meter meanwhile (`vdisk::forget` while frozen: the
entry stays, data zeros) is never repeated on a new file of the same name.
The 0xE5 alone is a move (Nautilus / gio "Move to Trash": the file's start
+ size written as an entry into a host directory, `T_MOVED`, noted when
that data is staged or when the 0xE5 comes later): the file is deleted at
the session's end if the trash still holds it; emptying the trash frees the
chain = a whole delete at once; a file taken out of the trash again (its
new entry in our directory is refused) stays. A refused rename keeps the
file (it is back after the re-plug).

**Firmware file.** Data in free clusters is staged whatever the order
(Linux writes the data, FAT and directory in separate write-backs; the
entry may come first with size 0): per volume cluster up to 8 SPI clusters
(first fit, SPI FAT untouched: the staging owns the SPI disk like
DBG:UPLOAD, `upload::busy`; screenshot / csvlog commits wait). A root
record `DM40*.BIN` whose chain is complete in `pair` with every sector up to
its size staged, quiet for 300 ms, is decoded (128-byte blocks as the
loader) and checked: the loader's header test, word1 = size - 12, the body
CRC-16 (word2, the 12 marker bytes excluded), no hazardous short last block.
Then `upload::commit_on`: every other dm40*.bin in the SPI root deleted,
the staged clusters linked as `DM40FW.BIN` (data → FAT → directory). The
meter asks **"Firmware received / Install it now?"** (dialog.rs; ui-render
`t04-confirm-fw-install`): Yes = `DBG:UPDATE` (zero the loader marker,
reset; the loader installs it), No, a side key or 60 s = `DM40FW.BIN`
deleted. A complete `DM40*.BIN` that is no update file: toast "Not a DM40
update file", nothing on the SPI disk. An overwrite (`cp` again) copies the
committed blocks first (copy-on-write) and commits again. The host's view
of its file stays exact (its reads come from the staged / committed
clusters; after No, zeros).

**Session.** While the host has the volume locked (PREVENT: Linux and
macOS mount) the layout is **frozen**: `vdisk::refresh` waits, no UNIT
ATTENTION is sent (on a removable disk Linux fails the I/O after a UNIT
ATTENTION and the dirty buffers written back later would carry the old
FAT / directories), so recordings / screenshots made meanwhile appear at
the eject / re-plug. That is the cost of the writable volume, and why it
is not the default (decisions-2026-10-22 #6: the read-only "Files live"
volume keeps the live updates, without delete / update from the PC; the
mode changes only at a bus reset, so a writable session is never cut short
by it: asked for "Files live" while a writable session runs, the volume
stays frozen and writable until the re-plug, then the session's records are
dropped as at any session end, tools/fatren-test vwrite "rw-ro"). A host that never locks: frozen while it has
written in the last 60 s. The session ends at the Allow (umount / eject)
or a bus reset: every record dropped, the flags cleared (except a delete in
progress), `refresh` regenerates the volume and raises the UNIT ATTENTION.
A DM40*.BIN whose check is still due at the Allow (its data came less than
300 ms before: `cp` + eject at once) holds the end back until `poll` has
decided it, at most 1.5 s (`END_WAIT_MS`; frozen meanwhile; fatren-test
vwrite "Allow at once"). Staged data idle for 5 s is **parked**: its SPI clusters linked as a
hidden system file `_VHOSTnn.TMP` in the SPI root (the SPI disk is free for
the on-device writers again; a later write to a parked block copies it
first; at most 100 temp files a session, then a later burst's data is
dropped: reads zeros); all `_VHOST*.TMP` are deleted at the session's end
and at the first main-loop pass after a boot (a power cut / unplug in a
session). Staging writes answer "busy" (02/04/01, the host retries) while
a screenshot / CSV commit / DBG:UPLOAD writes the SPI disk or the
constant-interval logger runs (LOG:STATe ON). Power cuts:
the staging never writes the SPI FAT; a commit / park is data → FAT →
directory, a purge directory → FAT: at worst lost clusters.

`DBG:VDISK:WR?` → `<accepted>,<refused>,<busy>,<last refused LBA>,<SPI
files deleted>,<firmware files committed>,<rejected>,<root records>,<FAT
pairs>,<staged clusters>,<frozen>,<staging>,<firmware state>`;
`DBG:VDISK:RO 0|1` = the Settings row (applies at the next bus reset) /
`RO?` → `<in force>,<asked for>` (1 = read-only; `1,1` after a boot).

Captured on Linux (kernel 7.0, GNOME automount: `rw, relatime, flush,
shortname=mixed, errors=remount-ro`; `tools/fatren-test/usbip_msc.py` = a
USB/IP mass-storage device running the volume code at host speed,
`kcapture.sh`, captures replayed by run.sh): mount = PREVENT + boot sector;
`rm` = FAT 1, FAT 2, directory sector at once; the parent's mtime / access
dates ~60 s later (root / directory sector, dates only); `gio trash` =
`.Trash-1000` (LFN + 8.3, 3 directory clusters zeroed + the trashinfo),
the 0xE5 in SHOTS and the entry in `files/`; `cp` of 500 KB = one
write-back burst at close (`flush`), FAT + root + 980 data sectors; umount =
boot sector, ALLOW, then a UNIT ATTENTION and udisks' re-probe (no
re-mount). The captures hold the volume's layout (README.TXT is padded to
a fixed 2048 bytes for that): a change of the generated CSV text or the
INFO files needs a new capture (`kcapture.sh`; the captures run writable:
`usbip_msc.py` serves "PC can edit" unless `--ro`; the README text of
decision #6 kept 2048 bytes and the replays passed unchanged). Emulator
(real firmware, kernel attach, read-only by default since decision #6:
Settings 3/3 tapped to "PC can edit", detach + attach = re-plug, `Write
Protect is off`, `RO?` `0,0`; 2026-10-22): `rm` of 2
files → deleted on the SPI disk; `cp` of a 500 KB update file (47 s at the
emulator's speed) → committed, the dialog declined at 60 s → DM40FW.BIN
gone (SPIFLASH.BIN fsck clean); `cp` again → committed, `DBG:TAP` on Yes →
reset into the loader. USBTMC queries during a staging can time out (the
main loop is busy with SPI writes; one SPI block read-modify-write per
512-byte sector).

### Firmware update without the writable disk (`src/upload.rs`)

`tools/reload.sh` / `tools/dm40dbg.py upload FILE`: `DBG:UPLOAD:BEGIN
<size>,<crc32>` (every `dm40*.bin` in the SPI root is deleted:
`fatren::delete_updates`), then `DBG:UPLOAD:DATA <offset>,<raw bytes>`
messages of 232 bytes (taken by main.rs before the SCPI parser, which
upper-cases and cuts at 64 bytes; offset checked), then `DBG:UPLOAD:END?` →
`<size>,<crc32>`: the last block written, every block re-read and the
CRC-32 compared, the first block's decoded header must be a valid update
header, then the chain in every FAT copy and the root entry `DM40FW.BIN`
(data → FAT → directory: a power cut = lost clusters, never a partial
file). The data goes to free clusters as it arrives (first fit, ascending,
FAT untouched until END); while an upload is active it owns the disk
(screenshot / csvlog commit / gallery wait), it is dropped after 30 s
without data. Then `DBG:UPDATE` as before (the loader installs the file and
deletes it). ~8 s for 480 KB in the emulator. reload.sh falls back to
staging on the writable USB disk when the running fw-rs has no
`DBG:UPLOAD` (older build). The recovery path is untouched (the loader's
update screen exposes its own USB disk).

### Host OS notes

The single removable volume avoids the two-LUN questions (one `sd` device
on Linux, one drive letter on Windows). FAT16 with 32 KiB clusters on a 2
GiB volume is the standard large-FAT16 format (Windows, macOS, Linux mount
it). Default mode "Files live" (read-only, decision 2026-10-22 #6): the
host writes nothing at all (Windows / macOS housekeeping files included),
the UNIT ATTENTION goes out at once on every change; whether Explorer and
Finder show new files then (with or without a refresh) is open (device
check; if Explorer does not refresh, try a volume serial that changes per
refresh, TODO). The notes below are about "PC can edit". Linux: captured
(above). **Windows and macOS are untested** (no capture possible here);
designed for:
- Windows: writes `System Volume Information\IndexerVolumeGuid` /
  `WPSettings.dat` on the first rw mount (accepted for the session, gone at
  the re-plug, written again next time), may set FAT[1] dirty bits
  (accepted). Explorer's Delete on a removable drive deletes at once (no
  recycle bin): both marks. Quick-removal policy = write-through, so its
  writes come in operation order. Probably no PREVENT: frozen for 60 s
  after its last write, then released with a UNIT ATTENTION (Windows then
  re-reads the volume; whether Explorer shows new files: open).
- macOS: `.fseventsd`, `.Spotlight-V100` (many files: the 28 staged
  clusters may run out, then its writes are refused), `.Trashes`, `._*`
  AppleDouble files, `.DS_Store` in the root (accepted), `.DS_Store` in
  LOGS / SHOTS / SCOPE (refused: one write error, harmless). Finder's
  "Move to Trash" = `.Trashes/501/` move (deleted at the eject, as Linux).
  `fsck_msdos` at mount only when the volume is dirty.
Not verified on the device yet: Linux kernel attach + udisks mount of the
real meter, the UNIT ATTENTION after the eject on Windows and macOS,
Explorer / Finder behaviour.

### Calibration safety copy (SPI disk root)

**Why it still exists:** the USB volume's INFO files are generated from the
internal flash. The one recovery path for a unit whose internal flash is
unusable — an SWD / BOOT0 session with read protection level 1 — can only
lift RDP by a **mass erase of the whole internal flash**, the calibration
(`0x080C0000`) and the UID-bound loader data (`0x080A4000`) included. The
W25Q128 is a separate chip and survives that, so one copy of the calibration
must already be there before it is needed.

**What:** `CALIB.BIN` (4096 B from `0x080C0000`), `BOOTDATA.BIN` (16384 B
from `0x080A4000`), `UID.BIN` (12 B; says which unit the loader data belongs
to) — `calbackup::FILES[..SAFE]`.

**Rule: write-once.** Each file is written only when the root has **no entry
of that name**; a present one is never overwritten (even when it differs, so
a later boot cannot replace the factory copy with a corrupted or foreign
record) and never deleted. The owner deletes a file to have it made again
from the flash as it is then. Present files are compared with the flash
(boot sector, root, FAT 1, 6 data blocks read; nothing written) only to
report `differs`.

**When:** every normal boot in `pre_main` (after board init and fatren,
before USB). First boot: 6 data blocks + FAT 1 + FAT 2 + 1 root block written
(~1 s); later boots: reads only.

**Write order** (unchanged writer, `run_inner`): allocate first-fit from FAT
1 → data blocks (read-back verified) → chains in every FAT copy → directory
entries → re-read every written file and compare. A power loss leaves at
worst lost clusters. The volume is only touched if `fatren::parse_bpb`
accepts it; a name taken by a directory or volume label is skipped
(`conflict`); not enough free clusters / root slots → `NOSPACE`, nothing
written.

**Status** (`calbackup::result()`), appended to `DBG:BOOT?` by
`native::put_extra`: `CALBK,<status>,<written>,<conflict>,<differs>`, status
`NOTRUN|NOMEDIUM|BADBPB|SAFECOPY_PRESENT|SAFECOPY_WRITTEN|NOSPACE|ERROR`;
written / conflict / differs = decimal bit masks (bit 0 CALIB, 1 BOOTDATA,
2 UID); `differs` = a kept copy no longer equals the flash. Then
`CALMS,<ms>` = `calbackup::boot` duration. `tools/dm40dbg.py boot` decodes it
(and recognises the old `UNCHANGED` / `WRITTEN` / `_NOFW` statuses of earlier
builds).

### Host tests (`tools/fatren-test/run.sh`, every check fatal)

Safety copy: first boot writes exactly the three files (nothing else); second
boot `PRESENT`, no block written (11 reads); a changed calibration is kept
(`differs=0x001`); a deleted `CALIB.BIN` is made again; a foreign `UID.BIN`
is kept (`differs=0x004`); a disk of an earlier build with all nine files →
nothing written or deleted, directory identical; a directory `UID.BIN` =
conflict; fatren on the same volume; nearly full disk → `NOSPACE`, nothing
written. fatdir: the three directories created (8 clusters, `.` / `..`),
nothing written on the second boot, a cut before every block write of the
first boot and of a grow (fsck at worst lost clusters / FAT 1 ahead, the
next boot completes), slots across directory clusters, numbering over root +
directory, rmdir. Writers on directories (screenshot / csvlog cut sweeps count
directory blocks as metadata). Upload: byte-identical `DM40FW.BIN`, staged
files deleted, `_DM40PRV.BIN` kept, fatren recognises it, chunk sizes 1 /
255 / 4096, wrong CRC / repeated chunk / not an update file refused, a cut
before every block write (never a partial file). USB volume (every sector
through `read_block` into a sparse dump, every non-zero sector read twice):
fsck.fat clean, label, geometry, root layout, the 7 INFO region files equal
their sources, `BACKUP.TXT` sizes and CRC-32s, read-only attribute,
`write_block` refused; `FIRMWARE.BIN` byte-identical for the native build,
stock `dm40_v_2_6.bin`, an unpadded body and a `pack --pad` image; no valid
record / a hazardous last block → none. Over a SPI image with logs, scope
records, screenshots and root files of earlier builds: every CSV == dlog.py
of its DLG, BMPs == the SPI files, `SPIFLASH.BIN` == the SPI image (fsck
clean), foreign / unnumbered files not shown, media change once per change;
then a new file (appended, everything else at the same slot / cluster), a
deleted one (slot shown deleted), a lock + bus reset (no compaction), unlock
(compacted: slots without holes), `forget` (gone at once).

Host writes (`vwrite_test.py`, run.sh "vwrite", every scenario through
`main ... vserve` = vdisk.rs + vwrite.rs over a SPI image; host operations
made by mtools on the volume dump, their sector diffs written in five
orders): deletes of LOGS / SHOTS / SCOPE and root legacy files (only those
SPI files gone, fsck clean, the volume = the host image until the Allow,
then regenerated with a UNIT ATTENTION); one mark only (nothing deleted,
back after the session); a delete on the meter while frozen; firmware
copies (`dm40fw.bin`, `DM40FW.BIN`, long names; data-first / metadata-first
/ random), Yes / No, into freed clusters (a non-contiguous chain), an
overwrite, a non-update `DM40*.BIN`; refusals (new file / directory in the
root or LOGS, README, INFO delete, rename, label: SPI files unchanged, the
volume after a bus reset = the original); trash (move: deleted at the
Allow; emptied: at once; unrelated file; taken back out); a temp file left
by a power cut deleted at boot; a lockless host; power cuts before every
SPI write of a firmware commit (81 of 1042: every commit cut + every 25th
staging write) and of a trash session + the next boot (402); the four
kernel captures replayed with the same accept / refuse answers.

Emulator (2026-10-21, `emu/usbip_client.py luns` / `mscdump` / `upload`,
`USBIP_PORT`): one LUN `DM40 files`, 4160541 × 512 B, WP, WRITE → 07/27/00,
no UNIT ATTENTION at the first command after boot; the volume dumped over
USB/IP (575 clusters, ~2 min): fsck clean, `LOGS/LOG000.CSV` (from a root
`LOG000.DLG`) == dlog.py, `SHOTS/S000..S002.BMP` == the stock root
screenshots, `DEBUG/SPIFLASH.BIN` fsck clean with LOGS / SHOTS / SCOPE and the
safety copy; `upload` of the native image: `END? -> 480512,<crc>` in 7.9 s;
a 3-finger-swipe screenshot under PREVENT: UNIT ATTENTION after 4 s, new
`SHOTS/S003.BMP` appended. (The emulated DWC2 sometimes stops answering bulk
URBs after a few dozen transfers; `mscdump` saves progress and resumes after
an emulator restart.)

## Stack depth (review 2026-10-13 B1)

`native::normal` used to hold fatren's block buffers (inlined, 9 KiB) while
calling `main()` (26.5 KiB frame) + `ScopeView::frame` (4.7 KiB): ~40.5 KB on a
39 KB stack. Now `normal` only calls `pre_main` (board init, fatren,
calbackup — each of `pre_main`, `fatren::run`, `calbackup::boot`,
`calbackup::run` is `#[inline(never)]`) and then `main()`, so those frames are
popped first. Prologues in the native ELF (2026-10-13):

| Frame | Bytes |
|---|---|
| `shim_main` | 32 |
| `native::start` | 8 |
| `native::normal` | 8 |
| `native::pre_main` (board init inlined) | 120 |
| `fatren::run` | 10 768 |
| `calbackup::boot` / `calbackup::run` | 212 / 14 260 (with ROMBOOT/LOADER); since 2026-10-21 `boot` (safety copy, writer inlined) 1 376, `run` gone |
| `vdisk::init` (2026-10-21, before main) | 3 952 |
| `VDisk::read_block` + `describe` (main loop, under `Msc::poll`) | 104 + 80 |
| `main` | 26 720 |
| `app::App::tick` | 1 096 |
| `ui::ScopeView::draw_static` | 2 360 |
| `ui::ScopeView::labels` | 224 |
| `stockfont::label` | 1 728 |
| `stockfont::layout` | 1 248 |
| `fatren::delete_file` | 4 152 |

Stack = `__bss_end` (0x100067A8) .. `_stack_top` (0x10010000) = 39 000 B.
Deepest paths (static estimate: prologue push/vpush/`sub sp` along `bl` edges,
no indirect calls; re-measured 2026-10-13 evening, review re-review B1):
- before main: 48 + `pre_main` 120 + `fatren::run` 10 768 + `delete_file`
  4 152 + `free_chain`/`flush`/`write_block` ≈ **15.9 KB**; the calbackup path
  (`calbackup::boot` 212 + `run` 14 296 + …) ≈ 15.4 KB;
- **main peak:** 48 + `main` 26 720 + `App::tick` 1 096 +
  `ScopeView::draw_static` 2 360 + `labels` 224 + `stockfont::label` 1 728 +
  `layout` 1 248 + the panic chain (~100) ≈ **33.5 KB** (the review measured
  33 428 B with `main` at 26 616), so the margin is **≈ 5.5 KB** (≈ 4.7 KB
  after allowing for the deepest indirect-call targets, `Msc::poll` 480 and
  `Usbtmc::endpoint_out` 352, which are called from `main` and sit below this
  path). Interrupts stay masked, so no exception frames stack on top. (The
  earlier "main + `ScopeView::frame` ≈ 31.5 KB, 7.5 KB margin" named the wrong
  path.) Keep the margin ≥ 4 KB when `main` or the UI frames grow.

**Run-time check:** `native::init_ram` (native and T1) paints the free stack
(`__bss_end` .. SP − 256) with `0x53544B21`; `native::stack_use()` returns the
high-water mark (first non-paint word from `__bss_end` up) and the stack size,
reported as the last `DBG:BOOT?` fields `STACK,<used>,<size>` (`used == size`
means it reached `.bss`).

`DBG:BOOT?` extra fields (appended by `boot::put_info` calling
`crate::native::put_extra(s)` as its last statement):
`…,STAGED,<n>,CALBK,<status>,<written>,<conflict>,<freed>,STACK,<used>,<size>,CALMS,<ms>,SPICLK,<div>,<check>`
(`STAGED`: fatren's count of dm40*.bin left because their header is not the
marker; `CALMS`: calbackup's boot time in ms; `SPICLK`: SPI-flash divider in
use, 4 or 16, and the boot self-check verdict of spiflash.rs: `OK` = block 0
and the first FAT block read identically at /16 and twice at /4 with 0x55AA,
else /16 for this boot because `DIFF1`/`DIFF2` (a /4 read differed),
`NOSIG` (no 0x55AA at /16), `NOCHIP` or `FORCED` (`spiflash::FORCE_SLOW`)).

## Risks / open items

- **RCU_RSTSCK clearing:** the shim clears these flags, which stock never did.
  Per the full-dump analysis, neither the ROM nor the loader reads RSTSCK, so
  this should be harmless. T3 still tests "back to stock" **first**.
- **PD6 polarity:** pressed = 1 is confirmed in the stock code. The shim now
  sets the pull-down itself (review B2).
- **Review items owned by other agents:**
  - **B1** (stack overflow in `native::normal`) and **H1** (fatren premise)
    are in `native.rs`/`fatren.rs`.
  - The `scpi.rs` DBG help text is in `scpi.rs`.
  - The `mark_healthy` call site is in `main.rs` (see "Crash counter").
- **Not done:**
  - **L2** (FWDGT: wait for PUD/RUD after the PSC/RLD writes, in `wdt.rs`) is
    not in this change set. It is harmless: the first period may be up to
    ~32 s.
  - **L4** (power loss during a fatren root-block rewrite) is documented in
    the review.
- **FSMC values:** decoded from the stock decompile. T1 reads the live stock
  registers via `DBG:BOOT?`.
- **Zeroing the marker is irreversible** without a vendor install. It is used
  only on purpose: the recovery decision, or `DBG:UPDATE`/`DBG:RECOVER`.
- **FS-loader launch checks:** not traced. The stock SP value is used to be
  safe.
- **PA2 at power-on:** not the power key (the key-hold test booted the app).
  PA2 is the debug header's TX pad; a TX-GND short is the non-destructive
  manual entry (owner, 2026-10-24; [hw/README.md, Hardware recovery](../../hw/README.md#hardware-recovery)).

## Test plan (coordinator + user)

T1 and T2 used RAM test images loaded over Bluetooth into the running stock
firmware (`./build.sh test-initcore | test-recovery`, `tools/reload.sh
<mode>`); both passed on the device on 2026-10-13 and the images were removed
with the RAM build on 2026-10-22. They are kept here as the record. Keep `firmware/patch/build-memrw/dm40_patched.bin` (`firmware/patch/kit/mkkit.sh`) and a stock
`dm40_v_2_6.bin` at hand. After T1 and T2, the device must be back on
(patched) stock.

### T1 — `test-initcore` (RAM, from the stock 200 MHz state)

1. `tools/reload.sh test-initcore`. Expected:
   - the screen re-initialises (black, then the fw-rs UI);
   - the composite 1209:0001 enumerates.
2. `dm40dbg.py scpi 'DBG:BOOT?'`. Expected:
   - `HXTAL`;
   - `bcr1,btr1,bwtr1` = the stock's live values. Compare them with
     `0x0000_5091`-ish / `0x00000F04` / `0x00000201`; if they differ, fix
     `board.rs` before any native flash;
   - `lcd_id` = `0x009488`;
   - fatren `DRYRUN` or `CLEAN`;
   - `PD6,0,2,0`: PD6 input with **pull-down** (`PUPD[13:12]` = 0b10), idle
     level 0; repeat with the key held (a short press, below the 1.5 s
     power-off) and expect `PD6,0,2,1`. In T1 this pull-down is the one
     `board::init` sets on top of the stock's; the shim's own `pd6_input` runs
     only in T2/native;
   - `CALBK,NOTRUN` (calbackup is not in the T1 image, see the calibration
     backup section) and `STACK,<used>,<size>`.
3. Exercise everything after the re-clock:
   - DMM, scope, generator;
   - BT (uses DMA0);
   - backlight levels, touch;
   - `DBG:FLASH?`;
   - disk mount, write and read;
   - `*IDN?`;
   - a screenshot.
4. Optional: stage a copy of `dm40_v_2_6.bin`, then build with
   `test-initcore --features fatren-live`.
   - Expect `RENAMED`, and `_DM40PRV.BIN` on the disk. (Observed on T3: the
     loader deletes the installed file, so fatren reports `CLEAN`;
     `_DM40PRV.BIN` is then written by calbackup from flash, `CALBK` bit 8.)
   - Rename it back afterwards.
5. `dm40dbg.py reset` → back to stock.

### T2 — `test-recovery` (RAM) — DESTRUCTIVE but recoverable

**This zeroes the installed stock app's marker.** The meter then comes up in
the vendor update screen on every boot until `dm40_patched.bin` (or
`dm40_v_2_6.bin`) is reinstalled through it. Have the file ready on the host.

1. `tools/reload.sh test-recovery`. There is no USB in this mode, so the
   script's final wait times out, as expected.
2. Expected within a fraction of a second: the shim sequence runs, the marker
   is zeroed, and the unit resets. The loader shows "open file error", and the
   vendor USB disk 28e9:028f appears.
3. Power-cycle once, and also after a quick power-key tap. The update screen
   must come back each time, since the marker no longer depends on PA2.
4. Copy `dm40_patched.bin` as `dm40_v_2_6.bin` onto the disk, then press
   "update". Patched stock is back. The stock should delete or ignore the
   file; keep only one `dm40*.bin`.
5. **Optional, non-destructive variants:**
   - `./build.sh test-recovery --features recovery-usb`: our own USB disk,
     plus the PD6 polarity check (a short press reboots to stock).
   - `--features recovery-file`: writes `DM40RCVR.BIN` and powers off. A cold
     power-on with the key held reaches the loader, which then zeroes the
     marker itself, so this one is destructive too.

### T3 — native image via the loader

1. **Install from the loader's update screen** (the proven path, review L7 /
   re-review N4). Reach the screen the T2 way (warm marker zeroing), or by the
   vendor key-hold entry only if T2b proved it; do **not** rely on "a long key
   hold at power-on reaches the loader" otherwise, and do not use patched
   stock's own update path (unverified for a non-stock length). On the
   loader's USB disk, copy `fw-rs/dm40-fw-native.bin` as `dm40_fwrs.bin` (the
   only `dm40*.bin`), `sync` and eject, then press "update". The loader
   installs it and resets (warm, SW reset), or the next cold power-on (short
   press) launches it.
   - fw-rs comes up in normal mode. The reason is normally `NONE`; it is
     `WDT_STALE` only if a watchdog flag from the stock session is still
     latched (stock never clears the flags; SW or POR set means it is never
     counted as our crash). Never `WATCHDOG`/`WDT_RETRY` here.
   - **The first boot is slow and dark:** board init hands the backlight to
     the PWM at duty 0 while fatren and calbackup run (review R2), and
     calbackup writes all its files on the first boot (several seconds). **Do
     not power-cycle during the first 10 s.** calbackup runs on hardware for
     the first time here (review R1); a fault there costs one `WDT_RETRY`
     reboot (~20 s), a second one recovery (loader screen): keep
     `dm40-fw-native.bin` and `dm40_patched.bin` at hand.
   - `DBG:BOOT?` should show:
     - `NORMAL`, `HXTAL`;
     - marker `VALID` (words = the header of our file);
     - fatren `RENAMED` `DM40_F~1BIN`;
     - `PD6,0,2,0`;
     - `CALBK,WRITTEN` (with this build: bits 7+8 at least = 384, BACKUP.TXT +
       `_DM40PRV.BIN`); the next boot `CALBK,UNCHANGED` and a small `CALMS`
       (builds since 2026-10-21: `CALBK,SAFECOPY_WRITTEN` / `SAFECOPY_PRESENT`,
       see "Unit data" and its device test plan);
     - `STACK,<used>,39000`; after using the scope view, `used` should be
       ≤ about 34 000.
   - The disk now holds `_DM40PRV.BIN` and the backup files.
2. **First prove the way back.**
   - Over fw-rs's own disk, copy `dm40_patched.bin` as `dm40_v_2_6.bin`, then
     send `DBG:UPDATE`.
   - The loader installs the patched stock (warm path).
   - Return to fw-rs the same way from stock, or through the loader screen.
   - Do this before relying on any recovery trigger.
3. **`DBG:RESET`:** normal boot. `DBG:BOOT?` shows SW (bit 28) and `NONE`.
   fatren runs on every normal boot (no reset-cause gate, review H1); with the
   installed file already renamed it finds nothing to do: `CLEAN`. calbackup:
   `CALBK,UNCHANGED`.
4. **Key gesture.**
   - **Normal press first:** a short power-on press should light the backlight
     at once. `DBG:BOOT?` then shows `hold_ms` < 300 and reason `NONE`.
   - **Abort:** hold the key from off until the beep (about 4 s; the backlight
     blinks), then release within 0.5 s. Expect a normal boot with
     `KEY_HOLD_ABORTED`.
   - **Confirm:** hold through the beep for at least 0.5 s more.
     - The marker is zeroed, and the loader update screen and its disk
       appear (PA2 is not the key; it is the debug header's TX pad).
   - Copy back `_DM40PRV.BIN` as `dm40_fwrs.bin`, then press "update". fw-rs
     returns.
   - `DBG:BOOT?` then shows `last` = `RCVR` and `last_trigger` = `0x54520010`.
5. **`DBG:RELEASE`:**
   - About 20 s later the WDT resets, and fw-rs reboots normally.
     `DBG:BOOT?` shows `WDT_RETRY` and crash word `0x43520001`.
   - `DBG:RELEASE` again within 60 s of that boot: the WDT reset now goes to
     recovery, and the loader screen appears. Reinstall as in step 4.
   - `mark_healthy`: 60 s after a boot, `DBG:BOOT?`
     shows `HEALTHY` and crash word `0x43520000`.
6. **`DBG:RECOVER`:** the loader screen appears immediately. Reinstall.
7. **Back to patched stock from recovery:** on the loader's disk, copy
   `dm40_patched.bin` as `dm40_v_2_6.bin` and press "update".
8. **Optional:** idle auto-off on USB power keeps the unit dark and fed (no
   recovery after 20 s); a key press reboots fw-rs.
9. **H3 check:**
   1. Under stock, cause an FWDGT reset without a power cycle.
   2. Reinstall fw-rs through the loader.
   3. Expect a normal boot: `WDT_STALE`, or `NONE`, never recovery.

### T3 recovery triggers: owner steps (2026-10-24)

What the shim does, checked in the emulator with the real ROM and FS-loader
(`emu/recoverytest.py`, all PASS on image sha256 `1372e957…`, built from
fb6be06; the docs-only changes since build the same bytes):

- **Key:** fw-rs is entered ~0.7 s after power-on (emulator; ROM + loader).
  The shim lights the backlight at once, waits up to 4 s with the key held,
  then blinks the backlight + beeps (~0.3 s), then needs 0.5 s more. Full
  gesture ≈ 5.5 s from power-on. Released earlier, or within the 0.5 s after
  the beep: normal boot.
- **Watchdog:** one hang (or DBG:RELEASE) = a reboot ~20 s later
  (`WDT_RETRY`, crash word `0x43520001`). A second one before fw-rs has run
  60 s (main loop) since that reboot = recovery. After 60 s of running the
  count is cleared (`HEALTHY`, `0x43520000`), so a later hang only reboots.
- **Recovery** = the 12 marker bytes at `0x08008000` zeroed, then a SW reset:
  the loader's "open file error / Update" screen with its USB disk (the SPI
  disk), on every power-on until a good file is installed.
- Stored in backup SRAM (`0x40024FF0` crash count, `FF4` last trigger, `FFC`
  previous mode). It survives resets, not necessarily a power-off without
  battery.

Before: fw-rs running, USB connected; on the PC a copy of the installed image
(`fw-rs/dm40-fw-native.bin` of the installed build, or `INFO/FIRMWARE.BIN`
from the fw-rs USB drive) to reinstall after each recovery. Reinstall: copy
it onto the loader's USB disk as `dm40_fwrs.bin` (the only `dm40*.bin`),
`sync`, eject, press "Update".

1. **Short press (normal start).** Power off. Press the power key ~0.5 s.
   Expect the backlight at once, fw-rs normally.
   `tools/dm40dbg.py boot`: `NORMAL`, reason `NONE`, `hold_ms` < 1000.
2. **Abort.** Power off. Hold the key; ~4.7 s after power-on the backlight blinks and it
   beeps; let go right at the beep (within 0.5 s). Expect fw-rs normally;
   `boot`: `KEY_HOLD_ABORTED`, `hold_ms` 4000..4500.
3. **Key recovery.** Power off. Hold the key through the beep and keep
   holding until the loader's "open file error / Update" screen appears
   (emulator: ~5.5 s after power-on; on the device the shim's 4 s start only
   after the ROM / loader hand-off, so it may be later). Holding longer is
   harmless: once the 0.5 s after the beep have passed, recovery runs whether
   the key is still down or not. Expect that screen and its USB disk. Reinstall fw-rs. `boot`: `last` = `0x52564352` (RCVR),
   `last_trigger` `0x54520010`, marker `VALID`.
4. **One hang.** `tools/dm40dbg.py release`. ~20 s later fw-rs reboots.
   `boot`: `WDT_RETRY`, crash word `0x43520001`, `-` (not yet healthy).
5. **Healthy clear.** Wait 70 s, then `boot`: `HEALTHY`, `0x43520000`.
   `release` again: reboot, `WDT_RETRY`, `0x43520001` (no recovery).
6. **Two hangs.** Right after step 5's reboot (within ~30 s of it),
   `release` once more. ~20 s later: the loader's update screen. The budget:
   uptime at the `release` + 20 s must stay under 60 s; sent later,
   `mark_healthy` clears the count during the unfed 20 s and the result is a
   plain `WDT_RETRY` reboot, which is correct behaviour, not a failure. Reinstall.
   `boot`: `last` = RCVR, `last_trigger` `0x54520010`, crash `0x43520000`.
7. Note anything that differs, especially the time from power-on to the
   beep in step 2/3 (emulator ≈ 4.7 s) and whether the loader screen in step
   3 comes up while the key is still held.

## Developer workflow

Proven on the device (2026-10-13): native fw-rs enumerates as `1209:0001`
(USBTMC + a USB disk; since 2026-10-21 the read-only generated volume, the
SPI FAT no longer exported); `DBG:UPDATE` zeroes the marker and resets, the
vendor loader then installs the single `dm40*.bin` in the SPI FAT's root
(also from a warm reset) and deletes it afterwards. Stock or patched stock
installs the same way.

`tools/reload.sh [cargo args]` (default):
1. `./build.sh [cargo args]`; `tools/dm40dbg.py image` validates the
   file (vendor header CRC, body CRC, size, reset vector) before anything is
   staged.
2. A running fw-rs that does not answer `DBG:BOOT?` (no `DBG:UPDATE`) is
   refused. `28e9:028f` (stock "USB Connect" or the loader
   update screen) is refused unless `--from-stock`.
3. `dm40dbg.py upload <file>` (2026-10-21, the USB volume is read-only):
   `DBG:UPLOAD` writes the file as `DM40FW.BIN` into the SPI root, every
   other `dm40*.bin` there removed first (`_DM40PRV.BIN` kept), CRC-32
   checked on a re-read by the meter ("Unit data" → "Firmware update without
   the writable disk"); then `DBG:FLASH?` must report no SPI write error. A
   running fw-rs without `DBG:UPLOAD` (older build): the old way, the fw-rs
   disk found by USB id through `/sys`, mounted with `udisksctl`, every other
   `dm40*.bin` removed, the image copied as `dm40_fwrs.bin`, synced, compared
   after dropping its pages from the page cache, unmounted.
4. `dm40dbg.py update --wait 90 --expect <file>`: `DBG:UPDATE`; if the device
   is still on USB after 5 s, `SYST:ERR?` is reported ("Flash error" = marker
   not zeroed, nothing changed). Waits for `1209:0001` + `/dev/usbtmc*`; if
   `28e9:028f` stays up for 20 s instead, the install failed (loader screen).
   Then prints `DBG:BOOT?` decoded and compares `marker_w0..2` with the
   decoded 12-byte update header of the file: "installed OK".

Options: `--no-build`, `--image FILE`, `--wait S`, `--stock FILE` (stock or
patched stock, same flow, no wait: stock does not enumerate `1209:0001`),
`--from-stock` (stage onto the `28e9:028f` disk, the user presses "update" or
power-cycles, waits up to 300 s for fw-rs).
