#![no_std]
#![no_main]

mod app;
mod bargraph;
mod bt;
mod clock;
mod dmm;
mod gen;
mod gesture;
mod msc;
mod spiflash;
mod hy3131;
mod hy3131_tables;
mod hyspi;
mod lcd;
mod logger;
mod ad2exp;
mod meter;
mod mmio;
mod persist;
mod power;
mod prof;
mod scope;
mod hwtrig;
mod scope_meas;
mod scope_ui;
mod fft;
mod sinc;
mod trigsoft;
mod scpi;
mod sentinel;
mod stockfont;
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

/// System control block: application interrupt and reset control.
const SCB_AIRCR: u32 = 0xE000_ED0C;

/// Watchdog policy (see the end of this file): the main loop feeds the FWDGT
/// until a deadline, INITIAL_HOLD_MS after start (in effect: for as long as
/// the loop runs), pushed forward by host requests. After DBG:RELEASE, a panic
/// or a hang the watchdog (~20 s) resets and the shim counts a crash.
const INITIAL_HOLD_MS: u64 = u64::MAX / 4;
const KEEPALIVE_MS: u64 = 15_000;
/// SCPI session / transport index (scpis, scpi_wait).
const USB: usize = 0;
const BT: usize = 1;
const KEEPALIVE_MAX_MS: u64 = 600_000;
/// MEASure:<f>? after a function change waits for a full averaging window at
/// most this long (then any fresh reading answers), ms. A full window
/// (dmm::AVG_FULL = 8 conversions at OSR 32768, 400 ms each) needs ~3.2 s
/// after the switch to the steady OSR, so within this budget the answer is
/// a partial average without the +1 digit; a later READ? has it. Kept short
/// for the 2 s PyVISA default timeout rather than raised to ~4.5 s.
const SCPI_SETTLE_MS: u64 = 1500;
/// READ? / MEASure? / FETCh? with no reading of the current function yet
/// (after a function change): give up this long after the converter's settle
/// deadline (`Dmm::deadline`, 2.5 s for ohms), not after the command, ms.
const SCPI_WAIT_MS: u64 = 1900;
/// READ? / MEASure:<f>? with a reading of the current function already
/// flowing: the next fresh reading if it comes within this, else the latest
/// one, ms. libsigrok's USBTMC transport reads Bulk-IN with a hard 1000 ms
/// timeout; the expiry is checked once per main-loop iteration, hence the
/// margin. Fits the slowest DC conversion (OSR 32768 chopped, 400 ms: the
/// steady MEDIUM and SLOW reading, both one per conversion) with room for
/// the poll.
const SCPI_QUICK_MS: u64 = 850;
// The next fresh steady reading: at most one conversion away + poll / loop.
const _: () = assert!(SCPI_QUICK_MS >= hy3131::ad1_period_us_r22(0x17) as u64 / 1000 + 300);

/// pid.codes test VID/PID, for the debug build only.
const USB_VID_PID: UsbVidPid = UsbVidPid(0x1209, 0x0001);

// CPU-only buffers in SRAM (.sram_bss, zeroed at reset) rather than TCM,
// which the stack shares (device check 2026-10-21: stack headroom).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut EP_MEMORY: [u32; 320] = [0; 320];
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut MSC_IO: [u8; 512] = [0; 512];
/// The MSC block buffer (was a 4 KiB field of `Msc` on the main stack).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut MSC_BLK: [u8; msc::MAX_BLOCK] = [0; msc::MAX_BLOCK];
/// Data-logger ring (FR-38). ~16 KiB; kept out of the main stack. Native: in
/// SRAM (.dmabuf, NOLOAD: `Logger::init_in_place` in main()), not TCM .bss,
/// which the stack shares.
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut LOGGER: logger::Logger = logger::Logger::new();

