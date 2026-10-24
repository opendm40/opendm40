//! Host test of src/dbgsrc.rs (DBG:SRC): the built rows differ from the
//! stock ones only in R31 SFUVR, R23 ref buffers, R24 mux (SDIO kept) and
//! R28; the current-source gate; the phase plan; volts from codes; the
//! parser.
//!   tools/dbgsrc-test/run.sh
#![allow(dead_code)]
mod hy3131 {
    pub const CFG_LEN: usize = 22;
}
#[path = "../../src/hy3131_tables.rs"]
mod hy3131_tables;
#[path = "../../src/dbgsrc.rs"]
mod dbgsrc;

use dbgsrc::*;
use hy3131_tables::{CFG_F10_P8CLR, CFG_F12};

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

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn main() {
    // Rows: only the allowed bytes / bits change.
    for p in 0..PATHS.len() as u8 {
        let stock = if PATHS[p as usize].mode == 12 { CFG_F12[0] } else { CFG_F10_P8CLR[PATHS[p as usize].range as usize] };
        for v in 0..16u8 {
            for s in 0..SENSES.len() as u8 {
                let r = build_row(v, p, s);
                for j in 0..22 {
                    let d = r[j] ^ stock[j];
                    let allowed = match j {
                        3 => 0x0C,
                        4 => 0xF7,
                        8 => 0xFF,
                        17 => 0x0F,
                        _ => 0,
                    };
                    check("row-diff", d & !allowed == 0, format!("path {p} vref {v} R{:02X} stock {:02X} built {:02X}", 0x20 + j, stock[j], r[j]));
                }
                check("sfuvr", r[17] & 0x0F == v, format!("path {p} vref {v}: R31 {:02X}", r[17]));
                check("smode3", r[15] & 0x08 == 0, format!("path {p}: R2F {:02X}", r[15]));
                check("enad1", r[3] & 0x80 != 0, format!("path {p}"));
                check("refbuf", r[3] & 0x0C == 0, format!("path {p}"));
                check("sdio", r[4] & 0x08 == stock[4] & 0x08, format!("path {p}"));
                // phase 0: REFO - AGND against VDDA - AGND
                check("ph0", r[4] & 0xF7 == 0x63 && r[8] == 0x51, format!("path {p}: R24 {:02X} R28 {:02X}", r[4], r[8]));
            }
        }
    }
    // The ohm rows' tap is SFUVR 2 (VDS15) as stock: vref 2 on R1K = stock but for the AD1 bytes.
    let r = build_row(2, 0, 0);
    check("stock-tap", r[17] == CFG_F10_P8CLR[1][17], format!("{:02X}", r[17]));
    // Phase plan.
    let row = build_row(5, 1, 0);
    let want: [(u8, u8); N_PH] = [(0x63, 0x51), (0x63, 0x55), (0x83, 0x31), (0x83, 0x51), (0x13, 0x31), (0x13, 0x51), (0x73, 0x31), (0x73, 0x51), (0x11, 0x31), (0xB3, 0x31)];
    for (k, w) in want.iter().enumerate() {
        let (a, b) = phase_regs(&row, k, 0);
        check("phase", (a & 0xF7, b) == *w, format!("phase {k}: {:02X} {:02X}", a, b));
    }
    let (a, _) = phase_regs(&row, 2, 1);
    check("sense-op1o", a & 0xF7 == 0x33, format!("{a:02X}"));
    let drow = build_row(2, 6, 0);
    let (a, _) = phase_regs(&drow, 4, 0);
    check("sdio-kept", a == 0x1B, format!("{a:02X}"));
    // Cycle: SKIP dropped per phase, then the next phase's registers.
    let mut c = Cycle::NEW;
    let mut writes = 0;
    for k in 0..(N_PH * (SKIP as usize + 1)) as i32 {
        if c.step(1000 + k, &row, 0).is_some() {
            writes += 1;
        }
    }
    check("cycle", c.n == 1 && writes == N_PH && c.phase == 0, format!("n {} writes {writes} phase {}", c.n, c.phase));
    check("cycle-codes", c.done[0] == 1000 + SKIP as i32 && c.done[N_PH - 1] == 1000 + (N_PH * 2 - 1) as i32, format!("{:?}", c.done));
    // Maths: VDDA - AGND 2.52 V, VDDA - VSSA 3.6 V; nodes.
    let ig = 0.9f32;
    let fs = 8_388_608.0f32;
    let code = |v: f32, vr: f32| (ig * v / vr * fs).round() as i32;
    let (w, s) = (2.52f32, 3.6f32);
    let vin = 0.4f32;
    let vfar = 2.1f32; // beyond the narrow range
    let mut codes = [0i32; N_PH];
    codes[0] = code(VREFO, w);
    codes[1] = code(VREFO, s);
    codes[2] = code(vin, VREFO);
    codes[3] = code(vin, w);
    codes[4] = 8_388_607; // narrow saturated
    codes[5] = code(vfar, w);
    codes[6] = code(-0.6, VREFO);
    codes[7] = code(-0.6, w);
    codes[8] = code(vfar - vin, VREFO).min(8_388_607);
    codes[9] = code(0.001, VREFO);
    let v = compute(&codes, ig, 0, 0);
    check("vdda", near(v.vdda, w, 1e-4), format!("{}", v.vdda));
    check("agnd-vss", near(v.agnd_vss, s - w, 1e-3), format!("{}", v.agnd_vss));
    check("v_in", near(v.v_in, vin, 1e-5), format!("{}", v.v_in));
    check("v_far-wide", near(v.v_far, vfar, 1e-4), format!("{}", v.v_far));
    check("v_vref", near(v.v_vref, -0.6, 1e-5), format!("{}", v.v_vref));
    check("i", near(v.i, (vfar - vin) / 1e3, 1e-7), format!("{}", v.i));
    check("v_com", near(v.v_com, 0.001, 1e-6), format!("{}", v.v_com));
    let vd = compute(&codes, ig, 6, 0);
    check("diode-div", near(vd.v_in, vin * 10.0, 1e-4) && vd.i.is_nan(), format!("{} {}", vd.v_in, vd.i));
    let vs = compute(&codes, ig, 6, 2);
    check("diode-sense", near(vs.v_in, vin, 1e-5), format!("{}", vs.v_in));
    check("cs-no-i", compute(&codes, ig, 5, 0).i.is_nan(), String::new());
    let z = compute(&[0; N_PH], ig, 0, 0);
    check("zero-codes", z.vdda.is_nan() && z.v_in == 0.0, format!("{:?}", z));
    // Gate.
    check("gate-ohm", (0..16).all(|t| allowed(t, 0, false)), String::new());
    check("gate-cs", allowed(0, 5, false) && allowed(2, 6, false) && !allowed(3, 5, false) && !allowed(8, 6, false), String::new());
    check("gate-force", allowed(8, 6, true) && !allowed(16, 0, true), String::new());
    // Parser.
    let cases: &[(&str, Option<Req>)] = &[
        ("OFF", Some(Req::Off)),
        ("off", Some(Req::Off)),
        ("AGNDP7,R1K", Some(Req::On(5, 0, 0, false))),
        ("agndn9, r10m, op1o", Some(Req::On(11, 4, 1, false))),
        ("VDSC17,CS", Some(Req::On(0, 5, 0, false))),
        ("vds3,DIODE,FORCE", Some(Req::On(10, 6, 0, true))),
        ("15,6,SENSE,FORCE", Some(Req::On(15, 6, 2, true))),
        ("PB7,R100K,RLU", Some(Req::On(15, 2, 3, false))),
        ("\"AGNDP0\",\"R1M\"", Some(Req::On(7, 3, 0, false))),
        ("16,R1K", None),
        ("AGNDP1,R1K", None),
        ("VDS4,R1K", None),
        ("AGNDP7", None),
        ("AGNDP7,R2K", None),
        ("AGNDP7,R1K,PB9", None),
        ("AGNDP7,R1K,PB0,X", None),
        ("", None),
    ];
    for (a, w) in cases {
        let r = parse(a.as_bytes());
        check("parse", r == *w, format!("{a:?}: {r:?}, want {w:?}"));
    }
    unsafe {
        let (n, f) = (*core::ptr::addr_of!(CASES), *core::ptr::addr_of!(FAILS));
        println!("dbgsrc-test: {n} cases, {f} failed");
        if f != 0 {
            std::process::exit(1);
        }
    }
}
