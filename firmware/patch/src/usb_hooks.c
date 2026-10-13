/*
 * Install the USB live-measurement hooks (code in usb_stream.S).
 *
 * Firmware V2.6:
 *   0x0803e256  b.w 0x0803cdd8   -> b.w meter_hook   (multimeter update tail)
 *   0x080181da  cmp r1,#0x40; itt ne -> b.w vhook    (EP0 SETUP dispatch)
 */
#include "../hooks.h"

extern void meter_hook(void);
extern void vhook(void);
extern void hook_msc(void);

/* 0x0803e256: fe f7 bf bd  (b.w 0x0803cdd8) */
HOOK_JUMP(0x0803e256, meter_hook, 0xbdbff7fe);

/* 0x080181da: 40 29 1c bf  (cmp r1,#0x40 ; itt ne) */
HOOK_JUMP(0x080181da, vhook, 0xbf1c2940);

/* USB-Connect (disk) path: restore the original MSC descriptors before
 * enabling USB. Both call sites of the stock usb_enable. */
HOOK_CALL(0x080243be, hook_msc, 0xfb75f7f2);   /* f2 f7 75 fb */
HOOK_CALL(0x08041ebe, hook_msc, 0xfdf5f7d4);   /* d4 f7 f5 fd */
