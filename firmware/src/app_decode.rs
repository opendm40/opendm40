//! The protocol decoder on the scope page (TODO "Protocol decoders"): each
//! frame the page draws (a running frame, a stopped record's zoom / pan) is
//! decoded over the screen's window of the record plus a character (UART)
//! or a frame (Modbus) on either side, and the result (ui_decode.rs `ANN`)
//! drawn as the bus band. Settings: MF > Settings > Decode (`Popup::Dec`)
//! or DECode:...; DECode:FRAMes? decodes the stopped record (running: the
//! screen's frames).
//!
//! No state but the settings and the bit rate found last (`DS`, a few
//! bytes) and one screen's annotations (`ANN`, < 1 KB, `.sram_bss`).

use super::*;
use crate::decode as dc;
use crate::scope_ui::dec as ud;

/// Threshold stepper, V.
const THR_STEP: f32 = 0.1;
/// Auto-baud looks at most this many samples.
const AUTO_MAX: usize = 65_536;

pub(super) struct DecState {
    pub cfg: dc::Cfg,
    /// Manual threshold, V at the probe tip (NaN: auto).
    pub thr_v: f32,
    /// The bit rate auto-baud found last (kept when a screen has too few
    /// edges to measure it).
    last_baud: u32,
    /// Changes with every decoded screen (Model::dec_gen; 0 = blank).
    gen: u16,
}

static mut DS: DecState = unsafe { core::mem::zeroed() };

pub(super) fn ds() -> &'static mut DecState {
    unsafe { &mut *core::ptr::addr_of_mut!(DS) }
}

/// Boot: the non-zero defaults.
pub(super) fn init() {
    let s = ds();
    s.cfg = dc::Cfg::DEFAULT;
    s.thr_v = f32::NAN;
    s.gen = 1;
}

/// Lines of DECode:FRAMes?: the reply buffer, frames before `first` and
/// those that do not fit left out.
struct Out<'a> {
    b: &'a mut [u8],
    n: usize,
    first: u32,
    full: bool,
    /// The line being built starts here (None: none open / skipped).
    open: Option<usize>,
    lines: u32,
}

impl Out<'_> {
    fn put(&mut self, s: &[u8]) {
        if self.full || self.open.is_none() {
            return;
        }
        if self.n + s.len() > self.b.len() {
            // The line does not fit: dropped, the reply ends before it.
            self.n = self.open.take().unwrap_or(self.n);
            self.full = true;
            return;
        }
        self.b[self.n..self.n + s.len()].copy_from_slice(s);
        self.n += s.len();
    }
    fn w(&mut self, f: impl FnOnce(&mut dc::W)) {
        let mut t = [0u8; 64];
        let mut w = dc::W { b: &mut t, n: 0 };
        f(&mut w);
        let n = w.n;
        self.put(&t[..n]);
    }
}

impl App {
    /// The band shown: 0 none, 1 UART, 2 Modbus.
    pub(super) fn dec_band(&self) -> u8 {
        match ds().cfg.proto {
            dc::P_UART => 1,
            dc::P_MODBUS => 2,
            _ => 0,
        }
    }

    /// Model::dec_gen: blank while a drag / pinch moves the trace.
    pub(super) fn dec_gen_shown(&self) -> u16 {
        if self.sp.frozen_or_pinch() { 0 } else { ds().gen }
    }

    /// Manual threshold as a code (the scope's code -> row map inverted).
    fn dec_thr_code(&self) -> Option<u16> {
        let v = ds().thr_v;
        if !v.is_finite() {
            return None;
        }
        let r0 = self.scope.row_of_code(0.0);
        let slope = (self.scope.row_of_code(1000.0) - r0) / 1000.0;
        let c = (self.scope.row_of_volts(v) - r0) / slope;
        Some(if c.is_finite() { c.clamp(0.0, 1023.0) as u16 } else { 512 })
    }

    /// The level in use, V (NaN: none).
    fn dec_level(&self) -> f32 {
        let a = ud::ann();
        if a.mid == 0 { f32::NAN } else { self.scope.volts(a.mid as f32) }
    }

    pub(super) fn dec_setup_vals(&self) -> ud::SetupVals {
        let s = ds();
        ud::SetupVals { proto: s.cfg.proto, fmt: s.cfg.fmt, inv: s.cfg.inverted, baud: s.cfg.baud, thr: s.thr_v, level: self.dec_level() }
    }

