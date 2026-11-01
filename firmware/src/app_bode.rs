//! The Bode page (View::Bode; firmware/docs/bode.md, src/bode.rs for the
//! arithmetic, src/ui_bode.rs for the drawing): the sweep's state machine,
//! the page's taps and keys, BODE:* SCPI. Region C (`.cold`,
//! link-native.x): the page runs no AD2 stream (CH2 is off outside the
//! Scope view) and the only per-pass hook is the view check in
//! App::scope_poll; the acquisition itself is Scope::poll (region B).
//!
//! A sweep, per grid point: the generator to a sine at the point's
//! frequency (BODE_VPP around 1.5 V); the scope to AC coupling, the
//! timebase that holds 4 .. 10 periods (bode::tb_for), the trigger source
//! GEN (so every record carries the generator's phase, gen::Sync) in Auto;
//! after the settling time (3 periods, at least 50 ms) the acquisition is
//! restarted and the first record with the generator's phase is taken:
//! bode::lockin gives CH1's amplitude and phase against the generator. A
//! record clipped or under a fifth of the codes is taken again at the
//! V/div bode::range_retry gives (at most RETRIES times). A "Thru" pass
//! (the probe on the generator's output) is kept in RAM for its grid and
//! is the reference of the later sweeps on that grid: the generator's
//! output stage, the scope's AC coupling and input, the path lag cancel.
//! Without one the gain is against the set amplitude and the phase raw.
//! The generator and the scope get the user's settings back at the end of
//! the sweep (also on Stop and on leaving the page).

use super::*;
use crate::bode::{self as bd, Pt, MAX_PTS};
use crate::scpi::Bode;

/// Generator amplitude (V peak-to-peak) and centre (V) during a sweep.
pub const BODE_VPP: f32 = 2.0;
const BODE_MID: f32 = 1.5;
/// Records taken again at another V/div per point, at most.
const RETRIES: u8 = 4;
/// Slowest timebase of a sweep (50 ms/div: 5 periods of 10 Hz; roll starts
/// at scope::TB_ROLL).
const TB_SLOW: usize = 20;
/// The first point's V/div (0.5 V/div: the generator's 1 V peak at a
/// quarter of the codes) and the settling time's floor.
const RANGE0: usize = 5;
const SETTLE_MIN_MS: u64 = 50;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// The output and the scope set; the acquisition restarts at `at`.
    Settle,
    /// Waiting for a record with the generator's phase until `at`.
    Wait,
}

#[derive(Clone, Copy)]
struct Run {
    idx: u16,
    stage: Stage,
    at: u64,
    tries: u8,
    timeouts: u8,
    range: u8,
}

pub struct Ses {
    pub on: bool,
    /// Points per decade (bode::PPD index) of the next sweep.
    pub ppd_i: u8,
    run: Option<Run>,
    /// The last sweep: a Thru pass, its points per decade, its grid size
    /// and the points measured (in grid order).
    pub thru_pass: bool,
    pub ppd: u8,
    pub grid: u16,
    pub n: u16,
    /// The stored Thru's points per decade (0 = none; points: `thru`).
    pub thru_ppd: u8,
    pub cursor: u16,
    /// The last sweep's end ("" while running or none yet).
    pub msg: &'static str,
    /// What to repaint on the next page pass: the charts / the texts.
    plot_due: bool,
    text_due: bool,
    prev: Option<(gen::Settings, scope::Settings)>,
}

static mut SES: Ses = Ses {
    on: false,
    ppd_i: 0,
    run: None,
    thru_pass: false,
    ppd: 0,
    grid: 0,
    n: 0,
    thru_ppd: 0,
    cursor: 0,
    msg: "",
    plot_due: false,
    text_due: false,
    prev: None,
};

impl Ses {
    /// A sweep runs (App::off_held: no auto power-off mid-sweep).
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// The user's generator and MCX scope settings while a sweep has
    /// replaced them (App::persist_words saves these instead).
    pub fn user_settings(&self) -> Option<(gen::Settings, scope::Settings)> {
        self.prev
    }
}

pub fn ses() -> &'static mut Ses {
    // SAFETY: main loop only.
    unsafe { &mut *core::ptr::addr_of_mut!(SES) }
}

/// The last sweep's points and the stored Thru, out of TCM (the stack's
/// memory: 2 KB there cut the stack headroom under its floor; all zero =
/// no point).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PTS: [[Pt; MAX_PTS]; 2] = [[Pt::NONE; MAX_PTS]; 2];

