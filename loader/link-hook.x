/* In the free part of the stock sector 0, entered by the patched BL at 0x08000660.
   Ends at the fixed calibration tail 0x08003CC0: the link fails beyond it. */
MEMORY {
    FLASH (rx) : ORIGIN = 0x08001974, LENGTH = 0x08003CC0 - 0x08001974
    RAM (rw)   : ORIGIN = 0x20000000, LENGTH = 8K
}
ENTRY(hook_entry);
EXTERN(hook_entry);
SECTIONS {
    .text : { KEEP(*(.entry)) *(.text .text.*) } > FLASH
    .rodata : { *(.rodata .rodata.*) } > FLASH
    .data : { *(.data .data.*) } > RAM AT > FLASH
    .bss (NOLOAD) : { *(.bss .bss.*) } > RAM
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.vectors) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "statics need a startup copy: none here");