    /// The line parameters for `fs` over the window `a..b` (levels from its
    /// min / max) of `src`; Err: 0 no signal, 1 no bit rate, 2 bits too
    /// short for the sample rate.
    fn dec_line<S: crate::pack10::Src + ?Sized>(&self, src: &S, a: usize, b: usize, mm: (u16, u16), fs: u32) -> Result<(dc::Line, u32), u8> {
        let s = ds();
        let Some((lo, mid, hi)) = dc::levels(mm.0, mm.1, self.dec_thr_code()) else { return Err(0) };
        let baud = if s.cfg.baud != 0 {
            s.cfg.baud
        } else {
            match dc::auto_baud(src, a, b.min(a + AUTO_MAX), lo, hi, fs) {
                Ok(x) => {
                    s.last_baud = x;
                    x
                }
                Err(dc::NoRate::TooFast) => return Err(2),
                Err(_) if s.last_baud != 0 => s.last_baud,
                Err(_) => return Err(1),
            }
        };
        let (bits, parity) = s.cfg.bits();
        let spb = (fs as u64 * 256 / baud.max(1) as u64) as u32;
        Ok((dc::Line { lo, mid, hi, inv: s.cfg.inverted, spb, bits, parity }, baud))
    }

    /// The bar (zoomed out, too few samples per bit): the columns where the
    /// trace spans more than half the screen's swing.
    fn dec_bar(&mut self) {
        let a = ud::ann();
        a.mode = ud::M_BAR;
        let seg = &scope_page::cache_seg();
        let (mut lo, mut hi) = (i16::MAX, i16::MIN);
        for &(p, q) in seg.iter().filter(|s| s.0 <= s.1) {
            lo = lo.min(p);
            hi = hi.max(q);
        }
        let need = ((hi as i32 - lo as i32) / 2).max(3);
        for (i, &(p, q)) in seg.iter().enumerate() {
            if p <= q && (q as i32 - p as i32) >= need {
                a.bar[i / 8] |= 1 << (i % 8);
            }
        }
    }

    /// Decode the frame the page just built (scope_page `sp_frame`).
    #[inline(never)]
    pub(super) fn dec_frame(&mut self, f: &scope::Frame) {
        let s = ds();
        let a = ud::ann();
        if s.cfg.proto == dc::P_OFF {
            if a.mode != ud::M_NONE {
                a.clear();
            }
            return;
        }
        crate::prof::enter(crate::prof::X2);
        a.clear();
        s.gen = s.gen.wrapping_add(1).max(1);
        let n = f.data.len();
        a.n = n.max(1) as u32;
        a.fs = f.fs;
        if f.cols.is_some() || n < 2 {
            self.dec_bar();
            crate::prof::exit(crate::prof::X2);
            return;
        }
        let rec = self.scope.record();
        let (src, off) = match rec.offset_of(&f.data) {
            Some(o) => (rec, o),
            None => (f.data, 0),
        };
        let (_, t0, _) = self.scope.record_info();
        a.t0 = off as i64 - t0 as i64;
        let mm = f.data.min_max(0, n);
        match self.dec_line(&src, off, off + n, mm, f.fs) {
            Ok((l, baud)) => {
                a.mid = l.mid;
                a.baud = baud;
                let cs = l.char_samples();
                if l.spb < dc::MIN_SPB << 8 {
                    a.mode = ud::M_NOTE;
                    a.note_baud = baud;
                } else if (cs as i64 * 399 / n as i64) < ud::MIN_BOX as i64 {
                    self.dec_bar();
                } else {
                    self.dec_boxes(&src, off, n, &l, baud, f.fs);
                }
            }
            Err(2) => {
                a.mode = ud::M_NOTE;
                a.note_baud = 0;
            }
            Err(_) => {
                a.mid = 0;
                a.mode = ud::M_NONE;
            }
        }
        crate::prof::exit(crate::prof::X2);
    }

