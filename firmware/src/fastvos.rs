//! FR-05 FAST in DC V / mV / A: AD1 unchopped with the converter offset
//! taken out in firmware (docs/dmm-rate-filter-survey.md section 8.4).
//!
//! FAST runs AD1 at OSR 16384 with AD1CHOP 00 (R22 0x06): one conversion
//! per 100 ms, a 100 ms sinc^2 window, so 50 and 60 Hz are nulled as at MED
//! (0x16, the same window chopped: 200 ms). Unchopped, each conversion
//! carries the converter offset: AD1CHOP 00 gives VX + VOS, 01 gives VX -
//! VOS (DS-HY3131 13.1). VOS is ~22.24 mV on the DC V 6 V row (13 270
//! codes), and it is taken out here:
//!   * per path (mode, range) a VOS kept for the session (`vos`); before
//!     the first measurement of a path the device's figure for it
//!     (VOS_DEFAULT: one unit; another unit's may differ by a few counts
//!     for those first 2 s), so the first FAST reading is not delayed;
//!   * the first refresh FIRST_MS after the row write, then every
//!     REFRESH_MS: NA clean 00 conversions, R22 -> 01 (R22_FAST_B), the
//!     next conversion dropped (mixed), NB clean 01 ones, R22 -> 00, the
//!     next dropped, NA clean 00 ones (NA = NB = 4: 1.4 s; one refresh's
//!     VOS has ~0.3 x the noise of one conversion, ~6 codes = 0.1 count on
//!     the device's 6 V row). VOS = (mean 00 - mean 01) / 2, the
//!     00 means before and after the 01 ones averaged, so a linear drift of
//!     the input cancels. Used only if the 00 means before and after agree
//!     within `thr` (THR, ~2 counts) plus 3 sd of their difference (the sd
//!     from the groups' own scatter, so a noisy steady source passes) and
//!     that scatter stays below SIG_MAX: a moving input or AC content (device: a 5 Hz square
//!     moved an ungated estimate by ~350 mV) is not taken for an offset; a
//!     rejected refresh is tried again RETRY_MS later. A later one moves VOS
//!     by ALPHA of the difference. The 01 conversions are readings too
//!     (01 + VOS): a refresh costs the two dropped conversions.
//!   * no unchopped gain correction (GAIN 1: FAST and MED agree within
//!     +5..+8 ppm at 5 V on 6 V and 24 V on 60 V on the device).
//!
//! Device facts this rests on (exp/ad1-chopseq, 2026-10-25,
//! dmm-rate-filter-survey.md 8.4): an AD1CHOP write does not restart AD1
//! (the 100 ms grid goes on); the switch lands inside the running window of
//! the next conversion with a ~+-5 ms spread, so that conversion reads a
//! mix of both polarities (sd ~1.1 mV: dropped) and the one after it is
//! clean; VOS moved by +-25 uV over 30 min and ~20-40 uV from one day to
//! the next at room temperature (the temperature coefficient is not
//! measured: the refresh covers it). The first refresh is not earlier than
//! FIRST_MS after the row write: an R22 change within ~1 s of a row write
//! spoiled two conversions on the device (dmm.rs R22_DROP; seen for OSR
//! changes, not tried for AD1CHOP).

