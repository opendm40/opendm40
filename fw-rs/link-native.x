/*
 * DM40 replacement firmware, NATIVE format: the app image the FS-loader
 * installs at 0x08004000 (stock update-file body), executed in place (XIP;
 * the GD32F4 low 512 KiB of flash is zero-wait, so nothing is copied to SRAM).
 *
 * Flash (body offset = address - 0x08004000):
 *   A  0x08004000..0x08008000  16 KiB  vector table + shim (.vectors, .shim)
 *      0x08008000..0x08008010  16 B    GAP, 0xFF in the binary. The ROM and the
 *                                      FS-loader read AND write a 12-byte record
 *                                      at 0x08008000..0x0800800B (excluded from
 *                                      the update body CRC by dm40fw.py); the
 *                                      next 4 bytes are padding.
 *   B  0x08008010..0x08080000  480 KiB .text, .rodata: everything that may
 *                                      run or be read while the AD2 stream
 *                                      ISR runs. The linker fails when it
 *                                      does not fit.
 *   C  0x08080000..0x080A0000  128 KiB .cold (boot-only / rarely run code:
 *                                      calibration backup, fatren, the
 *                                      gallery, screenshots, the DLG writer,
 *                                      FAT / vdisk generation, uploads, the
 *                                      bus self-tests / AD2 experiments (DBG),
 *                                      scope CSV, boot / DBG:BOOT?) and
 *                                      the .data load image. Must end below
 *                                      0x080A0000 (FS-loader, UID data,
 *                                      calibration above).
 *   Why the split: flash from 0x08080000 up is NOT zero-wait on the GD32F4
 *   (only the low 512 KiB are). A read there stalls the bus for ~20 us, and
 *   with it the AD2 stream ISR's entry (device 5f14d38: max_lat 18.8-21.8
 *   us whenever the logger panel drew ASCII26 glyphs, which had grown past
 *   0x08080000; 4.6 us without the panel; docs/dmm-input-scope-feasibility.md
 *   10.3). Code in C costs that per call: only what never runs while a
 *   stream does (or seldom, outside the main loop's every-pass path).
 *
 * RAM:
 *   TCM     0x10000000  64 KiB  .data, .bss, stack (top 0x10010000)
 *   SRAM    0x20000000 192 KiB  .dmabuf (DMA buffers, scope record),
 *                               .sram_bss
 */
MEMORY
{
    FLASH_A (rx) : ORIGIN = 0x08004000, LENGTH = 0x4000
    FLASH_B (rx) : ORIGIN = 0x08008010, LENGTH = 0x08080000 - 0x08008010
    FLASH_C (rx) : ORIGIN = 0x08080000, LENGTH = 0x080A0000 - 0x08080000
    TCM (rw)     : ORIGIN = 0x10000000, LENGTH = 64K
    SRAM (rw)    : ORIGIN = 0x20000000, LENGTH = 192K
}

ENTRY(reset);
EXTERN(__VECTORS);

_stack_top = ORIGIN(TCM) + LENGTH(TCM);       /* 0x10010000 */
/* Word[0] of the image: only the FS-loader's `msr msp` before `bx reset` uses
 * it (reset moves SP to _stack_top at once). Same value as the stock app so
 * any check the loader applies to it passes as for stock. */
_loader_sp = 0x2002F7F8;
_marker_start = 0x08008000;
_marker_end = 0x0800800C;

