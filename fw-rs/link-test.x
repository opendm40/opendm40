/*
 * DM40 replacement firmware, RAM TEST images (features test-initcore and
 * test-recovery, selected by build.sh; the default RAM image uses link.x).
 * Stored in internal flash sector 8 (0x08080000, 128 KiB, unused by the stock
 * firmware), executed from SRAM.
 *
 * GD32F4 flash is zero-wait only in its first 512 KiB; sector 8 lies above,
 * so code fetched from there runs several times slower. The reset code (in
 * .boot, runs from flash) therefore copies .text + .rodata into SRAM and the
 * firmware runs from there. Vectors stay in flash (exceptions are not used).
 *
 * Differs from link.x only in the SRAM split: the test images carry the native
 * boot code (init_core, board init, fatren, DBG:BOOT?) on top of the normal
 * image, which no longer fits link.x's 120 KiB CODE region. CODE gets all of
 * SRAM0+SRAM1 (128 KiB: no larger than the 128 KiB flash sector the same
 * bytes are loaded from, so the flash sector, not CODE, is the real ceiling),
 * and the DMA region is the 64 KiB SRAM2 above it (.dmabuf ~58 KiB).
 *
 * RAM after the takeover is all ours (SRAM ends at 0x20030000,
 * firmware/re/ANALYSIS.md section 4):
 *   SRAM0+1 0x20000000 128 KiB  code + constants (copied at reset)
 *   TCM     0x10000000  64 KiB  .data (copied), .bss, stack (CPU only)
 *   SRAM2   0x20020000  64 KiB  .dmabuf: DMA buffers incl. the scope record
 */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08080000, LENGTH = 128K
    CODE (rwx)  : ORIGIN = 0x20000000, LENGTH = 128K
    TCM (rw)    : ORIGIN = 0x10000000, LENGTH = 64K
    DMARAM (rw) : ORIGIN = 0x20020000, LENGTH = 64K
}

ENTRY(reset);
EXTERN(__VECTORS);

_stack_top = ORIGIN(TCM) + LENGTH(TCM);       /* 0x10010000 */

SECTIONS
{
    .vectors ORIGIN(FLASH) :
    {
        __image_start = .;
        KEEP(*(.vectors));
        KEEP(*(.boot));
        . = ALIGN(4);
    } > FLASH

    .text :
    {
        __stext = .;
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = ALIGN(4);
        __etext = .;
    } > CODE AT > FLASH
    __stext_lma = LOADADDR(.text);

    .data :
    {
        . = ALIGN(4);
        __sdata = .;
        *(.data .data.*);
        . = ALIGN(4);
        __edata = .;
    } > TCM AT > FLASH
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
    } > DMARAM

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
    } > DMARAM

    __image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}
