/*
 * DM40 replacement firmware, NATIVE format: the app image the FS-loader
 * installs at 0x08004000 (stock update-file body), executed in place (XIP;
 * the GD32F4 low 512 KiB of flash is zero-wait, so nothing is copied to SRAM).
 *
 * Flash (body offset = address - 0x08004000):
 *   A  0x08004000..0x08008000  16 KiB  vector table + shim (.vectors, .shim)
 *      0x08008000..0x08008010  16 B    GAP, 0xFF in the binary. The ROM and the
 *                                      FS-loader read AND write a 12-byte record
 *                                      at 0x08008000..0x0800800B (excluded from
 *                                      the update body CRC by dm40fw.py); the
 *                                      next 4 bytes are padding.
 *   B  0x08008010..0x08080000  480 KiB .text, .rodata: everything that may
 *                                      run or be read while the AD2 stream
 *                                      ISR runs. The linker fails when it
 *                                      does not fit.
 *   C  0x08080000..0x080A0000  128 KiB .cold (boot-only / rarely run code:
 *                                      calibration backup, fatren, the
 *                                      gallery, screenshots, the DLG writer,
 *                                      FAT / vdisk generation, uploads, the
 *                                      bus self-tests / AD2 experiments (DBG),
 *                                      scope CSV, boot / DBG:BOOT?, settings
 *                                      persistence) and
 *                                      the .data load image. Must end below
 *                                      0x080A0000 (FS-loader, UID data,
 *                                      calibration above).
 *   Why the split: flash from 0x08080000 up is NOT zero-wait on the GD32F4
 *   (only the low 512 KiB are). A read there stalls the bus for ~20 us, and
 *   with it the AD2 stream ISR's entry (device 5f14d38: max_lat 18.8-21.8
 *   us whenever the logger panel drew ASCII26 glyphs, which had grown past
 *   0x08080000; 4.6 us without the panel; docs/dmm-input-scope-feasibility.md
 *   10.3). Code in C costs that per call: only what never runs while a
 *   stream does (or seldom, outside the main loop's every-pass path).
 *
 * RAM:
 *   TCM     0x10000000  64 KiB  .data, .bss, stack (top 0x10010000)
 *   SRAM    0x20000000 192 KiB  .dmabuf (DMA buffers, scope record),
 *                               .sram_bss
 */
MEMORY
{
    FLASH_A (rx) : ORIGIN = 0x08004000, LENGTH = 0x4000
    FLASH_B (rx) : ORIGIN = 0x08008010, LENGTH = 0x08080000 - 0x08008010
    FLASH_C (rx) : ORIGIN = 0x08080000, LENGTH = 0x080A0000 - 0x08080000
    TCM (rw)     : ORIGIN = 0x10000000, LENGTH = 64K
    SRAM (rw)    : ORIGIN = 0x20000000, LENGTH = 192K
}

ENTRY(reset);
EXTERN(__VECTORS);

_stack_top = ORIGIN(TCM) + LENGTH(TCM);       /* 0x10010000 */
/* Word[0] of the image: only the FS-loader's `msr msp` before `bx reset` uses
 * it (reset moves SP to _stack_top at once). Same value as the stock app so
 * any check the loader applies to it passes as for stock. */
_loader_sp = 0x2002F7F8;
_marker_start = 0x08008000;
_marker_end = 0x0800800C;

