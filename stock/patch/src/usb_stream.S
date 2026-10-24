/*
 * DM40 live-measurement over USB (vendor control request on EP0).
 *
 * Two flow hooks (installed by usb_hooks.c):
 *   - meter_hook: tail-hook on the multimeter page update. Each frame it
 *     refreshes a measurement snapshot and, once, enables USB with our own
 *     vendor-class descriptors so a PC can talk to it while the meter runs.
 *   - vhook: EP0 SETUP dispatch hook. Answers our vendor IN request
 *     (bmRequestType 0xC0, bRequest 0x01) with the snapshot.
 *
 * USB is brought up with usb_enable_fast() below instead of the stock
 * usb_enable (0x08016aac): same hardware setup, but without the 500 ms
 * re-enumeration delay and with the independent watchdog fed, so it does not
 * block the UI thread long enough to trip the watchdog reboot.
 *
 * Firmware V2.6 only. All firmware addresses are Thumb.
 */
    .syntax unified
    .thumb
    .cpu cortex-m4

/* ---- firmware V2.6 entry points (odd = Thumb, for blx) ---- */
    .equ FW_BUILD_MEAS,  0x0803c98f   @ build_measurement(page, out[11])
    .equ FW_CTL_SEND,    0x080179e7   @ ctl_send(udev, buf, len)
    .equ FW_EP_STALL,    0x08017a55   @ ep_stall(udev, epaddr)
    .equ FW_METER_UPD,   0x0803cdd9    @ multimeter page update (tail target)
    .equ FW_EP0_RET,     0x080181e7    @ SETUP dispatch tail

/* sub-routines used to replicate usb_enable (0x08016aac) */
    .equ FW_CLK,         0x0800670d    @ clock enable/config(r0)
    .equ FW_GPIO_MODE,   0x08005f4f    @ gpio(base, a, b, mask)
    .equ FW_GPIO_PULL,   0x08005fa1
    .equ FW_GPIO_X1,     0x08005ff3
    .equ FW_GPIO_AF,     0x08006021
    .equ FW_CFG_AFD0,    0x0800afd1
    .equ FW_SYSCFG_A,    0x080069a9
    .equ FW_SYSCFG_B,    0x08006953
    .equ FW_SYSCFG_C,    0x080068e7
    .equ FW_USBD_INIT,   0x080176e5    @ usbd_init(udev, 1)
    .equ FW_NVIC_EN,     0x08006453    @ nvic_enable(irq, prio, sub)
    .equ FW_DELAY_MS,    0x0800966f    @ delay_ms(r0)
    .equ FW_IWDG_KICK,   0x08005dfd    @ reload the independent watchdog
    .equ FW_USB_ENABLE,  0x08016aad    @ stock usb_enable (used by USB-Connect)
    .equ DEVDESC_ORIG,   0x0804a5ac    @ original MSC device descriptor
    .equ CFGDESC_ORIG,   0x0804a5be    @ original MSC config descriptor

/* ---- firmware RAM / peripherals ---- */
    .equ UDEV,           0x20020128
    .equ USB_ON,         0x20020124
    .equ GPIOA,          0x40020000
    .equ RCU_ADDCTL,     0x40023800
    .equ RCU_RSTSCK,     0x40023874    @ reset source flags (FWDGTRSTF=bit29, RSTFC=bit24)
    .equ SNAP,           0x20027c74
    .equ SNAP_LEN,       11

/* =========================================================================
 * meter_hook: replaced the tail "b.w FW_METER_UPD" at 0x0803e256.
 * r0 = multimeter page object.
 * ========================================================================= */
    .global meter_hook
    .thumb_func
meter_hook:
    push    {r4, lr}
    mov     r4, r0
    ldr     r1, =SNAP
    mov     r0, r4
    ldr     r3, =FW_BUILD_MEAS
    blx     r3
    ldr     r0, =USB_ON
    ldrb    r0, [r0]
    cbnz    r0, 1f
    @ auto-rollback: if the previous boot ended in a free-watchdog reset
    @ (i.e. the feature hung last time), skip enabling USB this boot so the
    @ device comes up normally. Clear the flag; a clean power-on re-enables.
    ldr     r2, =RCU_RSTSCK
    ldr     r0, [r2]
    tst.w   r0, #0x20000000         @ FWDGTRSTF
    beq     2f
    orr     r0, r0, #0x01000000     @ RSTFC: clear reset flags
    str     r0, [r2]
    b       1f
