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
mod hy3131;
mod hy3131_tables;
mod lcd;
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
const INITIAL_HOLD_MS: u64 = 60_000;
const KEEPALIVE_MS: u64 = 15_000;
const KEEPALIVE_MAX_MS: u64 = 600_000;

/// pid.codes test VID/PID, for the debug build only.
const USB_VID_PID: UsbVidPid = UsbVidPid(0x1209, 0x0001);

static mut EP_MEMORY: [u32; 320] = [0; 320];
static mut MSC_IO: [u8; 512] = [0; 512];

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
    // storage (interface 1). Two IN endpoints + EP0 of the four of USBFS.
    let mut tmc = usbtmc::Usbtmc::new(&bus);
    let mut msc = msc::Msc::new(&bus, unsafe { &mut *core::ptr::addr_of_mut!(MSC_IO) });
    let mut disk = msc::NoMedia;
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
    // SCPI READ?/MEASure?: reply with the first reading whose seq is >= this.
    let mut scpi_wait: Option<u32> = None;
    let mut bt = bt::Bt::new();
    let mut gestures = gesture::Tracker::new();
    let mut touch_events = 0u32;
    let mut next_touch = 0u64;
    let mut next_poll = 0u64;
    let mut draw_hold_until = 0u64;

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
        if feeding && now < deadline {
            wdt::kick();
        }
        if reset_at.map(|t| now >= t).unwrap_or(false) {
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        dev.poll(&mut [&mut tmc, &mut msc]);
        msc.poll(&mut disk);
        let mut msg = [0u8; 256];
        let mut msg_n = 0usize;
        if let Some(cmd) = tmc.take_command() {
            msg_n = cmd.len().min(256);
            msg[..msg_n].copy_from_slice(&cmd[..msg_n]);
            // Any host traffic keeps the debug-build watchdog fed.
            deadline = deadline.max(now + KEEPALIVE_MS);
        }
        if msg_n > 0 {
            let reply = match scpi.handle(&msg[..msg_n], app.dmm.mode()) {
                scpi::Out::None => false,
                scpi::Out::Text => true,
                scpi::Out::Configure(m) => {
                    if m != app.dmm.mode() {
                        app.dmm.select(&mut clk, m);
                        app.meter.reset();
                        app.last_raw = None;
                    }
                    false
                }
                scpi::Out::Measure(m) => {
                    // The multimeter needs its analog side powered.
                    app.dmm_power(&mut clk);
                    if let Some(m) = m.filter(|&m| m != app.dmm.mode()) {
                        app.dmm.select(&mut clk, m);
                        app.meter.reset();
                        app.last_raw = None;
                    }
                    scpi_wait = Some(app.last_raw.map(|r| r.seq.wrapping_add(1)).unwrap_or(0));
                    false
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
                        scpi_wait = Some(0);
                        false
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
                            3 => Some(gesture::Gesture::KeyDown(x as u8)),
                            _ => None,
                        };
                        if let Some(g) = g {
                            app.gesture(&mut clk, g);
                        }
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
                    scpi::Dbg::Release => {
                        feeding = false;
                        false
                    }
                    scpi::Dbg::Reset => {
                        reset_at = Some(now + 50);
                        false
                    }
                },
            };
            if reply {
                tmc.respond(&scpi.buf[..scpi.len]);
            }
        }

        // No drawing until USB has enumerated (or 3 s passed): a long first
        // draw must not starve the enumeration; nor right after a GRAM
        // read-back, so a multi-request screenshot does not tear.
        let usb_up = dev.state() == UsbDeviceState::Configured || now > 3000;
        let can_draw = usb_up && now >= draw_hold_until;
        mark(0, &mut ts);
        app.tick(&mut clk, can_draw);
        mark(1, &mut ts);

        if now >= next_touch {
            next_touch = now + 10;
            let g = match tp.as_mut().and_then(|t| t.poll(now)) {
                Some(ev) => {
                    touch_events = touch_events.wrapping_add(1);
                    gestures.event(&ev, now)
                }
                None => gestures.tick(now),
            };
            if let Some(g) = g {
                app.gesture(&mut clk, g);
            }
        }
        mark(2, &mut ts);
        app.scope_poll(now, can_draw);
        mark(3, &mut ts);
        if now >= next_poll {
            next_poll = now + 5;
            if let Some(f) = bt.poll() {
                if app.bt_on {
                    let raw = app.last_raw;
                    bt.serve(&f, &mut clk, &mut app.dmm, &mut app.meter, raw.as_ref());
                }
            }
            mark(4, &mut ts);
            if let Some(raw) = app.dmm.poll(&mut clk) {
                if let Some(r) = app.reading(raw, can_draw) {
                    if let Some(min) = scpi_wait {
                        if r.seq.wrapping_sub(min) < 0x8000_0000 {
                            scpi_wait = None;
                            scpi.len = 0;
                            scpi.put_reading(&r);
                            tmc.respond(&scpi.buf[..scpi.len]);
                        }
                    }
                }
            }
            mark(5, &mut ts);
        }
    }
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
