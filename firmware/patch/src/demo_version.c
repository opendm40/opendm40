/*
 * Demo patch: report firmware version 9.9 on the About page and in the
 * Bluetooth version command (02), to check that a patched image runs.
 */
#include <stdint.h>

#include "../hooks.h"
#include "../fw_v2_6.h"

static void patched_get_version(uint8_t info[10])
{
    fw_get_version(info);
    info[6] = 9;
    info[7] = 9;
}

/* 0800cf92: BT command 02 handler, 08040484: About page */
HOOK_CALL(0x0800cf92, patched_get_version, 0xf81ef7fe);
HOOK_CALL(0x08040484, patched_get_version, 0xfda5f7ca);
