//! DBG:SRC Dmm glue (src/dbgsrc.rs has the tables and the maths): the one
//! running source row, its AD1 phase cycle, enter / leave, the SCPI reply.
//! A child module of dmm (#[path] in dmm.rs) for the Dmm fields.
//!
//! While active the Dmm runs the path's mode (10 ohm r1..r6, 12 diode) in
//! manual range, so Dmm::write_config switches the relay, PE0 / PE1
//! (both off, or the A-PROT latch's 10 mohm) and PC7 exactly as for that
//! stock row; the misconnect sentinel is off (mask 0, as during the fuse
//! check). Any other function / range (keys, touch, SCPI) ends it.

use super::{Dmm, Reading, FUNC_A_AUTO};
use crate::clock::Clock;
use crate::dbgsrc::{self as ds, Cycle, Req};
use crate::hy3131 as hy;
use crate::scpi::Scpi;

#[derive(Clone, Copy)]
struct St {
    on: bool,
    vref: u8,
    path: u8,
    sense: u8,
    row: [u8; hy::CFG_LEN],
    cyc: Cycle,
    /// The function to return to (Dmm::func) and its manual ohm range.
    prev: u8,
    prev_rng: Option<i8>,
}

// All-zero = off (.sram_bss is zeroed at reset).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut ST: St = St { on: false, vref: 0, path: 0, sense: 0, row: [0; hy::CFG_LEN], cyc: Cycle::NEW, prev: 0, prev_rng: None };

fn st() -> &'static mut St {
    unsafe { &mut *core::ptr::addr_of_mut!(ST) }
}

/// Whether DBG:SRC runs (cheap: every poll).
#[inline(always)]
pub fn active() -> bool {
    st().on
}

/// `write_config`: the source row when DBG:SRC runs on mode `m` range `r`;
/// any other row ends DBG:SRC. Restarts the phase cycle.
pub fn row(m: u8, r: usize) -> Option<[u8; hy::CFG_LEN]> {
    let s = st();
    if !s.on {
        return None;
    }
    let p = &ds::PATHS[s.path as usize];
    if p.mode != m || p.range as usize != r {
        s.on = false;
        return None;
    }
    s.cyc = Cycle::NEW;
    Some(s.row)
}

impl Dmm {
    /// DBG:SRC <vref>,<path>[,<sense>][,FORCE] / OFF. Err = the SCPI error.
    #[inline(never)]
    pub fn src_req(&mut self, clk: &mut Clock, req: Req) -> Result<(), &'static str> {
        let s = st();
        match req {
            Req::Off => {
                if s.on {
                    s.on = false;
                    let (prev, rng) = (s.prev, s.prev_rng);
                    self.select(clk, prev);
                    if let Some(r) = rng {
                        self.set_range(clk, Some(r as u8));
                    }
                }
                Ok(())
            }
            Req::On(v, p, sense, force) => {
                // Never from a current function / the power meter (the A
                // jack path), nor while the chip is not ours.
                let f = self.func();
                if self.pw.on || matches!(f, 4..=9 | 17 | FUNC_A_AUTO) || self.suspended || self.fuse_job.is_some() {
                    return Err("Settings conflict");
                }
                if !ds::allowed(v, p, force) {
                    return Err("Illegal parameter value");
                }
                if !s.on {
                    s.prev = f;
                    s.prev_rng = (f == 10 && self.manual).then_some(self.range);
                }
                let path = &ds::PATHS[p as usize];
                s.vref = v;
                s.path = p;
                s.sense = sense;
                s.row = ds::build_row(v, p, sense);
                s.on = true;
                self.acdc = false;
                self.cpl_auto = false;
                self.manual = true;
                // Force the write even on the same mode / range (a new tap).
                self.set(clk, path.mode, path.range as i8, 300);
                Ok(())
            }
        }
    }

    /// `poll` while DBG:SRC runs: one phase step per AD1 conversion, no
    /// reading (the watchdog's idle timer is fed here).
    #[inline(never)]
    pub(super) fn poll_src(&mut self, now: u64) -> Option<Reading> {
        let code = self.ad1()?;
        self.wd_t = now;
        let s = st();
        if let Some((r24, r28)) = s.cyc.step(code, &s.row, s.sense) {
            let r23 = s.row[3];
            hy::write(0x23, &[r23 & !0x80]);
            hy::write(0x24, &[r24]);
            hy::write(0x28, &[r28]);
            hy::write(0x23, &[r23]);
        }
        None
    }

    /// DBG:SRC? reply: `0` when off, else
    /// `1,<cycles>,<vref>,<path>,<sense>,<V_in>,<V_far>,<I>,<V_vref>,<V_drop>,<V_com>,<VDDA-AGND>,<AGND-VSS>;<10 AD1 codes>;<R20..R35 hex>`
    #[inline(never)]
    pub fn src_text(&self, out: &mut Scpi) {
        let s = st();
        if !s.on {
            out.put_bytes(b"0");
            return;
        }
        out.put_bytes(b"1,");
        out.put_u32(s.cyc.n);
        for v in [s.vref, s.path, s.sense] {
            out.put_bytes(b",");
            out.put_u32(v as u32);
        }
        let v = ds::compute(&s.cyc.done, ds::ig_of(&s.row), s.path, s.sense);
        for x in [v.v_in, v.v_far, v.i, v.v_vref, v.v_drop, v.v_com, v.vdda, v.agnd_vss] {
            out.put_bytes(b",");
            if s.cyc.n > 0 && x.is_finite() && x.abs() < 1e30 {
                out.put_f32(x);
            } else {
                out.put_bytes(crate::scpi::NAN);
            }
        }
        for (k, c) in s.cyc.done.iter().enumerate() {
            out.put_bytes(if k == 0 { b";" } else { b"," });
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
    }
}
