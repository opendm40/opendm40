/*
 * DM40 replacement firmware, NATIVE format: the app image the FS-loader
 * installs at 0x08004000 (stock update-file body), executed in place (XIP;
 * the GD32F4 low 512 KiB of flash is zero-wait, so nothing is copied to SRAM).
 *
 * Flash (body offset = address - 0x08004000):
 *   A  0x08004000..0x08008000  16 KiB  vector table + shim (.vectors, .shim),
 *                                      then region A2 (.a2) up to the marker:
 *                                      zero-wait flash in the update body's
 *                                      sector 1 (FS-loader writes it like any
 *                                      body byte), named hot read-only data
 *                                      (`.rodata.a2.*`: the ASCII26 and F14
 *                                      font blobs, tools/mkstockfont.py
 *                                      A2_FONTS). ~14.8 KB; the link fails
 *                                      when it reaches the marker, build.sh
 *                                      warns below 2 KiB free.
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
 *                                      scope CSV, boot / DBG:BOOT?). Must
 *                                      end below 0x080A0000 (FS-loader, UID
 *                                      data, calibration above). The .data
 *                                      load image follows .text in B (read
 *                                      once by the reset copy; in C until
 *                                      region C audit #1).
 *   Why the split: flash from 0x08080000 up is NOT zero-wait on the GD32F4
 *   (only the low 512 KiB are). A read there stalls the bus for ~20 us, and
 *   with it the AD2 stream ISR's entry (device 5f14d38: max_lat 18.8-21.8
 *   us whenever the logger panel drew ASCII26 glyphs, which had grown past
 *   0x08080000; 4.6 us without the panel; docs/dmm-input-scope-feasibility.md
 *   10.3). Code in C costs that per call: only what never runs while a
 *   stream does (or seldom, outside the main loop's every-pass path).
 *   DATA loads too, from anywhere >= 0x08080000 (region C's rodata, the
 *   calibration record, the loader data): device b6909d6, the USB volume's
 *   INFO/ files, 21.8 us per load. None while a stream runs (RAM copies,
 *   the volume answers NOT READY; docs/native-boot.md "The slow flash bank
 *   while an AD2 stream runs"; emu/regionc.py watches the loads).
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

    /* Region A2: the rest of FLASH_A after the shim, below the loader marker
     * (zero-wait, part of the update body). Claimed before .cold / .text
     * (first matching rule wins). Only hot, B-class data or code may go
     * here (it is as fast as B); nothing the shim / recovery needs (that
     * stays in .vectors / .shim). */
    .a2 :
    {
        __a2_start = .;
        *(.rodata.a2 .rodata.a2.*);
        *(.text.a2 .text.a2.*);
        . = ALIGN(4);
        __a2_end = .;
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
        *(.text.*7dm40_fw3uid4init*);
        *(.text.*7dm40_fw8recovery*);
        *(.text.*7dm40_fw7genuine*);
        *(.text.*7dm40_fw7gallery*);
        *(.text.*7dm40_fw10screenshot*);
        *(.text.*7dm40_fw4dlog*);
        *(.text.*7dm40_fw6fatdir*);
        *(.text.*7dm40_fw5vdisk*);
        *(.text.*7dm40_fw6upload*);
        *(.text.*7dm40_fw5hyspi*);
        /* ad2exp (DBG:AD2:TORN? / NOISE? / DCV?) and main.rs dbg_dcv: in
         * region B since region C audit #1 (relocated, 3.8 KB, while B had
         * ~80 KB free; on-demand DBG replies, zero-wait is no cost). */
        *(.text.*7dm40_fw9scope_csv*);
        /* NOT the scope WAVeform:* / ACQ:SRAT? / TRIG:STAT? / CHAN1:OFFS?
         * replies (src/scope_wav.rs) nor their parser scpi::wav_cmd: a host
         * polls them while the Wave view's stream runs (device e7ec2cc:
         * max_lat 6.16 us at scope_wav::query; emu/hy3131/wavetest.py
         * polls them under the region C hooks). Region B, ~2 KB. */
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
        *(.text.*7dm40_fw10stale_read*);
        /* NOT fast_scpi (LOG:DEST / LOG:DURation, set and query): region B.
         * Device b6909d6: LOG:DEST? / LOG:DUR? in the Wave view at 76800
         * gave late 1, max_lat 2.8 / 5.8 us at lat_pc fast_scpi. */
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
        /* "Leads in A?" reminder's beep / toast (once per function change;
         * the per-pass check App::leads_tick stays in B). */
        *(.text.*7dm40_fw3app*3App12leads_remind*);
        *(.text.*7dm40_fw3app*3App11leads_toast*);
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
        /* The power page after a coupling change (once per change). */
        *(.text.*7dm40_fw3app9app_power*3App15pw_coup_changed*);
        *(.text.*7dm40_fw3app9app_power*3App15pw_pending_draw*);
        *(.text.*7dm40_fw3dmm2pw*3Dmm10pw_pending);
        *(.text.*7dm40_fw2ui8settings*);
        *(.text.*7dm40_fw2ui9temp_page*);
        *(.text.*7dm40_fw2ui8temp_key*);
        *(.text.*7dm40_fw2ui9help_page*);
        *(.text.*7dm40_fw2ui10gen_values*);
        *(.text.*7dm40_fw2ui11gen_preview*);
        *(.text.*7dm40_fw2ui10gen_static*);
        *(.text.*7dm40_fw2ui14auto_cpl_label*);
        *(.text.*7dm40_fw2ui10range_page*);
        /* Pressed feedback of the MF range / Power Settings pages (per touch). */
        *(.text.*7dm40_fw2ui11range_press*);
        *(.text.*7dm40_fw2ui2pw14settings_press*);
        /* Debug / file replies, `#[inline(never)]` so they do not land in
         * main.rs `run` (hot): DBG:TOUCH?, DBG:DIAG?, MMEM:DEL (and the
         * modules above: DBG:HYSPI, DBG:UPLOAD, MMEM:DATA?, DBG:GENUINE?).
         * Not DBG:PROF? (prof::report, ~1 KB) nor MMEM:CAT? (csvlog::
         * catalog_scpi with fatren's / fatdir's `.text.fat_rd` helpers)
         * nor CONF?'s
         * App::range_si: region B, asked during fast sessions / the Wave
         * view (device a0985e4: DBG:PROF? 12 us, a sample lost at 38400;
         * emu/fastlogtest.py --region-c --host-read, emu/hy3131/
         * wavetest.py's whole-run region C check). Nor LOG:SCALe?
         * (main.rs log_scale: a host scales the session's data with it;
         * fastlogtest --poll, 2026-10-23: region C code and its literal
         * loads while streaming). */
        *(.text.*7dm40_fw4scpi7dcv_arg*);
        /* SCPI ranges / resolution: one command each. */
        *(.text.*7dm40_fw4scpi9range_cmd*);
        *(.text.*7dm40_fw4scpi8conf_cmd*);
        *(.text.*7dm40_fw3app*3App10scpi_range*);
        *(.text.*7dm40_fw4scpi8range_si*);
        *(.text.*7dm40_fw4scpi9range_arg*);
        *(.text.*7dm40_fw4scpi7res_arg*);
        *(.text.*7dm40_fw4scpi8step_exp*);
        *(.text.*7dm40_fw4scpi6pow10f*);
        *(.text.*7dm40_fw4scpi6n7_arg*);
        *(.text.*7dm40_fw6dbg_n7*);
        *(.text.*7dm40_fw5touch8log_text*);
        *(.text.*7dm40_fw10touch_diag*);
        *(.text.*7dm40_fw6csvlog6delete*);
        /* ... their disk bodies (the `_on` variants and their closures). */
        *(.text.*7dm40_fw6csvlog9delete_on*);
        /* Tap / page-only code that sat inlined in hot callers (region B
         * audit 2026-10-22 #2): the temperature page's draw / key helper
         * (App::with_temp_page), the scope's Save CSV
         * (App::sp_save_csv, with scope_csv::set and the DLG block plan). */
        *(.text.*7dm40_fw3app*3App14with_temp_page*);
        *(.text.*7dm40_fw3app*3App11sp_save_csv*);
        /* Region B audit #3 (2026-10-22, 11.9 KB free on c58ae48):
         * `#[inline(never)]` event / start-up code that sat inlined in hot
         * callers. App::wave_open runs at event rate. (App::tap with
         * pw_tap / kp_apply / range hits, App::key and App::show were here
         * too; back in B since region A2 took the ASCII26 / F14 font blobs
         * out of B, 2026-10-22: App::tap meets a stream on the inrush
         * page.) Start of a recording session (App::rec_start with
         * csvlog::start). NOT Logger::start / logger::isr::start: back in
         * B (2026-10-23): their tails run after stream::start, with the ISR
         * on (the slow-bank data watch, emu/regionc.py, caught
         * Logger::start's literal load of DWT_CYCCNT's address).
         * Page / dialog draws on a view change (power
         * settings page (not its per-tick settings_status), device page,
         * fuse dialog). DBG / calibration replies (DBG:ACDC?, DBG:SENT?,
         * SCOP:CAL with HwTrig::calibrate). Start-up only: Touch::new,
         * Bt::new (not init_module: polled every pass), SpiFlash::new,
         * usb::init_hw. */
        *(.text.*7dm40_fw3app*3App9wave_open*);
        *(.text.*7dm40_fw3app*3App9rec_start*);
        *(.text.*7dm40_fw2ui2pw13settings_page*);
        *(.text.*7dm40_fw2ui11device_page*);
        *(.text.*7dm40_fw6dialog10fuse_check*);
        *(.text.*7dm40_fw3dmm*3Dmm14acdc_diag_text*);
        /* acdcm.rs: the AC+DC methods' entry / range-change / SCPI steps
           (poll_am, the per-result path, stays in region B) */
        *(.text.*7dm40_fw3dmm2am*8am_enter*);
        *(.text.*7dm40_fw3dmm2am*11am_set_meth*);
        *(.text.*7dm40_fw3dmm2am*11am_set_zero*);
        *(.text.*7dm40_fw3dmm2am*7am_zero*);
        *(.text.*7dm40_fw3dmm2am*7am_stat*);
        *(.text.*7dm40_fw3dmm2am*7am_mode*);
        *(.text.*7dm40_fw3dmm2am*6am_row*);
        *(.text.*7dm40_fw3dmm2am*6am_ref*);
        *(.text.*7dm40_fw3dmm2am*11am_cal_step*);
        *(.text.*7dm40_fw8sentinel*8Sentinel9diag_text*);
        *(.text.*7dm40_fw5scope*5Scope12hw_calibrate*);
        *(.text.*7dm40_fw6hwtrig*6HwTrig9calibrate*);
        *(.text.*7dm40_fw5touch*5Touch3new*);
        *(.text.*7dm40_fw2bt*2Bt3new*);
        *(.text.*7dm40_fw8spiflash*8SpiFlash3new*);
        *(.text.*7dm40_fw3usb7init_hw*);
        /* Menu / list / keypad page helpers (already their own symbols):
         * drawn or hit-tested only on the Menu, Settings, Record, Limits,
         * Keypad, Help and Device pages, where no stream runs (leaving the
         * Wave view or the inrush page stops theirs; touch is off while
         * the logger runs). */
        *(.text.*7dm40_fw2ui9menu_item*);
        *(.text.*7dm40_fw2ui13main_menu_hit*);
        *(.text.*7dm40_fw2ui9list_page*);
        *(.text.*7dm40_fw2ui10page_title*);
        *(.text.*7dm40_fw2ui11set_stepper*);
        *(.text.*7dm40_fw2ui12keypad_input*);
        *(.text.*7dm40_fw2ui10keypad_key*);
        *(.text.*7dm40_fw2ui10keypad_hit*);
        *(.text.*7dm40_fw3app*3App9kp_labels*);
        *(.text.*7dm40_fw3app*3App8lim_unit*);
        /* The generator page's boxes and its 1 s status (Gen / Keypad page
         * only: no stream runs there) and vwrite's temp-file purge (the
         * first pass after a start, a host write session's purge). */
        *(.text.*7dm40_fw2ui7gen_box*);
        *(.text.*7dm40_fw2ui10gen_status*);
        *(.text.*7dm40_fw2ui13gen_box_frame*);
        *(.text.*7dm40_fw2ui16gen_box_value_bg*);
        *(.text.*7dm40_fw2ui12gen_wave_box*);
        *(.text.*7dm40_fw2ui12gen_box_note*);
        *(.text.*7dm40_fw6vwrite9purge_tmp*);
        /* ... and its post-write steps (a host wrote a file on the volume;
         * MSC writes are refused while a logger session runs): the update
         * file's park / check / finalize / commit. */
        *(.text.*7dm40_fw6vwrite4park*);
        *(.text.*7dm40_fw6vwrite5check*);
        *(.text.*7dm40_fw6vwrite8finalize*);
        *(.text.*7dm40_fw6vwrite6commit*);
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
    } > TCM AT > FLASH_B
    __sidata = LOADADDR(.data);
    /* End of region B's content: .text / .rodata, then the .data image. */
    __b_end = LOADADDR(.data) + SIZEOF(.data);

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

    __image_end = __cold_end;

    /DISCARD/ :
    {
        *(.ARM.exidx*);
        *(.ARM.extab*);
        *(.comment);
    }
}

