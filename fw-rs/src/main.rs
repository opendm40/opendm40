#![no_std]
#![no_main]

mod app;
mod bt;
mod clock;
mod dmm;
mod fonts;
mod gen;
mod gesture;
mod host;
mod hy3131;
mod hy3131_tables;
mod lcd;
mod meter;
mod mmio;
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
use usbd_serial::SerialPort;

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
    // USBTMC first (interface 0), then the CDC debug port (interfaces 1, 2):
    // with USBTMC behind CDC its capabilities request got lost at probe time.
    let mut tmc = usbtmc::Usbtmc::new(&bus);
    let mut port = SerialPort::new(&bus);
    let mut dev = UsbDeviceBuilder::new(&bus, USB_VID_PID)
        .strings(&[StringDescriptors::default()
            .manufacturer("dm40-fw")
            .product("DM40 multimeter (dm40-fw)")
            .serial_number("0001")])
        .unwrap()
        .composite_with_iads()
        .build();
    // build() enabled the core; now force the GD32 power/VBUS bits.
    usb::force_power_no_vbus();

    let mut host = host::Host::new();
    let mut deadline = clk.now_ms() + INITIAL_HOLD_MS;
    let mut feeding = true;

    let mut scpi = scpi::Scpi::new();
    // SCPI READ?/MEASure?: reply with the first reading whose seq is >= this.
    let mut scpi_wait: Option<(u32, bool)> = None;
    let mut scpi_line_ready = false;
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

    loop {
        let now = clk.now_ms();
        let t = unsafe { mmio::r32(0xE000_1004) };
        loop_max = loop_max.max(t.wrapping_sub(loop_t0) / 200);
        loop_t0 = t;
        if now >= loop_win {
            loop_win = now + 1000;
            loop_max_shown = loop_max;
            loop_max = 0;
        }
        if feeding && now < deadline {
            wdt::kick();
        }

        dev.poll(&mut [&mut tmc, &mut port]);
        // SCPI from USBTMC or from a text line on the serial port.
        let mut msg = [0u8; 256];
        let mut msg_n = 0usize;
        let mut from_tmc = false;
        if let Some(cmd) = tmc.take_command() {
            msg_n = cmd.len().min(256);
            msg[..msg_n].copy_from_slice(&cmd[..msg_n]);
            from_tmc = true;
        } else if scpi_line_ready {
            scpi_line_ready = false;
            msg_n = host.take_line(&mut msg);
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
                    app.scope.power(false, &mut clk);
                    if let Some(m) = m.filter(|&m| m != app.dmm.mode()) {
                        app.dmm.select(&mut clk, m);
                        app.meter.reset();
                        app.last_raw = None;
                    }
                    scpi_wait = Some((app.last_raw.map(|r| r.seq.wrapping_add(1)).unwrap_or(0), from_tmc));
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
                        scpi_wait = Some((0, from_tmc));
                        false
                    }
                },
            };
            if reply {
                if from_tmc {
                    tmc.respond(&scpi.buf[..scpi.len]);
                } else {
                    host.reply_text(&scpi.buf[..scpi.len]);
                }
            }
        }
        let action = host.poll(&mut port, || host::Info {
            uptime_ms: now as u32,
            flags: hw.pllsai_locked as u32 | (hw.core_idle as u32) << 1,
            keepalive_left_ms: if feeding { deadline.saturating_sub(now) as u32 } else { 0 },
            diag: touch_diag(&tp, touch_events, tmc.dbg_ctl, loop_max_shown, app.prof),
        });
        match action {
            host::Action::None => {}
            host::Action::Keepalive(ms) => {
                let ms = if ms == 0 { KEEPALIVE_MS } else { (ms as u64).min(KEEPALIVE_MAX_MS) };
                deadline = deadline.max(now + ms);
            }
            host::Action::StopFeeding => feeding = false,
            host::Action::Scpi => scpi_line_ready = true,
            host::Action::Input(kind, arg) => {
                deadline = deadline.max(now + KEEPALIVE_MS);
                let p = touch::Point { x: arg as u16, y: (arg >> 16) as u16 };
                let g = match kind {
                    0 => Some(gesture::Gesture::Tap(p)),
                    1 => Some(gesture::Gesture::SwipeLeft),
                    2 => Some(gesture::Gesture::SwipeRight),
                    3 => Some(gesture::Gesture::KeyDown(arg as u8)),
                    _ => None,
                };
                if let Some(g) = g {
                    app.gesture(&mut clk, g);
                }
            }
        }
        if host.reset_due() {
            // Give the host a moment to collect the reply, then reset to stock.
            let t = clk.now_ms() + 20;
            while clk.now_ms() < t {
                dev.poll(&mut [&mut tmc, &mut port]);
            }
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        // Draw only when no reply is streaming (a GRAM read-back may be in
        // progress on the LCD bus) and not for a while after one, so a
        // multi-request screenshot does not tear.
        if host.take_lcd_read() {
            draw_hold_until = now + 300;
        }
        // No drawing until USB has enumerated (or 3 s passed): a long first
        // draw must not starve the enumeration.
        let usb_up = dev.state() == UsbDeviceState::Configured || now > 3000;
        let can_draw = usb_up && !host.busy() && now >= draw_hold_until;
        app.tick(&mut clk, can_draw);

        if now >= next_touch && !host.busy() {
            next_touch = now + 10;
            let g = match tp.as_mut().and_then(|t| t.poll()) {
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
        if !host.busy() {
            app.scope_poll(now, can_draw);
        }
        if now >= next_poll && !host.busy() {
            next_poll = now + 5;
            if let Some(f) = bt.poll() {
                if app.bt_on {
                    let raw = app.last_raw;
                    bt.serve(&f, &mut clk, &mut app.dmm, &mut app.meter, raw.as_ref());
                }
            }
            if let Some(raw) = app.dmm.poll(&mut clk) {
                if let Some(r) = app.reading(raw, can_draw) {
                    if let Some((min, to_tmc)) = scpi_wait {
                        if r.seq.wrapping_sub(min) < 0x8000_0000 {
                            scpi_wait = None;
                            scpi.len = 0;
                            scpi.put_reading(&r);
                            if to_tmc {
                                tmc.respond(&scpi.buf[..scpi.len]);
                            } else {
                                host.reply_text(&scpi.buf[..scpi.len]);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn touch_diag(tp: &Option<touch::Touch>, events: u32, ctl: (u8, [u8; 16]), loop_us: u32, prof: [u32; 3]) -> [u8; host::DIAG_LEN] {
    let mut d = [0u8; host::DIAG_LEN];
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