SECTIONS
{
    .vectors ORIGIN(FLASH_A) :
    {
        __image_start = .;
        KEEP(*(.vectors));
        KEEP(*(.shim .shim.*));
        . = ALIGN(4);
        __region_a_end = .;
    } > FLASH_A

    /* Claimed before .text (the first matching rule wins), placed in C by
     * its region. Module paths in the v0-mangled section names:
     * <len><name> after the crate (7dm40_fw). */
    .cold ORIGIN(FLASH_C) :
    {
        __cold_start = .;
        *(.text.*7dm40_fw9calbackup*);
        *(.text.*7dm40_fw6fatren*);
        *(.text.*7dm40_fw6native*);
        *(.text.*7dm40_fw8recovery*);
        *(.text.*7dm40_fw7genuine*);
        *(.text.*7dm40_fw7gallery*);
        *(.text.*7dm40_fw10screenshot*);
        *(.text.*7dm40_fw4dlog*);
        *(.text.*7dm40_fw6fatdir*);
        *(.text.*7dm40_fw5vdisk*);
        *(.text.*7dm40_fw6upload*);
        *(.text.*7dm40_fw5hyspi*);
        *(.text.*7dm40_fw6ad2exp*);
        *(.text.*7dm40_fw9scope_csv*);
        *(.text.*7dm40_fw4boot*);
        *(.text.*7dm40_fw3app9app_press*);
        /* Inrush current: open / arm / close / results / save glue and the
         * full page (src/app_inrush.rs; app_inrush_live.rs and the page's
         * live parts stay in region B: they run while the AD2 stream does). */
        *(.text.*7dm40_fw3app10app_inrush*);
        *(.text.*7dm40_fw2ui3inr4page*);
        *(.text.*7dm40_fw2ui3inr5trace*);
        *(.text.*7dm40_fw6inrush*3Cap7results*);
        *(.text.*7dm40_fw6inrush*3Cap9linearize*);
        *(.text.*7dm40_fw6inrush*3Cap7records*);
        /* dlog's per-reading row coder is `.text.dlog_row` (src/dlog.rs),
         * not matched here: region B. So is `.text.vdisk_rd`: the USB
         * volume's per-sector read path (VDisk::read_block, the FAT / data
         * sectors, a FAST CSV's generator: Gen::read, the kind-3 decoder,
         * FatFile, flash_read; 2026-10-22: per 512-byte sector, ~6 KB). A
         * host reading the volume during a stream then costs the ISR
         * nothing, and the CSV is not generated from slow flash. upload::disk / spi and
         * vdisk::entries are inline(always) (vwrite::poll, every pass);
         * app_press's guard is inlined into App::gesture. Audit with
         * emulator hooks on every function of C, 2026-10-22 (TODO). */
        /* The fast logger's Prep / Stop (fastlog::prep: before / after the
         * logger session, never while the AD2 ISR streams); its Run and
         * fastenc.rs stay in B. */
        *(.text.*7dm40_fw7fastlog4prep*);
        *(.text.*7dm40_fw9fast_head*);
        *(.text.*7dm40_fw10fast_start*);
        *(.text.*7dm40_fw8fast_req*);
        *(.text.*7dm40_fw12log_on_check*);
        *(.text.*7dm40_fw9fast_scpi*);
        *(.text.*7dm40_fw12fast_next_hz*);
        *(.text.*7dm40_fw7fastlog5begin*);
        *(.text.*7dm40_fw7fastlog10plan_bytes*);
        *(.text.*7dm40_fw11fast_events*);
        *(.text.*7dm40_fw2ui11record_page*);
        *(.text.*7dm40_fw3app*3App11record_page*);
        *(.text.*7dm40_fw3app*3App9fast_rate*);
        *(.text.*7dm40_fw3app*3App15record_fast_tap*);
        *(.text.*7dm40_fw3app*3App16record_page_fast*);
        *(.text.*7dm40_fw3app*3App10fast_ended*);
        *(.text.*7dm40_fw3dmm*3Dmm7wd_slow*);
        /* Burden voltage SCPI (FETC:BURD?, DISP:BURD, POW:BURD's shunt). */
        *(.text.*7dm40_fw3app*3App11burden_scpi*);
        *(.text.*7dm40_fw3app*3App12burden_shunt*);
        *(.text.*7dm40_fw3dmm*3Dmm9burden_of*);
        /* NOT persist: its crc runs on every main-loop pass (device
         * f5f8e96: lat_pc in persist::crc, 9..18 us stalls). */
        /* ... and their named read-only data (tables, strings in statics). */
        *(.rodata.*7dm40_fw9calbackup*);
        *(.rodata.*7dm40_fw6fatren*);
        *(.rodata.*7dm40_fw6native*);
        *(.rodata.*7dm40_fw8recovery*);
        *(.rodata.*7dm40_fw7genuine*);
        *(.rodata.*7dm40_fw7gallery*);
        *(.rodata.*7dm40_fw10screenshot*);
        *(.rodata.*7dm40_fw4dlog*);
        *(.rodata.*7dm40_fw6fatdir*);
        *(.rodata.*7dm40_fw5vdisk*);
        *(.rodata.*7dm40_fw6upload*);
        *(.rodata.*7dm40_fw5hyspi*);
        *(.rodata.*7dm40_fw6ad2exp*);
        *(.rodata.*7dm40_fw9scope_csv*);
        *(.rodata.*7dm40_fw4boot*);
        *(.rodata.*7dm40_fw3app9app_press*);
        *(.rodata.*7dm40_fw7fastlog4prep*);
        /* Single functions (and their closures) that run only at start-up or
         * on a user's settings / page action, never per pass nor in the ISR:
         * the App construction (main.rs new_app, App::new and the sub-struct
         * constructors inlined there), the generator's plan (gen::plan, on a
         * setting change) and Gen::apply, the settings / keypad / limits /
         * temperature / help pages, the generator page's static parts (not its
         * 1 s status), the fuse-check dialog, the power page's entry. A
         * pattern that stops matching (a rename) leaves the function in B,
         * the safe side. */
        *(.text.*7dm40_fw7new_app*);
        *(.text.*7dm40_fw3gen4plan*);
        *(.text.*7dm40_fw3gen*3Gen5apply*);
        *(.text.*7dm40_fw3app*3App11fuse_dialog*);
        *(.text.*7dm40_fw3app*3App11limits_page*);
        *(.text.*7dm40_fw3app*3App7kp_open*);
        *(.text.*7dm40_fw3app9app_power*3App13pw_open_entry*);
        *(.text.*7dm40_fw2ui8settings*);
        *(.text.*7dm40_fw2ui9temp_page*);
        *(.text.*7dm40_fw2ui8temp_key*);
        *(.text.*7dm40_fw2ui9help_page*);
        *(.text.*7dm40_fw2ui10gen_values*);
        *(.text.*7dm40_fw2ui11gen_preview*);
        *(.text.*7dm40_fw2ui10gen_static*);
        *(.text.*7dm40_fw2ui14auto_cpl_label*);
        *(.text.*7dm40_fw2ui10range_page*);
        /* Debug / file replies, `#[inline(never)]` so they do not land in
         * main.rs `run` (hot): DBG:PROF?, DBG:TOUCH?, DBG:DIAG?, MMEM:CAT?,
         * MMEM:DEL (and the modules above: DBG:HYSPI, DBG:AD2:TORN / NOISE,
         * DBG:UPLOAD, MMEM:DATA?, DBG:GENUINE?). */
        *(.text.*7dm40_fw4prof6report*);
        *(.text.*7dm40_fw5touch8log_text*);
        *(.text.*7dm40_fw10touch_diag*);
        *(.text.*7dm40_fw6csvlog6delete*);
        *(.text.*7dm40_fw6csvlog12catalog_scpi*);
        /* ... their disk bodies (the `_on` variants and their closures). */
        *(.text.*7dm40_fw6csvlog9delete_on*);
        *(.text.*7dm40_fw6csvlog15catalog_scpi_on*);
        /* Tap / page-only code that sat inlined in hot callers (region B
         * audit 2026-10-22 #2): the temperature page's draw / key helper
         * (App::with_temp_page, all its instances), the scope's Save CSV
         * (App::sp_save_csv, with scope_csv::set and the DLG block plan). */
        *(.text.*7dm40_fw3app*3App14with_temp_page*);
        *(.text.*7dm40_fw3app*3App11sp_save_csv*);
        . = ALIGN(4);
        __cold_end = .;
    } > FLASH_C

    .text ORIGIN(FLASH_B) :
    {
        __stext = .;
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = ALIGN(4);
        __etext = .;
    } > FLASH_B

    .data :
    {
        . = ALIGN(4);
        __sdata = .;
        *(.data .data.*);
        . = ALIGN(4);
        __edata = .;
    } > TCM AT > FLASH_C
    __sidata = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        __bss_start = .;
        *(.bss .bss.* COMMON);
        . = ALIGN(4);
        __bss_end = .;
    } > TCM

    .dmabuf (NOLOAD) :
    {
        . = ALIGN(4);
        *(.dmabuf .dmabuf.*);
        *(.sram .sram.*);
    } > SRAM

    /* Zero-initialised CPU-only statics kept out of TCM (the stack's
     * memory): `#[link_section = ".sram_bss"]` on an all-zero static; zeroed
     * at reset next to .bss (native.rs init_ram, main.rs rust_start). */
    .sram_bss (NOLOAD) :
    {
        . = ALIGN(4);
        __sram_bss_start = .;
        *(.sram_bss .sram_bss.*);
        . = ALIGN(4);
        __sram_bss_end = .;
    } > SRAM

    __image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}

