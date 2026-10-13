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
- The ROM bootloader polls **PA2** at boot — it gates a *re-verify*, not a
  recovery button. (`ANALYSIS.md` §3)
- **FS-loader** (`0x080a0000`) has **its own USB mass-storage stack + FatFs**,
  runs **before the app on every boot**, calls
  `f_findfirst("/","dm40*.bin")` (`0x080a26c4`) and installs the **first**
  match (directory order, not newest; one file per boot; no version compare).
  It verifies the header CRC and body CRC before accepting, writes the body to
  `0x08004000`, and the writer is **bounded to `0x08004000`..`0x080a0000`**. It
  does **not** delete the file; the stock app deletes it after a good boot.
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

So `dm40fw.py pack body.bin out.bin` already produces a correct update file for
**any** body length, with no `--like` needed. **`firmware/README.md` is stale**
where it says words 0/2 are not understood — that was true earlier; the code now
computes both.

### Unknowns (and how to settle them live)

| Unknown | Impact | How to verify |
|---|---|---|
| Exact trigger that makes the FS-loader enter **its own** USB-disk mode at boot (not isolated — `ANALYSIS.md` §3). | Would let the shim bring up stock MSC with ~zero code (Option A below). Not required for safety. | Disassemble FS-loader around its USB init; or set candidate flags via the BT `memrw` patch (`firmware/patch/dm40_memrw.py`) and reboot, watch for USB disk enumerating. |
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
its wait loop.)

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
(`custom-fw-handover.txt`) still holds in spirit — the deliberate exits are now
(a) the panic/hang -> recovery path and (b) the explicit `recover` command.

---

## 7. Failure-mode / brick table

| Failure | What happens | What saves us | Residual risk |
|---|---|---|---|
| Our packed image has a bad header/body CRC | FS-loader rejects it ("update error"), keeps the current app | FS-loader verifies before writing | none (never half-flashed) |
| Our image hangs **before** the shim decision | WDT resets; next boot the shim sees `FWDGTRSTF` -> recovery | shim runs first, recovery brings up MSC | shim itself must be correct (keep it tiny) |
| Shim bug (wrong decision / hang) | could loop | FS-loader is untouched; a stock `dm40*.bin` already on the FAT is installed on the next boot regardless of the shim | shim must not erase/FAT-corrupt; it only reads |
| Power loss **during** FS-loader flashing | app region partly written, bad CRC | FS-loader re-runs next boot, re-installs the file (still on FAT, it does not delete) | needs the file still present + power |
| Corrupted SPI-disk / FAT | FS-loader may not mount | ROM bootloader + FS-loader still in flash; reflash SPI over the hardware fallback | hard case — see HW backstops |
| Stock `dm40*.bin` missing from FAT when app is dead | FS-loader has nothing to install, app won't run | **recovery mode's own MSC** (Option B) lets the user copy one | depends on the shim's recovery MSC working |
| Wrong-model update file | installs, but model byte mismatches | model is read from calibration (`0x080C0000` +0x168), labels/ranges only | cosmetic; not a brick |
| Dead battery mid-update | reset | FS-loader resumes when powered + file present | charge first |

**Hardware backstops (open questions for the user, not blockers):** are the SWD
pads accessible for an emergency reflash despite read-protection (write may
still work after a mass-erase)? Is `BOOT0` exposed to enter the GD32 system
bootloader (USART/USB DFU)? The prior hardware fallback was **shorting the SPI
flash** to force the FS-loader into a known state. Confirm at least one physical
backstop exists before Stage 1.

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
Remove `stub/`, `tools/flash.py`, the BT RAM loader path and the sector-8
`link.x`; update `README.md`, `custom-fw-handover.txt`, `TODO.md`.

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
  `custom-fw-handover.txt`: the stock app scans for `dm40*.bin` on USB unplug;
  that is the intended path for installing us, but keep only one present).
- **`tools/reload.sh` / `tools/flash.py`** — the BT RAM loader becomes obsolete
  after Stage 2. The new "load fw-rs" flow is: copy the update file to the FAT
  (host file copy over whichever MSC is up) + reboot.
- **`tools/dm40dbg.py reset`** — now a normal `SYSRESETREQ` reboot of fw-rs; add
  `recover` (set magic + reset) and keep `shot`/`scpi`/`ping`.
