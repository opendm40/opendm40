//! Host test of src/bargraph.rs (UF-30): the bar on the ILI9488 emulator of
//! tools/ui-render/host.rs. Random walks of values (unipolar and bipolar,
//! with overloads and sign changes): after every `update` the screen equals
//! a fresh `draw_static` + `update` of the same value; `frac` scale; the bus
//! words per update. Renders a few states for the contact sheet.
#![allow(dead_code, static_mut_refs)]

#[path = "../ui-render/host.rs"]
mod host;
pub use host::mmio;
#[path = "../../src/lcd.rs"]
mod lcd;
#[path = "../../src/bargraph.rs"]
mod bargraph;

use bargraph::{frac, Bar};

const BG: u16 = 0x0883;
static mut FAILS: u32 = 0;
static mut CASES: u32 = 0;

fn check(name: &str, ok: bool, msg: String) {
    unsafe {
        CASES += 1;
        if !ok {
            FAILS += 1;
            println!("FAIL {name}: {msg}");
        }
    }
}

/// The history-row geometry the bar is proposed for (ui.rs history boxes,
/// y 153..172) and a thin strip variant.
const GEOM: [(u16, u16, u16, u16); 2] = [(8, 154, 464, 18), (4, 148, 472, 7)];

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    host::map_flash();
    // frac: 60000-count scale from the decimals.
    check("frac 4.8176 V dec 4", (frac(4.8176, 4, false).unwrap() - 48176.0 / 60000.0).abs() < 1e-6, String::new());
    check("frac -600.00 mV dec 2", (frac(-600.0, 2, false).unwrap() + 1.0).abs() < 1e-6, String::new());
    check("frac clamps", frac(70000.0, 0, false) == Some(1.0), String::new());
    check("frac OL", frac(1.0, 4, true).is_none() && frac(f32::NAN, 1, false).is_none(), String::new());

    let mut seed = 0x1234_5678u32;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for &(x, y, w, h) in &GEOM {
        for bipolar in [false, true] {
            host::reset(BG);
            let mut bar = Bar::new(x, y, w, h, bipolar);
            bar.draw_static(BG);
            let mut v: f32 = 0.3;
            let (mut words, mut n) = (0u64, 0u64);
            let mut bad = 0;
            for i in 0..600 {
                // Random walk, jumps, sign changes, overloads.
                v += (rnd() - 0.5) * if i % 50 == 0 { 1.5 } else { 0.05 };
                v = v.clamp(-1.2, 1.2);
                let f = if i % 97 == 13 || v.abs() > 1.1 { None } else { Some(v) };
                host::emu().bus = 0;
                bar.update(f);
                words += host::emu().bus;
                n += 1;
                let got = host::emu().fb.clone();
                host::reset(BG);
                let mut fresh = Bar::new(x, y, w, h, bipolar);
                fresh.draw_static(BG);
                fresh.update(f);
                let diff = (0..got.len()).filter(|&k| got[k] != host::emu().fb[k]).count();
                if diff > 0 && bad == 0 {
                    check(&format!("bar {w}x{h} bipolar {bipolar} step {i}"), false, format!("{diff} stale pixels (f {f:?})"));
                    bad += 1;
                }
                check("incremental == fresh", diff == 0, String::new());
                host::emu().fb.copy_from_slice(&got);
            }
            println!("bar {w}x{h} bipolar {bipolar}: {:.0} bus words / update", words as f64 / n as f64);
        }
    }
    // Renders: proposed history-row geometry, unipolar 0.80, bipolar -0.35, OL.
    for (name, bipolar, f) in [("b01-uni-080", false, Some(0.8)), ("b02-bi-neg035", true, Some(-0.35)), ("b03-ol", false, None)] {
        host::reset(BG);
        let (x, y, w, h) = GEOM[0];
        let mut bar = Bar::new(x, y, w, h, bipolar);
        bar.draw_static(BG);
        bar.update(f);
        host::save_ppm(&format!("{out}/{name}.ppm"), 2);
    }
    let (f, c) = unsafe { (FAILS, CASES) };
    println!("cases {c}, failures {f}");
    std::process::exit(if f == 0 { 0 } else { 1 });
}