/// R22 of FAST: OSR 16384, AD1CHOP 00 (in the row, dmm.rs `write_config`).
pub const R22_FAST: u8 = 0x06;
/// The same with AD1CHOP 01 (the refresh's other polarity).
pub const R22_FAST_B: u8 = 0x0E;
/// The unchopped gain correction (code x GAIN): none. Device 1184b096
/// (2026-10-25), FAST against MED alternated in 8 s blocks: -87 ppm at 5 V
/// on 6 V and -90 ppm at 24 V on 60 V with a -95 ppm correction, i.e. +8
/// / +5 ppm without; the "+95 ppm" of the chopseq run was one 1 V point.
pub const GAIN: f32 = 1.0;
/// VOS (codes) of each path before it is measured in the session (`path`
/// order), device 1184b096 at 0 V: DC V 6 V 13 220..13 250 (OP1O - RLU),
/// 60 / 600 / 1000 V 12 573..12 607 (SENSE - PB5), 600 mV 13 252 (60 mV not
/// measured: its refresh was rejected, the 600 mV figure), A 6 A 12 867,
/// 10 A 12 607, mA 12 598 / 12 604, uA 12 592 / 12 612 (one refresh each).
const VOS_DEFAULT: [f32; PATHS] = [
    13_235.0, 12_605.0, 12_575.0, 12_600.0, 13_250.0, 13_250.0, 12_865.0, 12_605.0, 12_600.0, 12_605.0, 12_590.0, 12_610.0,
];
/// First refresh after a row write, ms (dmm.rs R22_DROP_WIN_MS).
pub const FIRST_MS: u64 = 2000;
/// Refresh interval, and the retry after a rejected refresh, ms.
pub const REFRESH_MS: u64 = 30_000;
pub const RETRY_MS: u64 = 5_000;
/// Input-moving threshold of the refresh (codes, any DC path: the AD1
/// noise is ~21 codes per FAST conversion on the device, 6 V row), on top
/// of 3 sd of the difference of the two 00 means (the sd from the groups'
/// own scatter: a calibrator's 1 V output, ~80 codes sd, failed a fixed
/// 120-code spread on 6 of 7 refreshes on the device).
pub const THR: i32 = 120;
/// Largest pooled sd within the refresh's groups (codes, ~0.5 mV on 6 V):
/// more is AC content or a moving input, not noise (`conv`).
const SIG_MAX: f32 = 300.0;
/// Clean 00 conversions on each side of the 01 ones, clean 01 conversions.
const NA: usize = 4;
const NB: usize = 4;
/// Weight of a new VOS measurement on a path measured before.
const ALPHA: f32 = 0.3;
/// Paths: DC V r0..r3, DC mV r0..r1, DC A / mA / uA r0..r1 (`path`).
const PATHS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    /// 00, collecting the conversions before a refresh.
    Idle,
    /// 01 (after the dropped mixed one).
    B,
    /// 00 again (after the dropped mixed one), collecting the ones after.
    After,
}

/// One conversion's result (`Fast::conv`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Out {
    /// Mixed by a polarity switch: no reading.
    Drop,
    /// The corrected code (VX in chopped codes).
    Code(f32),
}

pub struct Fast {
    vos: [f32; PATHS],
    /// When each path's VOS was last measured (ms; 0 = never).
    t_vos: [u64; PATHS],
    key: usize,
    /// The chip's AD1CHOP now: false = 00, true = 01.
    b: bool,
    drop: u8,
    st: St,
    /// Clean 00 codes of the current phase (Idle: the last NA, a ring).
    a: [i32; NA],
    na: usize,
    a_before: f32,
    a_var: f32,
    bv: [i32; NB],
    nb: usize,
    due: u64,
    /// Refreshes used / rejected this session (DBG).
    pub used: u32,
    pub rejected: u32,
}

impl Fast {
    pub const fn new() -> Self {
        Fast {
            vos: [0.0; PATHS],
            t_vos: [0; PATHS],
            key: 0,
            b: false,
            drop: 0,
            st: St::Idle,
            a: [0; NA],
            na: 0,
            a_before: 0.0,
            a_var: 0.0,
            bv: [0; NB],
            nb: 0,
            due: 0,
            used: 0,
            rejected: 0,
        }
    }

    /// The path of a DC function's range (None: not a FAST unchopped one).
    pub fn path(mode: u8, range: i8) -> Option<usize> {
        let r = range.max(0) as usize;
        match mode {
            1 => Some(r.min(3)),
            3 => Some(4 + r.min(1)),
            7..=9 => Some(6 + 2 * (mode as usize - 7) + r.min(1)),
            _ => None,
        }
    }

    /// R22 was just written with R22_FAST (a row, or the rate switched to
    /// FAST) at `now` on path `key`: polarity 00, the refresh rescheduled.
    pub fn start(&mut self, key: usize, now: u64) {
        self.key = key.min(PATHS - 1);
        self.b = false;
        self.drop = 0;
        self.st = St::Idle;
        self.na = 0;
        self.nb = 0;
        let t = self.t_vos[self.key];
        self.due = if t == 0 { now + FIRST_MS } else { (t + REFRESH_MS).max(now + FIRST_MS) };
    }

