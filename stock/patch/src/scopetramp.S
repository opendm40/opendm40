/*
 * Scope-page poll trampoline (EXPERIMENTAL scope-debug build).
 *
 * The stock scope page's 100 ms timer dispatch thunk at 0x080282a0 is:
 *     080282a0: ldr r0,[pc,#0x354]   ; r0 = *0x080285f8 = 0x20020548
 *     080282a2: ldr r0,[r0]          ; r0 = *0x20020548 (GUI manager)
 *     080282a4: ldr.w r0,[r0,#0x12c] ; r0 = scope state object
 *     080282a8: ldr r1,[r0]          ; r1 = vtable
 *     080282aa: ldr r1,[r1,#0x54]    ; r1 = poll method
 *     080282ac: bx  r1               ; tail-call
 *
 * We overwrite the first 4 bytes (080282a0/080282a2) with `b.w patch_scopepoll'
 * (HOOK_JUMP in scopedbg.c). This function:
 *   - flag (arm word) != MAGIC  -> do nothing new; reproduce exactly the two
 *     overwritten instructions (r0 = *(*0x080285f8)) and continue at 0x080282a4
 *     so the scope poll runs bit-identically to stock.
 *   - flag == MAGIC             -> call scope_work() (pump the BT parser and
 *     play one queued touch entry), preserving all caller state, then fall into
 *     the same passthrough.
 *
 * The arm word lives in verified-mapped SRAM0+1 and is cleared to 0 by
 * patch_init() at boot, so after any reset the flag != MAGIC and this is pure
 * passthrough until a BT command arms it from the DMM page.
 */
    .syntax unified
    .thumb
    .cpu cortex-m4

    .equ SCOPE_STATE,   0x2000f100      /* struct scope_state (arm word first) */
    .equ ARM_MAGIC,     0x5C09EDB6
    .equ ORIG_LITERAL,  0x080285f8      /* the thunk's literal pool word addr  */
    .equ CONT_THUMB,    0x080282a5      /* resume at 0x080282a4 (Thumb)        */

    .global patch_scopepoll
    .type patch_scopepoll, %function
    .thumb_func
patch_scopepoll:
    push    {r2, r3}                    @ scratch, keep the stack balanced
    ldr     r2, =SCOPE_STATE
    ldr     r3, [r2]                    @ arm word
    ldr     r2, =ARM_MAGIC
    cmp     r3, r2
    bne     1f                          @ not armed -> passthrough only
    push    {r0, r1, lr}
    bl      scope_work                  @ pump parser + play injection
    pop     {r0, r1, lr}
1:
    pop     {r2, r3}                    @ restore scratch (stack net zero)
    ldr     r0, =ORIG_LITERAL
    ldr     r0, [r0]                    @ r0 = 0x20020548  (== 0x080282a0 did)
    ldr     r0, [r0]                    @ r0 = *0x20020548 (== 0x080282a2 did)
    ldr     r1, =CONT_THUMB
    bx      r1                          @ continue the stock thunk at 0x080282a4
    .size patch_scopepoll, . - patch_scopepoll