fn pts() -> &'static mut [Pt; MAX_PTS] {
    // SAFETY: main loop only.
    unsafe { &mut (*core::ptr::addr_of_mut!(PTS))[0] }
}

fn thru() -> &'static mut [Pt; MAX_PTS] {
    // SAFETY: main loop only.
    unsafe { &mut (*core::ptr::addr_of_mut!(PTS))[1] }
}

impl Ses {
    /// The stored Thru is the reference of the last sweep (same grid).
    fn thru_ref(&self) -> bool {
        !self.thru_pass && self.thru_ppd == self.ppd && self.ppd != 0
    }

    /// Gain (dB) and phase (degrees) of point `i` against the Thru, else
    /// against the generator's set amplitude (phase raw).
    pub fn rel(&self, i: usize) -> (f32, f32) {
        let p = &pts()[i];
        if !p.amp.is_finite() {
            return (f32::NAN, f32::NAN);
        }
        if self.thru_ref() {
            let t = &thru()[i];
            if !t.amp.is_finite() {
                return (f32::NAN, f32::NAN);
            }
            bd::relative(p, t.amp, t.ph)
        } else {
            bd::relative(p, BODE_VPP / 2.0, 0.0)
        }
    }

    fn curves(&self, f: &mut [f32; MAX_PTS], g: &mut [f32; MAX_PTS], p: &mut [f32; MAX_PTS]) -> usize {
        let n = self.n as usize;
        for i in 0..n {
            f[i] = pts()[i].f;
            (g[i], p[i]) = self.rel(i);
        }
        n
    }
}

/// Copy `parts` into `buf` (truncated at a char boundary), the text.
fn cat<'a>(buf: &'a mut [u8], parts: &[&str]) -> &'a str {
    let mut n = 0;
    for p in parts {
        for &c in p.as_bytes() {
            if n < buf.len() {
                buf[n] = c;
                n += 1;
            }
        }
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// The CSV's text while it is written (screenshot.rs `start_bode`; 80
/// points of three NR3 numbers fit).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut CSV_TEXT: [u8; 4096] = [0; 4096];

/// Appends to a byte buffer (truncated at its end).
struct W<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl W<'_> {
    fn s(&mut self, t: &str) -> &mut Self {
        for &c in t.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }
}

/// "1.585 kHz" / "39.8 Hz".
fn hz_text(b: &mut [u8; 16], f: f32) -> (&str, &'static str) {
    if f >= 999.5 {
        (crate::dmm::format(b, f / 1000.0, if f < 9999.5 { 3 } else { 2 }), " kHz")
    } else {
        (crate::dmm::format(b, f, if f < 99.95 { 2 } else { 1 }), " Hz")
    }
}

impl App {
    /// The generator and MCX scope settings to persist: during a sweep the
    /// user's from before it, not the sweep's (a power loss or a preset
    /// save mid-sweep keeps the user's).
    pub(super) fn persisted_gen_scope(&self) -> (gen::Settings, scope::Settings) {
        ses().user_settings().unwrap_or((self.gen.s, self.scope.mcx_settings()))
    }

    /// Open the page (scope MF > Settings > Bode, BODE:RUN / BODE:THRU).
    #[inline(never)]
    pub(super) fn bode_open(&mut self, clk: &mut Clock) -> Result<(), &'static str> {
        let now = clk.now_ms();
        let s = ses();
        if s.on && self.view == View::Bode {
            return Ok(());
        }
        let why = if self.scope.held() {
            Some("Bode: scope record in use")
        } else if crate::hy3131::streaming() && !self.scope.wave_on() {
            Some("Bode: data logger running")
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
        s.on = true;
        s.run = None;
        s.msg = "";
        self.show(View::Bode, clk);
        Ok(())
    }

    /// Leaving the page (App::show): a sweep stops, the settings come back.
    #[inline(never)]
    pub(super) fn bode_close(&mut self, clk: &mut Clock) {
        self.bode_stop(clk.now_ms(), "");
        ses().on = false;
    }

