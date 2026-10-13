#![no_std]
#![no_main]

mod clock;
mod host;
mod lcd;
mod mmio;
mod startup;
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

/// Bring-up progress as a row of squares at the top left; one per stage.
fn status(slot: u16, color: lcd::Color) {
    lcd::fill_rect(8 + slot * 28, 8, 20, 20, color);
}

const ST_ALIVE: u16 = 0;
const ST_PLLSAI: u16 = 1;
const ST_CORE: u16 = 2;
const ST_CONFIGURED: u16 = 3;
const ST_HOST: u16 = 4;

fn main() -> ! {
    wdt::kick();
    let mut clk = Clock::new();
    lcd::set_madctl(lcd::MADCTL_LANDSCAPE);
    lcd::fill_screen(lcd::BLACK);
    status(ST_ALIVE, lcd::WHITE);

    let hw = usb::init_hw(&mut clk);
    wdt::kick();
    status(ST_PLLSAI, if hw.pllsai_locked { lcd::GREEN } else { lcd::RED });
    status(ST_CORE, if hw.core_idle { lcd::GREEN } else { lcd::RED });

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
    let mut configured = false;
    let mut host_seen = false;

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
                host_seen = true;
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

        // Status squares only while no reply is streaming (GRAM read-back).
        if !host.busy() {
            let c = dev.state() == UsbDeviceState::Configured;
            if c != configured {
                configured = c;
                status(ST_CONFIGURED, if c { lcd::GREEN } else { lcd::GREY });
            }
            if host_seen {
                host_seen = false;
                status(ST_HOST, if feeding { lcd::GREEN } else { lcd::YELLOW });
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