/// DIAG block size (DBG:DIAG?).
const DIAG_LEN: usize = 212;

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
    // With `vdisk` (native) the one LUN is the read-only volume generated by
    // vdisk.rs (its tables built in native::pre_main); the SPI flash is
    // internal storage. Without it, LUN 0 = the SPI disk.
    let mut msc = msc::Msc::new(&bus, unsafe { &mut *core::ptr::addr_of_mut!(MSC_IO) }, unsafe { &mut *core::ptr::addr_of_mut!(MSC_BLK) }, 1);
    #[allow(unused_mut)]
    let mut disk = spiflash::SpiFlash::new();
    #[cfg(feature = "vdisk")]
    let mut info = vdisk::VDisk;
    // Refresh debounce of the generated volume: dlog::gen() at the last
    // change seen, and when.
    #[cfg(feature = "vdisk")]
    let (mut vd_gen, mut vd_t) = (dlog::gen(), 0u64);
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

    // One SCPI session per transport (reply buffer, error queue, pending
    // READ?/MEAS? wait): [USB] = USBTMC, [BT] = Bluetooth serial (bt.rs;
    // untrusted, DBG:* refused). Replies go out through bt::ScpiPort.
    let mut scpis = [scpi::Scpi::new(), scpi::Scpi::new()];
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
    // Per transport ([USB], [BT]); a BT line is taken only while [BT] is None.
    let mut scpi_wait: [Option<(u32, bool, u64, u64)>; 2] = [None, None];
    // Who made the DMM's last function change, for the READ? cap: func_key =
    // (function, temperature sensor) as last seen; host_key[src] = that key
    // when transport src's SCPI made the change (CONF, MEAS:<f>?, FUNC,
    // TEMP:TRAN), None after any other change (touch / keys / phone app /
    // DBG:TAP / the other transport). Only then may READ? / MEAS? wait for
    // the settle; otherwise SCPI_QUICK_MS at most (scpi.rs header).
    let mut func_key = (app.dmm.func(), app.dmm.t_sensor());
    let mut host_key: [Option<(u8, u8)>; 2] = [None, None];
    // Data logger (FR-38): owns the HY3131 AD1 fast stream while active.
    let logger = unsafe { &mut *core::ptr::addr_of_mut!(LOGGER) };
    logger.set_osr(logger::DEFAULT_OSR);
    let mut logging_prev = false;
    // DBG:AD2:DRAW (src/ad2exp.rs): 0 no logger panel, 1 panel (default),
    // 2 panel + a full DMM-view redraw every 100 ms; next forced redraw, ms.
    let mut log_draw = 1u8;
    let mut log_redraw_at = 0u64;
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
        // Profiler (DBG:PROF?, src/prof.rs): loop period + the `lap` chain.
        prof::pass(now);
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
            app::finish_disk_writes(now); // never cut a FAT / dir write
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        dev.poll(&mut [&mut tmc, &mut msc]);
        // M4: no host disk writes while the logger needs a constant interval.
        msc.set_write_inhibit(logger.disk_writes_blocked());
        msc.set_usb_state(dev.state());
        #[cfg(feature = "vdisk")]
        msc.poll(&mut [&mut info]);
        #[cfg(not(feature = "vdisk"))]
        msc.poll(&mut [&mut disk]);
        // Constant-interval HY3131 sampling, paced by the DWT / AD1F flag and run
        // every iteration (cheap when inactive). Kept independent of the GUI loop.
        logger.poll();
        bt.pump(); // BT TX DMA: next segment of a long reply
        prof::lap(prof::USB);
        if tmc.take_cleared() {
            // USBTMC INITIATE_CLEAR / bus reset: no reply to a pending query.
            scpi_wait[USB] = None;
        }
        if (app.dmm.func(), app.dmm.t_sensor()) != func_key {
            // Changed on the meter (or by DBG:KEY's deferred key-up): no host
            // waits for its settle; a READ? already waiting is cut short.
            func_key = (app.dmm.func(), app.dmm.t_sensor());
            host_key = [None, None];
            for w in scpi_wait.iter_mut().flatten() {
                w.1 = false;
                w.3 = w.3.min(w.2 + SCPI_QUICK_MS);
            }
        }
        let mut msg = [0u8; 256];
        let mut msg_n = 0usize;
        let mut src = USB;
        if let Some(cmd) = tmc.take_command() {
            // H1: a host session counts as activity for idle dim / power-off.
            app.note_host_activity(now);
            msg_n = cmd.len().min(256);
            msg[..msg_n].copy_from_slice(&cmd[..msg_n]);
            // Update-file upload data: raw binary, taken before the SCPI
            // parser (upload.rs; USB only: BT lines never get here).
            if let Some(m) = msg[..msg_n].strip_prefix(b"DBG:UPLOAD:DATA ") {
                if let Err(e) = upload::data_msg(m, now) {
                    scpis[USB].set_err(e);
                }
                msg_n = 0;
            }
            // A new message while a READ?/MEAS? is still waiting: the host
            // gave up on it (IEEE 488.2 query interrupted); no late reply.
            if scpi_wait[USB].take().is_some() {
                scpis[USB].set_err("Query INTERRUPTED");
            }
            // Any host traffic keeps the debug-build watchdog fed.
            deadline = deadline.max(now + KEEPALIVE_MS);
        } else if scpi_wait[BT].is_none() {
            // SCPI over Bluetooth: one line at a time, the next only after
            // the previous query was answered (serial-instrument order).
            // DBG:* lines come back as Err (refused, nothing runs).
            match bt.take_line() {
                Some(Ok(l)) if app.bt_on => {
                    app.note_host_activity(now);
                    msg_n = l.len().min(256);
                    msg[..msg_n].copy_from_slice(&l[..msg_n]);
                    src = BT;
                }
                Some(Err(e)) if app.bt_on => scpis[BT].set_err(e),
                _ => {}
            }
        }
        if msg_n > 0 {
            let dbg_ctl = tmc.dbg_ctl;
            let bt_tx = (bt.tx_dma_ok, bt.tx_dropped);
            let (mut scpi, port): (&mut scpi::Scpi, &mut dyn bt::ScpiPort) =
                if src == BT { (&mut scpis[BT], &mut bt) } else { (&mut scpis[USB], &mut tmc) };
            // dmm.func(): the stock mode, 16 / 17 = V / A AC+DC (stable while
            // AC+DC alternates the HY3131 mode).
            let out = scpi.handle(&msg[..msg_n], app.dmm.func());
            // DBG:TAP / KEY / DRAG stand in for the user: a change they make
            // is not the host's (READ? cap).
            let by_host = !matches!(out, scpi::Out::Dbg(_));
            let reply = match out {
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
                        // The multimeter needs its analog side powered (a
                        // power-up starts a settle: the host's doing).
                        let d0 = app.dmm.deadline();
                        app.dmm_power(&mut clk);
                        let changed = m.is_some_and(|m| !app.is_func(m));
                        if let Some(m) = m.filter(|_| changed) {
                            app.select_func(&mut clk, m);
                        }
                        // The settle this query may wait for was requested by
                        // this host: this message (MEAS:<f>? changing the
                        // function), its earlier CONF / FUNC / TEMP:TRAN, or
                        // the power-up. Not after a change made on the meter:
                        // READ? then answers within SCPI_QUICK_MS (latest
                        // reading, else +9.9E37 and "Data stale"), as libsigrok
                        // times out after 1 s.
                        let host_settle = changed
                            || app.dmm.deadline() != d0
                            || host_key[src] == Some((app.dmm.func(), app.dmm.t_sensor()));
                        // meter.live() is reset on every function change.
                        // *IDN? emulation (scpi::idn_emul): the host does not
                        // wait after CONF, never hold a reply past its 1 s.
                        let expire = if app.meter.live().is_some() || scpi::idn_emul() != 0 || !host_settle {
                            now + SCPI_QUICK_MS
                        } else {
                            now.max(app.dmm.deadline()) + SCPI_WAIT_MS
                        };
                        let min_seq = app.last_raw.map(|r| r.seq.wrapping_add(1)).unwrap_or(0);
                        scpi_wait[src] = Some((min_seq, changed, now, expire));
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
                            scpi_wait[src] = Some((0, false, now, now.max(app.dmm.deadline()) + SCPI_WAIT_MS));
                            false
                        }
                    }
                },
                scpi::Out::Dbg(_) if !port.trusted() => {
                    scpi.set_err(bt::proto::REFUSED); // backstop; take_line refuses first
                    false
                }
                scpi::Out::Dbg(d) => match d {
                    scpi::Dbg::Upload(op, a, b) => {
                        scpi.len = 0;
                        let r = match op {
                            0 => upload::begin(a, b, now).map(|_| None),
                            1 => upload::end().map(|(size, crc)| Some(([size, crc, 0], 2))),
                            2 => {
                                upload::abort();
                                Ok(None)
                            }
                            _ => {
                                let (on, got, size) = upload::status();
                                Ok(Some(([on as u32, got, size], 3)))
                            }
                        };
                        match r {
                            Ok(Some((v, n))) => {
                                for (i, x) in v[..n].iter().enumerate() {
                                    if i > 0 {
                                        scpi.put_bytes(b",");
                                    }
                                    scpi.put_u32(*x);
                                }
                                true
                            }
                            Ok(None) => false,
                            Err(e) => {
                                scpi.set_err(e);
                                false
                            }
                        }
                    }
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
                        prof::touch(prof::cyc());
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
                        // All events ran in this pass: time from the release
                        // to the first frame drawn (acquisition restart cost).
                        prof::touch(prof::cyc());
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
                        let buf = port.rsp_buf();
                        buf[..6].copy_from_slice(&hdr);
                        lcd::set_window_cmd(x, y, x + w - 1, y + h - 1, lcd::CMD_RAMRD);
                        for i in 0..n {
                            let v = lcd::read_data().to_le_bytes();
                            buf[6 + 2 * i] = v[0];
                            buf[7 + 2 * i] = v[1];
                        }
                        port.respond_len(6 + bytes);
                        draw_hold_until = now + 300;
                        false
                    }
                    scpi::Dbg::Touch(clear) => {
                        if clear {
                            touch::log_clear();
                        } else {
                            let n = touch::log_text(port.rsp_buf(), now);
                            port.respond_len(n);
                        }
                        false
                    }
                    scpi::Dbg::Diag => {
                        let d = touch_diag(&tp, touch_events, dbg_ctl, loop_max_shown, app.prof, sec_shown, bt_tx);
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
                    scpi::Dbg::Prof(reset) => {
                        if reset {
                            prof::reset();
                        } else {
                            let n = prof::report(port.rsp_buf());
                            port.respond_len(n);
                        }
                        false
                    }
                    scpi::Dbg::Release => {
                        feeding = false;
                        false
                    }
                    scpi::Dbg::Reset => {
                        reset_at = Some(now + 50);
                        false
                    }
                    scpi::Dbg::HySpi(n, div) => {
                        // Bus self-test (hyspi.rs): the DMM is paused and its
                        // row rewritten afterwards; the logger owns the chip.
                        if logger.active() {
                            scpi.set_err("Settings conflict");
                            false
                        } else {
                            app.dmm_power(&mut clk);
                            let len = hyspi::self_test(n as u32, div, port.rsp_buf());
                            port.respond_len(len);
                            app.dmm.start(&mut clk);
                            app.meter.reset();
                            app.last_raw = None;
                            false
                        }
                    }
                    scpi::Dbg::Ad2(2, level, _, _) => {
                        log_draw = level;
                        false
                    }
                    scpi::Dbg::Ad2(kind, r26, n, b) => {
                        // Synchronous AD2 experiment in the current stock AC
                        // row; the row is rewritten afterwards.
                        if logger.active() || !logger::Logger::mode_ok(true, app.dmm.mode()) {
                            scpi.set_err("Settings conflict");
                            false
                        } else {
                            app.dmm_power(&mut clk);
                            let buf = port.rsp_buf();
                            let len = if kind == 0 {
                                ad2exp::torn(r26, n as u32, b != 0, buf)
                            } else {
                                ad2exp::noise(r26, n as u32, b, buf)
                            };
                            port.respond_len(len);
                            app.dmm.start(&mut clk);
                            app.meter.reset();
                            app.last_raw = None;
                            false
                        }
                    }
                },
                scpi::Out::Mmem(scpi::Mmem::Cat) => match csvlog::catalog_scpi(port.rsp_buf()) {
                    Ok(n) => {
                        port.respond_len(n);
                        false
                    }
                    Err(e) => {
                        scpi.set_err(if e == csvlog::GErr::Busy { "Mass storage busy" } else { "Mass storage error" });
                        false
                    }
                },
                scpi::Out::Mmem(scpi::Mmem::Data(name, off, len)) => {
                    // #4nnnn + up to 6000 bytes (dlog.rs: a LOGnnn.CSV is
                    // generated from its LOGnnn.DLG).
                    let b = port.rsp_buf();
                    let len = (len as usize).min(b.len().saturating_sub(6));
                    match dlog::data(&name, off, &mut b[6..6 + len]) {
                        Ok(n) => {
                            b[..2].copy_from_slice(b"#4");
                            for (i, d) in [1000usize, 100, 10, 1].iter().enumerate() {
                                b[2 + i] = b'0' + (n / d % 10) as u8;
                            }
                            port.respond_len(6 + n);
                        }
                        Err(dlog::DataErr::NotFound) => scpi.set_err("File name not found"),
                        Err(_) => scpi.set_err("Mass storage error"),
                    }
                    false
                }
                // Deleting files over an open radio link: USB only.
                scpi::Out::Mmem(scpi::Mmem::Del(_)) if !port.trusted() => {
                    scpi.set_err(bt::proto::REFUSED);
                    false
                }
                scpi::Out::Mmem(scpi::Mmem::Del(name)) => {
                    match csvlog::delete(&name) {
                        Ok(()) => {}
                        Err(csvlog::GErr::NotFound) => scpi.set_err("File name not found"),
                        Err(csvlog::GErr::Busy) => scpi.set_err("Mass storage busy"),
                        Err(_) => scpi.set_err("Mass storage error"),
                    }
                    false
                }
                // Starting a CSV recording over an open radio link: USB only
                // (it blocks host writes, e.g. staging an update, until
                // stopped; review 3443cea L5). LOG:CSV OFF / LOG:CSV? stay.
                scpi::Out::Log(scpi::Log::Csv(Some(true))) if !port.trusted() => {
                    scpi.set_err(bt::proto::REFUSED);
                    false
                }
                scpi::Out::Log(l) => match l {
                    scpi::Log::Csv(Some(on)) => {
                        if on {
                            // The DMM needs its analog side (as MEAS?).
                            app.dmm_power(&mut clk);
                            if !app.rec_start(now) {
                                scpi.set_err("Settings conflict");
                            }
                        } else {
                            csvlog::stop();
                        }
                        false
                    }
                    scpi::Log::Csv(None) => {
                        scpi.len = csvlog::status_scpi(&mut scpi.buf);
                        true
                    }
                    scpi::Log::State(Some(on)) => {
                        if on {
                            // AD1 needs a DC mode (1 DCV, 3 DCmV, 7/8/9 DCA),
                            // AD2 a stock AC row (0 ACV, 2 ACmV, 4/5/6 ACA);
                            // never AC+DC (16/17, alternating rows), ohm, cap,
                            // freq, temp (logger::Logger::mode_ok).
                            let m = app.dmm.mode();
                            if logger::Logger::mode_ok(logger.ad2(), m) {
                                app.dmm_power(&mut clk);
                                // Guarantee the full row is applied before the
                                // logger overrides R22 (AD1OSR) / R26 (AD2).
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
                    scpi::Log::Source(Some(ad2)) => {
                        // The source also selects what poll() reads: not
                        // switched under a running session.
                        if logger.active() && ad2 != logger.ad2() {
                            scpi.set_err("Settings conflict");
                        } else {
                            logger.set_ad2(ad2);
                        }
                        false
                    }
                    scpi::Log::Source(None) => {
                        scpi.len = 0;
                        scpi.put_bytes(if logger.ad2() { b"AD2" } else { b"AD1" });
                        true
                    }
                    scpi::Log::SRate(Some(hz)) => {
                        logger.set_req_hz(hz);
                        false
                    }
                    scpi::Log::SRate(None) => {
                        scpi.len = 0;
                        put_u32(&mut scpi, logger.req_hz());
                        true
                    }
                    scpi::Log::Gaps => {
                        scpi.len = 0;
                        let g = logger.gaps();
                        let vals = [logger.bad(), logger.max_gap_us(), g[0], g[1], g[2], g[3], g[4]];
                        for (i, v) in vals.iter().enumerate() {
                            if i > 0 {
                                scpi.put_bytes(b",");
                            }
                            put_u32(&mut scpi, *v);
                        }
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
                    scpi::Log::Chop(Some(on)) => {
                        logger.set_chop(on);
                        false
                    }
                    scpi::Log::Chop(None) => {
                        scpi.len = 0;
                        scpi.put_bytes(if logger.chop() { b"1" } else { b"0" });
                        true
                    }
                    scpi::Log::Rate => {
                        scpi.len = 0;
                        put_u32(&mut scpi, logger.measured_hz());
                        true
                    }
                    scpi::Log::Count => {
                        scpi.len = 0;
                        for (i, v) in [logger.buffered(), logger.captured(), logger.overwritten(), logger.missed(), logger.lost()]
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
                        let ad2 = logger.ad2();
                        let r26 = logger.r26();
                        let vals = [
                            logger.mode() as u32,
                            logger.range() as u32,
                            if ad2 { (r26 & 7) as u32 } else { logger.osr() as u32 },
                            logger.nominal_us(),
                            logger.measured_hz(),
                            logger.chop() as u32,
                            ad2 as u32,
                            if ad2 { logger.req_hz() } else { 0 },
                            logger.nominal_mhz(),
                            if ad2 { r26 as u32 } else { 0 },
                            (ad2 && logger.mode() == 0) as u32, // AC V: AC-coupled
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
                        let buf = port.rsp_buf();
                        let n = logger.encode(buf);
                        port.respond_len(n);
                        false
                    }
                },
            };
            if reply {
                port.respond(&scpi.buf[..scpi.len]);
            }
            let key = (app.dmm.func(), app.dmm.t_sensor());
            if key != func_key {
                func_key = key;
                host_key = [None, None];
                if by_host {
                    host_key[src] = Some(key);
                }
                // A wait of the other transport (or any, after DBG:TAP / KEY)
                // was not for this change: cut it to the quick budget.
                for (i, w) in scpi_wait.iter_mut().enumerate() {
                    if let Some(w) = w.as_mut().filter(|_| !by_host || i != src) {
                        w.1 = false;
                        w.3 = w.3.min(w.2 + SCPI_QUICK_MS);
                    }
                }
            }
            prof::lap(prof::SCPI);
        }

        // Any path that reconfigures the DMM (a host CONFigure, a touch/key
        // FUNC/RANGE change, a BT mode-byte) rewrites the 22-byte HY3131 config
        // and R22 behind the logger's back. Detect that and auto-stop so the
        // host sees STATe?=0 + a SYST:ERR? rather than silently corrupted
        // samples (FR-38 forbids silent loss). Captured data stays readable.
        if logger.active() && (app.dmm.mode() != logger.mode() || app.dmm.range() != logger.range()) {
            logger.stop();
            for s in scpis.iter_mut() {
                s.set_err("Settings conflict");
            }
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
        // DBG:AD2:DRAW 2: the DMM page's redraw load during a log session
        // (loss-per-rate experiment): a full view repaint every 100 ms, then
        // the panel again on top.
        let force_draw = logging && log_draw == 2 && usb_up && now >= log_redraw_at && !screenshot::frozen();
        if force_draw {
            log_redraw_at = now + 100;
            app.invalidate(); // drawn by app.tick below; the panel after it
        }
        if logging && log_draw >= 1 && usb_up && now >= draw_hold_until && !screenshot::frozen() {
            // The logger panel (dialog.rs; one part per pass, ~1 s refresh).
            dialog::log_panel(
                now,
                dialog::LogInfo {
                    hz: logger.measured_hz(),
                    n: logger.captured(),
                    nominal_hz: (logger.nominal_mhz() + 500) / 1000,
                    ad2: logger.ad2(),
                    lost: logger.lost(),
                },
            );
        }
        mark(0, &mut ts);
        // CSV recording: one bounded slice of a commit (csvlog.rs).
        prof::lap_skip();
        csvlog::poll(now);
        upload::poll(now);
        // The generated USB volume (vdisk.rs): rebuilt when the SPI files
        // changed (a recording ended, a screenshot / scope record, a delete),
        // once nothing changed for 1 s and no on-device writer holds the disk.
        #[cfg(feature = "vdisk")]
        {
            if dlog::gen() != vd_gen {
                vd_gen = dlog::gen();
                vd_t = now;
            } else if dlog::stale() && now.saturating_sub(vd_t) >= 1000 && !csvlog::writing() && !screenshot::busy() {
                vdisk::refresh(csvlog::recording_name());
            }
        }
        prof::lap(prof::CSV);
        app.tick(&mut clk, can_draw || force_draw);
        if force_draw {
            dialog::log_panel_reset(); // the repaint covered it: draw it again
        }
        prof::lap(prof::TICK);
        mark(1, &mut ts);
        for src in [USB, BT] {
            // Power meter: a wait for the host's own settle follows its range
            // hunt (each step restarts the P / V / I cycle, ~1.9 s).
            if let Some(w) = scpi_wait[src].as_mut().filter(|w| w.1 && app.dmm.pw.on) {
                w.3 = w.3.max(app.dmm.deadline() + SCPI_WAIT_MS).min(w.2 + 15_000);
            }
            if !scpi_wait[src].is_some_and(|w| now >= w.3) {
                continue;
            }
            // No fresh DMM reading in time (a slow capacitance / temperature
            // range, a range hunt, a stalled converter): answer now rather
            // than after the host's timeout, with the latest reading of the
            // current function if there is one.
            scpi_wait[src] = None;
            let scpi = &mut scpis[src];
            scpi.len = 0;
            match app.meter.live().copied() {
                Some(r) => scpi.put_reading(&r),
                None => {
                    scpi.put_bytes(b"+9.90000000E+37");
                    scpi.set_err("Data stale");
                }
            }
            let port: &mut dyn bt::ScpiPort = if src == BT { &mut bt } else { &mut tmc };
            port.respond(&scpi.buf[..scpi.len]);
        }
        if let Some((c, _)) = dbg_key_up.filter(|k| now >= k.1) {
            dbg_key_up = None;
            app.gesture(&mut clk, gesture::Gesture::KeyUp(c));
        }

        // Suspend touch while logging: there is no logger UI (the screen is
        // frozen), the power key lives in app.tick, and the touch I2C read
        // every 10 ms is the largest periodic jitter source for the sampler.
        // EXTI-latched INT edge (touch.rs): read the fresh report now, not on
        // the tick.
        let touch_irq = tp.as_ref().map_or(false, |t| t.irq_pending());
        if (now >= next_touch || touch_irq) && !logging {
            next_touch = now + 10;
            let t_rd = prof::cyc();
            prof::enter(prof::TOUCH);
            let mut reported = false;
            let g = match tp.as_mut().and_then(|t| t.poll(now)) {
                Some(ev) => {
                    touch_events = touch_events.wrapping_add(1);
                    reported = true;
                    gestures.event(&ev, now)
                }
                None => gestures.tick(now),
            };
            prof::exit(prof::TOUCH);
            if let Some(g) = g {
                if reported {
                    prof::touch(t_rd); // latency from the report read
                }
                prof::enter(prof::GEST);
                if app.gesture(&mut clk, g) {
                    if app.slider_active() { gestures.capture_direct() } else { gestures.capture() }
                }
                prof::exit(prof::GEST);
            }
        }
        mark(2, &mut ts);
        if !logging {
            prof::enter(prof::SCOPE);
            app.scope_poll(now, can_draw);
            prof::exit(prof::SCOPE);
        }
        mark(3, &mut ts);
        if now >= next_poll {
            next_poll = now + 5;
            prof::enter(prof::BT);
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
                    if bt.serve(&f, &mut clk, &mut app.dmm, &mut app.meter, raw.as_ref(), &app.power, app.locked) {
                        // Phone app changed the function (cmd 06): the same
                        // path as a touch / SCPI change.
                        app.host_func_changed();
                    }
                }
            }
            prof::exit(prof::BT);
            mark(4, &mut ts);
            prof::enter(prof::DMM);
            if !logging {
                if let Some(raw) = app.dmm.poll(&mut clk) {
                    if let Some(r) = app.reading(raw, can_draw) {
                        for src in [USB, BT] {
                        if let Some((_, settle, t0, _)) = scpi_wait[src].filter(|w| r.seq.wrapping_sub(w.0) < 0x8000_0000) {
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
                                scpi_wait[src] = None;
                                let scpi = &mut scpis[src];
                                scpi.len = 0;
                                scpi.put_reading(&r);
                                let port: &mut dyn bt::ScpiPort = if src == BT { &mut bt } else { &mut tmc };
                                port.respond(&scpi.buf[..scpi.len]);
                            }
                        }
                        }
                    }
                }
            }
            prof::exit(prof::DMM);
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
    bt_tx: (bool, u32),
) -> [u8; DIAG_LEN] {
    let mut d = [0u8; DIAG_LEN];
    // BT TX: 60 DMA path working (0 = blocking fallback), 64 bytes dropped
    // because the TX ring was full (u32).
    d[60] = bt_tx.0 as u8;
    d[64..68].copy_from_slice(&bt_tx.1.to_le_bytes());
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
        d[20..60].copy_from_slice(&t.diag()); // touch bus / IRQ counters
    }
    d
}

