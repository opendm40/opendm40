//! Host test of src/ohm600m.rs (the experimental 600 Mohm range): the row
//! differs from the stock 6 Mohm row only in R22, R23's reference
//! buffers, R24 / R28 and R31's tap (plus the knob overrides); the phase
//! plan and its cycle; R from model codes (X / F, Q, C, the offset, OL,
//! the window); the knobs' limits; the DBG:OHMX parser.
//!   tools/ohm600m-test/run.sh
#![allow(dead_code)]
mod hy3131 {
    pub const CFG_LEN: usize = 22;
}
#[path = "../../src/hy3131_tables.rs"]
mod hy3131_tables;
#[path = "../../src/ohm600m.rs"]
mod ohm600m;

use hy3131_tables::CFG_F10_P8CLR;
use ohm600m::*;

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

/// scpi::parse_num stand-in (plain numbers only).
fn num(t: &[u8]) -> Option<f32> {
    std::str::from_utf8(t).ok()?.trim().parse().ok()
}

const IG: f32 = 0.9;
const FS: f32 = 8_388_608.0;

/// AD1 code of `v` against reference `vr` at IG 0.9.
fn code(v: f32, vr: f32) -> i32 {
    (IG * v / vr * FS).round() as i32
}

/// The 10 Mohm path on the AGNDP9 tap with Rx from the jack to COM at
/// `vcom`: (jack, FB) volts. `leak` = jack leakage to ~1.05 V (ohm, 0 =
/// none: the ~1.3..1.9 Gohm inferred in docs/ohm-600m.md).
fn circuit(rx: f32, vcom: f32, leak: f32) -> (f32, f32) {
    let (vfb, rref) = (VFB_NOM, 10e6f32);
    // Node equation at the jack: (vfb - j) / rref + (1.05 - j) / leak = (j - vcom) / rx.
    let (gl, vl) = if leak > 0.0 { (1.0 / leak, 1.05f32) } else { (0.0, 0.0) };
    let gx = if rx.is_finite() { 1.0 / rx } else { 0.0 };
    let j = (vfb / rref + vl * gl + vcom * gx) / (1.0 / rref + gl + gx);
    (j, vfb)
}

fn codes_for(rx: f32, vcom: f32, leak: f32, xoff: f32) -> [i32; N_PH] {
    let (j, fb) = circuit(rx, vcom, leak);
    let mut c = [0i32; N_PH];
    c[PH_X] = code(j + xoff, VREFO);
    c[PH_F] = code(fb, VREFO);
    c[PH_C] = code(vcom, VREFO);
    c[PH_Q] = code(j, fb);
    c[PH_D] = code(0.86, VREFO);
    c
}

