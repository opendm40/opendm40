/*
 * Arbitrary memory read/write + code execution over Bluetooth: BT command 0x0B.
 *
 * Request:  AF 05 03 0B LEN [addr:4 LE][sub:1][n:1][payload n] CRC
 *   sub 0: read n bytes at addr  -> AF 05 03 0C n <data> CRC
 *   sub 1: write payload to addr  -> AF 05 03 0C 00 CRC (ack)
 *   sub 2: call function at addr with r0..r3 = a,b,c,d (LE words),
 *          answer 4 bytes: r0 -> AF 05 03 0C 04 <r0 LE> CRC.
 *          The called code must return normally (bx lr) and keep the
 *          BT stack sane (no big stack use, no interrupts disabled).
 *
 * Keep request LEN <= 0x97 so the parser memcpy does not reach the saved LR
 * of the BT receive handler: the device stays alive between requests.
 * Write payload must satisfy n + 6 <= LEN <= 0x97 (n <= 0x91).
 *
 * Hook: 0x0803cec0 `bl 0x800d2fa' (parser call of the BT receive handler).
 */
#include <stdint.h>

#include "../hooks.h"
#include "../fw_v2_6.h"

#define UART4_BASE  0x40004c00u

typedef uint32_t (*fw_fn4_t)(uint32_t, uint32_t, uint32_t, uint32_t);

static void uart_send(const uint8_t *p, unsigned n)
{
    while (n--) {
        fw_uart_put(UART4_BASE, *p++);
        while (fw_uart_ready(UART4_BASE, 7) == 0)
            ;
    }
}

static void respond(const uint8_t *data, uint8_t n)
{
    uint8_t hdr[5] = { 0xAF, 0x05, 0x03, 0x0C, n };
    uint8_t crc = 0x100 - hdr[0] - hdr[1] - hdr[2] - hdr[3] - hdr[4];

    for (unsigned i = 0; i < n; i++)
        crc -= data[i];
    uart_send(hdr, 5);
    uart_send(data, n);
    uart_send(&crc, 1);
}

__attribute__((section(".text.patch_memrw")))
static void patch_parse(void *frame)
{
    fw_parse(frame);

    const volatile uint8_t *G = FW_G;
    if (G[3] != 0x0B)                    /* cmd */
        return;

    const uint8_t *d = *(const uint8_t *const volatile *)(G + 5);  /* data ptr */
    uint32_t addr = *(const uint32_t *)(const void *)&d[0];
    uint8_t sub = d[4];
    uint8_t n = d[5];

    if (sub == 0) {                      /* read */
        respond((const uint8_t *)addr, n);
    } else if (sub == 1) {              /* write */
        fw_memcpy((void *)addr, (const void *)&d[6], n);
        respond((const uint8_t *)"", 0);
    }
}

/* 0803cec0: bl 0x800d2fa (parser) in the BT receive handler */
HOOK_CALL(0x0803cec0, patch_parse, 0xfa1bf7d0);
__attribute__((section(".text.patch_pad"),used))
static const uint8_t patch_pad[4] = {0x00,0xbf,0x00,0xbf}; /* nop;nop */
