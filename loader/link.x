/* Sector 0 only. No .data, no .bss: all state lives on the stack. */
MEMORY {
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 16K
    RAM (rw)   : ORIGIN = 0x20000000, LENGTH = 4K
}
_stack_top = ORIGIN(RAM) + LENGTH(RAM);
ENTRY(loader_main);
SECTIONS {
    .vectors : { KEEP(*(.vectors)) } > FLASH
    .text : { *(.text .text.*) } > FLASH
    .rodata : { *(.rodata .rodata.*) } > FLASH
    .data : { *(.data .data.*) } > RAM AT > FLASH
    .bss (NOLOAD) : { *(.bss .bss.*) } > RAM
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "statics need a startup copy: none here");
