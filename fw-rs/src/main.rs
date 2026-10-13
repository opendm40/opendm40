#![no_std]
#![no_main]

mod app;
mod bt;
mod clock;
mod dmm;
mod fonts;
mod gen;
mod gesture;
mod msc;
mod spiflash;
mod hy3131;
mod hy3131_tables;
mod lcd;
mod logger;
mod meter;
mod mmio;
mod persist;
mod power;
mod scope;
mod scope_meas;
mod scpi;
mod startup;
mod stockfont;
mod text;
mod touch;
mod ui;
mod usb;
mod usbtmc;
mod wdt;

use core::panic::PanicInfo;

// Links the single-core critical-section implementation the USB stack needs.
use cortex_m as _;

use usb_device::prelude::*;

use clock::Clock;

/// System control block: vector table offset register.
const SCB_VTOR: u32 = 0xE000_ED08;
const SCB_AIRCR: u32 = 0xE000_ED0C;

/// Watchdog policy. We must NEVER feed the watchdog unconditionally, or the
/// only way back to stock is pulling the battery. We feed only until a deadline:
/// INITIAL_HOLD_MS after start (time to plug USB / start the host tool), pushed
/// forward by every host request. Past the deadline, or after RELEASE, the
/// watchdog (~20 s) resets the device back to the stock firmware.
const INITIAL_HOLD_MS: u64 = if cfg!(feature = "native") { u64::MAX / 4 } else { 60_000 }; // native: feed while the loop runs
const KEEPALIVE_MS: u64 = 15_000;
const KEEPALIVE_MAX_MS: u64 = 600_000;
/// MEASure:<f>? after a function change waits for a full averaging window at
/// most this long (then any fresh reading answers), ms.
const SCPI_SETTLE_MS: u64 = 1500;
/// READ? / MEASure? / FETCh? with no reading of the current function yet
/// (after a function change): give up this long after the converter's settle
/// deadline (`Dmm::deadline`, 2.5 s for ohms), not after the command, ms.
const SCPI_WAIT_MS: u64 = 1900;
/// READ? / MEASure:<f>? with a reading of the current function already
/// flowing: the next fresh reading if it comes within this, else the latest
/// one, ms. libsigrok's USBTMC transport reads Bulk-IN with a hard 1000 ms
/// timeout; the expiry is checked once per main-loop iteration, hence the
/// margin.
const SCPI_QUICK_MS: u64 = 850;

/// pid.codes test VID/PID, for the debug build only.
const USB_VID_PID: UsbVidPid = UsbVidPid(0x1209, 0x0001);

static mut EP_MEMORY: [u32; 320] = [0; 320];
static mut MSC_IO: [u8; 512] = [0; 512];
/// Data-logger ring (FR-38). ~24 KiB; kept out of the main stack. Native: in
/// SRAM (.dmabuf, NOLOAD: `Logger::init_in_place` in main()), not TCM .bss,
/// which the stack shares.
#[cfg_attr(all(target_os = "none", feature = "native"), link_section = ".dmabuf")]
static mut LOGGER: logger::Logger = logger::Logger::new();

/// DIAG block size (DBG:DIAG?).
const DIAG_LEN: usize = 212;

extern "C" {
    static mut __bss_start: u32;
    static mut __bss_end: u32;
    static mut __sdata: u32;
    static mut __edata: u32;
    static __sidata: u32;
    static __VECTORS: u32;
}

/// Called from the reset trampoline (startup.rs) with MSP set and IRQs masked.
#[no_mangle]
pub unsafe extern "C" fn rust_start() -> ! {
    // Copy .data from flash.
    let mut src = core::ptr::addr_of!(__sidata);
    let mut dst = core::ptr::addr_of_mut!(__sdata);
    let dend = core::ptr::addr_of_mut!(__edata);
    while dst < dend {
        dst.write_volatile(src.read_volatile());
        dst = dst.add(1);
        src = src.add(1);
    }

    // Zero .bss.
    let mut p = core::ptr::addr_of_mut!(__bss_start);
    let end = core::ptr::addr_of_mut!(__bss_end);
    while p < end {
        p.write_volatile(0);
        p = p.add(1);
    }

    // Point the vector table at our table in SRAM.
    mmio::w32(SCB_VTOR, core::ptr::addr_of!(__VECTORS) as u32);

    // Calibration / boot-data backup to the SPI disk, before USB exists
    // (src/calbackup.rs; no-op without the `calbackup` feature).
    calbackup::boot();
    main()
}

