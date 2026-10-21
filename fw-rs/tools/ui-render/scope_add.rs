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
    // Trigger tab with the trigger-type rows (UF-13): every type x
    // condition x widest width text, both views; the runt marker.
    let tab_area = Bx::new(14, 116, 399, 70);
    let mut nt = 0;
    for combined in [false, true] {
        for ttype in 0..3u8 {
            for (pcond, w, rv) in [(0u8, 500e-9f32, -444.4e-3f32), (1, 200e-3, 9.876e3), (2, 10.0, 0.0)] {
                let mut m = base.m;
                m.combined = combined;
                m.runt_row = (ttype == 2).then_some(60);
                let mut pv = stock::popvals(&m);
                pv.ttype = ttype;
                pv.pcond = pcond;
                pv.pwidth = w;
                pv.runt_v = rv;
                let sc = stock::Scene { name: "trig", m, popup: scope_ui::Popup::Mf(2), trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
                stock::draw_with(&sc, &pv);
                let name = format!("trigtab/{}/{ttype}/{pcond}", combined as u8);
                // The option lists are the stock widget (abutting items,
                // 1 px frame): checked as neighbours of the new labels /
                // buttons / texts, not among their own items.
                let mut add = vec![rb(scope_ui::TT_LABEL), rb(scope_ui::TP_LABEL)];
                match ttype {
                    1 => add.extend([rb(scope_ui::TP_MINUS), rb(scope_ui::TP_VALUE), rb(scope_ui::TP_PLUS)]),
                    2 => add.extend([rb(scope_ui::TP_COND), rb(scope_ui::TR_HINT)]),
                    _ => {}
                }
                ck.check_add(&name, &[("page", tab_area)], &add, Bx::new(13, 20, 401, 167));
                nt += 1;
                if ttype == 1 && pcond == 0 && !combined {
                    save(out, "s03-trig-pulse");
                }
                if ttype == 2 && pcond == 0 && !combined {
                    save(out, "s03-trig-runt");
                }
            }
        }
    }
    // Cursor tab (UF-08), every state, both views, with the timebase tabs,
    // the FFT boxes and the cursors on; zoom buttons (UF-10) with it.
    for combined in [false, true] {
        for st in [scope_ui::CUR_OFF, scope_ui::CUR_X, scope_ui::CUR_Y] {
            let mut m = base.m;
            m.combined = combined;
            m.cur_tab = st;
            m.tabs = 3;
            m.math_on = true;
            m.math_mode = 5;
            m.fft = Some(fft_add(None, 1));
            if st == scope_ui::CUR_X {
                m.xcur = Some([100, 300]);
            }
            if st == scope_ui::CUR_Y {
                m.ycur = Some([70, 278]);
            }
            let sc = stock::Scene { name: "cur", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
            stock::draw(&sc);
            let name = format!("curtab/{}/{st}", combined as u8);
            ck.check_replica(&name);
            let graph = [("graph", Bx::new(14, m.top() as i32 + 1, 399, m.frame_h() as i32 - 2))];
            ck.check_add(&name, &graph, &[rb(scope_ui::CUR_TAB), rb(scope_ui::FFT_XDIV)], screen);
            if st == scope_ui::CUR_X && !combined {
                save(out, "s04-cur-x-zoom");
            }
        }
    }
    // The runt level marker on the strip (selected, guide line).
    let mut m = base.m;
    m.runt_row = Some(60);
    m.sel_v = 5;
    m.guide = true;
    let sc = stock::Scene { name: "runt", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
    stock::draw(&sc);
    ck.check_replica("runt-marker");
    save(out, "s03-runt-marker");
    println!("scope additions: {n} FFT scenes, {nt} trigger-tab scenes");
}
