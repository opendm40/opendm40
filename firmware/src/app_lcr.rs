//! The LCR meter (View::Lcr; src/lcr.rs, src/ui_lcr.rs; docs/lcr.md):
//! the generator drives the DUT with a sine, the power meter's row reads
//! U (V-COM) and I (A-COM) in Power Wave frames (pwave::capture), each
//! frame is fitted at the test frequency (lcr::fit) and corrected.
//! Entry: Power Settings "LCR", or SCPI LCR:STATe ON (the power meter must
//! be on). The page lends the scope's record buffer and holds the HY3131
//! (main.rs `busy`, as Power Wave); it sets the power meter to 6 V and its
//! own I range (lcr::next_range) and the generator to the drive; leaving
//! it gives both back as they were (the power row comes back: main.rs).
//! Region C (.cold, link-native.x): no stream runs while it is open.

use super::*;
use crate::lcr::{self as lc, Corr, Cx, Fit, Model, Res};
use crate::pwave as pv;

/// Least time between two frames (ms): touch, USB and the keys get passes.
const FRAME_MS: u64 = 150;
/// Frames averaged by a correction (short, open, R cal).
const CAL_N: u8 = 4;

/// What a correction job measures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Job {
    Short,
    Open,
    Phase,
}

/// All zero / off at reset (.sram_bss); `lcr_open` sets the rest. The
/// corrections are kept across openings (RAM only).
pub(super) struct Lcr {
    pub on: bool,
    run: bool,
    fi: u8,
    model: u8,
    /// The I range in force (power meter index k).
    k: u8,
    next: u64,
    pub frames: u32,
    /// The last frame's fit and readouts (None: none / not fitted).
    fit: Option<Fit>,
    zm: Cx,
    res: Option<Res>,
    /// What the power meter and the generator were at the entry.
    v_auto: bool,
    vr: u8,
    i_auto: bool,
    ik: u8,
    gen: gen::Settings,
    /// Corrections: the short's excess over the shunt (ohm, per
    /// frequency), the delay per I range (s), the open admittance (S, per
    /// frequency); `cal` bits: 1 short, 2 open (per frequency: << fi * 4),
    /// 0x100 << k the delay of range k.
    extra: [Cx; 2],
    tau: [f32; 3],
    open: [Cx; 2],
    pub cal: u16,
    /// A running correction job: what, frames left, the sum of raw Z.
    job: Option<Job>,
    job_n: u8,
    job_sum: Cx,
    /// The state line (a static text) and its colour.
    info: &'static str,
    info_fg: u16,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut LCR: Lcr = Lcr {
    on: false,
    run: false,
    fi: 0,
    model: 0,
    k: 0,
    next: 0,
    frames: 0,
    fit: None,
    zm: Cx::new(0.0, 0.0),
    res: None,
    v_auto: false,
    vr: 0,
    i_auto: false,
    ik: 0,
    gen: gen::Settings::DEFAULT,
    extra: [Cx::new(0.0, 0.0); 2],
    tau: [0.0; 3],
    open: [Cx::new(0.0, 0.0); 2],
    cal: 0,
    job: None,
    job_n: 0,
    job_sum: Cx::new(0.0, 0.0),
    info: "",
    info_fg: 0,
};

pub(super) fn lcr() -> &'static mut Lcr {
    // SAFETY: main loop only.
    unsafe { &mut *core::ptr::addr_of_mut!(LCR) }
}

/// The page holds the HY3131 (main.rs `busy`).
pub fn lcr_on() -> bool {
    lcr().on
}

const WIRE: &str = "GEN-DUT-A, GEN gnd-COM, V at GEN";

impl App {
    /// The correction of I range `k` at the frequency in force.
    fn lcr_corr(&self, k: u8) -> Corr {
        let s = lcr();
        let (iu, _) = crate::dmm::pw::iu_ir(k);
        let r = self.dmm.pw.r_path[iu as usize];
        Corr { short: Cx::new(r, 0.0).add(s.extra[s.fi as usize]), tau: s.tau[k.min(2) as usize], open: s.open[s.fi as usize] }
    }

    fn lcr_w(&self) -> f32 {
        2.0 * core::f32::consts::PI * self.gen.actual_hz()
    }

