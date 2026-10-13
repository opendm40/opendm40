/*
 * Arbitrary memory read/write + code execution over Bluetooth: BT command 0x0B.
 *
 * Request:  AF 05 03 0B LEN [addr:4 LE][sub:1][n:1][payload n] CRC
 *   sub 0: read n bytes at addr  -> AF 05 03 0C n <data> CRC
 *   sub 1: write payload to addr  -> AF 05 03 0C 00 CRC (ack)
 *   sub 2: call function at addr with r0..r3 = a,b,c,d (LE words).
 *          NO response is sent (the respond-after-call path hard-faults
 *          the BT handler on this device); the return value r0 is stored
 *          at CALL_RESULT (0x2002f800) - read it back with sub 0.
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
#define CALL_RESULT 0x2000f100u     /* scratch SRAM (verified mapped); the
                                       patch .data area 0x2002f800 is OUTSIDE
                                       the real RAM of this chip -> bus fault
                                       (sram probe faulted at 0x2002ffff) */

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
    } else if (sub == 2) {              /* call: [addr:4][02][00][r0:4][r1:4][r2:4][r3:4]
                                           The return value r0 is stored at
                                           CALL_RESULT (0x2000f100), read it
                                           back with sub 0. No response is sent
                                           from the call path. */
        uint32_t r0, r1, r2, r3, ret;
        r0 = *(const uint32_t *)(const void *)&d[6];
        r1 = *(const uint32_t *)(const void *)&d[10];
        r2 = *(const uint32_t *)(const void *)&d[14];
        r3 = *(const uint32_t *)(const void *)&d[18];
        ret = ((fw_fn4_t)(addr | 1u))(r0, r1, r2, r3);
        *(volatile uint32_t *)CALL_RESULT = ret;
    }
}

/* 0803cec0: bl 0x800d2fa (parser) in the BT receive handler */
HOOK_CALL(0x0803cec0, patch_parse, 0xfa1bf7d0);