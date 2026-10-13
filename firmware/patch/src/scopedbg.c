/*
 * EXPERIMENTAL scope-page remote debug over Bluetooth.
 *
 * Superset of src/stockdbg.c. Adds, on top of the DMM-page BT handler:
 *   - a scope-page poll hook (src/scopetramp.S, at the scope 100 ms timer thunk
 *     0x080282a0) that, ONLY when armed, pumps the BT parser so screenshots and
 *     memory reads work while the scope page is shown, and plays queued touch
 *     injection into the stock touch queue (0x2002561c);
 *   - BT cmd 0x0B sub 5 (program a touch-injection sequence) and sub 6
 *     (arm/disarm the scope hook).
 *
 * Safety model (see firmware/patch/README.md):
 *   - The scope hook's armed state is a MAGIC word in verified-mapped SRAM0+1
 *     (0x2000f100), cleared to 0 by patch_init() at boot, so after EVERY reset
 *     the hook is a bit-identical passthrough of the stock thunk until a BT
 *     command arms it from the DMM page. A bug in the armed path costs at most
 *     a reboot.
 *   - The scope page (and this hook) is reachable only by navigating into the
 *     scope page from the menu (build fn 0x080282ae, called from 0x08024148 /
 *     0x080242e6); it is NOT on the boot path, the main loop, or Settings.
 *   - The plain memrw image and dm40_dbg.bin are built separately and are
 *     unaffected by this file.
 *
 * Build:  make dbg-scope  ->  build-dbg/dm40_dbgscope.bin
 */
#include <stdint.h>

#include "../hooks.h"
#include "../fw_v2_6.h"

#define UART4_BASE  0x40004c00u
#define CALL_RESULT 0x2000f100u         /* (unused here; sub 2 kept for parity) */

/* stock entry points used here */
typedef uint16_t (*fw_rdpix_t)(uint32_t, uint32_t);
#define FW_LCD_READ_PIXEL ((fw_rdpix_t)0x0800a6abu)
typedef int (*fw_ready_t)(void);
#define FW_FRAME_READY ((fw_ready_t)0x0800d5f7u)   /* ==1 when a BT frame ready */
typedef uint32_t (*fw_fn4_t)(uint32_t, uint32_t, uint32_t, uint32_t);

#define RAW_PIX_MAX  125u
#define RLE_OUT_MAX  250u

/* ---- patch RAM state in verified-mapped SRAM0+1 ---- */
#define ARM_MAGIC   0x5C09EDB6u
#define TP_QUEUE    0x2002561cu          /* stock touch state block */
#define INJ_MAX     24u

struct inj_pt { uint16_t flags; int16_t x; int16_t y; uint16_t rsvd; };

struct scope_state {
    uint32_t arm;                        /* == ARM_MAGIC when armed */
    uint32_t inj_count;
    uint32_t inj_idx;
    uint32_t _pad;
    struct inj_pt inj[INJ_MAX];
    uint8_t frame[0x100];                /* fw_parse scratch frame */
};
#define SST ((volatile struct scope_state *)0x2000f100u)

/* Called from patch_startup() at boot (see src/startup.c). Writes only to
 * SRAM0+1 (mapped and free this early), so the hook is disarmed after reset. */
extern void patch_scopepoll(void);       /* trampoline, src/scopetramp.S */
extern void scope_work(void);

void patch_init(void)
{
    SST->arm = 0;
    SST->inj_count = 0;
    SST->inj_idx = 0;
}

/* ---- UART reply (identical framing to memrw) ---- */
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

static void lcd_read_span(uint16_t dx, uint16_t dy, uint8_t n, uint16_t *out)
{
    for (uint8_t i = 0; i < n; i++)
        out[i] = FW_LCD_READ_PIXEL((uint32_t)(dx + i), (uint32_t)dy);
}

/* Push one entry into the stock touch queue (ring of 32 x 22 bytes at +0x10,
 * write index +0x2d6, read index +0x2d7). Drops when nearly full. */
static void touch_push(const struct inj_pt *e)
{
    volatile uint8_t *base = (volatile uint8_t *)TP_QUEUE;
    uint8_t head = base[0x2d6];
    uint8_t tail = base[0x2d7];
    if ((uint8_t)(head - tail) >= 31u)       /* leave 1 slot headroom */
        return;
    volatile int16_t *ent = (volatile int16_t *)(base + 0x10u + (head & 0x1fu) * 22u);
    ent[0] = e->x;
    ent[1] = e->y;
    for (int i = 2; i < 10; i++)
        ent[i] = 0;
    *(volatile uint16_t *)((volatile uint8_t *)ent + 20) = e->flags;
    base[0x2d6] = (uint8_t)(head + 1);
}

