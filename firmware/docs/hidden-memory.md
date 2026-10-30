# Hidden memory on the GD32F427VGT6 (2026-10-30)

opendm40 is short of SRAM (the scope record lost 16 KB in PR #174) and of
zero-wait flash (region B, [native-boot.md](native-boot.md) "Native memory
map"). This page lists what the MCU has beyond the 64 KiB TCM + 192 KiB SRAM
that [link-native.x](../link-native.x) uses, and what was found on one
meter.

## Result

| candidate | range | size | result |
|---|---|---|---|
| ADDSRAM ("additional SRAM", user manual) | `0x20030000..0x200AFFFF` | 512 KiB on some GD32F4xx | **absent**: every read is a precise bus fault |
| Space after the TCM | `0x10010000..0x1FFEFFFF` | - | reads `0xFFFFFFFF`, no fault, 5 cycles; not RAM (*inferred*; no write tried) |
| Flash above 1 MiB | `0x08100000..0x0FFFFFFF` | - | reads `0xFFFFFFFF`, no fault, 5 cycles at every offset: no flash array behind it (*inferred* from the speed, see below) |
| Zero-wait flash | `0x08000000..0x0807FFFF` | 512 KiB | ends exactly at `0x08080000`; no register sets the size |
| Backup SRAM, free part | `0x40024124..0x40024FE7` | 3 780 B | **usable**: read / write OK, 5 cycles; kept over a reset, lost at power-off |
| USBHS FIFO RAM | `0x40060000..0x40060FFF` | 4 KiB | **usable** with `USBHSEN` on: read / write OK, 5 cycles, kept while the clock is gated off |
| USBFS FIFO RAM, unallocated tail | FIFO words `0x70..0x13F` | 832 B | not probed: the USB stack owns the FIFO |
| IPA (2D engine) | `0x4002B000` | - | **absent**: clock bit sets, registers read 0 and ignore writes |
| TLI (LCD controller) | `0x40016800` | - | **absent**: clock bit sets, `TLI_CTL` reads its reset value `0x2220`, writes ignored |
| CAN mailbox / filter RAM | `0x40006400`, `0x40006800` | < 1 KiB | not probed (clock off, too small) |

Usable: **3.7 KB backup SRAM and 4 KB USBHS FIFO RAM**, nothing large.

## Identification

Read with `DBG:PEEK?` (below):

| register | address | value | meaning |
|---|---|---|---|
| memory density | `0x1FFF7A20` | `0x04000100` | flash 0x400 = 1024 KiB, SRAM 0x100 = 256 KiB |
| DBG_ID | `0xE0042000` | `0x22020413` | device 0x413, revision 0x2202 |
| CPUID | `0xE000ED00` | `0x410FC241` | Cortex-M4 r0p1 |
| FMC_PID | `0x40023D00` | `0x4E475637` | product ID (no public decoding) |
| FMC_WS / FMC_WSEN | `0x40023C00` / `0x40023CFC` | `0` / `0` | no wait states, wait-state insertion off |
| FMC_OBCTL0 / 1 | `0x40023C14` / `0x40023C18` | `0x2FFFBBED` / `0x0FFF0000` | DBS = 0, DRP = 0, SPC = 0xBB (protection level low) |

The SRAM density field says 256 KiB, which matches the datasheet and the
bus faults above 0x2002FFFF.

## Per candidate

### SRAM beyond 256 KiB

The [GD32F4xx user manual][um] (Rev3.3, 1.3.2 and table 1-2) maps 512 KiB
of ADDSRAM at `0x20030000..0x200AFFFF` "available only in some particular
GD32F4xx devices" (the GD32F470 has 768 KiB in all). The GD32F427
[datasheet](../../hw/datasheets/gigadevice-gd32f427xx-datasheet-rev1.9.pdf)
(table 2-3) marks `0x20030000..0x2006FFFF` reserved. No RCU bit gates it.

On the device, reads at every 16 KiB step from `0x20030000` to `0x200AC000`
end in a precise bus fault (CFSR `0x00008200`: BFARVALID + PRECISERR).
`0x2002FFF8..0x2002FFFF` read normally. So there is no ADDSRAM and no
mirror of the 192 KiB SRAM (a mirror would read, not fault).

The reserved code space after the TCM (`0x10010000`, `0x10020000`, ...,
`0x1FFE0000`) reads `0xFFFFFFFF` without a fault, unlike the TCM itself
(`0x1000FFF8` holds stack words). Writes there were not tried.

### Flash

Datasheet table 2-1: the GD32F427**VG** has 512 KiB "code area" and 512
KiB "data area"; the **VK** has 512 + 2560 KiB. The user manual says "up to
1024K" executes without waiting time, and its option bit DBS (dual bank on 1
MiB parts) is "only valid for GD32F450xx and GD32F470xx". `FMC_WS` and
`FMC_WSEN` are wait-state controls; none sets the zero-wait size, so on this
part it is fixed at 512 KiB.

Load timing (`DBG:MTIME?`, interrupts masked, cycles at 200 MHz including
the DWT read; TCM / SRAM / backup SRAM take 5):

| address | per load | note |
|---|---|---|
| `0x08010000`, `0x0807F000`, `0x0807FFC0..0x0807FFFC` | 5 | zero-wait |
| `0x08080000..0x080FFFFF` | 5 on a hit, **305 on a miss** | one miss per 64-byte line, sequential or strided; repeated reads of one line hit |
| `0x08100000..0x0FFFFFFF` | 5 | always `0xFFFFFFFF` |
| `0x1FFF0000` (boot ROM), `0x1FFF7800` (OTP) | 5 | |

So a data load from region C costs ~1.5 us per new 64-byte line. Above 1
MiB the reads never stall, so they do not reach a flash array: there is no
hidden flash there. The ~21.8 us bus stall per region-C load in
[native-boot.md](native-boot.md) "The slow flash bank while an AD2 stream
runs" came from ISR-latency and lost-sample counts, not from a timed load;
why it is ~15x the line-miss time measured here is *unknown*.

Nothing was erased or programmed.

### Backup SRAM

4 KiB at `0x40024000`. Used by opendm40 (installed 0.5.0 image):

- `0x40024000..0x40024123`: the settings record v11 (magic, version, 70
  words, CRC; [persist.rs](../src/persist.rs));
- `0x40024FE8..0x40024FEF`: the settings-file sequence words;
- `0x40024FF0..0x40024FFF`: the boot shim's words ([boot.rs](../src/boot.rs)).

Free: `0x40024124..0x40024FE7` (3 780 B). Before the test it held random
power-up data. `DBG:MTEST?` over `0x40024200..0x40024EFF` (832 words):
pattern, inverse, pattern all OK; `DBG:MVER?` 4 minutes later OK. It keeps
its content over a reset and an update, but not over a power-off
([persist.rs](../src/persist.rs) header). Needs `BKPSRAMEN` and `PMU_CTL`
BKPWEN, which [persist.rs](../src/persist.rs) already sets.

### USBHS FIFO RAM

The datasheet lists USBHS (`0x40040000..0x4007FFFF`) for the GD32F427, and
the user manual gives it a 4 KiB FIFO RAM. The DM40 does not use USBHS (its
USB is USBFS on PA11 / PA12). The DWC2 core maps its FIFO RAM directly at
base + `0x20000` = `0x40060000`; the GigaDevice manual does not document
that window (STM32's OTG reference manuals do).

With `USBHSEN` (RCU_AHB1EN bit 29) on:

- `GRFLEN` (`0x40040024`) resets to `0x200`; the reset contents of the RAM
  are random;
- `DBG:MTEST? 0x40060000,1024`: 3 072 of 3 072 OK, no alias;
- load time 5 cycles;
- with the clock gated off and on again ~4 minutes later, `DBG:MVER?`: all
  1024 words intact.

With the clock off, or above `0x40060FFF`, reads return the last value the
bus carried (no fault). So the RAM is 4 KiB.

To use it: set `USBHSEN`, CPU word / byte access only. Whether DMA reaches
it is *unknown* (the AHB matrix table in the manual gives DMA1 a path to
AHB1, DMA0 none). The supply current with `USBHSEN` on is *unknown*.

### USBFS FIFO RAM

1.25 KiB (`FIFO_DEPTH_WORDS` in [usb.rs](../src/usb.rs)). The allocation
read from the core: RX `GRFLEN` = 64 words, TX FIFO 0..2 at words `0x40`,
`0x50`, `0x60`, 16 words each, TX FIFO 3 empty: words `0x70..0x13F` (832 B)
are unallocated. Not probed: the direct window (`0x50020000`) belongs to the
running USB core.

### IPA and TLI

Both are on the GD32F470 and are "Reserved" in the GD32F427 datasheet
memory map. Clock bits: `IPAEN` (RCU_AHB1EN bit 23) and `TLIEN`
(RCU_APB2EN bit 26) both set and read back.

- IPA: with the clock on, all registers read 0; writes to `IPA_FMADDR`
  (`0x4002B00C`) and `IPA_FLOFF` (`0x4002B010`) read back 0. No IPA, so no
  hardware rectangle fill to the FSMC LCD.
- TLI: with the clock on, `TLI_CTL` (`0x40016818`) reads `0x00002220` (its
  reset value: the dither-width fields), everything else 0; writes to
  `TLI_SPSZ` (`0x40016808`) and `TLI_INTEN` (`0x40016834`) read back 0. The
  block is fused off or not there (*inferred*).

Both clock bits were cleared again after the test.

## How it was probed

The cargo feature `memprobe` ([memprobe.rs](../src/memprobe.rs), not in
releases) adds debug commands:

| command | does |
|---|---|
| `DBG:PEEK? addr[,n]` | n (≤ 32) words, or `F<CFSR>/<BFAR>` for a word whose read faulted |
| `DBG:POKE[?] addr,value` | write one word, reply the read-back |
| `DBG:MTEST? addr,words[,seed]` | alias check, then pattern / inverse / pattern; `<ok>,<bad>,<first bad>,<faults>,<alias>` |
| `DBG:MVER? addr,words[,seed]` | verify the pattern MTEST left |
| `DBG:MTIME? addr,n[,stride]` | DWT cycles per load: `<first>,<min>,<max>,<total>` |
| `DBG:MCLK[?] IPA\|USBHS\|TLI,0\|1` | that clock-enable bit only; reply the RCU register |

- A fault does not hang the meter: with the feature the HardFault vector is
  `probe_fault`, which, while a probe access is armed, records CFSR / BFAR,
  clears them and resumes after the access. PRIMASK is set, so a BusFault
  escalates to HardFault. Any other fault still hangs as before (watchdog).
- POKE / MTEST write only inside a fixed list: ADDSRAM, the free backup
  SRAM slice `0x40024200..0x40024EFF`, the USBHS FIFO window, the first
  0x40 bytes of the IPA and TLI registers. Never the FMC, option bytes, RCU
  or GPIO.
- MTEST's alias check writes one word and reads every place in the 192 KiB
  SRAM and the TCM it could mirror to, restoring them; a mirror stops the
  test.
- The device image was the installed 0.5.0 (PR #176, 0c33b621) plus this
  feature; without the feature that tree builds byte-identical to the
  installed file. The installed image went back afterwards.

## Sources

- [GD32F427xx datasheet Rev1.9](../../hw/datasheets/gigadevice-gd32f427xx-datasheet-rev1.9.pdf):
  table 2-1 (code / data area per part), table 2-3 (memory map).
- [GD32F4xx user manual][um] Rev3.3: 1.2 (AHB matrix, ADDSRAM), table 1-2
  (memory map), 1.3.2 (on-chip SRAM), 1.6.1 (memory density), 2.2 / 2.4
  (FMC: zero-wait region, FMC_WS, FMC_OBCTL0, FMC_WSEN, FMC_PID), 4.3.10 /
  4.3.14 (RCU_AHB1EN / RCU_APB2EN), 11 (IPA), 23.7 (TLI
  registers), 29 (USBHS).

[um]: https://www.gigadevice.com/product/mcu/high-performance-mcus/gd32f4xx-series/gd32f427
