//! Reset vector and startup for the RAM-loaded image.
//!
//! The `.vectors` section holds the Cortex-M table: word[0] = initial MSP,
//! word[1] = `reset`. `reset` sets MSP, disables interrupts, points VTOR at our
//! table, zeroes .bss and jumps to `rust_start` (in main.rs).

use core::arch::global_asm;

global_asm!(
    r#"
    .section .vectors, "a", %progbits
    .global __VECTORS
    .balign 512
__VECTORS:
    .word _stack_top
    .word reset
    .word default_handler      /* NMI */
    .word default_handler      /* HardFault */
    .word default_handler      /* MemManage */
    .word default_handler      /* BusFault */
    .word default_handler      /* UsageFault */
    .word 0
    .word 0
    .word 0
    .word 0
    .word default_handler      /* SVCall */
    .word default_handler      /* DebugMon */
    .word 0
    .word default_handler      /* PendSV */
    .word default_handler      /* SysTick */

    .section .boot, "ax", %progbits
    .thumb_func
    .global reset
reset:
    cpsid i
    /* TCM SRAM (stack, .bss) clock: RCU_AHB1EN.TCMSRAMEN (bit 20). */
    ldr   r0, =0x40023830
    ldr   r1, [r0]
    orr   r1, r1, #0x100000
    str   r1, [r0]
    /* FPU: CPACR CP10/CP11 full access (the stock firmware enables it too). */
    ldr   r0, =0xE000ED88
    ldr   r1, [r0]
    orr   r1, r1, #0xF00000
    str   r1, [r0]
    dsb
    isb
    ldr   sp, =_stack_top
    /* Copy .text + .rodata from flash (slow, above 512 KiB) into SRAM0. */
    ldr   r0, =__stext_lma
    ldr   r1, =__stext
    ldr   r2, =__etext
1:  cmp   r1, r2
    bhs   2f
    ldr   r3, [r0], #4
    str   r3, [r1], #4
    b     1b
2:  dsb
    isb
    ldr   r0, =rust_start
    blx   r0
    b     .

    .section .boot, "ax", %progbits
    .thumb_func
    .global default_handler
default_handler:
    b     .
"#
);
