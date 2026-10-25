//! opendm40 additions on the stock-replica scope page that are not in the
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
    // The DMM wave view (src/app_wave.rs): the source badge "MCX | DMM"
    // (both states), the shortened record bar beside it (running, stopped
    // with its brackets, roll), the DMM V/div texts (widest: "250V",
    // "200uA", "200mV", "2.5A"), the CH1 label ("AC uA"), the current cells
    // ("Ip-p" / "Irms"), the trigger badge in A; updates between them and
    // to / from the stock page equal fresh draws.
    let mut nw = 0;
    let wave_scene = |src: u8, k: usize, status: u8, roll: bool, tabs: u8| {
        let mut m = base.m;
        m.src = src;
        m.status = status;
        m.roll = roll;
        m.tabs = tabs;
        m.record = if status == scope_ui::ST_STOP { (0.15, 0.85) } else { (0.0, 1.0) };
        let cells: Vec<&'static str> = if src == scope_ui::SRC_DMM {
            let (vd, amps, ch1, tv) = [(250.0f32, false, "AC V", -999.0f32), (200e-6, true, "AC uA", -1.23e-3), (0.15, false, "AC mV", 0.444), (2.5, true, "AC A", 9.99)][k % 4];
            m.wvdiv = vd;
            m.amps = amps;
            m.ch1_txt = ch1;
            m.ac = !amps && ch1 == "AC V";
            m.trig_v = tv;
            if amps {
                vec!["Freq:50.0Hz", "Ip-p:999.9mA", "Cycl:20.0ms", "Irms:-12.3uA"]
            } else {
                vec!["Freq:50.0Hz", "Vp-p:999.9mV", "Cycl:20.0ms", "Vrms:-12.3uV"]
            }
        } else {
            base.cells.clone()
        };
        stock::Scene { name: "wave", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells }
    };
    let top = Bx::new(0, 0, 480, 20);
    for src in [scope_ui::SRC_MCX, scope_ui::SRC_DMM] {
        for k in 0..4 {
            for (status, roll) in [(scope_ui::ST_TD, false), (scope_ui::ST_STOP, false), (scope_ui::ST_RUN, true)] {
                let sc = wave_scene(src, k, status, roll, 3);
                stock::draw(&sc);
                let name = format!("wave/{src}/{k}/{status}/{}", roll as u8);
                ck.check_replica(&name);
                // The badge (a new item: the full rules, the badge row's
                // vertical inset as the stock badges, y 1..17) against the
                // timebase badge and the shortened record bar (the stock
                // widget, its insides exempt as the replica's).
                ck.check_add(&name, &[("badge row", Bx::new(0, -2, 480, 24))], &[rb(scope_ui::SRC_BADGE)], top);
                nw += 1;
                if src == scope_ui::SRC_DMM && status == scope_ui::ST_TD && k < 2 {
                    save(out, &format!("s05-wave-{}", ["v", "ua"][k]));
                }
                if src == scope_ui::SRC_DMM && status == scope_ui::ST_STOP && k == 2 {
                    save(out, "s05-wave-stopped");
                }
                if src == scope_ui::SRC_DMM && roll && k == 3 {
                    save(out, "s05-wave-roll-a");
                }
                if src == scope_ui::SRC_MCX && status == scope_ui::ST_TD && k == 0 {
                    save(out, "s05-mcx-badge");
                }
            }
        }
    }
    // The wave view's V/div steps (scope::wave_steps): every DMM range at
    // its full-scale step (a sine of 98 % of +-full fills the 8 divisions)
    // and at an intermediate step of the same range (a screen-only zoom:
    // the sine at 98 % of +-full or of +-4 div, the smaller), V and A.
    for amps in [false, true] {
        let st = scope::wave_steps(amps);
        for (i, s) in st.iter().enumerate() {
            if !s.2 {
                continue;
            }
            let full = s.0 * 4.0;
            // The intermediate step: the one below on this range, else the
            // one above (60 mV, 600 uA: the list's first range).
            let mid = [i.wrapping_sub(1), i + 1].into_iter().find(|&j| j < st.len() && st[j].1 == s.1);
            for (tag, j) in [("full", Some(i)), ("step", mid)] {
                let Some(j) = j else { continue };
                let mut m = base.m;
                m.src = scope_ui::SRC_DMM;
                m.status = scope_ui::ST_TD;
                m.tabs = 3;
                m.wvdiv = st[j].0;
                m.amps = amps;
                m.ch1_txt = match (amps, full) {
                    (false, f) if f < 1.0 => "AC mV",
                    (false, _) => "AC V",
                    (true, f) if f < 0.01 => "AC uA",
                    (true, f) if f < 1.0 => "AC mA",
                    _ => "AC A",
                };
                m.ac = m.ch1_txt == "AC V";
                m.trig_row = 140;
                m.trig_v = 0.0;
                let amp = (0.98 * full / st[j].0).min(3.92);
                let mut lbl = scope_ui::Buf::new();
                scope_ui::fmt_div(&mut lbl, full, if amps { "A" } else { "V" });
                let mut vpp = scope_ui::Buf::new();
                scope_ui::fmt_div(&mut vpp, 2.0 * amp * st[j].0, if amps { "A" } else { "V" });
                let pp: &'static str = Box::leak(format!("{}p-p:{}", if amps { "I" } else { "V" }, vpp.as_str()).into_boxed_str());
                let cells = vec!["Freq:50.0Hz", pp, "Cycl:20.0ms", if amps { "Irms:-" } else { "Vrms:-" }];
                let sc = stock::Scene { name: "wave", m, popup: scope_ui::Popup::None, trace: Some(stock::Trace::sine(amp, 5.0, 0.0)), persist: None, cols: 399, cells };
                stock::draw(&sc);
                let name = format!("wave/step/{}/{}", lbl.as_str(), tag);
                ck.check_replica(&name);
                ck.check_add(&name, &[("badge row", Bx::new(0, -2, 480, 24))], &[rb(scope_ui::SRC_BADGE)], top);
                nw += 1;
                save(out, &format!("s06-wave-{}-{}", lbl.as_str(), tag));
            }
        }
    }
    // The texts the wave view changes are no wider than what the stock
    // page already shows in the same box (cells: "Vp-p:" vs "Ip-p:", "mV"
    // vs "mA"; the V/div badge text box 44 px; the CH1 label in 60 px; the
    // trigger badge: the widest MCX level text, e.g. "-1.23kV").
    let wmax = |f: stockfont::Font, ts: &[&str]| ts.iter().map(|t| stockfont::text_width(f, t, 0)).max().unwrap_or(0);
    let pairs: [(&str, stockfont::Font, &[&str], &[&str]); 4] = [
        ("cells", stockfont::SC_F12, &["Ip-p:999.9mA", "Irms:-12.3mA", "Imax:-999.9mA", "Iavg:-12.3uA", "Ivpp:999kA"], &["Vp-p:999.9mV", "Vrms:-12.3mV", "Vmax:-999.9mV", "Vavg:-12.3uV"]),
        ("vdiv", stockfont::SC_F12, &["250V", "200uA", "200mA", "200mV", "150mV", "150uA", "1.5mA", "15mV", "1.5V", "2.5A"], &["10kV", "500mV"]),
        ("trig", stockfont::SC_F12, &["-1.23mA", "-999uA", "-99.9A", "-12.3kV"], &["-1.23kV", "-12.3kV", "-999mV"]),
        ("ch1", stockfont::SC_F16, &["AC V", "AC mV", "AC A", "AC mA", "AC uA"], &[]),
    ];
    for (what, f, wave_t, stock_t) in pairs {
        ck.cases += 1;
        let (ww, sw) = (wmax(f, wave_t), wmax(f, stock_t));
        let lim = match what {
            "vdiv" => 44 - 2,
            "ch1" => 60 - 6,
            _ => sw,
        };
        if ww > lim {
            ck.fail("wave/text", format!("{what}: wave texts {ww} px > {lim} (stock {sw})"));
        }
    }
    let states = [
        stock::Scene { name: "wave", m: base.m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() },
        wave_scene(scope_ui::SRC_MCX, 0, scope_ui::ST_TD, false, 3),
        wave_scene(scope_ui::SRC_DMM, 0, scope_ui::ST_TD, false, 3),
        wave_scene(scope_ui::SRC_DMM, 1, scope_ui::ST_STOP, false, 0),
        wave_scene(scope_ui::SRC_DMM, 3, scope_ui::ST_RUN, true, 3),
    ];
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
            ck.same_as(&format!("wave/update/{i}->{j}"), &want);
            nw += 1;
        }
    }
    println!("scope additions: {n} FFT scenes, {nt} trigger-tab scenes, {nw} wave-view scenes");
}