/// Stop feeding the watchdog and wait for the FWDGT reset (the shim counts it
/// as a crash).
fn wait_for_watchdog() -> ! {
    loop {
        mmio::spin(1_000_000);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Mark the panic on screen; do not feed the watchdog: the FWDGT resets and
    // the shim counts a crash (WDT_RETRY, then recovery).
    lcd::fill_rect(0, lcd::H - 24, lcd::W, 24, lcd::MAGENTA);
    wait_for_watchdog()
}

// ---------------------------------------------------------------------------
// Native-format image (docs/native-boot.md "Implementation"). Watchdog policy
// (src/boot.rs): INITIAL_HOLD_MS above makes the main loop feed the FWDGT for
// as long as it runs.
// DBG:RELEASE, a panic or a hang stop the feeding -> FWDGT reset; the shim
// counts it as a crash: the 1st reboots fw-rs normally (WDT_RETRY), a 2nd one
// without 60 s of healthy main loop in between (boot::mark_healthy) enters
// recovery (marker zeroed -> vendor update screen). DBG:RESET = SYSRESETREQ ->
// normal reboot of fw-rs. DBG:UPDATE / DBG:RECOVER zero the 12-byte loader
// marker (boot::enter_loader_update) and reset -> the vendor loader's update
// screen with its own USB disk until a dm40*.bin is installed. (No backup-SRAM
// request word is set by any command now.)
mod board;
mod boot;
mod fatren;
mod calbackup;
#[cfg(feature = "vdisk")]
mod vdisk;
mod dialog;
mod screenshot;
mod scope_csv;
mod csvlog;
mod dlog;
mod gallery;
mod fatdir;
mod upload;
mod native;
// Stock device-authenticity check, informational only: DBG:GENUINE? reports
// it; nothing changes device behaviour on its result (user decision).
// (`check` / `verify` are for other callers / the host tests.)
#[allow(dead_code)]
mod genuine;
mod recovery;
mod shim;
