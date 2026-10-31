//! DMM Math (math.rs): the Math page (View::Math, from the V / A / ohm
//! range pages), its keypad targets and the SCPI CALCulate:FUNCtion /
//! :DBM:REFerence / :SCALe:* commands. A child of app.rs; page, tap and
//! command code only, linked into .cold (link-native.x): the per-reading
//! part is math::apply (region B).

use super::*;
use crate::math;

/// Keypad targets (after the temperature page's 6..8).
pub(super) const KP_MATH_M: u8 = 9;
pub(super) const KP_MATH_B: u8 = 10;

/// The dBm reference values as shown (math::REFS).
const REF_TXT: [&str; 3] = ["50\u{3a9}", "75\u{3a9}", "600\u{3a9}"];

/// m / b as typed back on the page and the keypad prefill: up to 7
/// characters, trailing zeros trimmed ("1000", "0.001", "-0.5").
pub(super) fn math_num(t: &mut [u8; 16], v: f32) -> &str {
    let a = v.abs();
    let dec = if a == 0.0 || a >= 1e4 {
        0
    } else if a >= 100.0 {
        2
    } else if a >= 0.1 {
        4
    } else {
        6
    };
    trim_zeros(crate::dmm::format(t, v, dec))
}

impl App {
    /// A math setting changed (page, keypad, SCPI): the DMM readout (and
    /// the range page's Math button, the Math page) is repainted (its unit caches compare by content, and math::unit's
    /// text changes in place); the settings save follows (persist_words).
    pub(super) fn math_changed(&mut self) {
        if matches!(self.view, View::Dmm | View::DmmScope | View::Range | View::Math) {
            self.redraw = true;
        }
    }

    /// View::Math.
    #[inline(never)]
    pub(super) fn math_page(&self) {
        let f = math::func();
        let (mut mb, mut bb) = ([0u8; 16], [0u8; 16]);
        let u = math::unit();
        let vals = [
            math::NAMES[f as usize],
            REF_TXT[math::ref_idx()],
            math_num(&mut mb, math::m()),
            math_num(&mut bb, math::b()),
            if u.is_empty() { "none" } else { kp_unit_text(u) },
        ];
        let formula = match f {
            math::DBM => "dBm = 10 log(V^2 / R / 1 mW)",
            math::DBV => "dBV = 20 log(V / 1 V)",
            math::SCAL => "m x X + b, X in V, A or ohm",
            _ => "dB of a voltage or m x X + b",
        };
        let note = if f != math::OFF && !math::applies(f, self.dmm.func_shown()) {
            "dBm / dBV: V functions only"
        } else {
            "Stats, limits, recording: raw value"
        };
        ui::math_page(vals, [formula, note]);
    }

    /// A tap on View::Math.
    #[inline(never)]
    pub(super) fn math_tap(&mut self, x: u16, y: u16, clk: &mut Clock) {
        if ui::title_back_hit(y) {
            self.show(View::Range, clk);
            return;
        }
        let d = ui::record_step(x) as i16;
        let step = |v: usize, n: usize| (v as i16 + d).clamp(0, n as i16 - 1) as usize;
        match ui::list_hit(y, ui::MATH_ROWS.len()) {
            Some(0) => {
                // Off and the functions of this DMM function (no dB in A /
                // ohm); a function set for another one steps from Off.
                let mode = self.dmm.func_shown();
                let mut fs = [math::OFF; 4];
                let mut n = 0;
                for f in [math::OFF, math::DBM, math::DBV, math::SCAL] {
                    if f == math::OFF || math::applies(f, mode) {
                        fs[n] = f;
                        n += 1;
                    }
                }
                let i = fs[..n].iter().position(|&f| f == math::func()).unwrap_or(0);
                math::set_func(fs[step(i, n)]);
            }
            Some(1) => math::set_ref_idx(step(math::ref_idx(), math::REFS.len())),
            Some(k @ (2 | 3)) => {
                self.kp_open(if k == 2 { KP_MATH_M } else { KP_MATH_B }, clk);
                return;
            }
            Some(4) => {
                // A unit typed over SCPI (not in the list) steps from "none".
                let i = math::UNITS.iter().position(|&u| u == math::unit()).unwrap_or(0);
                math::set_unit(math::UNITS[step(i, math::UNITS.len())].as_bytes());
            }
            _ => return,
        }
        self.redraw = true;
    }

    /// Keypad OK on KP_MATH_M / KP_MATH_B (m = 0 is refused: the value
    /// stays).
    pub(super) fn math_kp_apply(&mut self, v: f32) {
        if self.kp_target == KP_MATH_M {
            math::set_m(v);
        } else {
            math::set_b(v);
        }
        self.math_changed();
    }

    /// CALCulate:FUNCtion, :DBM:REFerence, :SCALe:GAIN / :OFFSet / :UNIT
    /// (scpi.rs Cmd::CalcFunc ..). Returns true with a reply in `out`.
    #[inline(never)]
    pub(super) fn math_scpi(&mut self, c: crate::scpi::Cmd, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Cmd as C;
        let ok = match c {
            C::CalcFunc(None) => {
                out.put_bytes([b"NONE" as &[u8], b"DBM", b"DBV", b"SCAL"][math::func() as usize]);
                return true;
            }
            C::CalcFunc(Some(f)) => {
                math::set_func(f);
                true
            }
            C::CalcDbmRef(None) => {
                out.put_f32(math::REFS[math::ref_idx()] as f32);
                return true;
            }
            C::CalcDbmRef(Some(v)) => match math::REFS.iter().position(|&r| r as f32 == v) {
                Some(i) => {
                    math::set_ref_idx(i);
                    true
                }
                None => false,
            },
            C::CalcScal(k, None) => {
                out.put_f32(if k == 0 { math::m() } else { math::b() });
                return true;
            }
            C::CalcScal(0, Some(v)) => math::set_m(v),
            C::CalcScal(_, Some(v)) => math::set_b(v),
            C::CalcScalUnit(None) => {
                out.put_bytes(b"\"");
                out.put_bytes(math::unit().as_bytes());
                out.put_bytes(b"\"");
                return true;
            }
            C::CalcScalUnit(Some(u)) => math::set_unit_word(u),
            _ => return false,
        };
        if ok {
            self.math_changed();
        } else {
            out.set_err("Illegal parameter value");
        }
        false
    }
}
