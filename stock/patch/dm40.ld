/*
 * Patch layout: code goes after the end of the original image, variables
 * into RAM not used by the original firmware.
 *
 * PRAM is a placeholder: free RAM of the original firmware is not verified yet.
 */
INCLUDE fw_v2_6.ld

MEMORY
{
    PFLASH (rx)  : ORIGIN = 0x0807d000, LENGTH = 0x080a0000 - 0x0807d000
    PRAM   (rwx) : ORIGIN = 0x2002f800, LENGTH = 0x800
}

SECTIONS
{
    .text : {
        __patch_flash_start = .;
        KEEP(*(.text.patch_startup))
        *(.text .text.*)
        *(.rodata .rodata.*)
        . = ALIGN(4);
    } > PFLASH

    .data : {
        __patch_data_start = .;
        *(.data .data.*)
        . = ALIGN(4);
        __patch_data_end = .;
    } > PRAM AT > PFLASH
    __patch_data_load = LOADADDR(.data);

    .bss (NOLOAD) : {
        __patch_bss_start = .;
        *(.bss .bss.* COMMON)
        . = ALIGN(4);
        __patch_bss_end = .;
    } > PRAM

    __patch_flash_end = LOADADDR(.data) + SIZEOF(.data);

    /* Not loaded, read by patcher.py */
    .hooks 0 (INFO) : { KEEP(*(.hooks)) }

    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.attributes) *(.comment) }
}
