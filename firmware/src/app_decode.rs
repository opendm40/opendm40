//! The protocol decoder view (TODO "Protocol decoders", phase 1; View::Decode):
//! opened from the scope page (MF > Settings > Decode) or DECode:STATe ON,
//! closed by MENU / SCOPE or DECode:STATe OFF. While it is open the scope's
//! input streams into the decoder (scope.rs `dec_*`, decode.rs); the frames
//! stay in memory after it closes (CLEAR / DECode:CLEar drop them).
//!
//! Main loop: `dec_poll` every pass (App::scope_poll) and between the list
//! rows a pass draws (`dec_tick`, at most ROWS_PER_PASS), so the ring
//! (CAP samples: 21 ms at 1.25 MS/s) is read well within its time. All of
//! it is region B (no .cold: it runs while the stream does).

use super::*;
use crate::decode::{self as dc, Cfg, Dec};
use crate::scope_ui::dec as ud;

/// Rows drawn per main-loop pass (the decoder runs between them).
const ROWS_PER_PASS: usize = 1;
/// Counter cells / top bar refresh, ms.
const STAT_MS: u64 = 250;
/// Threshold stepper, V.
const THR_STEP: f32 = 0.1;

/// Decoder and view state, in SRAM (`.sram_bss`, zeroed at reset; `init`
/// sets the non-zero fields), not in the App: TCM .bss shortens the stack.
pub(super) struct DecState {
    pub d: Dec,
    /// Settings (DECode:..., the setup popup); manual threshold in V (NaN:
    /// auto).
    pub cfg: Cfg,
    pub thr_v: f32,
    /// Decoding (the view is open), paused (STOP).
    pub on: bool,
    pub paused: bool,
    /// The stream generation the decoder was restarted for.
    gen: u32,
    /// The list: the top row's sequence number (when not following), LIVE.
    top: u32,
    live: bool,
    /// What each row shows: frame sequence + 1 (0: empty / a note).
    shown: [u32; ud::ROWS],
    /// The note shown (empty list), the counters' and top bar's state.
    note: u8,
    stat_at: u64,
    cnt: [u32; 4],
    top_key: [u32; ud::TOP_ITEMS],
    /// Time zero of the list: the first frame since the last clear.
    base_seq: u32,
    base_us: u64,
    /// Setup popup open; a press on a panel button (feedback), a drag.
    pub popup: bool,
    pressed: u8,
    drag: Option<(i16, u32)>,
    /// The whole view needs drawing.
    full: bool,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut DS: DecState = unsafe { core::mem::zeroed() };

pub(super) fn ds() -> &'static mut DecState {
    unsafe { &mut *core::ptr::addr_of_mut!(DS) }
}

/// Boot: the non-zero defaults.
pub(super) fn init() {
    let s = ds();
    s.d.init();
    s.cfg = Cfg::DEFAULT;
    s.thr_v = f32::NAN;
    s.live = true;
    s.gen = u32::MAX;
}