SECTIONS
{
    .vectors ORIGIN(FLASH_A) :
    {
        __image_start = .;
        KEEP(*(.vectors));
        KEEP(*(.shim .shim.*));
        . = ALIGN(4);
        __region_a_end = .;
    } > FLASH_A

    /* Claimed before .text (the first matching rule wins), placed in C by
     * its region. Module paths in the v0-mangled section names:
     * <len><name> after the crate (7dm40_fw). */
    .cold ORIGIN(FLASH_C) :
    {
        __cold_start = .;
        *(.text.*7dm40_fw9calbackup*);
        *(.text.*7dm40_fw6fatren*);
        *(.text.*7dm40_fw6native*);
        *(.text.*7dm40_fw8recovery*);
        *(.text.*7dm40_fw7genuine*);
        *(.text.*7dm40_fw7gallery*);
        *(.text.*7dm40_fw10screenshot*);
        *(.text.*7dm40_fw4dlog*);
        *(.text.*7dm40_fw6fatdir*);
        *(.text.*7dm40_fw5vdisk*);
        *(.text.*7dm40_fw6upload*);
        *(.text.*7dm40_fw5hyspi*);
        *(.text.*7dm40_fw6ad2exp*);
        *(.text.*7dm40_fw9scope_csv*);
        *(.text.*7dm40_fw4boot*);
        *(.text.*7dm40_fw7persist*);
        *(.text.*7dm40_fw3app9app_press*);
        /* ... and their named read-only data (tables, strings in statics). */
        *(.rodata.*7dm40_fw9calbackup*);
        *(.rodata.*7dm40_fw6fatren*);
        *(.rodata.*7dm40_fw6native*);
        *(.rodata.*7dm40_fw8recovery*);
        *(.rodata.*7dm40_fw7genuine*);
        *(.rodata.*7dm40_fw7gallery*);
        *(.rodata.*7dm40_fw10screenshot*);
        *(.rodata.*7dm40_fw4dlog*);
        *(.rodata.*7dm40_fw6fatdir*);
        *(.rodata.*7dm40_fw5vdisk*);
        *(.rodata.*7dm40_fw6upload*);
        *(.rodata.*7dm40_fw5hyspi*);
        *(.rodata.*7dm40_fw6ad2exp*);
        *(.rodata.*7dm40_fw9scope_csv*);
        *(.rodata.*7dm40_fw4boot*);
        *(.rodata.*7dm40_fw7persist*);
        *(.rodata.*7dm40_fw3app9app_press*);
        . = ALIGN(4);
        __cold_end = .;
    } > FLASH_C

    .text ORIGIN(FLASH_B) :
    {
        __stext = .;
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = ALIGN(4);
        __etext = .;
    } > FLASH_B

    .data :
    {
        . = ALIGN(4);
        __sdata = .;
        *(.data .data.*);
        . = ALIGN(4);
        __edata = .;
    } > TCM AT > FLASH_C
    __sidata = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        __bss_start = .;
        *(.bss .bss.* COMMON);
        . = ALIGN(4);
        __bss_end = .;
    } > TCM

    .dmabuf (NOLOAD) :
    {
        . = ALIGN(4);
        *(.dmabuf .dmabuf.*);
        *(.sram .sram.*);
    } > SRAM

    /* Zero-initialised CPU-only statics kept out of TCM (the stack's
     * memory): `#[link_section = ".sram_bss"]` on an all-zero static; zeroed
     * at reset next to .bss (native.rs init_ram, main.rs rust_start). */
    .sram_bss (NOLOAD) :
    {
        . = ALIGN(4);
        __sram_bss_start = .;
        *(.sram_bss .sram_bss.*);
        . = ALIGN(4);
        __sram_bss_end = .;
    } > SRAM

    __image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}

ASSERT(__region_a_end <= _marker_start, "region A (vectors + shim) overlaps the loader marker at 0x08008000");
ASSERT(ADDR(.text) >= _marker_end, "code placed in the loader marker at 0x08008000..0x0800800B");
ASSERT(__image_end <= 0x080A0000, "image reaches the FS-loader at 0x080A0000");
/* Stack = TCM above .bss. Measured high-water 28 412 B (emulator, scope walk
 * of the device check 2026-10-21: TB list, CHA popup, MF tabs, corner step,
 * AUTO; DBG:BOOT? STACK) + the required 16 KiB headroom: .data/.bss may not
 * grow into that. build.sh native also checks the static worst call path
 * (tools/stack-depth/stackdepth.py --check 16384), which catches frame
 * growth this assert cannot see. */
ASSERT(_stack_top - __bss_end >= 28K + 16K, "TCM stack below the measured high-water (28 KiB) + 16 KiB headroom: move statics to .sram_bss / .dmabuf");