    /// VOS in use on the current path (codes).
    pub fn vos(&self) -> f32 {
        if self.t_vos[self.key] == 0 { VOS_DEFAULT[self.key] } else { self.vos[self.key] }
    }

    /// One unchopped conversion `raw` at `now`; `thr` = the input-moving
    /// threshold in codes. Returns the reading's code (or Drop) and the R22
    /// to write right after this conversion (a polarity switch), if any.
    pub fn conv(&mut self, raw: i32, now: u64, thr: i32) -> (Out, Option<u8>) {
        if self.drop > 0 {
            self.drop -= 1;
            return (Out::Drop, None);
        }
        let s = if self.b { -1.0 } else { 1.0 };
        let out = Out::Code((raw as f32 - s * self.vos()) * GAIN);
        match self.st {
            St::Idle => {
                self.a[self.na % NA] = raw;
                self.na += 1;
                if now >= self.due && self.na >= NA && var(&self.a) <= SIG_MAX * SIG_MAX {
                    self.a_before = mean(&self.a);
                    self.a_var = var(&self.a);
                    self.st = St::B;
                    self.nb = 0;
                    self.b = true;
                    self.drop = 1;
                    return (out, Some(R22_FAST_B));
                }
            }
            St::B => {
                self.bv[self.nb] = raw;
                self.nb += 1;
                if self.nb == NB {
                    self.st = St::After;
                    self.na = 0;
                    self.b = false;
                    self.drop = 1;
                    return (out, Some(R22_FAST));
                }
            }
            St::After => {
                self.a[self.na] = raw;
                self.na += 1;
                if self.na == NA {
                    let after = mean(&self.a);
                    // Pooled variance of the three groups (the "before" one
                    // was within SIG_MAX to start), and the 00 means' shift
                    // against THR + 3 sd of that shift.
                    let v = (self.a_var + var(&self.a) + var(&self.bv)) / 3.0;
                    let d = (after - self.a_before).abs() - thr as f32;
                    let ok = v <= SIG_MAX * SIG_MAX && (d <= 0.0 || d * d <= 9.0 * v * 2.0 / NA as f32);
                    if ok {
                        let x = ((self.a_before + after) / 2.0 - mean(&self.bv)) / 2.0;
                        let k = self.key;
                        self.vos[k] = if self.t_vos[k] == 0 { x } else { self.vos[k] + ALPHA * (x - self.vos[k]) };
                        self.t_vos[k] = now.max(1);
                        self.used += 1;
                    } else {
                        self.rejected += 1;
                    }
                    self.due = now + if ok { REFRESH_MS } else { RETRY_MS };
                    self.st = St::Idle;
                    // These are the newest clean 00 conversions: the next
                    // refresh's "before" may start from them.
                }
            }
        }
        (out, None)
    }
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut FAST: core::mem::MaybeUninit<Fast> = core::mem::MaybeUninit::uninit();

/// Initialise and hand out the one `Fast` (call once: `Dmm::new`). In SRAM,
/// not in the Dmm: TCM .bss shortens the main stack (stackdepth check).
pub fn fast_static() -> &'static mut Fast {
    unsafe {
        (*core::ptr::addr_of_mut!(FAST)).write(Fast::new());
        (*core::ptr::addr_of_mut!(FAST)).assume_init_mut()
    }
}

fn mean<const N: usize>(v: &[i32; N]) -> f32 {
    v.iter().map(|&x| x as f32).sum::<f32>() / N as f32
}