2:
    bl      usb_enable_fast
    ldr     r0, =UDEV
    ldr     r1, =my_dev_desc
    str     r1, [r0, #0xfc]
    ldr     r1, =my_cfg_desc
    str     r1, [r0, #0x100]
1:
    mov     r0, r4
    pop     {r4, lr}
    ldr     r3, =FW_METER_UPD
    bx      r3
    .ltorg

/* =========================================================================
 * usb_enable_fast: faithful copy of usb_enable (0x08016aac) minus the 500 ms
 * delay_ms disconnect, with the watchdog fed at entry, in the clock-ready
 * wait, and at the end. Sets USB_ON = 1.
 * ========================================================================= */
    .thumb_func
usb_enable_fast:
    push    {r4, r5, r6, lr}
    ldr     r3, =FW_IWDG_KICK
    blx     r3

    movw    r0, #0xc00
    ldr     r3, =FW_CLK
    blx     r3

    ldr     r4, =GPIOA
    movw    r3, #0x1800
    movs    r2, #2
    movs    r1, #1
    mov     r0, r4
    ldr     r5, =FW_GPIO_MODE
    blx     r5
    movw    r3, #0x1800
    movs    r2, #3
    movs    r1, #0
    mov     r0, r4
    ldr     r5, =FW_GPIO_PULL
    blx     r5
    movs    r2, #0
    movw    r1, #0x1800
    mov     r0, r4
    ldr     r5, =FW_GPIO_X1
    blx     r5
    ldr     r0, =0x0a037a00
    ldr     r5, =FW_CFG_AFD0
    blx     r5

    @ (stock usb_enable does delay_ms(500) here for USB re-detect; skipped)

    movw    r0, #0x110e
    ldr     r3, =FW_CLK
    blx     r3
    movw    r0, #0xc00
    ldr     r3, =FW_CLK
    blx     r3
    movw    r3, #0x1800
    movs    r2, #0
    movs    r1, #2
    mov     r0, r4
    ldr     r5, =FW_GPIO_MODE
    blx     r5
    movw    r3, #0x1800
    movs    r2, #3
    movs    r1, #0
    mov     r0, r4
    ldr     r5, =FW_GPIO_PULL
    blx     r5
    movw    r2, #0x1800
    movs    r1, #0xa
    mov     r0, r4
    ldr     r5, =FW_GPIO_AF
    blx     r5
    movs    r0, #2
    ldr     r5, =FW_SYSCFG_A
    blx     r5
    movs    r0, #0
    ldr     r5, =FW_SYSCFG_B
    blx     r5
    movs    r3, #2
    mov     r2, r3
    mov     r1, r3
    movs    r0, #0x60
    ldr     r5, =FW_SYSCFG_C
    blx     r5

    @ enable the 48 MHz USB clock and wait for ready, feeding the watchdog
    ldr     r6, =RCU_ADDCTL
    ldr     r0, [r6]
    orr     r0, r0, #0x10000000
    str     r0, [r6]
    movw    r5, #0xbb8
2:
    ldr     r0, [r6]
    tst.w   r0, #0x20000000
    bne     3f
    ldr     r3, =FW_IWDG_KICK
    blx     r3
    movs    r0, #1
    ldr     r3, =FW_DELAY_MS
    blx     r3
    subs    r5, r5, #1
    bne     2b
3:
    movw    r0, #0xd07
    ldr     r3, =FW_CLK
    blx     r3
    movs    r4, #1
    mov     r1, r4
    ldr     r0, =UDEV
    ldr     r3, =FW_USBD_INIT
    blx     r3
    movs    r2, #0
    movs    r1, #2
    movs    r0, #0x43
    ldr     r3, =FW_NVIC_EN
    blx     r3
    ldr     r3, =FW_IWDG_KICK
    blx     r3
    ldr     r0, =USB_ON
    strb    r4, [r0]
    pop     {r4, r5, r6, pc}
    .ltorg

/* =========================================================================
 * hook_msc: replaces "bl usb_enable" in the USB-Connect path (0x080243be and
 * 0x08041ebe). Restores the original MSC descriptors (our meter hook leaves
 * the vendor ones in udev, and usbd_init does not reset them) so the disk mode
 * enumerates normally, then calls the stock usb_enable.
 * ========================================================================= */
    .global hook_msc
    .thumb_func
hook_msc:
    push    {lr}
    ldr     r0, =UDEV
    ldr     r1, =DEVDESC_ORIG
    str     r1, [r0, #0xfc]
    ldr     r1, =CFGDESC_ORIG
    str     r1, [r0, #0x100]
    ldr     r3, =FW_USB_ENABLE
    blx     r3
    pop     {pc}
    .ltorg

/* =========================================================================
 * vhook: replaced "cmp r1,#0x40; itt ne" at 0x080181da.
 * r0 = udev, r1 = type field, r2 = bmRequestType, setup at udev+0x24.
 * ========================================================================= */
    .global vhook
    .thumb_func
vhook:
    cmp     r1, #0x40
    bne     .Lstd
    ldrb    r3, [r0, #0x25]         @ bRequest
    cmp     r3, #1
    bne     .Ldone
    tst     r2, #0x80              @ IN?
    beq     .Ldone
    ldr     r1, =SNAP
    movs    r2, #SNAP_LEN
    ldr     r3, =FW_CTL_SEND
    blx     r3
.Ldone:
    ldr     r3, =FW_EP0_RET
    bx      r3
.Lstd:
    and     r1, r2, #0x80
    ldr     r3, =FW_EP_STALL
    blx     r3
    ldr     r3, =FW_EP0_RET
    bx      r3
    .ltorg

/* =========================================================================
 * Vendor-class descriptor copies served from patch flash.
 * ========================================================================= */
    .section .rodata
    .align 2
    .global my_dev_desc
my_dev_desc:
    .byte 0x12, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x40
    .byte 0xe9, 0x28, 0xf0, 0x02, 0x00, 0x02, 0x01, 0x02
    .byte 0x03, 0x01
    .align 2
    .global my_cfg_desc
my_cfg_desc:
    .byte 0x09, 0x02, 0x20, 0x00, 0x01, 0x01, 0x00, 0xc0, 0x32
    .byte 0x09, 0x04, 0x00, 0x00, 0x02, 0xff, 0x00, 0x00, 0x00
    .byte 0x07, 0x05, 0x81, 0x02, 0x40, 0x00, 0x00
    .byte 0x07, 0x05, 0x01, 0x02, 0x40, 0x00, 0x00
