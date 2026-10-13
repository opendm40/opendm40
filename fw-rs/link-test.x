/*
 * DM40 replacement firmware, stored in internal flash sector 8 (0x08080000,
 * 128 KiB, unused by the stock firmware), executed from SRAM.
 *
 * GD32F4 flash is zero-wait only in its first 512 KiB; sector 8 lies above,
 * so code fetched from there runs several times slower. The reset code (in
 * .boot, runs from flash) therefore copies .text + .rodata into SRAM0 and the
 * firmware runs from there. Vectors stay in flash (exceptions are not used).
 *
 * RAM after the takeover is all ours:
 *   SRAM0   0x20000000 112 KiB  code + constants (copied at reset)
 *   TCM     0x10000000  64 KiB  .data (copied), .bss, stack (CPU only)
 *   SRAM1+2 0x2001C000  80 KiB  .dmabuf: DMA buffers incl. the scope record
 */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08080000, LENGTH = 128K
    CODE (rwx)  : ORIGIN = 0x20000000, LENGTH = 120K
    TCM (rw)    : ORIGIN = 0x10000000, LENGTH = 64K
    DMARAM (rw) : ORIGIN = 0x2001E000, LENGTH = 72K
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

    __image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}
