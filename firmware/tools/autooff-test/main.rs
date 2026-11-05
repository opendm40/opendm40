//! Host test of src/autooff.rs: the idle shutdown runs on battery only.
//! Simulates App::tick's use of it (one pass per 100 ms). Needs no hardware.
//!   tools/autooff-test/run.sh
#[path = "../../src/autooff.rs"]
mod autooff;
use autooff::*;

static mut BAD: u32 = 0;
static mut N: u32 = 0;
fn t(name: &str, ok: bool) {
    unsafe {
        N += 1;
        if !ok {
            BAD += 1;
            println!("FAIL {name}");
        }
    }
}

const OFF: u64 = 600_000; // Shutdown 10 min

/// Runs App::tick's rule from 0 to `end` ms; `state(now)` -> (pins, configured).
/// Returns the power-off time, if any.
fn run(end: u64, off_ms: u64, held: bool, state: impl Fn(u64) -> (u8, bool)) -> Option<u64> {
    let (last_input, mut last_host) = (0u64, 0u64);
    let mut now = 0;
    while now <= end {
        let (pins, conf) = state(now);
        if usb_power(pins, conf) {
            last_host = last_host.max(now);
        }
        if off_due(idle_ms(now, last_input, last_host), off_ms, held) {
            return Some(now);
        }
        now += 100;
    }
    None
}

/// The Shutdown setting's codes in p[0], old records first.
fn setting_tests() {
    // A record from before the longer list: bits 4..5 hold 0..3, bits 6..7
    // are 0; its other p[0] fields do not change the meaning.
    for (code, ms, txt) in [(0u32, 600_000u64, "10 min"), (1, 1_800_000, "30 min"), (2, 3_600_000, "1 hour"), (3, 0, "Never")] {
        for other in [0u32, 0xFFFF_FF0F] {
            let c = off_code(code << 4 | other);
            t("old record's Shutdown keeps its meaning", OFF_MS[c] == ms && OFF_TXT[c] == txt);
        }
    }
    t("default 30 min", OFF_MS[OFF_DEFAULT] == 1_800_000);
    t("unknown code reads as the default", off_code(15 << 4) == OFF_DEFAULT);
    // Every code survives the write + read of bits 4..7.
    for c in 0..OFF_MS.len() {
        t("code round trip", off_code((c as u32) << 4) == c);
    }
    // "+" from the shortest walks all 15, rising, Never last; no wrap-around.
    let mut c = 4;
    t("1 min is the first", OFF_MS[c] == 60_000);
    t("no step below the first", off_step(c, -1) == c);
    for _ in 0..OFF_MS.len() - 2 {
        let n = off_step(c, 1);
        t("rising order", OFF_MS[n] > OFF_MS[c]);
        t("- undoes +", off_step(n, -1) == c);
        c = n;
    }
    c = off_step(c, 1);
    t("Never last", c == 3);
    t("no step above Never", off_step(c, 1) == 3);
    // The shortest choice: the warning (30 s before) has its whole time.
    let min = OFF_MS.iter().copied().filter(|&m| m != 0).min().unwrap_or(0);
    t("1 min is the shortest", min == 60_000);
    t("warning fits the shortest", min >= 2 * OFF_WARN_MS);
    t("not due at the warning", !off_due(min - OFF_WARN_MS, min, false));
}

fn main() {
    setting_tests();
    // The rule itself.
    t("PB10 high", usb_power(0b01, false));
    t("PB10 + PB11 (charging)", usb_power(0b11, false));
    t("configured host, PB10 low", usb_power(0b00, true));
    t("PB11 alone is not USB power", !usb_power(0b10, false));
    t("battery", !usb_power(0, false));
    t("idle from the later of input / host", idle_ms(1000, 200, 700) == 300);
    t("idle before an event stamp is 0", idle_ms(100, 200, 0) == 0);
    t("Never", !off_due(u64::MAX, 0, false));
    t("held", !off_due(OFF + 1, OFF, true));
    t("due only past the time", !off_due(OFF, OFF, false) && off_due(OFF + 1, OFF, false));

    let h = 3 * 3_600_000;
    // Battery: off after the shutdown time.
    t("battery: off at 10 min", run(h, OFF, false, |_| (0, false)) == Some(OFF + 100));
    // USB power on PB10 (a wall charger: never configured).
    t("charger: never off", run(h, OFF, false, |_| (1, false)).is_none());
    // A PC: configured, PB10 reads 0 (the 2026-11-01 power-offs).
    t("PC, PB10 low: never off", run(h, OFF, false, |_| (0, true)).is_none());
    // PB10 low ~10 ms in every 2 s (the stock's battery-reading pulse) or
    // flickering: never off.
    t("PB10 pulses: never off", run(h, OFF, false, |n| ((n % 2000 >= 100) as u8, false)).is_none());
    // A suspended host (no Configured state) with VBUS on: PB10 holds it.
    t("suspended host: never off", run(h, OFF, false, |_| (1, false)).is_none());
    // Unplug at 1 h: counts afresh from the unplug.
    let unplug = 3_600_000;
    t(
        "unplug: off 10 min after it",
        run(h, OFF, false, |n| if n < unplug { (1, true) } else { (0, false) }) == Some(unplug - 100 + OFF + 100),
    );
    // DBG:IDLE 60000 on battery.
    t("DBG:IDLE 60 s on battery", run(h, 60_000, false, |_| (0, false)) == Some(60_100));

    let (bad, n) = unsafe { (BAD, N) };
    println!("autooff-test: {}/{} passed", n - bad, n);
    if bad != 0 {
        std::process::exit(1);
    }
}
