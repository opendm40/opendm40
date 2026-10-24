/*
 * Stock remote-debug over Bluetooth: BT command 0x0B, superset of memrw.
 *
 * This is the DEBUG variant handler. It is a strict superset of src/memrw.c:
 * sub-commands 0/1/2 are byte-for-byte the same behaviour as the plain memrw
 * patch, so the plain memrw image (src/memrw.c) is kept separate and stays
 * byte-identical. The debug build (`make dbg`) uses THIS file instead.
 *
 * Everything here runs ONLY from the BT command handler, on an explicit
 * command. There is NO new always-executed code: like memrw, the only
 * modified instruction in the stock image is the single `bl` at the parser
 * call site (0x0803cec0), and our code runs exactly when a BT frame is
 * processed. A reset leaves the stock 100 % stock until the first BT frame.
 *
 * Because the parser is pumped only by the DMM-page update loop
 * (0x0803cdd8; the UART4 ISR 0x0800d566 only buffers bytes), the whole BT
 * command interface - memrw AND these new sub-commands - is serviced only
 * while the MULTIMETER page is on screen. Screenshots therefore capture the
 * DMM page. Scope-page capture / touch injection need a scope-page hook and
 * live in a separate experimental build (see README, src/scopedbg.c).
 *
 * Request:  AF 05 03 0B LEN [addr:4 LE][sub:1][n:1][payload n] CRC
 *   sub 0: read n bytes at addr                 -> AF 05 03 0C n <data> CRC
 *   sub 1: write payload to addr                -> AF 05 03 0C 00 CRC (ack)
 *   sub 2: call addr(r0..r3), result at 0x2000f100 (broken, kept for parity)
 *   sub 3: LCD raw read-back. addr = (dy<<16)|dx (ILI9488 DRIVER coords,
 *          320 x 480 portrait as the stock configures the panel). Read n
 *          pixels along +dx at driver row dy; n <= 125.
 *          -> AF 05 03 0C (2*n) <n x RGB565 LE> CRC
 *   sub 4: LCD RLE read-back. Same addr/params, n <= 255 pixels attempted.
 *          The device run-length-encodes RGB565 and stops when the reply
 *          would exceed the frame budget.
 *          -> AF 05 03 0C m <consumed:1><(count:1,color:2 LE) * runs> CRC
 *             m = 1 + 3*runs, consumed = pixels actually encoded (<= n).
 *
 * Keep request LEN <= 0x97 (parser memcpy must not reach the saved LR of the
 * BT receive handler). sub 3/4 requests are tiny (6 bytes of data).
 *
 * Hook: 0x0803cec0 `bl 0x800d2fa' (parser call of the BT receive handler).
 */
#include <stdint.h>

#include "../hooks.h"
#include "../fw_v2_6.h"

#define UART4_BASE  0x40004c00u
#define CALL_RESULT 0x2000f100u     /* scratch SRAM (verified mapped) */

/* Stock per-pixel GRAM read: uint16_t read_pixel(uint32_t x, uint32_t y),
 * returns RGB565. Sets the 0x2A/0x2B window, issues 0x2E, drops the dummy
 * word and converts the ILI9488 RGB666 read to RGB565 (0x0800a6aa). This is
 * the stock's own screenshot primitive (display vtable slot 3), so it uses
 * the exact coordinate/colour convention the stock uses for itself. */
typedef uint16_t (*fw_rdpix_t)(uint32_t, uint32_t);
#define FW_LCD_READ_PIXEL ((fw_rdpix_t)0x0800a6abu)

typedef uint32_t (*fw_fn4_t)(uint32_t, uint32_t, uint32_t, uint32_t);

/* Reply budget: keep the whole response frame well under the 0x97/0xff limit.
 * sub 3 -> 2*125 = 250 data bytes; sub 4 -> up to 1 + 3*RUNS_MAX bytes. */
#define RAW_PIX_MAX  125u
#define RLE_OUT_MAX  250u            /* max data bytes placed in the reply */

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

/* RGB565 span read into out[], left to right from driver (dx, dy). */
static void lcd_read_span(uint16_t dx, uint16_t dy, uint8_t n, uint16_t *out)
{
    for (uint8_t i = 0; i < n; i++)
        out[i] = FW_LCD_READ_PIXEL((uint32_t)(dx + i), (uint32_t)dy);
}

__attribute__((section(".text.patch_stockdbg")))
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

    if (sub == 0) {                      /* read (identical to memrw) */
        respond((const uint8_t *)addr, n);
    } else if (sub == 1) {              /* write (identical to memrw) */
        fw_memcpy((void *)addr, (const void *)&d[6], n);
        respond((const uint8_t *)"", 0);
    } else if (sub == 2) {              /* call (identical to memrw, broken) */
        uint32_t r0, r1, r2, r3, ret;
        r0 = *(const uint32_t *)(const void *)&d[6];
        r1 = *(const uint32_t *)(const void *)&d[10];
        r2 = *(const uint32_t *)(const void *)&d[14];
        r3 = *(const uint32_t *)(const void *)&d[18];
        ret = ((fw_fn4_t)(addr | 1u))(r0, r1, r2, r3);
        *(volatile uint32_t *)CALL_RESULT = ret;
    } else if (sub == 3) {              /* LCD raw span -> RGB565 */
        uint16_t px[RAW_PIX_MAX];
        uint16_t dx = (uint16_t)(addr & 0xffffu);
        uint16_t dy = (uint16_t)(addr >> 16);
        uint8_t k = n > RAW_PIX_MAX ? (uint8_t)RAW_PIX_MAX : n;
        lcd_read_span(dx, dy, k, px);
        respond((const uint8_t *)px, (uint8_t)(2u * k));
    } else if (sub == 4) {              /* LCD RLE span -> RGB565 runs */
        uint8_t out[1u + RLE_OUT_MAX];
        uint16_t dx = (uint16_t)(addr & 0xffffu);
        uint16_t dy = (uint16_t)(addr >> 16);
        unsigned olen = 1;              /* out[0] = consumed, filled at end */
        unsigned committed = 0;         /* pixels whose runs are in the reply */
        unsigned i = 0;                 /* pixels examined */
        uint16_t cur = 0;
        unsigned run = 0;

        /* committed advances only when a run is actually written out, so the
         * host can always resume at dx + committed with no gap or overlap. */
        while (i < n) {
            uint16_t c = FW_LCD_READ_PIXEL((uint32_t)(dx + i), (uint32_t)dy);
            if (run && (c != cur || run >= 255u)) {
                if (olen + 3u > 1u + RLE_OUT_MAX)
                    break;              /* no room: stop before examining c */
                out[olen++] = (uint8_t)run;
                out[olen++] = (uint8_t)(cur & 0xffu);
                out[olen++] = (uint8_t)(cur >> 8);
                committed += run;
                run = 0;
            }
            if (run == 0) {
                cur = c; run = 1;
            } else {
                run++;                  /* c == cur */
            }
            i++;
        }
        /* flush the tail run if it fits */
        if (run && olen + 3u <= 1u + RLE_OUT_MAX) {
            out[olen++] = (uint8_t)run;
            out[olen++] = (uint8_t)(cur & 0xffu);
            out[olen++] = (uint8_t)(cur >> 8);
            committed += run;
        }
        out[0] = (uint8_t)committed;
        respond(out, (uint8_t)olen);
    }
}

/* 0803cec0: bl 0x800d2fa (parser) in the BT receive handler */
HOOK_CALL(0x0803cec0, patch_parse, 0xfa1bf7d0);
