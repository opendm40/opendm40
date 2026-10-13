/*
 * Patch startup: the reset handler of the original firmware calls
 * fw_cpp_init() right before main(); that call is redirected here to set up
 * patch variables first.
 */
#include <stdint.h>

#include "../hooks.h"
#include "../fw_v2_6.h"

extern uint32_t __patch_data_start[], __patch_data_end[], __patch_data_load[];
extern uint32_t __patch_bss_start[], __patch_bss_end[];

void patch_init(void) __attribute__((weak));
void patch_init(void) {}

__attribute__((section(".text.patch_startup")))
void patch_startup(void)
{
    uint32_t *src = __patch_data_load;
    for (uint32_t *p = __patch_data_start; p < __patch_data_end; p++)
        *p = *src++;
    for (uint32_t *p = __patch_bss_start; p < __patch_bss_end; p++)
        *p = 0;

    patch_init();
    fw_cpp_init();
}

/* 080041b4: bl fw_cpp_init */
HOOK_CALL(0x080041b4, patch_startup, 0xfbaef041);
