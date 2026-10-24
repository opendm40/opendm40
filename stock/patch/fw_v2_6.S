/* Firmware V2.6 functions (Thumb, bit 0 set) and data addresses */
    .syntax unified
    .thumb

.macro FW_FUNC name, addr
    .global \name
    .type \name, %function
    .set \name, \addr
.endm

FW_FUNC fw_cpp_init,          0x08045915
FW_FUNC fw_get_version,       0x0800afd3
FW_FUNC fw_build_measurement, 0x0803c98f
FW_FUNC fw_memcpy,            0x080457f9
FW_FUNC fw_parse,             0x0800d2fb
FW_FUNC fw_uart_put,          0x080082a1
FW_FUNC fw_uart_ready,        0x0800849d