/* ---- shared 0x0B dispatch (DMM hook and scope hook both call this) ---- */
static void handle_cmd(void)
{
    const volatile uint8_t *G = FW_G;
    if (G[3] != 0x0B)                        /* cmd */
        return;

    const uint8_t *d = *(const uint8_t *const volatile *)(G + 5);  /* data ptr */
    uint32_t addr = *(const uint32_t *)(const void *)&d[0];
    uint8_t sub = d[4];
    uint8_t n = d[5];

    if (sub == 0) {                          /* read (memrw) */
        respond((const uint8_t *)addr, n);
    } else if (sub == 1) {                   /* write (memrw) */
        fw_memcpy((void *)addr, (const void *)&d[6], n);
        respond((const uint8_t *)"", 0);
    } else if (sub == 2) {                   /* call (memrw, broken) */
        uint32_t r0, r1, r2, r3, ret;
        r0 = *(const uint32_t *)(const void *)&d[6];
        r1 = *(const uint32_t *)(const void *)&d[10];
        r2 = *(const uint32_t *)(const void *)&d[14];
        r3 = *(const uint32_t *)(const void *)&d[18];
        ret = ((fw_fn4_t)(addr | 1u))(r0, r1, r2, r3);
        *(volatile uint32_t *)CALL_RESULT = ret;
    } else if (sub == 3) {                   /* LCD raw span -> RGB565 */
        uint16_t px[RAW_PIX_MAX];
        uint16_t dx = (uint16_t)(addr & 0xffffu);
        uint16_t dy = (uint16_t)(addr >> 16);
        uint8_t k = n > RAW_PIX_MAX ? (uint8_t)RAW_PIX_MAX : n;
        lcd_read_span(dx, dy, k, px);
        respond((const uint8_t *)px, (uint8_t)(2u * k));
    } else if (sub == 4) {                   /* LCD RLE span -> RGB565 runs */
        uint8_t out[1u + RLE_OUT_MAX];
        uint16_t dx = (uint16_t)(addr & 0xffffu);
        uint16_t dy = (uint16_t)(addr >> 16);
        unsigned olen = 1, committed = 0, i = 0;
        uint16_t cur = 0;
        unsigned run = 0;
        while (i < n) {
            uint16_t c = FW_LCD_READ_PIXEL((uint32_t)(dx + i), (uint32_t)dy);
            if (run && (c != cur || run >= 255u)) {
                if (olen + 3u > 1u + RLE_OUT_MAX)
                    break;
                out[olen++] = (uint8_t)run;
                out[olen++] = (uint8_t)(cur & 0xffu);
                out[olen++] = (uint8_t)(cur >> 8);
                committed += run;
                run = 0;
            }
            if (run == 0) { cur = c; run = 1; } else { run++; }
            i++;
        }
        if (run && olen + 3u <= 1u + RLE_OUT_MAX) {
            out[olen++] = (uint8_t)run;
            out[olen++] = (uint8_t)(cur & 0xffu);
            out[olen++] = (uint8_t)(cur >> 8);
            committed += run;
        }
        out[0] = (uint8_t)committed;
        respond(out, (uint8_t)olen);
    } else if (sub == 5) {                   /* program touch injection */
        uint8_t count = d[6];
        if (count > INJ_MAX)
            count = INJ_MAX;
        for (uint8_t k = 0; k < count; k++) {
            const uint8_t *p = &d[7] + k * 6u;
            SST->inj[k].flags = (uint16_t)(p[0] | (p[1] << 8));
            SST->inj[k].x = (int16_t)(p[2] | (p[3] << 8));
            SST->inj[k].y = (int16_t)(p[4] | (p[5] << 8));
            SST->inj[k].rsvd = 0;
        }
        SST->inj_idx = 0;
        SST->inj_count = count;
        respond(&count, 1);
    } else if (sub == 6) {                   /* arm / disarm the scope hook */
        uint8_t on = d[6];
        SST->arm = on ? ARM_MAGIC : 0u;
        {
            uint8_t st = (SST->arm == ARM_MAGIC);
            respond(&st, 1);
        }
    }
}

/* ---- DMM-page hook: parse a frame, then dispatch (same as stockdbg) ---- */
__attribute__((section(".text.patch_scopedbg")))
static void patch_parse(void *frame)
{
    fw_parse(frame);
    handle_cmd();
}

/* ---- scope-page work (called from patch_scopepoll only when armed) ---- */
void scope_work(void)
{
    if (FW_FRAME_READY() == 1) {
        fw_parse((void *)SST->frame);        /* dedicated scratch frame */
        handle_cmd();
    }
    if (SST->inj_idx < SST->inj_count) {
        struct inj_pt e = { SST->inj[SST->inj_idx].flags,
                            SST->inj[SST->inj_idx].x,
                            SST->inj[SST->inj_idx].y, 0 };
        touch_push(&e);
        SST->inj_idx++;
    }
}

/* 0803cec0: bl 0x800d2fa (parser) in the DMM-page BT handler */
HOOK_CALL(0x0803cec0, patch_parse, 0xfa1bf7d0);
/* 080282a0: scope 100 ms timer thunk: `ldr r0,[pc,#0x354]; ldr r0,[r0]'
 * (bytes d5 48 00 68) -> b.w patch_scopepoll (src/scopedbg.S) */
HOOK_JUMP(0x080282a0, patch_scopepoll, 0x680048d5);
