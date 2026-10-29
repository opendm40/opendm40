//! The experimental 600 Mohm range, Dmm glue (src/ohm600m.rs has the row,
//! the phase plan and the maths): the knobs and the running cycle, the row
//! for `write_config`, the poll, DBG:OHMX. A child module of dmm (#[path]
//! in dmm.rs) for the Dmm fields. The range rules (manual only, never in
//! AUTO or the AUTO key's cycle) are in dmm.rs (`OHM_EXP`).

use super::{cal, Dmm, Reading, Val};
use crate::hy3131 as hy;
use crate::ohm600m::{self as xr, Cycle, Knobs, Req};
use crate::scpi::Scpi;

#[derive(Clone, Copy)]
struct St {
    /// `k` holds Knobs::DEF (all-zero .sram_bss until the first use).
    init: bool,
    k: Knobs,
    row: [u8; hy::CFG_LEN],
    cyc: Cycle,
    /// Row write (ms), two cycles agreed since, the last cycle's R (ohm,
    /// infinite = over range) and the window's.
    t0: u64,
    ok: bool,
    prev: f32,
    win: f32,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut ST: St = St {
    init: false,
    k: Knobs::DEF,
    row: [0; hy::CFG_LEN],
    cyc: Cycle::NEW,
    t0: 0,
    ok: false,
    prev: f32::NAN,
    win: f32::NAN,
};

fn st() -> &'static mut St {
    let s = unsafe { &mut *core::ptr::addr_of_mut!(ST) };
    if !s.init {
        s.init = true;
        s.k = Knobs::DEF;
    }
    s
}

/// `write_config` of ohm r7 at `now`: the row (the knobs' tap / OSR /
/// overrides, the plan's first phase); restarts the cycle and the vet.
pub fn row(now: u64) -> [u8; hy::CFG_LEN] {
    let s = st();
    s.row = xr::build_row(&s.k);
    s.cyc = Cycle::start(&s.k);
    s.t0 = now;
    s.ok = false;
    s.prev = f32::NAN;
    s.win = f32::NAN;
    s.row
}

/// The 6 Mohm calibration gain x the knob's gain x Rref (ohm per jack /
/// drop ratio). Starting point; the knob is to be calibrated (ohm-600m.md).
fn g(k: &Knobs) -> f32 {
    cal(0x124 + 4 * xr::BASE_ROW as u32, 1.0) * k.gain * k.rref
}

impl Dmm {
    /// `poll` on ohm r7: one phase step per AD1 conversion, a reading per
    /// complete cycle (the window's mean), provisional until two cycles
    /// agree and `settle_ms` has passed since the row write.
    #[inline(never)]
    pub(super) fn poll_ohm_exp(&mut self, now: u64) -> Option<Reading> {
        let code = self.ad1()?;
        // The converter runs: the idle watchdog sees it also while the
        // readings are provisional (the settle is long).
        self.wd_t = now;
        let s = st();
        let (r24, r28, done) = s.cyc.step(code, &s.row, &s.k)?;
        let r23 = s.row[3];
        hy::write(0x23, &[r23 & !0x80]);
        hy::write(0x24, &[r24]);
        hy::write(0x28, &[r28]);
        hy::write(0x23, &[r23]);
        if !done {
            return None;
        }
        let v = xr::volts(&s.cyc.done, s.k.plan, xr::ig_of(&s.row), s.k.offs);
        let g = g(&s.k);
        let ov = self.over();
        let x = xr::ohms(v.vx, v.drop, g);
        let x = if xr::is_ol(x, ov) { f32::INFINITY } else { x };
        let p = core::mem::replace(&mut s.prev, x);
        let agree = xr::agree(x, p, s.k.vet);
        s.ok |= agree;
        // A cycle that disagrees with the one before starts the window over.
        s.cyc.push(&v, s.k.navg, !agree);
        let (mx, md) = s.cyc.mean();
        let r = xr::ohms(mx, md, g);
        s.win = r;
        self.live = v.vx < -xr::LIVE_V;
        self.ohm_agreed = s.ok;
        let prov = !s.ok || now < s.t0 + s.k.settle_ms as u64;
        let ol = xr::is_ol(r, ov);
        let r = if self.ohm_neg { r } else { r.max(0.0) };
        let val = if ol { Val::ol(xr::DEC, "M\u{3a9}") } else { Val::new(r * 1e-6, xr::DEC, "M\u{3a9}") };
        let mut rd = self.reading(val, r.min(1e10));
        rd.prov = prov;
        self.prov_side = prov;
        Some(rd)
    }

    /// DBG:OHMX <req>: a knob; on r7 the row is written again (the cycle,
    /// the window and the settle restart). Err = the SCPI error.
    #[inline(never)]
    pub fn ohmx_req(&mut self, clk: &mut crate::clock::Clock, req: Req) -> Result<(), &'static str> {
        if !st().k.apply(req) {
            return Err("Illegal parameter value");
        }
        if self.mode == 10 && self.range == super::OHM_EXP && !self.pw.on {
            self.set(clk, 10, super::OHM_EXP, 150);
        }
        Ok(())
    }

    /// DBG:OHMX? reply:
    /// `tap,plan,skip,osr,navg,settle_ms,vet,gain,offs,rref,cal;`
    /// `on,cycles,ok,V_x,V_fb,V_com,V_drop,V_d,R_cycle,R_window;`
    /// `<5 AD1 codes X,F,C,Q,D>;<R20..R35 hex>;<overrides reg=val ...>`
    /// (volts of the last complete cycle; NaN where not in the plan).
    #[inline(never)]
    pub fn ohmx_text(&self, out: &mut Scpi) {
        let s = st();
        let k = &s.k;
        for (i, v) in [k.tap as u32, k.plan as u32, k.skip as u32, k.osr as u32, k.navg as u32, k.settle_ms as u32].into_iter().enumerate() {
            if i > 0 {
                out.put_bytes(b",");
            }
            out.put_u32(v);
        }
        for v in [k.vet, k.gain, k.offs, k.rref, cal(0x124 + 4 * xr::BASE_ROW as u32, 1.0)] {
            out.put_bytes(b",");
            out.put_f32(v);
        }
        let on = self.mode == 10 && self.range == super::OHM_EXP && !self.pw.on;
        out.put_bytes(if on { b";1," } else { b";0," });
        out.put_u32(s.cyc.n);
        out.put_bytes(if s.ok { b",1" } else { b",0" });
        let v = xr::volts(&s.cyc.done, k.plan, xr::ig_of(&s.row), k.offs);
        for x in [v.vx, v.vfb, v.vcom, v.drop, v.vd, s.prev, s.win] {
            out.put_bytes(b",");
            if s.cyc.n > 0 && x.is_finite() {
                out.put_f32(x);
            } else {
                out.put_bytes(crate::scpi::NAN);
            }
        }
        for (i, c) in s.cyc.done.iter().enumerate() {
            out.put_bytes(if i == 0 { b";" } else { b"," });
            if *c < 0 {
                out.put_bytes(b"-");
            }
            out.put_u32(c.unsigned_abs());
        }
        out.put_bytes(b";");
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for b in s.row {
            out.put_bytes(&[HEX[(b >> 4) as usize], HEX[(b & 15) as usize]]);
        }
        out.put_bytes(b";");
        for (reg, b) in k.ovr.into_iter().filter(|o| o.0 != 0) {
            out.put_bytes(&[HEX[(reg >> 4) as usize], HEX[(reg & 15) as usize], b'=', HEX[(b >> 4) as usize], HEX[(b & 15) as usize], b' ']);
        }
    }
}