fn main() {
    let k = Knobs::DEF;
    // Row: the stock 6 Mohm row but for the allowed bytes / bits.
    let stock = CFG_F10_P8CLR[BASE_ROW];
    let r = build_row(&k);
    for j in 0..22 {
        let d = r[j] ^ stock[j];
        let allowed = match j {
            2 => 0xFF,
            3 => 0x0C,
            4 => 0xF7,
            8 => 0xFF,
            17 => 0x0F,
            _ => 0,
        };
        check("row-diff", d & !allowed == 0, format!("R{:02X} stock {:02X} built {:02X}", 0x20 + j, stock[j], r[j]));
    }
    check("row-tap", r[17] & 0x0F == TAP_AGNDP9 && r[17] & 0xF0 == stock[17] & 0xF0, format!("R31 {:02X}", r[17]));
    check("row-stock-tap", stock[17] & 0x0F == 2, format!("stock R31 {:02X} (VDS15)", stock[17]));
    check("row-osr", r[2] == 0x16, format!("R22 {:02X}", r[2]));
    check("row-ad1", r[3] & 0x80 != 0 && r[3] & 0x0C == 0 && r[3] & 0x03 == stock[3] & 0x03, format!("R23 {:02X}", r[3]));
    // The default plan starts on X: OP1O - AGND against PB6 - AGND.
    check("row-phase-x", (r[4] & 0xF7, r[8]) == (0x33, 0x31), format!("R24 {:02X} R28 {:02X}", r[4], r[8]));
    check("row-ig", ig_of(&r) == 0.9, String::new());
    // Overrides land; R24 / R28 stay the plan's.
    let mut ko = k;
    check("ovr-set", ko.apply(Req::Row(0x33, 0xA0)) && ko.apply(Req::Row(0x24, 0x00)), String::new());
    let ro = build_row(&ko);
    check("ovr-r33", ro[0x13] == 0xA0, format!("{:02X}", ro[0x13]));
    check("ovr-r24-plan", ro[4] & 0xF7 == 0x33, format!("{:02X}", ro[4]));
    check("ovr-same-reg", ko.apply(Req::Row(0x33, 0xA8)) && ko.ovr.iter().filter(|o| o.0 == 0x33).count() == 1, format!("{:?}", ko.ovr));
    check("ovr-full", ko.apply(Req::Row(0x2F, 1)) && ko.apply(Req::Row(0x30, 1)) && !ko.apply(Req::Row(0x31, 1)), format!("{:?}", ko.ovr));
    check("ovr-reg", !ko.apply(Req::Row(0x36, 1)) && !ko.apply(Req::Row(0x1F, 1)), String::new());
    check("ovr-clr", ko.apply(Req::RowClr) && ko.ovr == [(0, 0); ROW_OVR], String::new());
    // Phase registers: X, F, C, Q, D.
    let want: [(u8, u8); N_PH] = [(0x33, 0x31), (0x13, 0x31), (0xB3, 0x31), (0x33, 0x01), (0x11, 0x31)];
    for (p, w) in want.iter().enumerate() {
        let (a, b) = phase_regs(&r, p);
        check("phase", (a & 0xF7, b) == *w, format!("phase {p}: {a:02X} {b:02X}"));
    }
    // Plans.
    check("plan-ok", Knobs::plan_ok(0b00011) && Knobs::plan_ok(0b01000) && Knobs::plan_ok(0b11111) && Knobs::plan_ok(0b00111), String::new());
    check("plan-bad", !Knobs::plan_ok(0b00001) && !Knobs::plan_ok(0b00010) && !Knobs::plan_ok(0b10100) && !Knobs::plan_ok(32), String::new());
    check("next-phase", next_phase(0b01010, 0) == PH_F && next_phase(0b01010, 2) == PH_Q && next_phase(0b01010, 4) == PH_F, String::new());
    // Cycle: `skip` dropped per phase, one kept, the registers of the next.
    for (plan, skip) in [(0b00011u8, 1u8), (0b01000, 0), (0b11111, 2), (0b00111, 1)] {
        let kc = Knobs { plan, skip, ..k };
        let row = build_row(&kc);
        let mut c = Cycle::start(&kc);
        let n_ph = plan.count_ones() as usize;
        let (mut writes, mut dones, mut last) = (0, 0, None);
        for i in 0..(n_ph * (skip as usize + 1)) as i32 {
            if let Some((a, b, done)) = c.step(100 + i, &row, &kc) {
                writes += 1;
                dones += done as u32;
                last = Some((a, b));
            }
        }
        let first = next_phase(plan, 0);
        check("cycle", c.n == 1 && dones == 1 && writes == n_ph && c.phase as usize == first, format!("plan {plan:05b}: n {} writes {writes} phase {}", c.n, c.phase));
        check("cycle-next-regs", last == Some(phase_regs(&row, first)), format!("plan {plan:05b}"));
        check("cycle-codes", c.done[first] == 100 + skip as i32, format!("plan {plan:05b}: {:?}", c.done));
    }
    // R from model codes, 50 .. 600 Mohm, plan X / F (no leak, no offset):
    // within the codes' rounding (0.1 uV on a 17 mV drop at 600 M: 6 ppm).
    let g = 10e6f32;
    for rx in [50e6f32, 100e6, 300e6, 600e6] {
        let v = volts(&codes_for(rx, 0.0, 0.0, 0.0), k.plan, IG, 0.0);
        let r = ohms(v.vx, v.drop, g);
        check("r-xf", (r / rx - 1.0).abs() < 2e-4, format!("{rx}: {r}"));
        let q = volts(&codes_for(rx, 0.0, 0.0, 0.0), 1 << PH_Q | 1 << PH_F, IG, 0.0);
        let rq = ohms(q.vx, q.drop, g);
        check("r-q", (rq / rx - 1.0).abs() < 2e-4, format!("{rx}: {rq}"));
        // Q alone: FB nominal (VFB_NOM, the model's FB here).
        let q1 = volts(&codes_for(rx, 0.0, 0.0, 0.0), 1 << PH_Q, IG, 0.0);
        check("r-q-nom", (ohms(q1.vx, q1.drop, g) / rx - 1.0).abs() < 2e-4, String::new());
    }
    // An X / F offset of 0.1 mV: 0.6 % at 600 M; the OFFS knob takes it out.
    let c = codes_for(600e6, 0.0, 0.0, 1e-4);
    let r0 = ohms(volts(&c, k.plan, IG, 0.0).vx, volts(&c, k.plan, IG, 0.0).drop, g);
    check("offset-err", (r0 / 600e6 - 1.0) > 0.005, format!("{r0}"));
    let v = volts(&c, k.plan, IG, -1e-4);
    let r1 = ohms(v.vx, v.drop, g);
    check("offset-knob", (r1 / 600e6 - 1.0).abs() < 2e-4, format!("{r1}"));
    // COM above AGND: C in the plan takes it out of V_x.
    let cc = codes_for(300e6, 0.002, 0.0, 0.0);
    let vc = volts(&cc, k.plan | 1 << PH_C, IG, 0.0);
    check("com", (ohms(vc.vx, vc.drop, g) / 300e6 - 1.0).abs() < 3e-4 && (vc.vcom - 0.002).abs() < 1e-6, format!("{:?}", vc));
    // Leakage to ~1.05 V (1.5 Gohm, inferred): a near-constant gain (the
    // calibration's part) at 100 .. 600 Mohm, ~0.7 %.
    let lg: Vec<f32> = [100e6f32, 300e6, 600e6]
        .iter()
        .map(|&rx| {
            let v = volts(&codes_for(rx, 0.0, 1.5e9, 0.0), k.plan, IG, 0.0);
            ohms(v.vx, v.drop, g) / rx
        })
        .collect();
    check("leak-gain", lg.iter().all(|&x| (x - lg[0]).abs() < 0.002) && (lg[0] - 1.0).abs() < 0.012, format!("{lg:?}"));
    // Open jacks: the jack follows FB -> no drop -> OL; a short reads 0.
    let vo = volts(&codes_for(f32::INFINITY, 0.0, 0.0, 0.0), k.plan, IG, 0.0);
    check("open-ol", is_ol(ohms(vo.vx, vo.drop, g), 1.03), format!("{:?}", vo));
    let vs = volts(&codes_for(0.0, 0.0, 0.0, 0.0), k.plan, IG, 0.0);
    check("short", ohms(vs.vx, vs.drop, g).abs() < 1e3, format!("{:?}", vs));
    check("ol-edge", !is_ol(618e6, 1.03) && is_ol(619e6, 1.03) && is_ol(f32::NAN, 1.03), String::new());
    // LIVE?: -2 V behind 1 Mohm on V/ohm (COM at AGND, C not in the plan).
    check("live", vs.vx > -LIVE_V && volts(&codes_for(1e6, -2.0, 0.0, 0.0), k.plan, IG, 0.0).vx < -LIVE_V, String::new());
    // Vet.
    check("agree", agree(600e6, 601e6, 0.01) && !agree(600e6, 640e6, 0.01) && agree(f32::INFINITY, f32::INFINITY, 0.01) && !agree(5e8, f32::INFINITY, 0.01) && !agree(5e8, f32::NAN, 0.01), String::new());
    // Window: the mean of the last `navg`, a reset empties it.
    let mut w = Cycle::NEW;
    let val = |x: f32, d: f32| Vals { vx: x, drop: d, vfb: 1.0, vcom: 0.0, vd: f32::NAN };
    for i in 0..6 {
        w.push(&val(1.0 + i as f32, 0.01), 4, false);
    }
    let (mx, md) = w.mean();
    check("window", (mx - 4.5).abs() < 1e-5 && (md - 0.01).abs() < 1e-7 && w.wn == 4, format!("{mx} {md} {}", w.wn));
    w.push(&val(9.0, 0.02), 4, true);
    check("window-reset", w.mean() == (9.0, 0.02) && w.wn == 1, format!("{:?}", w.mean()));
    w.push(&val(1.0, 0.0), 2, false);
    check("window-shrink", w.wn <= 2, format!("{}", w.wn));
    // Knobs.
    let mut kk = k;
    for (req, ok) in [
        (Req::Set(Key::Tap, 7.0), true),
        (Req::Set(Key::Tap, 16.0), false),
        (Req::Set(Key::Tap, 2.5), false),
        (Req::Set(Key::Plan, 8.0), true),
        (Req::Set(Key::Plan, 1.0), false),
        (Req::Set(Key::Skip, 5.0), false),
        (Req::Set(Key::Navg, 0.0), false),
        (Req::Set(Key::Navg, 8.0), true),
        (Req::Set(Key::Settle, 10000.0), true),
        (Req::Set(Key::Gain, 1.007), true),
        (Req::Set(Key::Gain, 3.0), false),
        (Req::Set(Key::Offs, -1e-4), true),
        (Req::Set(Key::Offs, 0.1), false),
        (Req::Set(Key::Rref, 9.95e6), true),
        (Req::Set(Key::Vet, f32::NAN), false),
    ] {
        check("apply", kk.apply(req) == ok, format!("{req:?}"));
    }
    check("apply-vals", kk.tap == 7 && kk.plan == 8 && kk.navg == 8 && kk.settle_ms == 10000 && kk.gain == 1.007 && kk.offs == -1e-4 && kk.rref == 9.95e6, format!("{kk:?}"));
    check("apply-def", kk.apply(Req::Def) && kk == Knobs::DEF, String::new());
    // Parser.
    let cases: &[(&str, Option<Req>)] = &[
        ("DEF", Some(Req::Def)),
        ("def", Some(Req::Def)),
        ("TAP,3", Some(Req::Set(Key::Tap, 3.0))),
        ("gain, 1.0123", Some(Req::Set(Key::Gain, 1.0123))),
        ("OFFS,-1.2e-4", Some(Req::Set(Key::Offs, -1.2e-4))),
        ("\"PLAN\",11", Some(Req::Set(Key::Plan, 11.0))),
        ("ROW,0x33,0xA0", Some(Req::Row(0x33, 0xA0))),
        ("ROW,51,160", Some(Req::Row(0x33, 0xA0))),
        ("row,clr", Some(Req::RowClr)),
        ("ROW,0x133,1", None),
        ("ROW,51,256", None),
        ("ROW,51", None),
        ("ROW,0x,1", None),
        ("TAP", None),
        ("TAP,3,4", None),
        ("FOO,1", None),
        ("", None),
    ];
    for (a, w) in cases {
        let r = parse(a.as_bytes(), num);
        check("parse", r == *w, format!("{a:?}: {r:?}, want {w:?}"));
    }
    unsafe {
        let (n, f) = (*core::ptr::addr_of!(CASES), *core::ptr::addr_of!(FAILS));
        println!("ohm600m-test: {n} cases, {f} failed");
        if f != 0 {
            std::process::exit(1);
        }
    }
}