    /// Start a sweep (a Thru pass with `thru`).
    #[inline(never)]
    pub(super) fn bode_start(&mut self, now: u64, thru: bool) {
        let s = ses();
        if s.run.is_some() {
            return;
        }
        if s.prev.is_none() {
            s.prev = Some((self.gen.s, self.scope.mcx_settings()));
        }
        // The GEN trigger at the sine's centre (kept in volts by every
        // `apply` of the sweep).
        self.scope.set_trig_volts(BODE_MID);
        s.thru_pass = thru;
        s.ppd = bd::PPD[s.ppd_i.min(2) as usize];
        s.grid = bd::grid_n(s.ppd) as u16;
        s.n = 0;
        s.msg = "";
        s.cursor = 0;
        s.run = Some(Run { idx: 0, stage: Stage::Settle, at: 0, tries: 0, timeouts: 0, range: RANGE0 as u8 });
        s.plot_due = true;
        s.text_due = true;
        self.bode_point(now, true);
    }

    /// Stop the sweep (`msg`: why; "" = keep the last), the user's
    /// generator and scope settings back.
    #[inline(never)]
    pub(super) fn bode_stop(&mut self, now: u64, msg: &'static str) {
        let s = ses();
        if s.run.take().is_some() && !msg.is_empty() {
            s.msg = msg;
        }
        if let Some((g, sc)) = s.prev.take() {
            self.gen.apply(g);
            // The trigger level's volts are the sweep's: the user's row
            // (at the user's V/div) gives them back.
            self.scope.levels_reset();
            self.scope.apply(sc, now);
        }
        s.text_due = true;
        s.plot_due = true;
    }

    /// Set the generator (`gen_too`) and the scope for the current point.
    fn bode_point(&mut self, now: u64, gen_too: bool) {
        let s = ses();
        let Some(r) = s.run.as_mut() else { return };
        let f = bd::grid_f(s.ppd, r.idx as usize);
        if gen_too {
            let gs = gen::Settings { on: true, wave: Wave::Sine, freq_hz: (f + 0.5) as u32, amp_v: BODE_VPP, duty: 5000, offset_v: Some(BODE_MID) };
            self.gen.apply(gs);
        }
        let mut st = self.scope.mcx_settings();
        st.run = true;
        st.tb = bd::tb_for(f, &scope::SDIV, scope::TB_MIN, TB_SLOW);
        st.range = r.range as usize;
        st.ac = true;
        st.pos = 0;
        st.mode = scope::TrigMode::Auto;
        st.tsrc = scope::TSRC_GEN;
        st.ttype = 0;
        st.rising = true;
        st.hpos = 200;
        self.scope.apply(st, now);
        let periods_ms = (3000.0 / f) as u64;
        r.stage = Stage::Settle;
        r.at = now + periods_ms.max(SETTLE_MIN_MS);
    }

    /// The sweep's per-pass step (App::scope_poll on the Bode page, instead
    /// of the scope page's frame handling): the settling time, then the
    /// first record with the generator's phase.
    #[inline(never)]
    pub(super) fn bode_poll(&mut self, now: u64) {
        let polled = self.scope.poll(now).is_some();
        let s = ses();
        let Some(mut r) = s.run else { return };
        match r.stage {
            Stage::Settle => {
                if now >= r.at {
                    // A fresh acquisition: its record starts after the
                    // settling time.
                    let st = self.scope.s;
                    self.scope.apply(st, now);
                    let rec_ms = (scope::CAP as u64 * 1000) / self.scope.sample_rate().max(1) as u64;
                    r.stage = Stage::Wait;
                    r.at = now + 3 * rec_ms + 1500;
                    s.run = Some(r);
                }
            }
            Stage::Wait if polled && now < r.at => {
                let rec = self.scope.record();
                let Some(sy) = self.scope.gen_sync() else { return };
                let Some(l) = bd::lockin(rec, sy.t0, sy.s, sy.q, sy.k) else { return };
                if r.tries < RETRIES {
                    if let Some(nr) = bd::range_retry(l.lo, l.hi, r.range as usize, &scope::VDIV) {
                        r.tries += 1;
                        r.range = nr as u8;
                        s.run = Some(r);
                        self.bode_point(now, false);
                        return;
                    }
                }
                // Volts per code (signed: an inverted map turns the phase).
                let b = self.scope.volts(513.0) - self.scope.volts(512.0);
                let ph = if b < 0.0 { bd::wrap_deg(l.ph + 180.0) } else { l.ph };
                let amp = l.amp * if b < 0.0 { -b } else { b };
                self.bode_store(now, Pt { f: self.gen.actual_hz(), amp, ph });
            }
            Stage::Wait => {
                if now >= r.at {
                    // No record with the phase in time (the generator's
                    // reading failed): restart once, then skip the point.
                    if r.timeouts == 0 {
                        r.timeouts = 1;
                        s.run = Some(r);
                        self.bode_point(now, true);
                    } else {
                        self.bode_store(now, Pt { f: bd::grid_f(s.ppd, r.idx as usize) as f32, amp: f32::NAN, ph: f32::NAN });
                    }
                }
            }
        }
    }