    fn dec_boxes(&mut self, src: &crate::pack10::Codes, off: usize, n: usize, l: &dc::Line, baud: u32, fs: u32) {
        let s = ds();
        let a = ud::ann();
        a.mode = ud::M_BOXES;
        let modbus = s.cfg.proto == dc::P_MODBUS;
        let cs = l.char_samples() as usize;
        // 16 characters (UART: back-to-back characters realign at an idle
        // gap) or 6 (Modbus: the gap before a frame) either side.
        let margin = cs * if modbus { 6 } else { 16 } + (l.spb >> 8) as usize * 2;
        let lo = off.saturating_sub(margin);
        let hi = (off + n + margin).min(src.len());
        let mut fr = dc::Framer::new(modbus, cs as u32, fs, baud, lo as u32, hi as u32);
        // The frame number of each frame box (the reply -> request link).
        let mut fno = [u32::MAX; ud::NF];
        let (vis0, vis1) = (off as i64, (off + n) as i64);
        let mut on_frame = |f: &dc::Fr, req: u32, nframes: u32| {
            let a = ud::ann();
            if (f.end as i64) < vis0 || f.start as i64 >= vis1 || a.nf as usize >= ud::NF {
                return;
            }
            let k = a.nf as usize;
            let r = fno[..k].iter().position(|&x| x == req).map_or(0xFF, |p| p as u8);
            fno[k] = nframes;
            a.f[k] = ud::FAnn { s0: (f.start as i64 - vis0) as i32, s1: (f.end as i64 - vis0) as i32, kind: f.kind, fl: f.flags, len: f.len, head: f.head, req: r };
            a.nf += 1;
        };
        dc::chars(src, lo, hi, l, &mut |c| {
            let a = ud::ann();
            if (c.end as i64) >= vis0 && (c.start as i64) < vis1 {
                if (a.nb as usize) < ud::NB {
                    a.b[a.nb as usize] = ud::BAnn { s0: (c.start as i64 - vis0) as i32, s1: (c.end as i64 - vis0) as i32, v: c.v as u8, fl: c.err };
                    a.nb += 1;
                } else {
                    a.more = true;
                }
            }
            if modbus {
                let nf = fr.nframes;
                fr.push(c, &mut |f, req| on_frame(f, req, nf));
            }
        });
        if modbus {
            let nf = fr.nframes;
            fr.finish(&mut |f, req| on_frame(f, req, nf));
        }
    }

    // ---- setup popup ----

    pub(super) fn dec_setup_tap(&mut self, h: ud::SetupHit, now: u64) {
        let s = ds();
        let old = s.cfg;
        match h {
            ud::SetupHit::Close => {
                self.sp_popup_close();
                return;
            }
            ud::SetupHit::Proto(p) => s.cfg.proto = p,
            ud::SetupHit::Fmt(f) => s.cfg.fmt = f,
            ud::SetupHit::Inv(i) => s.cfg.inverted = i,
            ud::SetupHit::Baud(up) => {
                let i = ud::BAUDS.iter().position(|&b| b == s.cfg.baud).unwrap_or(0);
                let j = if up { (i + 1).min(ud::BAUDS.len() - 1) } else { i.saturating_sub(1) };
                s.cfg.baud = ud::BAUDS[j];
            }
            ud::SetupHit::ThrAuto(auto) => {
                s.thr_v = if auto { f32::NAN } else { self.dec_manual_start() };
            }
            ud::SetupHit::Thr(up) => {
                let v = if s.thr_v.is_finite() { s.thr_v } else { self.dec_manual_start() };
                let q = v / THR_STEP;
                let q = if q >= 0.0 { (q + 0.5) as i32 } else { (q - 0.5) as i32 };
                s.thr_v = ((q + if up { 1 } else { -1 }) as f32 * THR_STEP).clamp(-100.0, 100.0);
            }
            _ => return,
        }
        if s.cfg.baud != old.baud || s.cfg.proto != old.proto {
            s.last_baud = 0;
        }
        self.dec_changed();
        self.sp_popup_draw();
        let _ = now;
    }

    /// A manual threshold starts at the level in use (1.5 V without one).
    fn dec_manual_start(&self) -> f32 {
        let v = self.dec_level();
        if v.is_finite() { v } else { 1.5 }
    }

    /// Settings changed: the band follows (stopped: the record is decoded
    /// again at once; running: with the next frame).
    fn dec_changed(&mut self) {
        let a = ud::ann();
        if self.dec_band() == 0 {
            a.clear();
        }
        ds().gen = ds().gen.wrapping_add(1).max(1);
        self.scope.refresh();
    }

    // ---- SCPI ----