ASSERT(__region_a_end <= _marker_start, "region A (vectors + shim) overlaps the loader marker at 0x08008000");
ASSERT(__a2_start == __region_a_end, "region A2 does not start right after the shim");
ASSERT(__a2_end <= _marker_start, "region A2 (.rodata.a2.*) overlaps the loader marker at 0x08008000: move a font out of A2_FONTS (tools/mkstockfont.py)");
ASSERT(ADDR(.text) >= _marker_end, "code placed in the loader marker at 0x08008000..0x0800800B");
ASSERT(__image_end <= 0x080A0000, "image reaches the FS-loader at 0x080A0000");
/* Region C budget: >= 4 KiB free below 0x080A0000 (build.sh warns below
 * 8 KiB). C grows by whole moved functions (0.3..5 KB each);
 * 4 KiB still takes one more small move after the warning. */
ASSERT(0x080A0000 - __image_end >= 4K, "less than 4 KiB of region C (.cold) free below 0x080A0000: cut code or move .cold code back to region B");
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
 *   Region C: >= 4 KiB free below 0x080A0000 (build.sh warns below 8 KiB;
 *   nothing above 0x080A0000 is within the update file's reach).
 *   Region B: >= 8 KiB free below 0x08080000 (build.sh ends with a warning
 *   banner below 16 KiB; hot code / fonts only, rarely run code goes to
 *   .cold above). After the fast logger 9 304 / 16 568 B were left; the
 *   region B audit #2 (2026-10-22: cold-module code that had been inlined
 *   into hot callers, Gallery::poll into App::tick above all, and the
 *   f32 quicksort of poll_cap) brought it to 30 368 / 37 840 B; region C
 *   then has 31 700 / 31 764 B left below 0x080A0000 (was 48 364 / 48 428).
 *   Audit #3 (on c58ae48: 11 928 / 20 672 B): 26 160 / 32 808 B; region
 *   C 10 256 / 12 352 B left (docs/native-boot.md "Region B audit #3").
 *   Region A2 (2026-10-22, on e5dd09b: B 26 136 / 33 320, C 10 120 /
 *   12 240 B): ASCII26 + F14 blobs in A2 (2 652 B left; the link fails at
 *   the marker, build.sh warns below 2 KiB), App::tap / key / show and
 *   vdisk::describe back in B: B 30 572 / 37 836, C 17 840 / 19 880 B
 *   (docs/native-boot.md "Region A2"). */
ASSERT(_stack_top - __bss_end >= 42K, "TCM stack below 42 KiB (worst path ~21.5 KiB + 20 KiB headroom): move CPU-only statics to .sram_bss");
ASSERT(ORIGIN(SRAM) + LENGTH(SRAM) - __sram_bss_end >= 8K, "less than 8 KiB of SRAM free: overlay or shrink a buffer (docs/native-boot.md memory map)");
ASSERT(0x08080000 - __b_end >= 8K, "less than 8 KiB of zero-wait flash (region B) free: move rarely run code to .cold");