    /// Point `p` measured: the next one, or the end of the sweep.
    fn bode_store(&mut self, now: u64, p: Pt) {
        let s = ses();
        let Some(mut r) = s.run else { return };
        pts()[r.idx as usize] = p;
        s.n = r.idx + 1;
        s.cursor = r.idx;
        s.plot_due = true;
        s.text_due = true;
        r.idx += 1;
        r.tries = 0;
        r.timeouts = 0;
        if r.idx >= s.grid {
            if s.thru_pass {
                *thru() = *pts();
                s.thru_ppd = s.ppd;
            }
            let failed = pts()[..s.n as usize].iter().any(|p| !p.amp.is_finite());
            let msg = match (s.thru_pass, failed) {
                (_, true) => "Done: points missing (no GEN phase)",
                (true, _) => "Thru stored",
                (false, _) => "Done",
            };
            self.bode_stop(now, msg);
            self.beep(now, 80);
            return;
        }
        s.run = Some(r);
        self.bode_point(now, true);
    }

    /// The page drawn whole (App::draw_static).
    #[inline(never)]
    pub(super) fn bode_page(&mut self) {
        ui::bode::frame();
        ses().plot_due = true;
        self.bode_tick();
    }

    /// The page's repaints after a point / a tap (App::tick, drawing; the
    /// texts alone while only they changed).
    #[inline(never)]
    pub(super) fn bode_tick(&mut self) {
        if let Some(r) = crate::screenshot::take_bode_result() {
            let now = self.now;
            let mut b = *b"Saved SCOPE/BODE000.CSV";
            let t = match r {
                crate::screenshot::Shot::Saved(n) => {
                    b[16] = b'0' + (n / 100 % 10) as u8;
                    b[17] = b'0' + (n / 10 % 10) as u8;
                    b[18] = b'0' + (n % 10) as u8;
                    core::str::from_utf8(&b).unwrap_or("Saved")
                }
                crate::screenshot::Shot::Full => "Not saved: BODE000..023 all taken",
                _ => "Not saved: disk error",
            };
            self.toast(now, t, matches!(r, crate::screenshot::Shot::Saved(_)));
            return;
        }
        let s = ses();
        if !s.plot_due && !s.text_due {
            return;
        }
        let mut f = [0f32; MAX_PTS];
        let mut g = [0f32; MAX_PTS];
        let mut p = [0f32; MAX_PTS];
        let n = s.curves(&mut f, &mut g, &mut p);
        let mut sb = [0u8; 40];
        let mut cb = [0u8; 48];
        let mut mb = [0u8; 32];
        let (state, state_fg) = self.bode_state(&mut sb);
        let (cursor, m3db) = Self::bode_texts(&mut cb, &mut mb, &f[..n], &g[..n], &p[..n], s.cursor as usize);
        let ppd_i = s.ppd_i.min(2) as usize;
        let v = ui::bode::Vals { state, state_fg, cursor, m3db, running: s.run.is_some(), ppd: ["10/dec", "20/dec", "5/dec"][ppd_i], thru: s.thru_ppd == bd::PPD[ppd_i] };
        if s.plot_due {
            ui::bode::parts(&v, &ui::bode::Curves { f: &f[..n], g: &g[..n], p: &p[..n], cursor: (n > 0).then_some(s.cursor as usize) });
        } else {
            ui::bode::state(v.state, v.state_fg);
            ui::bode::readout(v.cursor, v.m3db);
        }
        s.plot_due = false;
        s.text_due = false;
    }

