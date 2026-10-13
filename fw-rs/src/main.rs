#![no_std]
#![no_main]

mod clock;
mod dmm;
mod fonts;
mod host;
mod hy3131;
mod lcd;
mod mmio;
mod startup;
mod text;
mod ui;
mod usb;
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
    let mut dev = UsbDeviceBuilder::new(&bus, USB_VID_PID)
        .strings(&[StringDescriptors::default()
            .manufacturer("dm40-fw")
            .product("DM40 RAM firmware (debug)")
            .serial_number("0001")])
        .unwrap()
        .device_class(usbd_serial::USB_CLASS_CDC)
        .build();
    // build() enabled the core; now force the GD32 power/VBUS bits.
    usb::force_power_no_vbus();

    let mut host = host::Host::new();
    let mut deadline = clk.now_ms() + INITIAL_HOLD_MS;
    let mut feeding = true;

    let mut dmm = dmm::Dmm::new();
    dmm.set_mode(&mut clk, dmm::Mode::DcV);
    let mut screen = ui::Screen::new();
    let mut next_poll = 0u64;
    let mut draw_hold_until = 0u64;

    loop {
        let now = clk.now_ms();
        if feeding && now < deadline {
            wdt::kick();
        }

        dev.poll(&mut [&mut port]);
        let action = host.poll(&mut port, || host::Info {
            uptime_ms: now as u32,
            flags: hw.pllsai_locked as u32 | (hw.core_idle as u32) << 1,
            keepalive_left_ms: if feeding { deadline.saturating_sub(now) as u32 } else { 0 },
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
                dev.poll(&mut [&mut port]);
            }
            unsafe { mmio::w32(SCB_AIRCR, 0x05FA_0004) };
        }

        // Measure and draw, but never while a reply is streaming (a GRAM
        // read-back may be in progress on the LCD bus), and not for a while
        // after one, so a multi-request screenshot does not tear.
        if host.take_lcd_read() {
            draw_hold_until = now + 300;
        }
        if now >= next_poll && !host.busy() {
            next_poll = now + 5;
            if let Some(r) = dmm.poll() {
                if now >= draw_hold_until {
                    screen.reading(&r);
                }
            }
        }
    }
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
