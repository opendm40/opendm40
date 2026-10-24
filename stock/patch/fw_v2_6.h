/* Firmware V2.6 functions and data, addresses in fw_v2_6.S. */
#ifndef DM40_FW_V2_6_H
#define DM40_FW_V2_6_H

#include <stdint.h>

/* C++ static constructors, called from the reset handler */
void fw_cpp_init(void);

/* Fill version info: [6] = firmware major, [7] = firmware minor */
void fw_get_version(uint8_t info[10]);

/* Build the 11-byte measurement (BT command 9 payload) from the multimeter page */
void fw_build_measurement(void *page, uint8_t out[11]);

/* memcpy(dst, src, n) from the C library of the firmware */
void *fw_memcpy(void *dst, const void *src, uint32_t n);

/* Packet parser called by the BT receive handler with the handler frame */
int fw_parse(void *frame);

/* UART4 (BT bridge) byte send / ready-poll, base 0x40004c00 */
void fw_uart_put(uint32_t base, uint8_t b);
int fw_uart_ready(uint32_t base, uint32_t bit);

/* Parsed packet struct: [0]=hdr, [3]=cmd, [4]=len, [5..8]=data ptr */
#define FW_G ((volatile uint8_t *)0x20025a20)

#endif