/// Sample variance (N - 1).
fn var<const N: usize>(v: &[i32; N]) -> f32 {
    let m = mean(v);
    v.iter().map(|&x| (x as f32 - m) * (x as f32 - m)).sum::<f32>() / (N - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chip model: VOS by polarity, a conversion right after a switch
    /// mixed 50 / 50 (the device), the input `vx(t)`.
    fn run(f: &mut Fast, n: usize, vos: f32, vx: &dyn Fn(u64) -> f32) -> std::vec::Vec<(u64, Out)> {
        let mut out = std::vec::Vec::new();
        let mut b = false;
        let mut mixed = false;
        for i in 0..n {
            let t = 100 * i as u64 + 5000;
            let s = if mixed { 0.0 } else if b { -1.0 } else { 1.0 };
            mixed = false;
            let raw = (vx(t) / GAIN + s * vos).round() as i32;
            let (o, w) = f.conv(raw, t, 120);
            if let Some(r) = w {
                b = r == R22_FAST_B;
                mixed = true;
            }
            out.push((t, o));
        }
        out
    }

    #[test]
    fn paths() {
        assert_eq!(Fast::path(1, 0), Some(0));
        assert_eq!(Fast::path(1, 3), Some(3));
        assert_eq!(Fast::path(3, 1), Some(5));
        assert_eq!(Fast::path(7, 0), Some(6));
        assert_eq!(Fast::path(9, 1), Some(11));
        assert_eq!(Fast::path(10, 0), None);
    }

    #[test]
    fn first_refresh_measures_vos_and_corrects() {
        let mut f = Fast::new();
        f.start(1, 5000);
        let o = run(&mut f, 400, 15_000.0, &|_| 500_000.0);
        // Before the first refresh (2 s): the default VOS.
        if let (_, Out::Code(c)) = o[1] {
            assert!((c - (500_000.0 + (15_000.0 - VOS_DEFAULT[1]) * GAIN)).abs() < 2.0, "{c}");
        }
        assert_eq!(f.used, 2, "first refresh at 2 s, the next 30 s later");
        assert!((f.vos() - 15_000.0).abs() < 1.0, "{}", f.vos());
        // After it every reading is the input; two drops per refresh.
        let late: std::vec::Vec<_> = o.iter().filter(|(t, _)| *t > 8000).collect();
        let drops = late.iter().filter(|(_, x)| *x == Out::Drop).count();
        assert_eq!(drops, 2);
        for (_, x) in late {
            if let Out::Code(c) = x {
                assert!((c - 500_000.0).abs() < 1.5, "{c}");
            }
        }
    }

    #[test]
    fn moving_input_is_not_an_offset() {
        let mut f = Fast::new();
        f.start(0, 5000);
        // A square of +-1000 codes at 2.5 Hz (period 4 conversions).
        let _ = run(&mut f, 300, 13_000.0, &|t| if (t / 200) % 2 == 0 { 1000.0 } else { -1000.0 });
        // Never NA steady 00 conversions in a row: no refresh is even tried.
        assert_eq!(f.used, 0);
        assert_eq!(f.vos(), VOS_DEFAULT[0]);
        // A square too slow for that (period 20 conversions): tried, the
        // 00 means before / after or the 01 ones disagree on every edge.
        let mut h = Fast::new();
        h.start(0, 5000);
        let _ = run(&mut h, 600, 13_000.0, &|t| if (t / 1000) % 2 == 0 { 1000.0 } else { -1000.0 });
        assert!(h.rejected > 0, "rejected {}", h.rejected);
        assert!((h.vos() - 13_000.0).abs() < 1.0 || h.vos() == VOS_DEFAULT[0], "{}", h.vos());
        // Back to a steady input: the next try is taken.
        let mut g = Fast::new();
        g.start(0, 5000);
        let _ = run(&mut g, 100, 13_000.0, &|t| t as f32 * 0.1); // a slow ramp: 10 codes per conversion
        assert!(g.used >= 1);
        assert!((g.vos() - 13_000.0).abs() < 1.0, "a linear ramp cancels: {}", g.vos());
    }

    #[test]
    fn known_path_waits_for_its_interval() {
        let mut f = Fast::new();
        f.start(2, 5000);
        let _ = run(&mut f, 40, 14_000.0, &|_| 0.0);
        assert_eq!(f.used, 1);
        // Another path has its own default until it is measured.
        f.start(3, 9000);
        assert_eq!(f.vos(), VOS_DEFAULT[3]);
        // Back on path 2 soon: no refresh before its 30 s are up.
        f.start(2, 9500);
        assert!(f.due >= f.t_vos[2] + REFRESH_MS);
    }
}
