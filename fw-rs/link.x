/*
 * DM40 replacement firmware, RAM-loaded PoC.
 *
 * The image is a bare binary loaded at IMAGE (0x20008000). Its first word is the
 * initial MSP, its second word is the reset entry (Cortex-M vector-table layout),
 * so a loader only has to copy the image to IMAGE and branch to word[1].
 *
 * The image (vectors, code, rodata, data) sits in SRAM0 0x20008000..0x2001C000,
 * which the stock firmware leaves free in multimeter mode (free from
 * 0x20005024). Keep the low 32 KiB of SRAM0 (stock scope buffer) untouched.
 *
 * .bss and the stack live in the 64 KiB TCM SRAM (0x10000000, CPU only, no
 * DMA): the loader does not write there, and once we run, the stock firmware's
 * use of it no longer matters. The reset code enables the TCM clock first.
 */
MEMORY
{
    IMAGE (rwx) : ORIGIN = 0x20008000, LENGTH = 80K
    TCM (rw)    : ORIGIN = 0x10000000, LENGTH = 64K
}

ENTRY(reset);
EXTERN(__VECTORS);

_stack_top = ORIGIN(TCM) + LENGTH(TCM);       /* 0x10010000 */

SECTIONS
{
    .vectors ORIGIN(IMAGE) :
    {
        __image_start = .;
        KEEP(*(.vectors));
    } > IMAGE

    .text :
    {
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = ALIGN(4);
    } > IMAGE

    .data :
    {
        . = ALIGN(4);
        *(.data .data.*);
        . = ALIGN(4);
    } > IMAGE

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        __bss_start = .;
        *(.bss .bss.* COMMON);
        . = ALIGN(4);
        __bss_end = .;
    } > TCM

    __image_end = .;

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}