- **Switch to stock from the host:** copy `dm40_v_2_6.bin` onto the FAT over
  fw-rs's MSC, then `dm40dbg reset` (or power-cycle). The FS-loader flashes
  stock; the now-stock app deletes the file after its first good boot.

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
1. Physical backstop: are SWD pads reachable, is `BOOT0` exposed (GD32 system
   DFU), or is SPI-flash shorting the only fallback? Confirm one before Stage 1.
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
  normal boot. It is not deleted.

## Files

| File | Role |
|---|---|
| `link-native.x` | native layout (below) |
| `link-test.x` | RAM layout for the T1/T2 test builds: `link.x` with CODE = SRAM0+SRAM1 (`0x20000000`, 128 KiB, as large as the flash sector) and the DMA RAM = SRAM2 (`0x20020000`, 64 KiB; `.dmabuf` ~58 KiB) |
| `build.sh` | modes `native`, `test-initcore`, `test-recovery`; no mode = the original build, unchanged |
| `src/shim.rs` | native vector table, `reset` (asm), `shim_main`; all in region A |
| `src/boot.rs` | shim decision `shim()`, reset flags, power key, power hold on/off, backup-SRAM words, loader marker check + `zero_marker()` (FMC), `DBG:BOOT?` text |
| `src/native.rs` | after the decision: `.data`/`.bss` init, then recovery, or board init + fatren + `main`; T1/T2 entry points |
| `src/board.rs` | native board bring-up (everything the stock used to do for us) |
| `src/recovery.rs` | recovery: zero the loader marker + SYSRESETREQ. Features `recovery-file` (`DM40RCVR.BIN` + power off) and `recovery-usb` (our own USB-MSC) keep the earlier variants for tests |
| `src/fatren.rs` | FAT16 root-directory edits: rename the installed image, remove a leftover `DM40RCVR.BIN`; with `recovery-file`, create `DM40RCVR.BIN` |
| `src/clock.rs` | `init_core(Src)`: measured stock values, bounded waits, IRC16M fallback, no FLASH_ACR write |
| `src/wdt.rs` | `start::fwdgt()` (shim); `powered_off()` (end of the app's power-off) |
| `src/msc.rs` | `STOP_UNIT` flag on SCSI START STOP UNIT (native / test-recovery builds only) |
| `src/scpi.rs` | `DBG:BOOT?`; `DBG:UPDATE` = `DBG:RECOVER`: zero the marker + reset (native only) |
| `src/main.rs`, `src/app.rs`, `src/startup.rs` | hooks (see "Default build") |
| `tools/fatren-test/run.sh` | host test of fatren on mkfs.fat images (fsck.fat + mtools) |

Cargo features:
- `native` (implies `embedded-fonts`, since the stock fonts in flash are gone
  once we replace the app);
- `test-initcore`, `test-recovery`;
- `fatren-dry-run`, `fatren-live`;
- `recovery-file`, `recovery-usb`.

## Default build (RAM image)

`./build.sh` with no mode runs exactly the old command. Two traps:

- **Panic `Location`s** embed source line numbers in `.rodata`. Inserting a line
  anywhere above a panic site in `main.rs`, `app.rs`, `scpi.rs` or `msc.rs`
  changes the binary. Every edit in those files therefore keeps the line count:
  - `INITIAL_HOLD_MS` stays one line;
  - the `scpi.rs` DBG:RECOVER/BOOT block replaces a stale 14-line comment
    with 14 lines;
  - the `msc.rs` START STOP arm replaces 4 lines with 4;
  - `app.rs` `loop {}` → `crate::wdt::powered_off()`;
  - new `mod` lines are appended at the end of `main.rs`.
- **Any `build.rs`** (even one that only adds a linker search path) changed
  codegen by 4 bytes. So `build.sh` chooses the linker script through
  `RUSTFLAGS` (`-Tlink-native.x` / `-Tlink-test.x`) with a separate
  `CARGO_TARGET_DIR`, and `.cargo/config.toml` (`-Tlink.x`) is unchanged. A
  plain `cargo build --features native` fails to link (`_loader_sp` is
  undefined) instead of producing a wrong image.

Checked by `cmp`:
- **Native-boot work alone:** the default `dm40-fw.bin` (114624 bytes) stayed
  identical.
- **Concurrent `scope.rs`/`ui.rs` edits by another agent:** for a while the
  default build overflowed the 112 KiB SRAM0 code region. Neutrality was then
  checked on two snapshots of the same tree, with and without the native
  changes, both linked with `link-test.x`. They were identical (115416 bytes).
- **Coordinator review fixes in `main.rs`/`msc.rs`** (not line-neutral, so they
  intentionally change the default build):
  - H1: `app.note_host_activity()` on USBTMC commands and on BT frames.
  - M4: `Msc::set_write_inhibit(logger.disk_writes_blocked())`. A host WRITE
    then fails with NOT READY / becoming ready (02/04/00).
  - BT AT init: skipped while the logger is active.
  
  The default build now links at **114560 bytes**, `.text` 114388 of 114688,
  so **300 bytes are left** in the code region.

**`link-test.x` as the default dev layout** (done: `link.x` now uses the
120 KiB / 72 KiB split below; `link-test.x` has since moved on to CODE 128 KiB
/ DMARAM `0x20020000` 64 KiB for the larger T1 image, see the calibration
backup section):
- **Layout:** CODE `0x20000000` 120 KiB, DMARAM `0x2001E000` 72 KiB.
  `.dmabuf` is 59368 B and ends at `0x2002C7E8`, leaving 14 KB.
- **No collision:**
  - The BT stub (`0x20008000`, 16 KiB) and its payload
    (`0x2000C000..0x2001C000`) are used only before the image starts. The
    image's reset code runs from flash and overwrites them anyway, as today.
  - No source file hard-codes `0x2001C000`.
  - The 128 KiB sector-8 flash limit still bounds the image.
- **What it takes:**
  - `cp link-test.x link.x` (two numbers change).
  - Every `.dmabuf` address moves by 8 KiB, so the default binary changes once.
  - Nothing else: `build.sh` and the BT tools are unaffected.

## Native memory map and the loader marker

```
0x08004000  .vectors: word0 0x2002F7F8, word1 reset (0x08004041)   region A, 16 KiB
0x08004040  .shim:    reset, default_handler, boot::shim,
                      clock::init_core, wdt::start::fwdgt, shim_main  (-> 0x080043D4)
0x080043D4  0xFF padding (objcopy --gap-fill=0xff)
0x08008000  12-byte loader/ROM marker, 0xFF in our image             GAP
0x0800800C  4 bytes 0xFF padding
0x08008010  .text + .rodata (XIP), then the .data load image         region B
0x0803C4C4  end of image (limit 0x080A0000, 408380 bytes free)
RAM: TCM 0x10000000 .data/.bss (~26.5 KB) + stack (top 0x10010000);
     SRAM 0x20000000.. .dmabuf (59 KB; SRAM0 no longer holds a code copy)
```

**Link-time asserts** (`link-native.x`):
- region A ends at or below `0x08008000`;
- `.text` starts at or above `0x0800800C`;
- the image ends at or below `0x080A0000`;
- at least 16 KiB of TCM stack.

**`build.sh native`** checks the binary again:
- the 16 bytes at `0x08008000`, and all padding from the end of region A to
  `0x08008010`, are 0xFF;
- the end is below `0x080A0000`;
- word0 = `0x2002F7F8`;
- the reset vector is a Thumb address in region A.

It then runs `../firmware/dm40fw.py pack`, which computes both header words
itself (no `--like`):
- word1 = body length;
- word2 = `0x10000 | CRC16(body without the 12 marker bytes)`;
- word0 = CRC16 of words 1–2, with its complement in the high half.

Finally it runs `dm40fw.py info`. Current `fw-rs/dm40-fw-native.bin`:

```
word0  0xa83657c9   length 0x000384c4 (end 0x0803c4c4)   word2  0x0001e49e
SP     0x2002f7f8   Reset  0x08004041
```

`dm40fw.py unpack` of the packed file gives back the body byte-for-byte.

**Word0 is the stock app's own initial SP** (`0x2002F7F8`). The FS-loader
launches the app with `msr msp, [0x08004000]; ldr r0, [0x08004004]; bx r0`
(`0x080a18f4`). The routine that decides whether to launch was not found, so
any SP plausibility check it may make passes exactly as it does for stock.
`reset` moves SP to the TCM top before anything uses the stack.

## Shim (reset path)

*Revised after the safety review `docs/native-review-2026-10-13.md` (B2, H2,
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
  Until it is added, the counter is cleared only on recovery entry. Two hangs
  in a row, at any distance, then take recovery.
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
  no USBTMC, serial `REC1`).
  - Any SCSI START STOP UNIT, or a key hold of 2 s or more, powers off.
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
  - `STAGED`, `n`, `CALBK`, `status`, `written`, `conflict`, `freed`,
    `STACK`, `used`, `size` (`native::put_extra`, see "Stack depth" and the
    calibration backup section).
  
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

`fatren-dry-run` (native) only reports. `test-initcore` is always a dry run
unless built with `fatren-live`.

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

## Calibration / boot-data backup (`src/calbackup.rs`)

Copies of the unit's irreplaceable data as plain files in the root of the
SPI-flash FAT (the USB disk), so the owner can archive them from the PC
(UF-20), enough for a full restore after an SWD mass erase (`ROMBOOT.BIN` at
0x08000000, `LOADER.BIN` at 0x080A0000, `CALIB.BIN` at 0x080C0000, then any
firmware). Internal flash and the system areas are only read. 180 KiB (45
clusters) of the 16 MiB volume.

| File | Source | Bytes | Why this size |
|---|---|---|---|
| `CALIB.BIN` | `0x080C0000` | 4096 | Settings + calibration record: 800 B (u16 crc, u16 ~crc, u32 version 1, data; calibration struct at +0x168, `hy3131_v2_6.h`). This unit's 1 MiB dump has nothing but 0xFF in the 128 KiB sector beyond 0x31F. 4 KiB = one cluster = one flash block, with room for a longer record. |
| `BOOTDATA.BIN` | `0x080A4000` | 16384 | UID-bound FS-loader data. The device-specific block starts at `0x080A4000` (`dumps/dump_boot_data_uid.bin`, 2 KiB; the 16 KiB `dump_boot_data.bin` is in fact the loader at `0x080A0000`). The whole device-specific non-0xFF run is `0x080A4000..0x080AE6D3`; 16 KiB covers its start (the data words and the version bytes BT command 02 reports). A subset of `LOADER.BIN` (+0x4000..+0x8000), kept for compatibility: earlier builds wrote it and calbackup never deletes a file, so a dropped name would go stale. |
| `UID.BIN` | `0x1FFF7A10` | 12 | 96-bit unique ID. |
| `OPTBYTES.BIN` | `0x1FFFC000` | 16 | Option bytes. |
| `OTP.BIN` | `0x1FFF7800` | 528 | OTP + lock bytes (all 0xFF on this unit; cheap). |
| `ROMBOOT.BIN` | `0x08000000` | 16384 | Primary bootloader (sector 0). |
| `LOADER.BIN` | `0x080A0000` | 131072 | The whole update-loader sector 9, including all of the UID-bound data. |
| `BACKUP.TXT` | — | ~760 | Name, address, size and CRC-32 (zlib) of each file + a restore note. Deterministic (no time, no counters), so it does not defeat the wear check. |

None of the names matches the loader's `dm40*.bin`.

**When:** every normal boot, before USB exists: native from `pre_main`
(after board init and fatren), the RAM build from `rust_start` before
`main()`. Cargo feature `calbackup`: implied by `native`; off in the RAM
images for CODE space (~6.5 KB; as of 2026-10-13 the default image has
~250 B of CODE left, so `--features calbackup` overflows by ~6.3 KB). The T1
`test-initcore` image links again since `link-test.x` gives CODE all of
SRAM0+SRAM1 (128 KiB) and moves `.dmabuf` to SRAM2 (`0x20020000`, 64 KiB), but
without calbackup: the 128 KiB flash sector 8 the RAM images load from is the
real ceiling (T1 image 130 316 B on 2026-10-13 evening, ~0.75 KB left and
shrinking with UI work), and calbackup does not fit.
So calbackup's first run on hardware is still T3 step 1 (review R1).

**Wear:** each file is first compared with its source by reading it: the root
entry (size), the whole cluster chain through FAT 1 (exact length, valid
links, EOC) and the contents. Equal → nothing at all is written, not even the
directory entry. Measured on the host image: a second run writes no block.
Only a missing or different file (calibration changed, a corrupted or foreign
copy) is rewritten, always under the same name (no versions).

**Write order** (copy-on-write; a power loss anywhere leaves at worst lost
clusters, never a live entry on free or half-written clusters, never two
entries with the same name):
1. allocate new clusters first-fit from FAT 1 (the old chain stays intact and
   referenced);
2. write the data blocks (read, overlay, write, read back);
3. link the new chains (EOC at the end) in every FAT copy — one write per
   touched FAT block and copy for all files together (0x0000 → link needs an
   erase, so this is batched);
4. directory entries: new start cluster and size (new entries: attr 0x20,
   date 2026-10-13, no LFN) — one write per touched root block for all files;
5. free the old chains in every FAT copy (`fatren::free_chain`; only clears
   bits, so no erase) — only chains that were well formed, so a broken old
   chain is left as lost clusters rather than risk freeing another file's;
6. re-read every written file through its new chain and compare.

Every block write is verified by read-back. The volume is only touched if
`fatren::parse_bpb` accepts it. A name taken by a directory or volume label is
skipped (`conflict`). The residual risk is the erase window of a FAT or root
block itself (no copy-on-write below the FAT layer on this NOR disk; host
writes have the same window): at most FAT 1, FAT 2 (one after the other) and
one root block per boot that changes something.

**Time:** unchanged ≈ 1 root + 1 FAT + 45 data blocks read (~14 ms per 4 KiB
block at the 3.125 MHz polled SPI clock: ~0.65 s per boot; ~0.15 s before
ROMBOOT/LOADER were added) + the BACKUP.TXT CRC-32 over ~165 KiB of sources
(nibble table, ~10 ms). The first write adds 45 data + 2 FAT + 1 root block
writes (erase + program + read-back, ~4-5 s once); a loader update rewrites
LOADER.BIN + BACKUP.TXT (~3 s once). The FWDGT is fed between blocks (and by
spiflash during erases).

**Status** (`calbackup::result()`), appended to `DBG:BOOT?` by
`native::put_extra` (see below): `CALBK,<status>,<written>,<conflict>,<freed>`
with status `NOTRUN|NOMEDIUM|BADBPB|UNCHANGED|WRITTEN|NOSPACE|ERROR`, written
/ conflict bit masks (bit 0 CALIB, 1 BOOTDATA, 2 UID, 3 OPTBYTES, 4 OTP,
5 ROMBOOT, 6 LOADER, 7 BACKUP.TXT; new files are appended so old bits keep
their meaning; 8 is the u8 limit), freed = old clusters freed.

**Host test** (`tools/fatren-test/run.sh`, every check fatal): first run
writes all eight files (ROMBOOT/LOADER/BOOTDATA sliced from the 1 MiB dump;
45 data blocks, FAT 1 + FAT 2 once each, root once),
fsck.fat clean, every file equal to its source, BACKUP.TXT CRCs match zlib;
second run `UNCHANGED` with no block written; one changed calibration byte →
`CALIB.BIN` + `BACKUP.TXT` rewritten, 2 old clusters freed, fsck clean,
contents equal, then unchanged again; a 100000-byte foreign `UID.BIN` is
replaced and its 25 clusters freed; a directory `OTP.BIN` is a conflict and
the rest is written; fatren + calbackup on one volume, fsck clean; one byte
changed deep in `LOADER.BIN` → `LOADER.BIN` + `BACKUP.TXT` rewritten, 33 old
clusters freed, fsck clean, contents equal, then unchanged.

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
| `calbackup::boot` / `calbackup::run` | 212 / 14 260 (with ROMBOOT/LOADER) |
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
`…,STAGED,<n>,CALBK,<status>,<written>,<conflict>,<freed>,STACK,<used>,<size>`
(`STAGED`: fatren's count of dm40*.bin left because their header is not the
marker).

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
  - The `mark_healthy` call site goes in `main.rs` (see "Crash counter").
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
- **PA2 at power-on:** a long key hold may already reach the loader's update
  screen without any marker change. This is the vendor's own entry, still to be
  tested (fsloader-install-trigger.md §6). If confirmed, it is the preferred,
  non-destructive manual entry.
- **Code space:** the default RAM build has 300 bytes left in its code region
  (see "Default build").

## Test plan (coordinator + user)

Build with `./build.sh <mode>`. RAM images load as before (`tools/reload.sh
<mode>`). Keep `firmware/patch/build/dm40_patched.bin` and a stock
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
   - Expect `RENAMED`, and `_DM40PRV.BIN` on the disk.
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
     - `CALBK,WRITTEN`;
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
     - If PA2 is the key, the loader screen may appear with the marker intact
       (see the review).
     - Otherwise the marker is zeroed, and the loader update screen and its
       disk appear.
   - Copy back `_DM40PRV.BIN` as `dm40_fwrs.bin`, then press "update". fw-rs
     returns.
   - `DBG:BOOT?` then shows `last` = `RCVR` and `last_trigger` = `0x54520010`.
5. **`DBG:RELEASE`:**
   - About 20 s later the WDT resets, and fw-rs reboots normally.
     `DBG:BOOT?` shows `WDT_RETRY` and crash word `0x43520001`.
   - `DBG:RELEASE` again within 60 s of that boot: the WDT reset now goes to
     recovery, and the loader screen appears. Reinstall as in step 4.
   - With the `mark_healthy` call in place: 60 s after a boot, `DBG:BOOT?`
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
