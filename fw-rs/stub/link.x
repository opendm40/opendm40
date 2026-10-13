/*
 * RAM stub, loaded by the Bluetooth loader at 0x20008000 (word[0] = MSP,
 * word[1] = entry). Code up to 16 KiB; the payload (header + chunk of the
 * flash image) follows at 0x2000C000, appended by tools/legacy/flash.py.
 */
MEMORY
{
    STUB (rwx) : ORIGIN = 0x20008000, LENGTH = 16K
}

ENTRY(reset);
EXTERN(__VECTORS);

/* Stack below the image, in the SRAM0 part the stock leaves free. */
_stack_top = 0x20008000;

SECTIONS
{
    .vectors ORIGIN(STUB) : { KEEP(*(.vectors)); } > STUB
    .text : { *(.text .text.*); *(.rodata .rodata.*); . = ALIGN(4); } > STUB
    .data : { *(.data .data.*); . = ALIGN(4); } > STUB
    .bss (NOLOAD) : { __bss_start = .; *(.bss .bss.* COMMON); . = ALIGN(4); __bss_end = .; } > STUB
    /DISCARD/ : { *(.ARM.exidx*); *(.ARM.extab*); *(.comment); }
}