impl App {
    /// Open the decoder view and start decoding. Err: the SCPI error (and
    /// a toast).
    pub(super) fn dec_open(&mut self, clk: &mut Clock) -> Result<(), &'static str> {
        let now = clk.now_ms();
        let why = if self.scope.wave_on() {
            Some("Decode: MCX input only")
        } else if self.scope.held() {
            Some("Decode: scope record in use")
        } else if self.view == View::Inrush {
            Some("Decode: inrush page open")
        } else {
            None
        };
        if let Some(t) = why {
            self.toast(now, t, false);
            return Err("Settings conflict");
        }
        if self.view != View::Decode {
            self.show(View::Decode, clk);
        }
        let s = ds();
        s.paused = false;
        s.popup = false;
        self.dec_start(now);
        Ok(())
    }

    /// (Re)start the stream at the settings' rate.
    fn dec_start(&mut self, now: u64) {
        let s = ds();
        s.on = true;
        if !s.paused {
            self.scope.dec_begin(dc::fs_for(s.cfg.baud), now);
        }
        s.gen = u32::MAX;
        s.full = true;
    }

    /// Leaving the view (App::show): the scope gets its input back.
    pub(super) fn dec_leave(&mut self, now: u64) {
        let s = ds();
        s.on = false;
        s.popup = false;
        s.drag = None;
        self.scope.dec_end(now);
    }

    /// Settings changed: restart with them (thresholds only: in place).
    fn dec_changed(&mut self, now: u64) {
        let s = ds();
        if s.on {
            self.dec_start(now);
        }
        s.full = true;
        self.redraw |= self.view == View::Decode && !s.popup;
    }

    fn dec_thr_code(&self) -> Option<u16> {
        let v = ds().thr_v;
        v.is_finite().then(|| self.scope.dec_code_of(v))
    }

    /// Decode what the stream wrote (every main-loop pass; cheap without a
    /// decoder session).
    #[inline(never)]
    pub fn dec_poll(&mut self, now: u64) {
        let s = ds();
        if !s.on || s.paused {
            return;
        }
        let Some((wr, gen, laps)) = self.scope.dec_poll(now) else { return };
        if gen != s.gen {
            s.gen = gen;
            let code = self.dec_thr_code();
            s.d.restart(s.cfg, dc::fs_for(s.cfg.baud), now * 1000, code);
            return;
        }
        if laps > 0 {
            s.d.lost();
        }
        crate::prof::enter(crate::prof::ACQ);
        s.d.poll(self.scope.dec_ring(), wr);
        crate::prof::exit(crate::prof::ACQ);
    }

    // ---- drawing ----

    /// The whole view (draw_static).
    #[inline(never)]
    pub(super) fn dec_page(&mut self) {
        let now = self.now;
        ud::chrome(&mut || self.dec_poll(now));
        let s = ds();
        ud::panel(s.live, s.paused, &mut || self.dec_poll(now));
        let s = ds();
        s.shown = [u32::MAX; ud::ROWS];
        s.note = u8::MAX;
        s.top_key = [u32::MAX; ud::TOP_ITEMS];
        s.cnt = [u32::MAX; 4];
        s.stat_at = 0;
        s.full = false;
        if s.popup {
            self.dec_setup_draw();
        }
    }

    /// One pass of the view: the top bar / counters when they changed (at
    /// most every STAT_MS), then up to ROWS_PER_PASS changed rows.
    #[inline(never)]
    pub(super) fn dec_tick(&mut self, now: u64) {
        let s = ds();
        if s.full {
            self.redraw = true;
            return;
        }
        if s.popup {
            return;
        }
        // A top-bar item or a counter cell, or else the rows.
        if now >= s.stat_at {
            crate::prof::enter(crate::prof::STAT);
            let drew = self.dec_stats();
            crate::prof::exit(crate::prof::STAT);
            if drew {
                self.dec_poll(now);
                return;
            }
            ds().stat_at = now + STAT_MS;
        }
        let s = ds();
        let st = &s.d.st;
        // Time zero: the first frame since the clear.
        if s.base_us == 0 && st.next > s.base_seq {
            if let Some(h) = st.get(s.base_seq.max(st.first)) {
                s.base_us = h.t_us;
            }
        }
        let n = ud::ROWS as u32;
        // LIVE: the list moves by half a page when the newest frame passes
        // its end (a burst: to the newest page), so a new frame costs one
        // row, not a redraw of every row.
        if s.live && st.next > s.top + n {
            s.top = (s.top + n / 2).max(st.next.saturating_sub(n));
        }
        let top = s.top.clamp(st.first, st.next.saturating_sub(1).max(st.first));
        s.top = top;
        // An empty list: one note.
        if st.next == st.first {
            let k = if s.paused {
                1
            } else if s.d.threshold().is_none() {
                2
            } else if s.d.baud() == 0 {
                3
            } else {
                4
            };
            if s.note != k {
                s.note = k;
                let t = ["", "Stopped", "Waiting for a signal (no swing)", "Measuring the bit rate", "No frames yet"][k as usize];
                ud::row(0, Some(&ud::Row { time: "", text: t, ascii: "", status: "", fg: 0 }));
                s.shown[0] = 0;
                for i in 1..ud::ROWS {
                    if s.shown[i] != 0 {
                        ud::row(i, None);
                        s.shown[i] = 0;
                    }
                }
            }
            return;
        }
        s.note = 0;
        let mut drawn = 0;
        for i in 0..ud::ROWS {
            let seq = top + i as u32;
            let want = if seq < st.next { seq + 1 } else { 0 };
            if s.shown[i] == want {
                continue;
            }
            s.shown[i] = want;
            if want == 0 {
                ud::row(i, None);
            } else {
                crate::prof::enter(crate::prof::X2);
                ud::frame_row(i, st, seq, s.base_us);
                crate::prof::exit(crate::prof::X2);
            }
            drawn += 1;
            self.dec_poll(now);
            if drawn >= ROWS_PER_PASS {
                break;
            }
        }
    }

    /// The first top-bar item or counter cell that changed, drawn (one
    /// paint per pass: a badge is a few ms); false when none changed.
    fn dec_stats(&mut self) -> bool {
        let s = ds();
        let c = s.d.st.cnt;
        let mb = s.cfg.modbus;
        let lock = s.d.baud();
        let state = if s.paused {
            2
        } else if s.d.threshold().is_none() || lock == 0 {
            0
        } else {
            1
        };
        let mut rb = [0u8; 24];
        let mut r = dc::W { b: &mut rb, n: 0 };
        if s.cfg.baud == 0 {
            r.s("Auto ");
        }
        if lock != 0 {
            r.u(lock).c(b' ');
        } else if s.cfg.baud != 0 {
            r.u(s.cfg.baud).c(b' ');
        }
        r.s(dc::FMT_NAMES[s.cfg.fmt as usize % 4]);
        let rn = r.n;
        let mut hb = [0u8; 16];
        let hn = self.dec_thr_text(&mut hb, true);
        let t = ud::Top {
            state,
            proto: if mb { "MODBUS" } else { "UART" },
            rate: core::str::from_utf8(&rb[..rn]).unwrap_or(""),
            thr: core::str::from_utf8(&hb[..hn]).unwrap_or(""),
            inv: s.cfg.inverted,
            battery: self.power.bars.min(5),
            charging: self.power.charging(),
        };
        for k in 0..ud::TOP_ITEMS {
            let key = ud::top_key(k, &t);
            if s.top_key[k] != key {
                s.top_key[k] = key;
                ud::top_item(k, &t);
                return true;
            }
        }
        let v = [c.frames, c.fe, c.pe, c.crc, c.ovr, mb as u32];
        for i in 0..4 {
            let key = ud::cell_key(i, &v);
            if s.cnt[i] != key {
                s.cnt[i] = key;
                ud::cell_draw(i, &v);
                return true;
            }
        }
        false
    }

    /// The threshold text: "1.65V", auto with the level in use "A 1.65V"
    /// (`top`) / "Auto", or "A --" before one is known.
    fn dec_thr_text(&self, b: &mut [u8; 16], top: bool) -> usize {
        let s = ds();
        let v = if s.thr_v.is_finite() { Some(s.thr_v) } else { s.d.threshold().map(|t| self.scope.dec_volts(t.0)) };
        let mut w = dc::W { b, n: 0 };
        if !s.thr_v.is_finite() && top {
            w.s("A ");
        }
        match v {
            Some(v) => {
                let mut t = [0u8; 16];
                w.s(crate::dmm::format(&mut t, v, 2)).s("V");
            }
            None => {
                w.s("--");
            }
        }
        w.n
    }

    // ---- setup popup ----

    fn dec_setup_draw(&mut self) {
        let s = ds();
        let mut bb = [0u8; 16];
        let mut b = dc::W { b: &mut bb, n: 0 };
        if s.cfg.baud == 0 {
            b.s("Auto");
        } else {
            b.u(s.cfg.baud);
        }
        let bn = b.n;
        let mut hb = [0u8; 16];
        let hn = self.dec_thr_text(&mut hb, false);
        let v = ud::SetupVals {
            modbus: s.cfg.modbus,
            fmt: s.cfg.fmt,
            inv: s.cfg.inverted,
            baud: core::str::from_utf8(&bb[..bn]).unwrap_or(""),
            thr_auto: !s.thr_v.is_finite(),
            thr: core::str::from_utf8(&hb[..hn]).unwrap_or(""),
        };
        let now = self.now;
        ud::setup(&v, &mut || self.dec_poll(now));
    }

    fn dec_setup_tap(&mut self, h: ud::SetupHit, now: u64) {
        let s = ds();
        match h {
            ud::SetupHit::Close => {
                s.popup = false;
                self.redraw = true;
                return;
            }
            ud::SetupHit::Modbus(m) => s.cfg.modbus = m,
            ud::SetupHit::Fmt(f) => s.cfg.fmt = f,
            ud::SetupHit::Inv(i) => s.cfg.inverted = i,
            ud::SetupHit::Baud(up) => {
                let i = ud::BAUDS.iter().position(|&b| b == s.cfg.baud).unwrap_or(0);
                let j = if up { (i + 1).min(ud::BAUDS.len() - 1) } else { i.saturating_sub(1) };
                s.cfg.baud = ud::BAUDS[j];
            }
            ud::SetupHit::ThrAuto(auto) => {
                s.thr_v = if auto { f32::NAN } else { self.dec_level().unwrap_or(1.5) };
                self.dec_set_thr();
                self.dec_setup_draw();
                return;
            }
            ud::SetupHit::Thr(up) => {
                let v = if s.thr_v.is_finite() { s.thr_v } else { self.dec_level().unwrap_or(1.5) };
                let q = v / THR_STEP;
                let q = if q >= 0.0 { (q + 0.5) as i32 } else { (q - 0.5) as i32 };
                let v = (q + if up { 1 } else { -1 }) as f32 * THR_STEP;
                s.thr_v = v.clamp(-8.0 * self.scope.s.probe, 8.0 * self.scope.s.probe);
                self.dec_set_thr();
                self.dec_setup_draw();
                return;
            }
            _ => return,
        }
        self.dec_changed(now);
        self.dec_setup_draw();
    }

    /// The level in use, V.
    fn dec_level(&self) -> Option<f32> {
        ds().d.threshold().map(|t| self.scope.dec_volts(t.0))
    }

    fn dec_set_thr(&mut self) {
        let code = self.dec_thr_code();
        let s = ds();
        s.d.set_threshold(code);
        s.top_key = [u32::MAX; ud::TOP_ITEMS];
    }

    // ---- input ----

    /// Touches and keys of the view: Some(captured) when handled.
    pub(super) fn dec_gesture(&mut self, clk: &mut Clock, g: Gesture) -> Option<bool> {
        let now = clk.now_ms();
        let s = ds();
        match g {
            Gesture::KeyDown(k) => {
                self.beep(now, 50);
                match k {
                    0 if s.popup => {
                        s.popup = false;
                        self.redraw = true;
                    }
                    0 => self.show(View::Scope, clk),
                    1 => self.dec_pause(now),
                    3 => {
                        s.popup = !s.popup;
                        self.redraw = true;
                    }
                    _ => {}
                }
                Some(false)
            }
            Gesture::Down(p) if !s.popup => {
                if let Some(k) = ud::panel_hit(p.x, p.y) {
                    s.pressed = k as u8 + 1;
                    ud::panel_button(k, s.live, s.paused, true);
                } else if ud::LIST.has(p.x as i16, p.y as i16) {
                    s.drag = Some((p.y as i16, s.top));
                }
                Some(false)
            }
            Gesture::Drag(_, p, _) | Gesture::Move(_, p) if !s.popup => {
                if let Some((y0, t0)) = s.drag {
                    let rows = (y0 - p.y as i16) / ud::ROW_H;
                    let t = (t0 as i64 + rows as i64).clamp(s.d.st.first as i64, s.d.st.next.saturating_sub(1).max(s.d.st.first) as i64) as u32;
                    if rows != 0 || !s.live {
                        let live = t + ud::ROWS as u32 >= s.d.st.next && rows > 0;
                        self.dec_scroll(t, live);
                    }
                }
                Some(true)
            }
            Gesture::Up(_) | Gesture::Cancel => {
                s.drag = None;
                self.dec_unpress();
                Some(false)
            }
            Gesture::Tap(p) => {
                self.beep(now, 30);
                self.dec_unpress();
                let s = ds();
                if s.popup {
                    let h = ud::setup_hit(p.x as i16, p.y as i16);
                    self.dec_setup_tap(h, now);
                    return Some(false);
                }
                match ud::panel_hit(p.x, p.y) {
                    Some(0) => self.dec_page_step(false),
                    Some(1) => self.dec_scroll(0, true),
                    Some(2) => self.dec_page_step(true),
                    Some(3) => {
                        s.popup = true;
                        self.redraw = true;
                    }
                    Some(4) => self.dec_clear(),
                    Some(5) => self.dec_pause(now),
                    Some(6) => self.show(View::Scope, clk),
                    _ => {}
                }
                Some(false)
            }
            Gesture::MultiSwipeDown(_) => None,
            _ => Some(false),
        }
    }

    fn dec_unpress(&mut self) {
        let s = ds();
        if s.pressed != 0 {
            let k = s.pressed as usize - 1;
            s.pressed = 0;
            if !s.popup {
                ud::panel_button(k, s.live, s.paused, false);
            }
        }
    }

    fn dec_scroll(&mut self, top: u32, live: bool) {
        let s = ds();
        if live != s.live {
            s.live = live;
            ud::panel_button(1, live, s.paused, false);
        }
        s.top = top;
    }

    fn dec_page_step(&mut self, down: bool) {
        let s = ds();
        let n = ud::ROWS as u32;
        let cur = s.top;
        let t = if down { cur + n - 1 } else { cur.saturating_sub(n - 1).max(s.d.st.first) };
        let live = down && t + n >= s.d.st.next;
        self.dec_scroll(t.min(s.d.st.next.saturating_sub(1).max(s.d.st.first)), live);
    }

    fn dec_clear(&mut self) {
        let s = ds();
        s.d.clear();
        s.base_seq = s.d.st.next;
        s.base_us = 0;
        s.top = s.d.st.next;
        s.shown = [u32::MAX; ud::ROWS];
        s.note = u8::MAX;
        if !s.live {
            self.dec_scroll(s.d.st.next, true);
        }
    }

    fn dec_pause(&mut self, now: u64) {
        let s = ds();
        s.paused = !s.paused;
        if s.paused {
            self.scope.dec_halt();
        } else {
            self.dec_start(now);
        }
        ud::panel_button(5, s.live, s.paused, false);
        s.top_key = [u32::MAX; ud::TOP_ITEMS];
        s.note = u8::MAX;
    }

    // ---- SCPI ----

    /// DECode:... except FRAMes?: true when a reply text is in `out`.
    #[inline(never)]
    pub fn dec_scpi(&mut self, clk: &mut Clock, c: crate::scpi::Dcd, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Dcd as D;
        let now = clk.now_ms();
        let s = ds();
        out.len = 0;
        let mut changed = false;
        match c {
            D::State(None) => out.put_bytes(if s.on { b"1" } else { b"0" }),
            D::State(Some(true)) => {
                if !s.on || s.paused {
                    s.paused = false;
                    if let Err(e) = self.dec_open(clk) {
                        out.set_err(e);
                    }
                }
                return false;
            }
            D::State(Some(false)) => {
                if self.view == View::Decode {
                    self.show(View::Scope, clk);
                } else if s.on {
                    self.dec_leave(now);
                }
                return false;
            }
            D::Modbus(None) => out.put_bytes(if s.cfg.modbus { b"MODB" } else { b"UART" }),
            D::Modbus(Some(m)) => {
                changed = s.cfg.modbus != m;
                s.cfg.modbus = m;
            }
            D::Baud(None) => {
                if s.cfg.baud == 0 {
                    out.put_bytes(b"AUTO");
                } else {
                    out.put_u32(s.cfg.baud);
                }
            }
            D::Baud(Some(b)) => {
                changed = s.cfg.baud != b;
                s.cfg.baud = b;
            }
            D::Rate => out.put_u32(s.d.baud()),
            D::Fmt(None) => out.put_bytes(dc::FMT_NAMES[s.cfg.fmt as usize % 4].as_bytes()),
            D::Fmt(Some(f)) => {
                changed = s.cfg.fmt != f;
                s.cfg.fmt = f;
            }
            D::Inv(None) => out.put_bytes(if s.cfg.inverted { b"INV" } else { b"NORM" }),
            D::Inv(Some(i)) => {
                changed = s.cfg.inverted != i;
                s.cfg.inverted = i;
            }
            D::Thr(None) => {
                if s.thr_v.is_finite() {
                    out.put_f32(s.thr_v);
                } else {
                    out.put_bytes(b"AUTO");
                }
            }
            D::Thr(Some(v)) => {
                s.thr_v = v;
                self.dec_set_thr();
                return false;
            }
            D::Level => match self.dec_level() {
                Some(v) => out.put_f32(v),
                None => out.put_bytes(crate::scpi::NAN),
            },
            D::Count => {
                let c = s.d.st.cnt;
                for (i, v) in [c.frames, c.bytes, c.fe, c.pe, c.crc, c.ovr].iter().enumerate() {
                    if i > 0 {
                        out.put_bytes(b",");
                    }
                    out.put_u32(*v);
                }
            }
            D::Clear => {
                self.dec_clear();
                return false;
            }
            D::Frames(_) => return false,
        }
        if changed {
            self.dec_changed(now);
        }
        out.len > 0
    }

    /// DECode:FRAMes? [<n>]: a #4nnnn block of frame lines (decode.rs
    /// `scpi_line`), the host's unread frames oldest first, at most `max`
    /// and as many whole lines as fit; the host cursor moves past them.
    #[inline(never)]
    pub fn dec_frames(&mut self, max: u32, buf: &mut [u8]) -> usize {
        let s = ds();
        let st = &mut s.d.st;
        let lim = buf.len().min(10_000);
        let mut w = dc::W { b: &mut buf[6..lim], n: 0 };
        let mut k = 0;
        while st.host < st.next && k < max {
            let n0 = w.n;
            dc::scpi_line(&mut w, st, st.host);
            if w.n == w.b.len() {
                // Did not fit: the next reply.
                w.n = n0;
                break;
            }
            st.host += 1;
            k += 1;
        }
        let n = w.n;
        buf[0] = b'#';
        buf[1] = b'4';
        for (i, d) in [1000usize, 100, 10, 1].iter().enumerate() {
            buf[2 + i] = b'0' + ((n / d) % 10) as u8;
        }
        6 + n
    }
}