    /// DECode:... except FRAMes?: true when a reply text is in `out`.
    #[inline(never)]
    pub fn dec_scpi(&mut self, _clk: &mut Clock, c: crate::scpi::Dcd, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Dcd as D;
        let s = ds();
        out.len = 0;
        let old = s.cfg;
        match c {
            D::Proto(None) => out.put_bytes(match s.cfg.proto {
                dc::P_UART => b"UART",
                dc::P_MODBUS => b"MODB",
                _ => b"OFF",
            }),
            D::Proto(Some(p)) => s.cfg.proto = p,
            D::Baud(None) => {
                if s.cfg.baud == 0 {
                    out.put_bytes(b"AUTO");
                } else {
                    out.put_u32(s.cfg.baud);
                }
            }
            D::Baud(Some(b)) => s.cfg.baud = b,
            D::Rate => out.put_u32(if self.dec_band() != 0 { ud::ann().baud } else { 0 }),
            D::Fmt(None) => out.put_bytes(dc::FMT_NAMES[s.cfg.fmt as usize % 4].as_bytes()),
            D::Fmt(Some(f)) => s.cfg.fmt = f,
            D::Inv(None) => out.put_bytes(if s.cfg.inverted { b"INV" } else { b"NORM" }),
            D::Inv(Some(i)) => s.cfg.inverted = i,
            D::Thr(None) => {
                if s.thr_v.is_finite() {
                    out.put_f32(s.thr_v);
                } else {
                    out.put_bytes(b"AUTO");
                }
            }
            D::Thr(Some(v)) => s.thr_v = v,
            D::Level => {
                let v = self.dec_level();
                if v.is_finite() {
                    out.put_f32(v)
                } else {
                    out.put_bytes(crate::scpi::NAN)
                }
            }
            D::Count => {
                let c = self.dec_count();
                for (i, v) in c.iter().enumerate() {
                    if i > 0 {
                        out.put_bytes(b",");
                    }
                    out.put_u32(*v);
                }
            }
            D::Frames(_) => return false,
        }
        if out.len == 0 {
            if s.cfg.baud != old.baud || s.cfg.proto != old.proto {
                s.last_baud = 0;
            }
            self.dec_changed();
            if self.sp.popup == crate::scope_ui::Popup::Dec && !self.redraw {
                self.sp_popup_draw();
            }
        }
        out.len > 0
    }

    /// DECode:COUNt?: frames, bytes, framing, parity, CRC errors of what
    /// FRAMes? reports.
    fn dec_count(&mut self) -> [u32; 5] {
        let mut c = [0u32; 5];
        let mut buf = [0u8; 0];
        self.dec_lines(0, &mut buf, &mut c);
        c
    }

    /// DECode:FRAMes? [<first>]: a #4nnnn block of frame lines (see
    /// `dec_lines`).
    #[inline(never)]
    pub fn dec_frames(&mut self, first: u32, buf: &mut [u8]) -> usize {
        let lim = buf.len().min(10_000);
        let mut c = [0u32; 5];
        let n = self.dec_lines(first, &mut buf[6..lim], &mut c);
        buf[0] = b'#';
        buf[1] = b'4';
        for (i, d) in [1000usize, 100, 10, 1].iter().enumerate() {
            buf[2 + i] = b'0' + ((n / d) % 10) as u8;
        }
        6 + n
    }

