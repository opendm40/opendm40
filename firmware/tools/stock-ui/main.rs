//! Host renderer of the STOCK DM40 V2.6 oscilloscope UI (tools/stock-ui).
//!
//! Draws the stock scope page from the spec in stock/re/docs/stock-scope-ui.md
//! (element by element, stock coordinates, colours, fonts, pixel rules) with
//! the real stock fonts read from the stock image mapped at 0x08000000, so
//! reference images exist for every state even without device screenshots,
//! and the same scene code renders our combined-view mockup in the stock's
//! visual language.
//!   main <dm40_v_2_6.dec.bin> <out dir> [scene ...]
//! Writes <out dir>/<scene>.ppm (480x320) per scene; run.sh converts to PNG.
#![allow(dead_code, unused)]

#[path = "../ui-render/host.rs"]
mod host;
pub use host::mmio;
mod check;
mod gfx;
mod scope;

use host::{H, W};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: main <dm40_v_2_6.dec.bin> <out dir> [scene ...]");
        std::process::exit(2);
    }
    host::map_flash();
    let img = std::fs::read(&a[1]).expect("stock image");
    unsafe {
        core::ptr::copy_nonoverlapping(img[12..].as_ptr(), 0x0800_4000 as *mut u8, img.len() - 12);
    }
    let out = &a[2];
    std::fs::create_dir_all(out).unwrap();
    let want: Vec<&str> = a[3..].iter().map(|s| s.as_str()).collect();
    let mut n = 0;
    let mut ck = check::Checker { cases: 0, violations: 0, seen: Default::default() };
    let regions: Vec<(&str, check::Bx)> = scope::REGIONS.iter().map(|(n, r)| (*n, check::Bx::new(r.x, r.y, r.w, r.h))).collect();
    let mut report = String::new();
    for (name, sc) in scope::scenes() {
        if !want.is_empty() && !want.iter().any(|w| name.contains(w)) {
            continue;
        }
        host::reset(0);
        gfx::clip_screen();
        scope::draw(&sc);
        host::save_ppm(&format!("{out}/{name}.ppm"), 1);
        let before = ck.violations;
        ck.check_opts(&name, &regions, false);
        report += &format!("{name}: {} violations\n", ck.violations - before);
        n += 1;
    }
    for (msg, (count, scene)) in &ck.seen {
        report += &format!("  x{count} (first {scene}): {msg}\n");
    }
    std::fs::write(format!("{out}/check.txt"), &report).unwrap();
    println!("{n} scenes, {} layout-rule violations (see check.txt)", ck.violations);
}
