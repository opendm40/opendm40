/*
 * DM40 replacement firmware, RAM-loaded PoC.
 *
 * The image is a bare binary loaded at IMAGE (0x20008000). Its first word is the
 * initial MSP, its second word is the reset entry (Cortex-M vector-table layout),
 * so a loader only has to copy the image to IMAGE and branch to word[1].
 *
 * The region sits in the ~100 KiB of SRAM0 the stock firmware leaves free in
 * multimeter mode (0x20005024+). Keep the low 32 KiB of SRAM0 (scope buffer)
 * untouched until the peripherals are re-initialised.
 */
MEMORY
{
    IMAGE (rwx) : ORIGIN = 0x20008000, LENGTH = 64K
}

ENTRY(reset);
EXTERN(__VECTORS);

_stack_top = ORIGIN(IMAGE) + LENGTH(IMAGE);   /* 0x20018000 */

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
    } > IMAGE

    __image_end = .;

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}
