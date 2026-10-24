# DM40 Patch Notes (for agents)

Working knowledge for building and flashing patched DM40 firmware.
Everything below was verified on the live device (2026-10-13).

## Toolchain

- `arm-none-eabi-gcc` ( Debian: `gcc-arm-none-eabi`), `make`, python3.
- Build: `cd firmware/patch && make ORIG=../../dm40_v_2_6.bin SRCS="src/startup.c src/<your>.c"`
- Output: `build/dm40_patched.bin`. Flash: copy to the USB MSC disk of the
  device (Settings -> Other Settings -> USB Connection), eject, power-cycle.
  The bootloader accepts any `dm40*.bin` name.
- Header words (`word0`/`word2`) are computed by `firmware/dm40fw.py`;
  `patcher.py` already calls it. V2.6 and V2.8 bodies are byte-identical,
  one patch fits both (verified: `cmp` of decoded 2.6 vs 2.8 = 0 diffs).

## The 64-byte pad rule (IMPORTANT)

The bootloader **rejects** an update whose body ends right after the patch
literal pool ("update error" on screen, no other message). Observed:

- patch code 308 bytes (ends with `.word` literals) -> **update error**
- same code + 64 trailing NOP bytes -> **flashes fine**
- 280-byte code, 280+64 NOPs: fine; 280+128: fine; bare 280: fine.

So a patch that ends in code (no literals) needs no pad; any patch whose
last bytes are data/literals must be padded. `patcher.py` pads with
`PATCH_PAD` (default 64) NOP bytes (`00 bf`) automatically. Keep it.

## Patches

- `src/memrw.c` - BT command `AF 05 03 0B LEN [addr:4][sub:1][n:1][payload] CRC`
  - sub 0: read n (<=0xff) bytes at addr, answer `AF 05 03 0C n <data> CRC`
    (header is 5 bytes: `AF 05 03 0C n`, NOT `DF..`; clients were bitten by this)
  - sub 1: write payload to addr, ack `.. 0C 00 CRC`. **Request LEN = n+6 must
    stay <= 0x83** (chunk <= 0x7d bytes): a longer memcpy clobbers the saved
    r4..sb (sp+0x84..0x98) of the BT rx handler and the device dies.
    The old `dm40_loadfw.py` loader used 0x7d chunks (it is not kept in this repo).
  - sub 2: call function at addr|1 with r0..r3, answer r0 (4 bytes).
    **NOT WORKING YET** - the call path hangs the device (blx into the target
    never returns or kills the BT handler). read/write are fine.
  - Hook: `bl 0x800d2fa` -> `patch_parse` at the BT receive handler call
    site `0x0803cec0` (expect `0xfa1bf7d0`).
  - Client: `kit/tools/dm40_memrw.py` (`read`/`dump`/`measure`/`set-model`;
    `write`/`call` only with `--unsafe`). `kit/mkkit.sh` builds this patch
    (`SRCS="src/startup.c src/memrw.c"`) and the kit zip into `dist/`.
- `src/demo_version.c` - version 9.9 demo (flash-safe).
- `src/usb_hooks.c` + `src/usb_stream.S` (by another agent) - USB live
  measurement on EP0 vendor request; has a watchdog auto-rollback: if the
  previous reset was a free-watchdog reset, USB hooks are not installed for
  that boot. `make usb` builds it. NOT included in the memrw build.
- `src/startup.c` - always required; hooks the ctor call at `0x080041b4`
  (expect `0xfbaef041`).

## Firmware addresses (V2.6 == V2.8, all Thumb, +1 for calls)

- parser `0x0800d2fa`, memcpy `0x080457f8`, UART4 send/ready `0x080082a0/0x0800849c`
  (UART4 base literal `0x40004c00` at `0x0800d6b0`), fw_get_version `0x0800afd2`,
  BT rx handler `0x0803cdd8` (its parser call site `0x0803cec0`),
  parsed-packet struct G = `0x20025a20` ([0]=hdr,[3]=cmd,[4]=len,[5..8]=data ptr),
  big W25Q writer `FUN_08009ff0` (4k erase + paged program, len<=0xffff,
  16-bit counters), small page writer `FUN_08009bb0`.
- The memrw patch is the interface to use: full speed, no reboots, as long
  as request LEN <= 0x97 (so the parser memcpy does not reach the saved LR
  of the BT rx handler).

## Misc device facts (verified)

- MSC "disk" sectors are **4096 bytes** (not 512); FAT16 with 4096-byte
  sectors, total 4096 sectors = 16 MB SPI flash. PC `dd` on `/dev/sda` uses
  512-byte kernel sectors, LBA mapping is linear either way.
- RDP (readout protection) Level 1 is set (FLASH_OPTCR `0x2FFFBBED`,
  OB at `0x1FFFC008` = `ff 3f 00 c0`): SWD cannot read the flash. BT memrw
  reads everything (flash/RAM/periph/UID `0x1FFF7A10`).
- OTP area `0x1FFF7800..0x1FFF7A10` is all `0xFF` (never programmed).
- Full flash image saved as `flash_full_1mb.bin` (md5 89e4a7cb49...), built
  from `boot_full.bin` + `dump_*.bin` pieces; all earlier echo dumps match.
- Calibration/boot data at `0x080a0000..`, settings at `0x080c0000..`,
  bootloader marker hole at body offset `0x4000` (flash `0x08008000`),
  which the bootloader writes itself (excluded from word2 CRC).

## Todo / known broken

- `memrw.c` sub 2 (call) - **still broken**, full history on device:
  1. call of `bx lr` stub (SRAM/CCM) -> BT handler dies, watchdog reboot.
  2. call of a `str marker; b .` stub -> **executes** (marker path), screen
     keeps running: the blx ENTRY works, the handler just hangs in the loop.
  3. call with respond after blx -> dies; call without respond (result in
     CALL_RESULT) -> dies; call on the patch epilogue itself -> dies.
  4. sub2 with NO blx at all (just store addr) -> ALSO dies, when
     CALL_RESULT was 0x2002f800. **That address is beyond the real RAM**
     (the earlier sram2 probe faulted at 0x2002ffff), so 1-3 may all have
     been the SAME bus fault on the result store... but after moving
     CALL_RESULT to 0x2000f100 (mapped, verified) call STILL dies.
  So: blx entry reaches the target (see 2), everything after return dies.
  Leading hypothesis: on this GD32F4-clone **`blx reg` does not write LR
  (or corrupts it)**, so the callee's `bx lr` returns to a wrong place and
  the BT handler crashes. Workaround to try next: an SRAM thunk that does
  `push {r3}; ldr r3,=ret_in_patch; bl target; str r0,[res]; pop {pc}` -
  i.e. never return via blx-LR, use bl (literal offset, client-encoded)
  and stack-return; or verify LR handling with a stub that stores r14.
  read (sub 0) and write (sub 1) are solid and are the working interface.
- `memrw_pad4.c` is a leftover experiment, ignore.