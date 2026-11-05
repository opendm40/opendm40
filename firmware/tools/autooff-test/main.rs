//! Host test of src/autooff.rs: the idle shutdown runs on battery only.
//! Simulates App::tick's use of it (one pass per 100 ms). Needs no hardware.
//!   tools/autooff-test/run.sh
#[path = "../../src/autooff.rs"]
mod autooff;
#[path = "../../src/lowbat.rs"]
mod lowbat;
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

/// The low-battery warning (src/lowbat.rs): feed reads, count the warnings.
fn lowbat_tests() {
    use lowbat::*;
    let n = CONFIRM as usize;
    let run = |l: &mut LowBat, pct: f32, chg: bool, reads: usize| {
        let mut w = 0;
        for _ in 0..reads {
            l.feed(pct, chg);
            if l.due() {
                l.shown();
                w += 1;
            }
        }
        w
    };
    let mut l = LowBat::new();
    t("lb: above the levels", run(&mut l, 17.0, false, 100) == 0 && !l.low());
    t("lb: short dip no warning", run(&mut l, 10.0, false, n - 1) == 0 && !l.low());
    t("lb: interrupted dip starts over", run(&mut l, 50.0, false, 1) == 0 && run(&mut l, 10.0, false, n - 1) == 0);
    run(&mut l, 50.0, false, 1);
    t("lb: warns once at 17", run(&mut l, 10.0, false, n) == 1 && l.low());
    t("lb: no repeat", run(&mut l, 10.0, false, 1000) == 0);
    t("lb: rebound does not re-arm", run(&mut l, 30.0, false, 100) == 0 && l.low() && run(&mut l, 10.0, false, 100) == 0);
    t("lb: warns again at 8", run(&mut l, 5.0, false, n) == 1);
    t("lb: no third", run(&mut l, -5.0, false, 1000) == 0);
    t("lb: charger clears", run(&mut l, 5.0, true, 1) == 0 && !l.low());
    t("lb: never on a charger", run(&mut l, 5.0, true, 1000) == 0 && !l.low());
    t("lb: warns after the unplug", run(&mut l, 5.0, false, n) == 1);
    let mut l = LowBat::new();
    t("lb: 0 V is no battery reading", run(&mut l, -366.0, false, 1000) == 0 && !l.low());
    // The power-off: 30 s under 1 %, then the 30 s countdown.
    let confirm = CUT_CONFIRM as usize;
    let warn = CUT_WARN as usize;
    let mut l = LowBat::new();
    for _ in 0..confirm - 1 {
        l.feed(0.5, false);
    }
    t("cut: no warning before the confirm", l.take_cut_warn().is_none() && !l.cut_due());
    l.feed(5.0, false);
    for _ in 0..confirm - 1 {
        l.feed(0.5, false);
    }
    t("cut: a good read starts over", l.take_cut_warn().is_none());
    let mut said = vec![];
    let mut reads = confirm - 1;
    while !l.cut_due() && reads < 100 {
        l.feed(0.5, false);
        reads += 1;
        if let Some(s) = l.take_cut_warn() {
            said.push((reads, s));
        }
    }
    t("cut: 30, 20, 10 s in time", said == vec![(confirm, 30), (confirm + 5, 20), (confirm + 10, 10)]);
    t("cut: due after 60 s", reads == confirm + warn && l.cut_due());
    t("cut: no warning once due", l.take_cut_warn().is_none());
    // USB power: never, also mid-countdown and after it ran out.
    let mut l = LowBat::new();
    for _ in 0..1000 {
        l.feed(0.5, true);
    }
    t("cut: never on USB power", !l.cut_due() && l.take_cut_warn().is_none());
    for _ in 0..confirm + 2 {
        l.feed(0.5, false);
    }
    t("cut: countdown runs", l.take_cut_warn().is_some());
    l.feed(0.5, true);
    t("cut: USB power ends it", !l.cut_due() && l.take_cut_warn().is_none());
    for _ in 0..confirm - 1 {
        l.feed(0.5, false);
    }
    t("cut: confirm again after USB", l.take_cut_warn().is_none());
    // A recovery in the countdown: 1..3 % holds it, 3 % ends it.
    let mut l = LowBat::new();
    for _ in 0..confirm + 2 {
        l.feed(0.5, false);
    }
    l.take_cut_warn();
    for _ in 0..1000 {
        l.feed(2.0, false);
    }
    t("cut: a read under the clear level holds", !l.cut_due());
    l.feed(3.0, false);
    for _ in 0..confirm - 1 {
        l.feed(0.5, false);
    }
    t("cut: the clear level ends it", l.take_cut_warn().is_none() && !l.cut_due());
    // A dead ADC (0 V, the emulator) is no empty battery.
    let mut l = LowBat::new();
    for _ in 0..1000 {
        l.feed(-366.0, false);
    }
    t("cut: 0 V is no reading", !l.cut_due() && l.take_cut_warn().is_none());
    // Straight under both levels: one warning, stays due until shown.
    let mut l = LowBat::new();
    for _ in 0..n {
        l.feed(5.0, false);
    }
    t("lb: both levels at once, one due", l.due());
    l.shown();
    for _ in 0..1000 {
        l.feed(5.0, false);
    }
    t("lb: both levels at once, one warning", !l.due());
    // A warning kept through a recording; a charger cancels it.
    let mut l = LowBat::new();
    for _ in 0..n {
        l.feed(10.0, false);
    }
    l.feed(10.0, true);
    t("lb: charger cancels a waiting warning", !l.due());
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

fn main() {
    lowbat_tests();
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