    /// The frame lines "seq,time_s,proto,hex,status,info\n" from frame
    /// `first` on, as many as fit `b` (its length used); `cnt` counts all
    /// of them (frames, bytes, FE, PE, CRC). Stopped: the whole record
    /// decoded; running: the screen's bytes / frames (`ANN`).
    fn dec_lines(&mut self, first: u32, b: &mut [u8], cnt: &mut [u32; 5]) -> usize {
        let s = ds();
        if s.cfg.proto == dc::P_OFF {
            return 0;
        }
        let modbus = s.cfg.proto == dc::P_MODBUS;
        let mut o = Out { b, n: 0, first, full: false, open: None, lines: 0 };
        let rec = self.scope.record();
        if self.scope.s.run || rec.len() < 2 {
            self.dec_lines_screen(&mut o, cnt, modbus);
            return o.n;
        }
        let (fs, t0, _) = self.scope.record_info();
        let len = rec.len();
        let mm = rec.min_max(0, len);
        let Ok((l, baud)) = self.dec_line(&rec, 0, len, mm, fs) else { return 0 };
        if l.spb < dc::MIN_SPB << 8 {
            return 0;
        }
        let cs = l.char_samples();
        let mut fr = dc::Framer::new(modbus, cs, fs, baud, 0, len as u32);
        let proto: &[u8] = if modbus { b",MB," } else { b",UART," };
        let close = |o: &mut Out, f: &dc::Fr, req: u32, cnt: &mut [u32; 5]| {
            cnt[0] += 1;
            cnt[4] += (f.flags & dc::F_CRC != 0) as u32;
            if o.open.is_some() {
                o.w(|w| {
                    w.c(b',').s(dc::status(f.flags)).c(b',');
                    if modbus {
                        dc::mb_text(w, f);
                        if req != u32::MAX {
                            w.s(" req=").u(req);
                        }
                    }
                    w.c(b'\n');
                });
                if o.open.take().is_some() {
                    o.lines += 1;
                }
            }
        };
        dc::chars(&rec, 0, len, &l, &mut |c| {
            cnt[1] += 1;
            cnt[2] += (c.err & dc::F_FE != 0) as u32;
            cnt[3] += (c.err & dc::F_PE != 0) as u32;
            fr.push(c, &mut |f, req| close(&mut o, f, req, cnt));
            if fr.open_len() == 1 && !o.full && fr.nframes >= o.first {
                // A new frame: its line.
                o.open = Some(o.n);
                let ns = (c.start as i64 - t0 as i64) * 1_000_000_000 / fs.max(1) as i64;
                let seq = fr.nframes;
                o.w(|w| {
                    w.u(seq).c(b',').secs_ns(ns);
                });
                o.put(proto);
            }
            let v = c.v as u8;
            o.w(|w| {
                w.hex(v);
            });
        });
        fr.finish(&mut |f, req| close(&mut o, f, req, cnt));
        o.n
    }

    /// Running: the screen's frames from `ANN` (Modbus: the frame boxes
    /// with the bytes on the screen; UART: the bytes, grouped by a gap of
    /// two characters).
    fn dec_lines_screen(&self, o: &mut Out, cnt: &mut [u32; 5], modbus: bool) {
        let a = ud::ann();
        if a.mode != ud::M_BOXES {
            return;
        }
        let fs = a.fs.max(1) as i64;
        let cs = if a.baud != 0 { (a.fs as u64 * ds().cfg.char_bits() as u64 / a.baud as u64) as i32 } else { 0 };
        let time = |s0: i32| (a.t0 + s0 as i64) * 1_000_000_000 / fs;
        let line = |o: &mut Out, seq: u32, s0: i32, s1: i32, fl: u8, f: Option<&ud::FAnn>, cnt: &mut [u32; 5]| {
            cnt[0] += 1;
            if seq < o.first || o.full {
                return;
            }
            o.open = Some(o.n);
            let ns = time(s0);
            o.w(|w| {
                w.u(seq).c(b',').secs_ns(ns);
            });
            o.put(if modbus { b",MB," } else { b",UART," });
            for b in a.b[..a.nb as usize].iter().filter(|b| b.s0 >= s0 && b.s1 <= s1) {
                let v = b.v;
                o.w(|w| {
                    w.hex(v);
                });
            }
            o.w(|w| {
                w.c(b',').s(dc::status(fl)).c(b',');
                if let Some(f) = f {
                    let fr = dc::Fr { start: 0, end: 0, len: f.len, head: f.head, flags: f.fl, kind: f.kind };
                    dc::mb_text(w, &fr);
                    if f.req != 0xFF {
                        w.s(" req=").u(f.req as u32);
                    }
                }
                w.c(b'\n');
            });
            if o.open.take().is_some() {
                o.lines += 1;
            }
        };
        for b in &a.b[..a.nb as usize] {
            cnt[1] += 1;
            cnt[2] += (b.fl & dc::F_FE != 0) as u32;
            cnt[3] += (b.fl & dc::F_PE != 0) as u32;
        }
        if modbus {
            for (k, f) in a.f[..a.nf as usize].iter().enumerate() {
                cnt[4] += (f.fl & dc::F_CRC != 0) as u32;
                line(o, k as u32, f.s0, f.s1, f.fl, Some(f), cnt);
            }
            return;
        }
        let mut k = 0;
        let mut seq = 0;
        while k < a.nb as usize {
            let (s0, mut s1, mut fl) = (a.b[k].s0, a.b[k].s1, a.b[k].fl);
            let mut j = k + 1;
            while j < a.nb as usize && j - k < 16 && a.b[j].s0 - s1 <= 2 * cs {
                s1 = a.b[j].s1;
                fl |= a.b[j].fl;
                j += 1;
            }
            line(o, seq, s0, s1, fl, None, cnt);
            seq += 1;
            k = j;
        }
    }
}
