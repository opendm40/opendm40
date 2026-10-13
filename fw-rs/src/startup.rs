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

    .section .text.reset, "ax", %progbits
    .thumb_func
    .global reset
reset:
    cpsid i
    ldr   sp, =_stack_top
    bl    rust_start
    b     .

    .thumb_func
    .global default_handler
default_handler:
    b     .
"#
);