ASSERT(__region_a_end <= _marker_start, "region A (vectors + shim) overlaps the loader marker at 0x08008000");
ASSERT(ADDR(.text) >= _marker_end, "code placed in the loader marker at 0x08008000..0x0800800B");
ASSERT(__image_end <= 0x080A0000, "image reaches the FS-loader at 0x080A0000");
/* Memory budgets (2026-10-22 memory rework; the numbers then, ad2-isr (the
 * default) / without it: static worst stack path 21 760 / 21 396 B, stack
 * 46 620 / 46 688 B, SRAM free 12 856 / 13 040 B, region B free 24 508 /
 * 31 884 B).
 *   Stack = TCM above .bss. .data/.bss may not take it below 42 KiB (the
 *   worst path + the 20 KiB build.sh requires, with ~4 KiB for statics);
 *   build.sh also checks the static worst call path itself
 *   (tools/stack-depth/stackdepth.py --check 20480), which catches frame
 *   growth this assert cannot see. (Before: measured high-water 28 412 B,
 *   emulator 2026-10-21, + 16 KiB.)
 *   SRAM: >= 8 KiB free above .sram_bss (the last SRAM section) for DMA
 *   descriptors / buffers still to come.
 *   Region B: >= 8 KiB free below 0x08080000 (build.sh ends with a warning
 *   banner below 16 KiB; hot code / fonts only, rarely run code goes to
 *   .cold above). After the fast logger 9 304 / 16 568 B were left; the
 *   region B audit #2 (2026-10-22: cold-module code that had been inlined
 *   into hot callers, Gallery::poll into App::tick above all, and the
 *   f32 quicksort of poll_cap) brought it to 30 368 / 37 840 B; region C
 *   then has 31 700 / 31 764 B left below 0x080A0000 (was 48 364 / 48 428). */
ASSERT(_stack_top - __bss_end >= 42K, "TCM stack below 42 KiB (worst path ~21.5 KiB + 20 KiB headroom): move CPU-only statics to .sram_bss");
ASSERT(ORIGIN(SRAM) + LENGTH(SRAM) - __sram_bss_end >= 8K, "less than 8 KiB of SRAM free: overlay or shrink a buffer (docs/native-boot.md memory map)");
ASSERT(0x08080000 - __etext >= 8K, "less than 8 KiB of zero-wait flash (region B) free: move rarely run code to .cold");