/// Bring-up failures as red squares at the top left, one slot per stage.
fn fail(slot: u16, failed: bool) {
    if failed {
        lcd::fill_rect(8 + slot * 28, 40, 20, 20, lcd::RED);
    }
}

const ST_PLLSAI: u16 = 0;
const ST_CORE: u16 = 1;

fn main() -> ! {
    wdt::kick();
    screenshot::init(); // JOB and LOGGER are in NOLOAD .dmabuf
    logger::Logger::init_in_place(unsafe { core::ptr::addr_of_mut!(LOGGER) });
    let mut clk = Clock::new();
    lcd::set_madctl(lcd::MADCTL_LANDSCAPE);
    if !cfg!(feature = "keep-screen") {
        lcd::fill_rect(0, 0, lcd::W, lcd::H, ui::BG);
    }

    // Slow init (relays, touch reset ~220 ms) first: once USB is attached the
    // host expects answers within milliseconds, and a long pause here made it
    // give up on enumeration.
    let mut app = app::App::new(&mut clk);
    let mut tp = touch::Touch::new(&mut clk);
    wdt::kick();

    let hw = usb::init_hw(&mut clk);
    wdt::kick();
    fail(ST_PLLSAI, !hw.pllsai_locked);
    fail(ST_CORE, !hw.core_idle);

    let bus = synopsys_usb_otg::UsbBus::new(usb::UsbFs, unsafe {
        &mut *core::ptr::addr_of_mut!(EP_MEMORY)
    });
    // USBTMC (interface 0, SCPI incl. the DBG: debug commands) and USB mass
    // storage (interface 1) on the SPI flash. Two IN endpoints + EP0 of the
    // four of USBFS.
    let mut tmc = usbtmc::Usbtmc::new(&bus);
    let mut msc = msc::Msc::new(&bus, unsafe { &mut *core::ptr::addr_of_mut!(MSC_IO) });
    let mut disk = spiflash::SpiFlash::new();
    let mut dev = UsbDeviceBuilder::new(&bus, USB_VID_PID)
        .strings(&[StringDescriptors::default()
            .manufacturer("dm40-fw")
            .product("DM40 multimeter (dm40-fw)")
            .serial_number("0001")])
        .unwrap()
        .build();
    // build() enabled the core; now force the GD32 power/VBUS bits.
    usb::force_power_no_vbus();

    let mut deadline = clk.now_ms() + INITIAL_HOLD_MS;
    let mut feeding = true;
    let mut reset_at: Option<u64> = None;

    let mut scpi = scpi::Scpi::new();
    // SCPI READ?/MEASure?/FETCh? waiting for a DMM reading: (min seq, settle,
    // start ms, expiry ms). The reply is the first reading with seq >= min,
    // at whatever averaging it has (READ?: the next conversion, ~0.1-0.4 s).
    // settle (MEASure:<f>? that changed the function): also wait for a full
    // averaging window, but at most SCPI_SETTLE_MS after the command.
    // Expiry: with a reading of the current function already there (READ? /
    // MEAS? without a function change) SCPI_QUICK_MS after the command, then
    // that latest reading answers (sigrok's 1 s USBTMC timeout). Without one
    // (just switched: MEAS:<f>?, CONF then READ?) SCPI_WAIT_MS after the
    // settle deadline, then +9.9E37 and "Data stale". A reply is never sent
    // after its query was given up (abort / clear / a new command): a late
    // reply would shift every later one.
    let mut scpi_wait: Option<(u32, bool, u64, u64)> = None;
    // Data logger (FR-38): owns the HY3131 AD1 fast stream while active.
    let logger = unsafe { &mut *core::ptr::addr_of_mut!(LOGGER) };
    logger.set_osr(logger::DEFAULT_OSR);
    let mut logging_prev = false;
    let mut bt = bt::Bt::new();
    let mut gestures = gesture::Tracker::new();
    let mut touch_events = 0u32;
    let mut next_touch = 0u64;
    let mut next_poll = 0u64;
    let mut draw_hold_until = 0u64;
    // DBG:KEY n,ms: the deferred key-up (code, due time).
    let mut dbg_key_up: Option<(u8, u64)> = None;

    // Loop timing (debug): longest iteration in the last second, us.
    let mut loop_t0 = unsafe { mmio::r32(0xE000_1004) };
    let mut loop_max = 0u32;
    let mut loop_max_shown = 0u32;
    let mut loop_win = 0u64;
    // Section profile (debug): max us per section in the last second:
    // 0 usb+scpi, 1 tick, 2 touch, 3 scope, 4 bt, 5 dmm poll+draw.
    let mut sec = [0u32; 6];
    let mut sec_shown = [0u32; 6];
    let cyc = || unsafe { mmio::r32(0xE000_1004) };

    loop {
        let now = clk.now_ms();
        #[cfg(feature = "native")]
        boot::mark_healthy(now);
        let t = unsafe { mmio::r32(0xE000_1004) };
        loop_max = loop_max.max(t.wrapping_sub(loop_t0) / 200);
        loop_t0 = t;
        if now >= loop_win {
            loop_win = now + 1000;
            loop_max_shown = loop_max;
            loop_max = 0;
            sec_shown = sec;
            sec = [0; 6];
        }
        let mut ts = cyc();
        let mut mark = |k: usize, ts: &mut u32| {
            let t = cyc();
            sec[k] = sec[k].max(t.wrapping_sub(*ts) / 200);
            *ts = t;
        };
        let fed = feeding && now < deadline;
        wdt::set_feeding(fed);
        if fed {
            wdt::kick();
        }
        if reset_at.map(|t| now >= t).unwrap_or(false) {
            screenshot::finish_blocking(8000); // never cut its FAT / dir writes
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        dev.poll(&mut [&mut tmc, &mut msc]);
        // M4: no host disk writes while the logger needs a constant interval.
        msc.set_write_inhibit(logger.disk_writes_blocked());
        msc.set_configured(dev.state() == UsbDeviceState::Configured);
        msc.poll(&mut disk);
        // Constant-interval HY3131 sampling, paced by the DWT / AD1F flag and run
        // every iteration (cheap when inactive). Kept independent of the GUI loop.
        logger.poll();
        if tmc.take_cleared() {
            // USBTMC INITIATE_CLEAR / bus reset: no reply to a pending query.
            scpi_wait = None;
        }
        let mut msg = [0u8; 256];
        let mut msg_n = 0usize;
        if let Some(cmd) = tmc.take_command() {
            // H1: a host session counts as activity for idle dim / power-off.
            app.note_host_activity(now);
            msg_n = cmd.len().min(256);
            msg[..msg_n].copy_from_slice(&cmd[..msg_n]);
            // A new message while a READ?/MEAS? is still waiting: the host
            // gave up on it (IEEE 488.2 query interrupted); no late reply.
            if scpi_wait.take().is_some() {
                scpi.set_err("Query INTERRUPTED");
            }
            // Any host traffic keeps the debug-build watchdog fed.
            deadline = deadline.max(now + KEEPALIVE_MS);
        }
        if msg_n > 0 {
            // dmm.func(): the stock mode, 16 / 17 = V / A AC+DC (stable while
            // AC+DC alternates the HY3131 mode).
            let reply = match scpi.handle(&msg[..msg_n], app.dmm.func()) {
                scpi::Out::None => false,
                scpi::Out::Text => true,
                scpi::Out::Configure(m) => {
                    app.select_func(&mut clk, m); // no-op when already selected
                    false
                }
                scpi::Out::Measure(m) => {
                    if logger.active() {
                        // The DMM poll is suspended while logging owns the HY3131;
                        // do not arm scpi_wait or the reply would never come.
                        scpi.len = 0;
                        scpi.put_bytes(b"+9.90000000E+37");
                        true
                    } else {
                        // The multimeter needs its analog side powered.
                        app.dmm_power(&mut clk);
                        let changed = m.is_some_and(|m| !app.is_func(m));
                        if let Some(m) = m.filter(|_| changed) {
                            app.select_func(&mut clk, m);
                        }
                        // meter.live() is reset on every function change.
                        let expire = if app.meter.live().is_some() {
                            now + SCPI_QUICK_MS
                        } else {
                            now.max(app.dmm.deadline()) + SCPI_WAIT_MS
                        };
                        let min_seq = app.last_raw.map(|r| r.seq.wrapping_add(1)).unwrap_or(0);
                        scpi_wait = Some((min_seq, changed, now, expire));
                        false
                    }
                }
                scpi::Out::Instr(c) => {
                    scpi.len = 0;
                    app.scpi(&mut clk, c, &mut scpi)
                }
                scpi::Out::Fetch => match app.meter.shown().copied() {
                    Some(r) => {
                        scpi.len = 0;
                        scpi.put_reading(&r);
                        true
                    }
                    None => {
                        if logger.active() {
                            scpi.len = 0;
                            scpi.put_bytes(b"+9.90000000E+37");
                            true
                        } else {
                            scpi_wait = Some((0, false, now, now.max(app.dmm.deadline()) + SCPI_WAIT_MS));
                            false
                        }
                    }
                },
                scpi::Out::Dbg(d) => match d {
                    scpi::Dbg::Ping(ms) => {
                        if ms > 0 {
                            deadline = deadline.max(now + (ms as u64).min(KEEPALIVE_MAX_MS));
                        }
                        let flags = hw.pllsai_locked as u32 | (hw.core_idle as u32) << 1;
                        let left = if feeding { deadline.saturating_sub(now) as u32 } else { 0 };
                        scpi.len = 0;
                        for (i, v) in [now as u32, flags, left].iter().enumerate() {
                            if i > 0 {
                                scpi.put_bytes(b",");
                            }
                            let mut t = [0u8; 10];
                            let mut i = t.len();
                            let mut v = *v;
                            loop {
                                i -= 1;
                                t[i] = b'0' + (v % 10) as u8;
                                v /= 10;
                                if v == 0 {
                                    break;
                                }
                            }
                            scpi.put_bytes(&t[i..]);
                        }
                        true
                    }
                    scpi::Dbg::Input(kind, x, y) => {
                        let p = touch::Point { x, y };
                        let g = match kind {
                            0 => Some(gesture::Gesture::Tap(p)),
                            1 => Some(gesture::Gesture::SwipeLeft),
                            2 => Some(gesture::Gesture::SwipeRight),
                            // DBG:KEY n[,ms]: a full click; y = hold ms
                            // (0: release now, else the key-up is deferred).
                            3 => {
                                if let Some((c, _)) = dbg_key_up.take() {
                                    app.gesture(&mut clk, gesture::Gesture::KeyUp(c)); // still held: release first
                                }
                                app.gesture(&mut clk, gesture::Gesture::KeyDown(x as u8));
                                if y == 0 {
                                    Some(gesture::Gesture::KeyUp(x as u8))
                                } else {
                                    dbg_key_up = Some((x as u8, now + y as u64));
                                    None
                                }
                            }
                            4 => gestures.synth_swipe_down(x as u8, now), // DBG:SWIPE D,n
                            _ => None,
                        };
                        if let Some(g) = g {
                            app.gesture(&mut clk, g);
                        }
                        false
                    }
                    scpi::Dbg::Drag(v) => {
                        let (p0, p1) = (touch::Point { x: v[0], y: v[1] }, touch::Point { x: v[2], y: v[3] });
                        gestures.synth_drag(p0, p1, v[4], v[5], now, |g| app.gesture(&mut clk, g));
                        false
                    }
                    scpi::Dbg::Lcd(x, y, w, h, n) => {
                        // Raw GRAM words after RAMRD (dummy first) as #4 block.
                        let n = (n as usize).min((usbtmc::RSP_MAX - 8) / 2);
                        let mut hdr = [b'#', b'4', 0, 0, 0, 0];
                        let bytes = n * 2;
                        for (i, d) in [1000, 100, 10, 1].iter().enumerate() {
                            hdr[2 + i] = b'0' + ((bytes / d) % 10) as u8;
                        }
                        let buf = tmc.rsp_buf();
                        buf[..6].copy_from_slice(&hdr);
                        lcd::set_window_cmd(x, y, x + w - 1, y + h - 1, lcd::CMD_RAMRD);
                        for i in 0..n {
                            let v = lcd::read_data().to_le_bytes();
                            buf[6 + 2 * i] = v[0];
                            buf[7 + 2 * i] = v[1];
                        }
                        tmc.respond_len(6 + bytes);
                        draw_hold_until = now + 300;
                        false
                    }
                    scpi::Dbg::Diag => {
                        let d = touch_diag(&tp, touch_events, tmc.dbg_ctl, loop_max_shown, app.prof, sec_shown);
                        scpi.len = 0;
                        scpi.put_bytes(b"#3");
                        let n = d.len();
                        scpi.put_bytes(&[b'0' + (n / 100) as u8, b'0' + (n / 10 % 10) as u8, b'0' + (n % 10) as u8]);
                        scpi.put_bytes(&d);
                        true
                    }
                    scpi::Dbg::Flash => {
                        scpi.len = 0;
                        let mut t = [0u8; 16];
                        for i in 0..6 {
                            t[i] = b"0123456789ABCDEF"[(disk.id >> (20 - 4 * i) & 0xF) as usize];
                        }
                        scpi.put_bytes(&t[..6]);
                        for v in [msc::BlockDevice::block_count(&disk), disk.failed() as u32] {
                            scpi.put_bytes(b",");
                            let s = dmm::format(&mut t, v as f32, 0);
                            scpi.put_bytes(s.as_bytes());
                        }
                        true
                    }
                    scpi::Dbg::Release => {
                        feeding = false;
                        false
                    }
                    scpi::Dbg::Reset => {
                        reset_at = Some(now + 50);
                        false
                    }
                },
                scpi::Out::Log(l) => match l {
                    scpi::Log::State(Some(on)) => {
                        if on {
                            // Fast stream needs an AD1 (DC) mode: 1 DCV, 3 DCmV,
                            // 7/8/9 DCA. AC/ohm/cap/freq/temp drive AD2 or the
                            // counter, so AD1 would be meaningless.
                            let m = app.dmm.mode();
                            if matches!(m, 1 | 3 | 7..=9) {
                                app.dmm_power(&mut clk);
                                // Guarantee the full DC config is applied before
                                // the logger overrides R22 (AD1OSR).
                                app.dmm.start(&mut clk);
                                logger.start(m, app.dmm.range());
                            } else {
                                scpi.set_err("Settings conflict");
                            }
                        } else {
                            logger.stop();
                        }
                        false
                    }
                    scpi::Log::State(None) => {
                        scpi.len = 0;
                        scpi.put_bytes(if logger.active() { b"1" } else { b"0" });
                        true
                    }
                    scpi::Log::Osr(Some(n)) => {
                        logger.set_osr(n);
                        false
                    }
                    scpi::Log::Osr(None) => {
                        scpi.len = 0;
                        put_u32(&mut scpi, logger.osr() as u32);
                        true
                    }
                    scpi::Log::Rate => {
                        scpi.len = 0;
                        put_u32(&mut scpi, logger.measured_hz());
                        true
                    }
                    scpi::Log::Count => {
                        scpi.len = 0;
                        for (i, v) in [logger.buffered(), logger.captured(), logger.overwritten(), logger.missed()]
                            .iter()
                            .enumerate()
                        {
                            if i > 0 {
                                scpi.put_bytes(b",");
                            }
                            put_u32(&mut scpi, *v);
                        }
                        true
                    }
                    scpi::Log::Preamble => {
                        scpi.len = 0;
                        let vals = [
                            logger.mode() as u32,
                            logger.range() as u32,
                            logger.osr() as u32,
                            logger.nominal_us(),
                            logger.measured_hz(),
                        ];
                        for (i, v) in vals.iter().enumerate() {
                            if i > 0 {
                                scpi.put_bytes(b",");
                            }
                            put_u32(&mut scpi, *v);
                        }
                        true
                    }
                    scpi::Log::Data => {
                        // Definite-length #4nnnn block straight into the USBTMC
                        // response buffer (6144 B), like Dbg::Lcd. scpi.buf is
                        // only 512 B and #3 caps at 999 bytes, so neither fits.
                        let buf = tmc.rsp_buf();
                        let n = logger.encode(buf);
                        tmc.respond_len(n);
                        false
                    }
                },
            };
            if reply {
                tmc.respond(&scpi.buf[..scpi.len]);
            }
        }

        // Any path that reconfigures the DMM (a host CONFigure, a touch/key
        // FUNC/RANGE change, a BT mode-byte) rewrites the 22-byte HY3131 config
        // and R22 behind the logger's back. Detect that and auto-stop so the
        // host sees STATe?=0 + a SYST:ERR? rather than silently corrupted
        // samples (FR-38 forbids silent loss). Captured data stays readable.
        if logger.active() && (app.dmm.mode() != logger.mode() || app.dmm.range() != logger.range()) {
            logger.stop();
            scpi.set_err("Settings conflict");
        }

        // While logging, the HY3131 belongs to the logger and the GUI is frozen
        // (FR-38: minimal on-screen change, constant sample interval). On the
        // ON->OFF edge, hand the chip back to the DMM with a clean reconfigure.
        let logging = logger.active();
        if logging_prev && !logging {
            app.dmm.start(&mut clk);
            app.meter.reset();
            app.last_raw = None;
            // Take the logger panel down: repaint the view.
            dialog::log_panel_reset();
            app.invalidate();
        }
        logging_prev = logging;

        // No drawing until USB has enumerated (or 3 s passed): a long first
        // draw must not starve the enumeration; nor right after a GRAM
        // read-back, so a multi-request screenshot does not tear.
        let usb_up = dev.state() == UsbDeviceState::Configured || now > 3000;
        // A screenshot reading the panel (screenshot.rs) freezes it too.
        let can_draw = usb_up && now >= draw_hold_until && !logging && !screenshot::frozen();
        if logging && usb_up && now >= draw_hold_until && !screenshot::frozen() {
            // The logger panel (dialog.rs; one part per pass, ~1 s refresh).
            dialog::log_panel(now, logger.measured_hz(), logger.captured(), logger.nominal_us());
        }
        mark(0, &mut ts);
        app.tick(&mut clk, can_draw);
        mark(1, &mut ts);
        if scpi_wait.is_some_and(|w| now >= w.3) {
            // No fresh DMM reading in time (a slow capacitance / temperature
            // range, a range hunt, a stalled converter): answer now rather
            // than after the host's timeout, with the latest reading of the
            // current function if there is one.
            scpi_wait = None;
            scpi.len = 0;
            match app.meter.live().copied() {
                Some(r) => scpi.put_reading(&r),
                None => {
                    scpi.put_bytes(b"+9.90000000E+37");
                    scpi.set_err("Data stale");
                }
            }
            tmc.respond(&scpi.buf[..scpi.len]);
        }
        if let Some((c, _)) = dbg_key_up.filter(|k| now >= k.1) {
            dbg_key_up = None;
            app.gesture(&mut clk, gesture::Gesture::KeyUp(c));
        }

        // Suspend touch while logging: there is no logger UI (the screen is
        // frozen), the power key lives in app.tick, and the bit-banged I2C read
        // every 10 ms is the largest periodic jitter source for the sampler.
        if now >= next_touch && !logging {
            next_touch = now + 10;
            let g = match tp.as_mut().and_then(|t| t.poll(now)) {
                Some(ev) => {
                    touch_events = touch_events.wrapping_add(1);
                    gestures.event(&ev, now)
                }
                None => gestures.tick(now),
            };
            if let Some(g) = g {
                if app.gesture(&mut clk, g) { gestures.capture(); }
            }
        }
        mark(2, &mut ts);
        if !logging {
            app.scope_poll(now, can_draw);
        }
        mark(3, &mut ts);
        if now >= next_poll {
            next_poll = now + 5;
            // The BT module AT init blocks; not while the logger samples.
            if app.bt_on && !logging {
                bt.init_module(&mut clk, app.dmm.model());
            }
            if let Some(f) = bt.poll() {
                if app.bt_on {
                    app.note_host_activity(now); // H1
                }
                // Don't let a BT mode-byte (cmd 06) reconfigure the HY3131 while
                // the logger owns it; the auto-stop guard above is the backstop.
                if app.bt_on && !logging {
                    let raw = app.last_raw;
                    bt.serve(&f, &mut clk, &mut app.dmm, &mut app.meter, raw.as_ref(), &app.power, app.locked);
                }
            }
            mark(4, &mut ts);
            if !logging {
                if let Some(raw) = app.dmm.poll(&mut clk) {
                    if let Some(r) = app.reading(raw, can_draw) {
                        if let Some((_, settle, t0, _)) = scpi_wait.filter(|w| r.seq.wrapping_sub(w.0) < 0x8000_0000) {
                            // MEASure:<f>? after a function change: prefer a
                            // value at the full (+1 digit) resolution (the
                            // moving average holds AVG_FULL conversions; avg_n
                            // 0 = not an averaged range; overload never
                            // averages), but not past SCPI_SETTLE_MS.
                            let ready = !settle
                                || r.avg_n == 0
                                || r.avg_n >= dmm::AVG_FULL
                                || r.main.ol
                                || now >= t0 + SCPI_SETTLE_MS;
                            if ready {
                                scpi_wait = None;
                                scpi.len = 0;
                                scpi.put_reading(&r);
                                tmc.respond(&scpi.buf[..scpi.len]);
                            }
                        }
                    }
                }
            }
            mark(5, &mut ts);
        }
    }
}

/// Append a u32 as decimal to a SCPI reply (exact; no f32 rounding).
fn put_u32(scpi: &mut scpi::Scpi, mut v: u32) {
    let mut t = [0u8; 10];
    let mut i = t.len();
    loop {
        i -= 1;
        t[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    scpi.put_bytes(&t[i..]);
}

fn touch_diag(
    tp: &Option<touch::Touch>,
    events: u32,
    ctl: (u8, [u8; 16]),
    loop_us: u32,
    prof: [u32; 3],
    sec: [u32; 6],
) -> [u8; DIAG_LEN] {
    let mut d = [0u8; DIAG_LEN];
    for (i, v) in sec.iter().enumerate() {
        d[128 + 4 * i..132 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in prof.iter().enumerate() {
        d[116 + 4 * i..120 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in scope::debug().iter().enumerate() {
        d[100 + 4 * i..104 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    d[97] = (loop_us.min(0xFFFF) & 0xFF) as u8;
    d[98] = (loop_us.min(0xFFFF) >> 8) as u8;
    d[80] = ctl.0;
    d[81..97].copy_from_slice(&ctl.1);
    if let Some(t) = tp {
        d[152] = t.ring_i;
        for (i, f) in t.ring.iter().enumerate() {
            d[153 + 7 * i..160 + 7 * i].copy_from_slice(f);
        }
        d[0..4].copy_from_slice(&t.chip);
        d[4] = 1;
        d[8..12].copy_from_slice(&events.to_le_bytes());
        d[12..19].copy_from_slice(&t.raw);
    }
    d
}

/// Stop feeding the watchdog and wait for it to reset us back to stock.
fn release_to_stock() -> ! {
    loop {
        mmio::spin(1_000_000);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Mark the panic on screen; do not feed the watchdog, so a crash recovers to
    // the stock firmware on reset.
    lcd::fill_rect(0, lcd::H - 24, lcd::W, 24, lcd::MAGENTA);
    release_to_stock()
}

// ---------------------------------------------------------------------------
// Native-format image (docs/native-boot.md "Implementation"). Appended at the
// end of the file so the default build stays byte-identical (panic locations
// carry line numbers). Native watchdog policy (src/boot.rs): INITIAL_HOLD_MS
// above makes the main loop feed the FWDGT for as long as it runs.
// DBG:RELEASE, a panic or a hang stop the feeding -> FWDGT reset; the shim
// counts it as a crash: the 1st reboots fw-rs normally (WDT_RETRY), a 2nd one
// without 60 s of healthy main loop in between (boot::mark_healthy) enters
// recovery (marker zeroed -> vendor update screen). DBG:RESET = SYSRESETREQ ->
// normal reboot of fw-rs. DBG:UPDATE / DBG:RECOVER zero the 12-byte loader
// marker (boot::enter_loader_update) and reset -> the vendor loader's update
// screen with its own USB disk until a dm40*.bin is installed. (No backup-SRAM
// request word is set by any command now.)
#[cfg(any(feature = "test-initcore", feature = "test-recovery"))]
#[cfg(feature = "native")]
compile_error!("native cannot be combined with the RAM test features");
#[cfg(all(feature = "test-initcore", feature = "test-recovery"))]
compile_error!("test-initcore and test-recovery are separate builds");
#[cfg(any(feature = "native", feature = "test-initcore"))]
mod board;
#[cfg(any(feature = "native", feature = "test-initcore", feature = "test-recovery"))]
mod boot;
// fatren: also its FAT16 helpers for calbackup in every build.
mod fatren;
mod calbackup;
mod dialog;
mod screenshot;
#[cfg(any(feature = "native", feature = "test-initcore", feature = "test-recovery"))]
mod native;
// Stock device-authenticity check, informational only: DBG:GENUINE? reports
// it; nothing changes device behaviour on its result (user decision).
// (`check` / `verify` are for other callers / the host tests.)
#[allow(dead_code)]
mod genuine;
#[cfg(any(feature = "native", feature = "test-recovery"))]
mod recovery;
#[cfg(feature = "native")]
mod shim;
