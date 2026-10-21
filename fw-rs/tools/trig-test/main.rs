//! Host test of src/trigsoft.rs (UF-13 software pulse-width / runt
//! triggers) on synthetic 10-bit records. Needs no hardware.
//!   tools/trig-test/run.sh
#[allow(dead_code)]
#[path = "../../src/trigsoft.rs"]
mod trigsoft;
use trigsoft::*;

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

/// Pulses (start, width, high code) on a base code with +-1 code noise.
fn rec(n: usize, base: u16, pulses: &[(usize, usize, u16)]) -> Vec<u16> {
    let mut d: Vec<u16> = (0..n).map(|i| base + (i % 3) as u16).collect();
    for &(s, w, h) in pulses {
        for v in &mut d[s..s + w] {
            *v = h;
        }
    }
    d
}

fn main() {
    // Pulses of 10, 50, 100 samples, high (800) on 200.
    let d = rec(1000, 200, &[(100, 10, 800), (300, 50, 800), (600, 100, 800)]);
    t("> 40 -> the 50-sample pulse ends at 350", pulse(&d, 0, 1000, 500, true, GT, 40.0) == Some(350));
    t("> 60 -> the 100-sample pulse", pulse(&d, 0, 1000, 500, true, GT, 60.0) == Some(700));
    t("< 20 -> the 10-sample pulse", pulse(&d, 0, 1000, 500, true, LT, 20.0) == Some(110));
    t("= 50 -> 350", pulse(&d, 0, 1000, 500, true, EQ, 50.0) == Some(350));
    t("= 51 (within 5 %) -> 350", pulse(&d, 0, 1000, 500, true, EQ, 51.0) == Some(350));
    t("= 70 -> none", pulse(&d, 0, 1000, 500, true, EQ, 70.0).is_none());
    t("> 200 -> none", pulse(&d, 0, 1000, 500, true, GT, 200.0).is_none());
    t("lo bound: > 5 from 200 -> 350", pulse(&d, 200, 1000, 500, true, GT, 5.0) == Some(350));
    t("hi bound: trailing edge 700 excluded", pulse(&d, 0, 700, 500, true, GT, 60.0).is_none());
    // Negative pulses: low (100) on a high base (800).
    let n = rec(1000, 800, &[(100, 30, 100), (400, 80, 100)]);
    t("negative > 50 -> 480", pulse(&n, 0, 1000, 500, false, GT, 50.0) == Some(480));
    t("negative < 50 -> 130", pulse(&n, 0, 1000, 500, false, LT, 50.0) == Some(130));
    t("positive search on low pulses: the gaps (>= 270 samples) only", pulse(&n, 0, 1000, 500, true, LT, 100.0).is_none());
    // A pulse already high at the record start is not measured (no leading edge).
    let s = rec(500, 200, &[(0, 40, 800), (200, 40, 800)]);
    t("no leading edge at the start", pulse(&s, 0, 500, 500, true, GT, 30.0) == Some(240));
    // Runt: full pulses to 800, a runt to 500 (levels 300 / 700).
    let r = rec(1200, 200, &[(100, 50, 800), (400, 50, 500), (700, 50, 800)]);
    t("runt + -> 450", runt(&r, 0, 1200, 300, 700, true) == Some(450));
    t("runt +, levels swapped", runt(&r, 0, 1200, 700, 300, true) == Some(450));
    t("runt + none when the runt reaches the upper level", runt(&r, 0, 1200, 300, 480, true).is_none());
    t("runt + lo bound past it", runt(&r, 460, 1200, 300, 700, true).is_none());
    let rn = rec(1200, 900, &[(100, 50, 100), (400, 50, 450)]);
    t("runt - -> 450", runt(&rn, 0, 1200, 300, 700, false) == Some(450));
    t("runt levels too close", runt(&r, 0, 1200, 300, 303, true).is_none());
    // Width steps.
    t("step up 1us -> 2us", (width_step(1e-6, true) - 2e-6).abs() < 1e-12);
    t("step down 1us -> 500ns", (width_step(1e-6, false) - 5e-7).abs() < 1e-13);
    t("step clamps", width_step(10.0, true) == W_MAX && (width_step(20e-9, false) - W_MIN).abs() < 1e-15);
    t("step from off-grid 3us up -> 5us", (width_step(3e-6, true) - 5e-6).abs() < 1e-12);
    let (n, bad) = unsafe { (N, BAD) };
    println!("cases {n}, failures {bad}");
    std::process::exit(if bad == 0 { 0 } else { 1 });
}