    /// The title bar's state text.
    fn bode_state<'a>(&self, b: &'a mut [u8; 40]) -> (&'a str, u16) {
        let s = ses();
        if let Some(r) = s.run {
            let mut nb = [0u8; 16];
            let mut gb = [0u8; 16];
            let t = cat(b, &[if s.thru_pass { "Thru " } else { "Sweep " }, crate::dmm::format(&mut nb, r.idx as f32 + 1.0, 0), "/", crate::dmm::format(&mut gb, s.grid as f32, 0)]);
            return (t, ui::bode::C_RUN);
        }
        if !s.msg.is_empty() {
            let warn = s.msg.as_bytes().first() == Some(&b'D') && s.msg.len() > 4;
            let fg = if warn { ui::bode::C_WARN } else { ui::bode::C_OK };
            if s.msg == "Done" {
                return (if s.thru_ref() { "Done, against Thru" } else { "Done, against GEN (no Thru)" }, ui::bode::C_OK);
            }
            return (s.msg, fg);
        }
        if s.n == 0 {
            (if s.thru_ppd != 0 { "Thru stored: DUT in, Run" } else { "Probe on OUT: Thru, then Run" }, ui::bode::C_PLAIN)
        } else {
            ("", ui::bode::C_PLAIN)
        }
    }

    /// The cursor line and the -3 dB text.
    fn bode_texts<'a, 'b>(cb: &'a mut [u8; 48], mb: &'b mut [u8; 32], f: &[f32], g: &[f32], p: &[f32], cur: usize) -> (&'a str, &'b str) {
        let n = f.len();
        let cur_t = if cur < n {
            let mut fb = [0u8; 16];
            let mut gb = [0u8; 16];
            let mut pb = [0u8; 16];
            let (ft, fu) = hz_text(&mut fb, f[cur]);
            if g[cur].is_finite() {
                let gt = crate::dmm::format(&mut gb, g[cur], 2);
                let pt = crate::dmm::format(&mut pb, p[cur], 1);
                cat(cb, &[ft, fu, "  ", gt, " dB  ", pt, " deg"])
            } else {
                cat(cb, &[ft, fu, "  ---"])
            }
        } else {
            ""
        };
        let m3 = match bd::minus3db(f, g) {
            Some(x) => {
                let mut fb = [0u8; 16];
                let (ft, fu) = hz_text(&mut fb, x);
                cat(mb, &["-3dB ", ft, fu])
            }
            None => "",
        };
        (cur_t, m3)
    }

    /// Page touch.
    #[inline(never)]
    pub(super) fn bode_tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let now = clk.now_ms();
        let s = ses();
        match ui::bode::page_hit(x, y) {
            Some(0) => {
                let v = self.back_view;
                self.show(v, clk);
            }
            Some(1) if s.run.is_none() => self.bode_start(now, true),
            Some(2) => self.bode_button(now),
            Some(3) => self.bode_save(now),
            Some(4) if s.run.is_none() => {
                s.ppd_i = (s.ppd_i + 1) % 3;
                s.plot_due = true;
            }
            Some(5) => {
                let mut f = [0f32; MAX_PTS];
                let n = s.n as usize;
                for (o, p) in f.iter_mut().zip(pts()[..n].iter()) {
                    *o = p.f;
                }
                if let Some(i) = ui::bode::nearest(&f[..n], x) {
                    s.cursor = i as u16;
                    s.plot_due = true;
                }
            }
            _ => {}
        }
    }

    /// Run / Stop (the button and the RUN key).
    #[inline(never)]
    pub(super) fn bode_button(&mut self, now: u64) {
        if ses().run.is_some() {
            self.bode_stop(now, "Stopped");
        } else {
            self.bode_start(now, false);
        }
    }

    /// Save: the last sweep as SCOPE/BODEnnn.CSV (screenshot.rs
    /// `start_bode`; the result toast from `bode_tick`).
    fn bode_save(&mut self, now: u64) {
        let s = ses();
        if s.run.is_some() || s.n == 0 {
            self.toast(now, if s.n == 0 { "No sweep to save" } else { "Bode: sweep running" }, false);
            return;
        }
        if crate::screenshot::busy() || crate::csvlog::writing() {
            self.toast(now, "Disk busy, try again", false);
            return;
        }
        // SAFETY: main loop only; the job reads it until its result.
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(CSV_TEXT) };
        let mut w = W { b: buf, n: 0 };
        let mut t = [0u8; 16];
        w.s("# Alientek ").s(crate::ident::model_name()).s(", opendm40 ").s(crate::ident::VERSION).s(", Bode sweep (no real-time clock)\r\n");
        w.s("# generator sine ").s(crate::dmm::format(&mut t, BODE_VPP, 1)).s(" Vpp around 1.5 V, CH1 AC, ");
        w.s(crate::dmm::format(&mut t, s.ppd as f32, 0)).s(" points/decade, reference ");
        w.s(if s.thru_ref() { "Thru" } else if s.thru_pass { "generator (this is the Thru pass)" } else { "generator (no Thru)" }).s("\r\n");
        w.s("freq_hz,gain_db,phase_deg\r\n");
        let head = w.n;
        let lim = w.b.len() - MAX_PTS - 2; // room for the CR of every line
        let n = self.bode_data(&mut w.b[head..lim]);
        // bode_data's commas between points become line ends.
        let mut k = 0;
        for c in w.b[head..head + n].iter_mut() {
            if *c == b',' {
                k += 1;
                if k % 3 == 0 {
                    *c = b'\n';
                }
            }
        }
        w.n = head + n;
        // CR LF line ends as the scope CSV: rewrite in place from the end.
        let lf = w.b[head..w.n].iter().filter(|&&c| c == b'\n').count();
        let end = w.n + lf + 2;
        let (mut i, mut o) = (w.n, end - 2);
        w.b[o] = b'\r';
        w.b[o + 1] = b'\n';
        while i > head {
            i -= 1;
            let c = w.b[i];
            if c == b'\n' {
                o -= 1;
                w.b[o] = b'\n';
                o -= 1;
                w.b[o] = b'\r';
            } else {
                o -= 1;
                w.b[o] = c;
            }
        }
        let text: &'static [u8] = unsafe { &(&*core::ptr::addr_of!(CSV_TEXT))[..end] };
        if crate::screenshot::start_bode(text) {
            self.toast(now, "Saving ...", true);
        } else {
            self.toast(now, "Disk busy, try again", false);
        }
    }

    /// BODE:* (SCPI). True: a reply in `out`.
    #[inline(never)]
    pub fn bode_scpi(&mut self, clk: &mut Clock, c: Bode, out: &mut crate::scpi::Scpi) -> bool {
        let now = clk.now_ms();
        let s = ses();
        out.len = 0;
        match c {
            Bode::Run(thru) => {
                if s.run.is_none() {
                    if self.bode_open(clk).is_err() {
                        out.set_err("Settings conflict");
                        return false;
                    }
                    self.bode_start(now, thru);
                }
                false
            }
            Bode::Stop => {
                self.bode_stop(now, "Stopped");
                false
            }
            Bode::Points(Some(p)) => {
                match bd::PPD.iter().position(|&x| x == p) {
                    Some(i) if s.run.is_none() => {
                        s.ppd_i = i as u8;
                        s.plot_due = true;
                    }
                    Some(_) => out.set_err("Settings conflict"),
                    None => out.set_err("Illegal parameter value"),
                }
                false
            }
            Bode::Points(None) => {
                out.put_u32(bd::PPD[s.ppd_i.min(2) as usize] as u32);
                true
            }
            Bode::Stat => {
                out.put_bytes(match s.run {
                    Some(_) if s.thru_pass => b"THRU",
                    Some(_) => b"RUN",
                    None if s.n > 0 => b"DONE",
                    None => b"IDLE",
                });
                out.put_bytes(b",");
                out.put_u32(s.n as u32);
                out.put_bytes(b",");
                out.put_u32(s.grid as u32);
                out.put_bytes(if s.thru_ppd == bd::PPD[s.ppd_i.min(2) as usize] { b",1" } else { b",0" });
                true
            }
            Bode::Data => false,
        }
    }

    /// BODE:DATA?: f,gain,phase per measured point (NR3, comma separated;
    /// a point without a result: 9.91E37 for gain and phase).
    #[inline(never)]
    pub fn bode_data(&self, buf: &mut [u8]) -> usize {
        let s = ses();
        let mut n = 0;
        for i in 0..s.n as usize {
            let (g, p) = s.rel(i);
            for (k, v) in [pts()[i].f, g, p].iter().enumerate() {
                if n + 22 > buf.len() {
                    return n;
                }
                if i + k > 0 {
                    buf[n] = b',';
                    n += 1;
                }
                if v.is_finite() {
                    let mut b = [0u8; 20];
                    let m = crate::scpi::nr3(&mut b, *v);
                    buf[n..n + m].copy_from_slice(&b[..m]);
                    n += m;
                } else {
                    buf[n..n + 7].copy_from_slice(b"9.91E37");
                    n += 7;
                }
            }
        }
        n
    }
}
