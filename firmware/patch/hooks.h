/* Hook descriptors, collected by patcher.py from the .hooks section. */
#ifndef DM40_HOOKS_H
#define DM40_HOOKS_H

#include <stdint.h>

#define HOOK_BL    1u   /* write "bl target" at addr (4 bytes) */
#define HOOK_BW    2u   /* write "b.w target" at addr (4 bytes) */
#define HOOK_WORD  3u   /* write 32-bit target value at addr (vector, pointer, literal) */

struct dm40_hook {
    uint32_t type;
    uint32_t addr;
    const void *target;
    uint32_t expect;    /* original 4 bytes at addr (little-endian), 0 = do not check */
};

#define HOOK_CAT2(a, b) a##b
#define HOOK_CAT(a, b) HOOK_CAT2(a, b)

#define DM40_HOOK(type_, addr_, target_, expect_) \
    __attribute__((used, section(".hooks"))) \
    static const struct dm40_hook HOOK_CAT(hook_, __LINE__) = { (type_), (addr_), (const void *)(target_), (expect_) }

#define HOOK_CALL(addr, fn, expect)    DM40_HOOK(HOOK_BL, addr, fn, expect)
#define HOOK_JUMP(addr, fn, expect)    DM40_HOOK(HOOK_BW, addr, fn, expect)
#define HOOK_POINTER(addr, fn, expect) DM40_HOOK(HOOK_WORD, addr, fn, expect)

#endif
