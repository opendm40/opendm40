//! fw-rs additions on the stock-replica scope page that are not in the
//! stock (src/scope_ui.rs): the FFT axis-scale boxes, the peak readout and
//! marker (UF-11), the trigger-type rows of the Trigger tab (UF-13), the
//! cursor tab (UF-08) and the zoom buttons (UF-10). Every item under the
//! full layout rules (`check_add`) against the replica around it, both
//! views, worst-case texts, and updates equal a fresh draw.
use super::*;

/// The FFT additions with worst-case (widest) texts.
fn fft_add(peak: Option<(i16, i16)>, k: usize) -> scope_ui::FftAdd {
    let t = scope_ui::Txt::new;
    let (y, x, p) = [
        ("10dB/div", "50kHz/div", "1.00kHz -3.0dBV"),
        ("500mdB/div", "200mHz/div", "12.5MHz -99.9dBV"),
        ("200mV/div", "500kHz/div", "12.5MHz 999mV"),
        ("100dB/div", "20MHz/div", "--"),
    ][k % 4];
    scope_ui::FftAdd { ydiv: t(y), xdiv: t(x), peak: t(p), peak_at: peak }
}

fn rb(r: stockfont::Rect) -> Bx {
    Bx::new(r.x as i32, r.y as i32, r.w as i32, r.h as i32)
}

pub fn run(ck: &mut Checker, out: &str) {
    let base = &stock::scenes()[0];
    let screen = Bx::new(0, 0, 480, 320);
    let mut n = 0;
    for combined in [false, true] {
        for k in 0..4 {
            for tabs in [0u8, 3] {
                let mut m = base.m;
                m.combined = combined;
                m.math_on = true;
                m.math_mode = 5;
                m.math_colour = (k as u8 * 2) % 9;
                m.math_off = [-105, 0, 140, -140][k];
                m.fft_hz = scope_ui::Txt::new(["250kHz", "1.25kHz", "12.5MHz", "999mHz"][k]);
                m.tabs = tabs;
                m.fft = Some(fft_add(Some(([200, 3, 395, 120][k], [40, 100, 278, 20][k])), k));
                if k == 2 {
                    m.xcur = Some([180, 390]);
                    m.sel_h = 1;
                    m.guide = true;
                }
                let sc = stock::Scene { name: "fft", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
                stock::draw(&sc);
                let name = format!("fft/{}/{k}/t{tabs}", combined as u8);
                ck.check_replica(&name);
                let add = [rb(scope_ui::fft_ydiv_rect(&m)), rb(scope_ui::FFT_XDIV), rb(scope_ui::fft_peak_rect(&m))];
                let graph = [("graph", Bx::new(14, m.top() as i32 + 1, 399, m.frame_h() as i32 - 2))];
                ck.check_add(&name, &graph, &add, screen);
                n += 1;
                if k == 0 && tabs == 3 && !combined {
                    save(out, "s02-fft");
                }
                if k == 1 && combined {
                    save(out, "s02-fft-dmmscope");
                }
            }
        }
    }
    // Updates: FFT on / texts change / peak moves / off, each equal to its
    // fresh draw (the hole rows come back, no stale label pixels).
    let st = |k: usize, on: bool, combined: bool| {
        let mut m = base.m;
        m.combined = combined;
        m.math_on = on;
        m.math_mode = 5;
        m.fft_hz = scope_ui::Txt::new(["250kHz", "1.25kHz", "12.5MHz", "999mHz"][k]);
        m.fft = if k == 3 { None } else { Some(fft_add(Some(([200, 3, 395, 120][k], [40, 100, 278, 20][k])), k)) };
        m.guide = k == 1;
        m.sel_v = if k == 1 { 2 } else { 0 };
        stock::Scene { name: "fft", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() }
    };
    for combined in [false, true] {
        let states = [st(0, true, combined), st(1, true, combined), st(2, true, combined), st(3, true, combined), st(0, false, combined)];
        for (i, a) in states.iter().enumerate() {
            for (j, b) in states.iter().enumerate() {
                if i == j {
                    continue;
                }
                stock::draw(b);
                let want = emu().fb.clone();
                stock::draw(a);
                stock::load(b);
                scope_ui::update(&b.m);
                scope_ui::cells(&b.m, &b.cells);
                ck.same_as(&format!("fft/update/{}/{i}->{j}", combined as u8), &want);
            }
        }
    }
    println!("scope additions: {n} FFT scenes");
}