    /// The generator's drive at the test frequency.
    fn lcr_drive(&mut self) {
        let s = lcr();
        let g = gen::Settings {
            on: true,
            wave: gen::Wave::Sine,
            freq_hz: lc::FREQS[s.fi as usize],
            amp_v: lc::DRIVE_VPP,
            duty: self.gen.s.duty,
            offset_v: Some(lc::DRIVE_OFS),
        };
        // (Not gen_set: the Signal page is not open; no page draw on the
        // stack of `show`, the static worst path.)
        self.gen.apply(g.normalised());
    }

    /// Open the page (Power Settings "LCR", LCR:STATe ON); Err = why not
    /// (also a toast).
    #[inline(never)]
    pub(super) fn lcr_open(&mut self, clk: &mut Clock) -> Result<(), &'static str> {
        let s = lcr();
        if s.on {
            return Ok(());
        }
        let now = clk.now_ms();
        let pw = &self.dmm.pw;
        let why = if !pw.on {
            Some("LCR: power meter only")
        } else if pw.alt {
            Some("LCR: not with POW:METH ALT")
        } else if pw.job != 0 {
            Some("LCR: a Zero / V cal runs")
        } else if self.logger_busy || crate::hy3131::streaming() || crate::fastlog::active() {
            Some("LCR: data logger running")
        } else if crate::csvlog::active() {
            Some("LCR: recording runs")
        } else if self.scope.held() {
            Some("LCR: scope record in use")
        } else {
            None
        };
        if let Some(t) = why {
            self.toast(now, t, false);
            return Err("Settings conflict");
        }
        if self.scope.wave_on() {
            self.wave_close(clk, false);
        }
        self.scope.lend_buf();
        let pw = &self.dmm.pw;
        (s.v_auto, s.vr, s.i_auto, s.ik) = (pw.v_auto, pw.vr, pw.i_auto, pw.ik);
        s.gen = self.gen.s;
        s.on = true;
        s.run = true;
        s.k = lc::K_START;
        s.next = now;
        s.frames = 0;
        s.fit = None;
        s.res = None;
        s.job = None;
        s.info = WIRE;
        s.info_fg = ui::lcr::C_DIM;
        self.dmm.pw_set_ranges(clk, Some(Some(0)), Some(Some(s.k)));
        self.lcr_drive();
        self.show(View::Lcr, clk);
        Ok(())
    }

    /// Close the page (`show` leaving it): the buffer back to the scope,
    /// the power meter's ranges and the generator as they were.
    #[inline(never)]
    pub(super) fn lcr_close(&mut self, clk: &mut Clock) {
        let s = lcr();
        if !s.on {
            return;
        }
        s.on = false;
        self.scope.return_buf();
        self.gen.apply(s.gen);
        let v = if s.v_auto { None } else { Some(s.vr) };
        let i = if s.i_auto { None } else { Some(s.ik) };
        self.dmm.pw_set_ranges(clk, Some(v), Some(i));
        self.dmm.pw_wave_gap();
    }

    #[inline(never)]
    pub(super) fn lcr_page(&mut self) {
        self.lcr_draw(true);
    }

    #[inline(never)]
    fn lcr_draw(&mut self, full: bool) {
        let s = lcr();
        let (mut bt, mut bv) = ([0u8; 24], [0u8; 16]);
        let mut cb = [[0u8; 24]; 6];
        let mut ub = [0u8; 24];
        let f = lc::FREQS[s.fi as usize];
        let title = {
            let mut nb = [0u8; 16];
            let n = crate::dmm::format(&mut nb, if f >= 1000 { (f / 1000) as f32 } else { f as f32 }, 0);
            let mut tb = [0u8; 16];
            let nn = n.len();
            tb[..nn].copy_from_slice(n.as_bytes());
            let fs = core::str::from_utf8(&tb[..nn]).unwrap_or("");
            ui::lcr::cat(&mut bt, &[fs, if f >= 1000 { "kHz  I " } else { "Hz  I " }, crate::dmm::pw::I_NAMES[s.k as usize]])
        };
        let (name, val, unit, cells): (&str, &str, &str, [&str; 6]);
        let mut c6 = cb.iter_mut();
        let mut nx = || c6.next().unwrap();
        match (s.res, s.fit) {
            (Some(r), Some(ft)) => {
                let par = r.par;
                let (nm, v, u) = match r.kind {
                    lc::Kind::L => (if par { "Lp" } else { "Ls" }, r.l, "H"),
                    lc::Kind::C => (if par { "Cp" } else { "Cs" }, r.c, "F"),
                    lc::Kind::R => (if par { "Rp" } else { "Rs" }, r.r, "\u{3a9}"),
                };
                name = nm;
                if r.z > 50e6 || !r.z.is_finite() {
                    (val, unit) = ("OL", "");
                } else {
                    match ui::lcr::eng(&mut bv, v) {
                        Some((n, p)) => {
                            val = n;
                            unit = ui::lcr::cat(&mut ub, &[p, u]);
                        }
                        None => (val, unit) = ("", ""),
                    }
                }
                let rn = if par { "Rp" } else { "ESR" };
                cells = [
                    ui::lcr::cell(nx(), rn, r.r, "\u{3a9}"),
                    ui::lcr::plain(nx(), "D", r.d, 4),
                    ui::lcr::plain(nx(), "Q", r.q, 2),
                    ui::lcr::cell(nx(), "|Z|", r.z, "\u{3a9}"),
                    ui::lcr::plain(nx(), "Phase", r.theta, 2),
                    ui::lcr::cell(nx(), "I", ft.i.abs() * core::f32::consts::FRAC_1_SQRT_2, "A"),
                ];
            }
            _ => {
                (name, val, unit) = ("", "", "");
                cells = [ui::lcr::cell(nx(), "ESR", f32::NAN, ""), "D --", "Q --", "|Z| --", "Phase --", "I --"];
            }
        }
        let v = ui::lcr::Vals { title, name, val, unit, cells, info: s.info, info_fg: s.info_fg, fi: s.fi, model: s.model, run: s.run };
        if full {
            ui::lcr::page(&v);
        } else {
            ui::lcr::live(&v);
        }
    }

    /// Per pass on the page (App::tick): a frame when due.
    #[inline(never)]
    pub(super) fn lcr_tick(&mut self, clk: &mut Clock) {
        let s = lcr();
        if !s.on {
            return;
        }
        let pw = &self.dmm.pw;
        if !pw.on || pw.alt || pw.job != 0 {
            let v = self.back_view;
            self.show(v, clk);
            return;
        }
        let now = clk.now_ms();
        if !s.run || now < s.next || crate::hy3131::streaming() {
            return;
        }
        let Some(buf) = self.scope.lent_buf() else {
            return;
        };
        let (row, r36, sc, _, _) = self.dmm.pw_wave_setup();
        pv::arm(&row, r36);
        let cap = pv::capture(buf, lc::SPAN_US, None, row[20]);
        s.frames = s.frames.wrapping_add(1);
        s.next = clk.now_ms() + FRAME_MS;
        let fit = lc::fit(buf, &cap, &sc, self.gen.actual_hz());
        // Something else drives the circuit: the generator cannot reach
        // the 60 mA range's rail (~245 mA) or 12 V on V.
        if cap.clip_u || (cap.clip_i && s.k == lc::K_MAX) {
            let v = if matches!(self.back_view, View::Dmm | View::DmmScope) { self.back_view } else { View::Dmm };
            self.show(v, clk);
            self.toast(now, if cap.clip_u { "LCR: external voltage" } else { "LCR: external current" }, false);
            return;
        }
        let k = lc::next_range(s.k, fit.map_or(0, |f| f.ipk_raw), cap.clip_i);
        if k != s.k {
            // A range change: this frame is not used (and a job restarts).
            s.k = k;
            self.dmm.pw_set_ranges(clk, None, Some(Some(k)));
            if s.job.is_some() {
                s.job_n = CAL_N;
                s.job_sum = Cx::new(0.0, 0.0);
            }
            self.lcr_draw(false);
            return;
        }
        s.fit = fit;
        let w = self.lcr_w();
        match fit {
            Some(ft) if ft.z().is_finite() => {
                s.zm = ft.z();
                let z = lc::correct(s.zm, &self.lcr_corr(s.k), w);
                let m = [Model::Auto, Model::Series, Model::Parallel][s.model.min(2) as usize];
                s.res = Some(lc::solve(z, w, m));
                if s.job.is_some() {
                    self.lcr_job_step(now);
                }
            }
            _ => s.res = None,
        }
        self.lcr_draw(false);
    }

    /// One frame of a correction job (`s.zm` the raw Z).
    #[inline(never)]
    fn lcr_job_step(&mut self, now: u64) {
        let s = lcr();
        let Some(job) = s.job else {
            return;
        };
        s.job_sum = s.job_sum.add(s.zm);
        s.job_n = s.job_n.saturating_sub(1);
        if s.job_n > 0 {
            return;
        }
        s.job = None;
        let zm = s.job_sum.scale(1.0 / CAL_N as f32);
        let w = self.lcr_w();
        let fi = s.fi as usize;
        let mut k0 = self.lcr_corr(s.k);
        let (ok, t): (bool, &'static str) = match job {
            Job::Short => {
                // Only on the 60 mA range (a short draws ~30 mA) and a
                // few ohm at most (fuse, leads, contacts).
                let e = zm.sub(k0.short.sub(s.extra[fi]));
                if s.k == lc::K_MAX && e.abs() < 5.0 {
                    s.extra[fi] = e;
                    s.cal |= 1 << (fi * 4);
                    (true, "Short stored")
                } else {
                    (false, "Short failed: join the clips")
                }
            }
            Job::Open => {
                k0.open = Cx::new(0.0, 0.0);
                let z = lc::correct(zm, &k0, w);
                if s.k == 0 && z.abs() > 100e3 {
                    s.open[fi] = z.inv();
                    s.cal |= 2 << (fi * 4);
                    (true, "Open stored")
                } else {
                    (false, "Open failed: remove the DUT")
                }
            }
            Job::Phase => match lc::tau_from_resistor(zm, &k0, w) {
                Some(t) => {
                    s.tau[s.k.min(2) as usize] = t;
                    s.cal |= 0x100 << s.k;
                    (true, "R cal stored for this range")
                }
                None => (false, "R cal failed: not a resistor"),
            },
        };
        s.info = t;
        s.info_fg = if ok { ui::lcr::C_OK } else { ui::lcr::C_WARN };
        self.beep(now, if ok { 60 } else { 300 });
    }

    /// Start a correction job (the buttons, LCR:CORR:SHOR / OPEN / PHAS).
    pub(super) fn lcr_job(&mut self, job: Job) {
        let s = lcr();
        s.job = Some(job);
        s.job_n = CAL_N;
        s.job_sum = Cx::new(0.0, 0.0);
        s.run = true;
        s.info = match job {
            Job::Short => "Short: clips joined...",
            Job::Open => "Open: no DUT...",
            Job::Phase => "R cal: a resistor in the clips...",
        };
        s.info_fg = ui::lcr::C_PLAIN;
    }

    /// The corrections back to none.
    pub(super) fn lcr_clear(&mut self) {
        let s = lcr();
        (s.extra, s.tau, s.open, s.cal) = ([Cx::new(0.0, 0.0); 2], [0.0; 3], [Cx::new(0.0, 0.0); 2], 0);
    }

    /// The test frequency (index into lcr::FREQS).
    pub(super) fn lcr_set_freq(&mut self, fi: u8) {
        let s = lcr();
        s.fi = fi.min(1);
        s.job = None;
        if s.on {
            self.lcr_drive();
            s.res = None;
            s.run = true;
        }
    }

    pub(super) fn lcr_set_model(&mut self, m: u8) {
        let s = lcr();
        s.model = m.min(2);
        if let Some(ft) = s.fit.filter(|_| s.on) {
            let w = self.lcr_w();
            let z = lc::correct(ft.z(), &self.lcr_corr(s.k), w);
            s.res = Some(lc::solve(z, w, [Model::Auto, Model::Series, Model::Parallel][s.model as usize]));
        }
    }

    /// Page taps.
    #[inline(never)]
    pub(super) fn lcr_tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let now = clk.now_ms();
        match ui::lcr::page_hit(x, y) {
            Some(0) => {
                let v = self.back_view;
                self.show(v, clk);
                return;
            }
            Some(1) => self.lcr_job(Job::Short),
            Some(2) => self.lcr_job(Job::Open),
            Some(3) => self.lcr_job(Job::Phase),
            Some(4) => self.lcr_run(now),
            Some(c @ 10..=11) => self.lcr_set_freq(c - 10),
            Some(c @ 20..=22) => self.lcr_set_model(c - 20),
            _ => return,
        }
        self.lcr_draw(false);
    }

    /// RUN / STOP (the button, the RUN key).
    pub(super) fn lcr_run(&mut self, now: u64) {
        let s = lcr();
        s.run = !s.run;
        s.next = now;
        if !s.run {
            s.job = None;
        }
        ui::lcr::run_button(s.run);
    }

    /// LCR:FETCh? : frames, f (Hz), |Z|, theta (deg), R (ESR or Rp), X,
    /// L, C (the model's), D, Q, the main reading (L / C / R), the model
    /// (SER / PAR), the I range (A), the corrections in force (`cal`
    /// bits). 9.91E37 = none.
    #[inline(never)]
    pub(super) fn lcr_fetch(&self, out: &mut crate::scpi::Scpi) {
        let s = lcr();
        let mut t = [0u8; 24];
        out.put_bytes(crate::dmm::pw::i64_text(&mut t, s.frames as i64));
        let r = s.res.filter(|_| s.on);
        let f = if s.on { self.gen.actual_hz() } else { f32::NAN };
        let nan = f32::NAN;
        let v = r.map_or([nan; 9], |r| [r.z, r.theta, r.r, r.x, r.l, r.c, r.d, r.q, 0.0]);
        for x in [f, v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]] {
            out.put_bytes(b",");
            out.put_f32(if x.is_finite() { x } else { 9.91e37 });
        }
        out.put_bytes(match r.map(|r| r.kind) {
            Some(lc::Kind::L) => b",L",
            Some(lc::Kind::C) => b",C",
            Some(lc::Kind::R) => b",R",
            None => b",NONE",
        });
        out.put_bytes(if r.is_some_and(|r| r.par) { b",PAR," } else { b",SER," });
        out.put_f32(crate::dmm::pw::I_LIM[s.k.min(5) as usize]);
        out.put_bytes(b",");
        out.put_bytes(crate::dmm::pw::i64_text(&mut t, s.cal as i64));
    }

    /// LCR:CORRection? : the corrections at the frequency in force: the
    /// short's excess R, X (ohm), the open G, B (S), the delays of the
    /// three ranges (us), whether a job runs.
    #[inline(never)]
    pub(super) fn lcr_corr_query(&self, out: &mut crate::scpi::Scpi) {
        let s = lcr();
        let fi = s.fi as usize;
        let v = [s.extra[fi].re, s.extra[fi].im, s.open[fi].re, s.open[fi].im, s.tau[0] * 1e6, s.tau[1] * 1e6, s.tau[2] * 1e6];
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                out.put_bytes(b",");
            }
            out.put_f32(*x);
        }
        out.put_bytes(if s.job.is_some() { b",1" } else { b",0" });
    }

    /// The SCPI LCR subsystem (scpi.rs `Lcr`); true = a reply was written.
    #[inline(never)]
    pub(super) fn lcr_scpi(&mut self, clk: &mut Clock, c: crate::scpi::Lcr, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Lcr as L;
        let s = lcr();
        match c {
            L::State(Some(true)) => {
                if self.lcr_open(clk).is_err() {
                    out.set_err("Settings conflict");
                }
                false
            }
            L::State(Some(false)) => {
                if self.view == View::Lcr {
                    let v = self.back_view;
                    self.show(v, clk);
                }
                false
            }
            L::State(None) => {
                out.put_bytes(if s.on { b"1" } else { b"0" });
                true
            }
            L::Freq(Some(f)) => {
                self.lcr_set_freq(if f >= 1000 { 1 } else { 0 });
                self.redraw |= self.view == View::Lcr;
                false
            }
            L::Freq(None) => {
                out.put_u32(lc::FREQS[s.fi as usize]);
                true
            }
            L::Model(Some(m)) => {
                self.lcr_set_model(m);
                self.redraw |= self.view == View::Lcr;
                false
            }
            L::Model(None) => {
                let m: [&[u8]; 3] = [b"AUTO", b"SER", b"PAR"];
                out.put_bytes(m[s.model.min(2) as usize]);
                true
            }
            L::Fetch => {
                self.lcr_fetch(out);
                true
            }
            L::Corr(j) => {
                if !s.on {
                    out.set_err("Settings conflict");
                } else {
                    self.lcr_job([Job::Short, Job::Open, Job::Phase][j.min(2) as usize]);
                    self.redraw |= self.view == View::Lcr;
                }
                false
            }
            L::CorrClear => {
                self.lcr_clear();
                false
            }
            L::CorrQuery => {
                self.lcr_corr_query(out);
                true
            }
        }
    }
}
