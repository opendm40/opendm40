#![no_std]
#![no_main]

mod clock;
mod dmm;
mod fonts;
mod gesture;
mod host;
mod hy3131;
mod hy3131_tables;
mod lcd;
mod mmio;
mod scpi;
mod startup;
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
    static __VECTORS: u32;
}

/// Called from the reset trampoline (startup.rs) with MSP set and IRQs masked.
#[no_mangle]
pub unsafe extern "C" fn rust_start() -> ! {
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
        ui::draw_static();
    }

    let hw = usb::init_hw(&mut clk);
    wdt::kick();
    fail(ST_PLLSAI, !hw.pllsai_locked);
    fail(ST_CORE, !hw.core_idle);

    let bus = synopsys_usb_otg::UsbBus::new(usb::UsbFs, unsafe {
        &mut *core::ptr::addr_of_mut!(EP_MEMORY)
    });
    let mut port = SerialPort::new(&bus);
    let mut tmc = usbtmc::Usbtmc::new(&bus);
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

    let mut dmm = dmm::Dmm::new();
    dmm.select(&mut clk, 1);
    let mut scpi = scpi::Scpi::new();
    let mut last_reading: Option<dmm::Reading> = None;
    // SCPI READ?/MEASure?: reply with the first reading whose seq is >= this.
    let mut scpi_wait: Option<u32> = None;
    let mut screen = ui::Screen::new();
    let mut tp = touch::Touch::new(&mut clk);
    let mut gestures = gesture::Tracker::new();
    let mut touch_events = 0u32;
    let mut next_touch = 0u64;
    let mut next_poll = 0u64;
    let mut draw_hold_until = 0u64;

    loop {
        let now = clk.now_ms();
        if feeding && now < deadline {
            wdt::kick();
        }

        dev.poll(&mut [&mut port, &mut tmc]);
        if let Some(cmd) = tmc.take_command() {
            let mut msg = [0u8; usbtmc::CMD_MAX];
            let n = cmd.len();
            msg[..n].copy_from_slice(cmd);
            match scpi.handle(&msg[..n], dmm.mode()) {
                scpi::Out::None => {}
                scpi::Out::Text => tmc.respond(&scpi.buf[..scpi.len]),
                scpi::Out::Configure(m) => {
                    if dmm::button_of(m) != dmm.button() || m != dmm.mode() {
                        dmm.select(&mut clk, m);
                        last_reading = None;
                    }
                }
                scpi::Out::Measure(m) => {
                    if let Some(m) = m.filter(|&m| m != dmm.mode()) {
                        dmm.select(&mut clk, m);
                        last_reading = None;
                    }
                    scpi_wait = Some(last_reading.map(|r| r.seq.wrapping_add(1)).unwrap_or(0));
                }
                scpi::Out::Fetch => match last_reading {
                    Some(r) => {
                        scpi.len = 0;
                        scpi.put_reading(&r);
                        tmc.respond(&scpi.buf[..scpi.len]);
                    }
                    None => scpi_wait = Some(0),
                },
            }
        }
        let action = host.poll(&mut port, || host::Info {
            uptime_ms: now as u32,
            flags: hw.pllsai_locked as u32 | (hw.core_idle as u32) << 1,
            keepalive_left_ms: if feeding { deadline.saturating_sub(now) as u32 } else { 0 },
            diag: touch_diag(&tp, touch_events, tmc.dbg_ctl),
        });
        match action {
            host::Action::None => {}
            host::Action::Keepalive(ms) => {
                let ms = if ms == 0 { KEEPALIVE_MS } else { (ms as u64).min(KEEPALIVE_MAX_MS) };
                deadline = deadline.max(now + ms);
            }
            host::Action::StopFeeding => feeding = false,
        }
        if host.reset_due() {
            // Give the host a moment to collect the reply, then reset to stock.
            let t = clk.now_ms() + 20;
            while clk.now_ms() < t {
                dev.poll(&mut [&mut port, &mut tmc]);
            }
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        // Measure and draw, but never while a reply is streaming (a GRAM
        // read-back may be in progress on the LCD bus), and not for a while
        // after one, so a multi-request screenshot does not tear.
        if host.take_lcd_read() {
            draw_hold_until = now + 300;
        }
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
                if now >= draw_hold_until {
                    screen.gesture_debug(&g);
                }
                // Temporary: swipes change the function button, a tap on the
                // value cycles the sub-mode.
                let bs = dmm::Button::ALL;
                let i = bs.iter().position(|&b| b == dmm.button()).unwrap_or(0);
                let next = match g {
                    gesture::Gesture::SwipeLeft => Some(bs[(i + 1) % bs.len()]),
                    gesture::Gesture::SwipeRight => Some(bs[(i + bs.len() - 1) % bs.len()]),
                    gesture::Gesture::Tap(p) if p.y > 40 && p.y < 160 => Some(dmm.button()),
                    _ => None,
                };
                if let Some(b) = next {
                    dmm.press(&mut clk, b);
                    last_reading = None;
                    screen.invalidate();
                }
            }
        }
        if now >= next_poll && !host.busy() {
            next_poll = now + 5;
            if let Some(r) = dmm.poll(&mut clk) {
                last_reading = Some(r);
                if let Some(min) = scpi_wait {
                    if r.seq.wrapping_sub(min) < 0x8000_0000 {
                        scpi_wait = None;
                        scpi.len = 0;
                        scpi.put_reading(&r);
                        tmc.respond(&scpi.buf[..scpi.len]);
                    }
                }
                if now >= draw_hold_until {
                    screen.reading(&r);
                }
            }
        }
    }
}

/// DIAG block 0: touch chip[4], found, -, -, -, events u32, last raw frame[7];
/// USBTMC control log at 80..97.
fn touch_diag(tp: &Option<touch::Touch>, events: u32, ctl: (u8, [u8; 16])) -> [u8; host::DIAG_LEN] {
    let mut d = [0u8; host::DIAG_LEN];
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
