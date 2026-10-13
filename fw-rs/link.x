/*
 * DM40 replacement firmware: runs from internal flash sector 8.
 *
 * Sector 8 (0x08080000, 128 KiB) is unused by the stock firmware (all 0xFF in
 * the full dump); the stock app ends below 0x0807D000, sector 9 holds the
 * bootloader data, sector 10 the settings/calibration record. The image is
 * written there by the RAM stub (stub/), which also starts it: the vector
 * table is at the start of the image, word[0] = initial MSP, word[1] = reset.
 *
 * RAM after the takeover is all ours:
 *   TCM   0x10000000 64 KiB  .data (copied from flash), .bss, stack (CPU only)
 *   SRAM0 0x20000000 112 KiB .sram: large CPU buffers (not zeroed)
 *   SRAM1+2 0x2001C000 80 KiB .dmabuf: DMA-reachable buffers (not zeroed)
 */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08080000, LENGTH = 128K
    TCM (rw)    : ORIGIN = 0x10000000, LENGTH = 64K
    SRAM (rw)   : ORIGIN = 0x20000000, LENGTH = 112K
    DMARAM (rw) : ORIGIN = 0x2001C000, LENGTH = 80K
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
    } > FLASH

    .text :
    {
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = ALIGN(4);
    } > FLASH

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

    .sram (NOLOAD) :
    {
        . = ALIGN(4);
        *(.sram .sram.*);
    } > SRAM

    .dmabuf (NOLOAD) :
    {
        . = ALIGN(4);
        *(.dmabuf .dmabuf.*);
    } > DMARAM

    __image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}
