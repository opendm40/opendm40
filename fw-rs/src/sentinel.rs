//! Wrong-connection detection ("misconnect sentinel"),
//! docs/power-meter-and-misconnect.md section 4.
//!
//! In every non-current mode the A jack rests on the 47.5 ohm uA shunt
//! (PE0 = PE1 = 0, always in circuit), so a current in the A jack - the
//! leads in A/COM while the meter measures V / ohm / temperature, or a
//! source across A-COM - shows as a voltage on the shunt node PB1. Each mode
//! watches it with a block of the HY3131 that its stock row leaves idle:
//!
//! | modes | sentinel | row override (`sentinel_row`) |
//! |---|---|---|
//! | DC V 1, DC mV 3, ohm 10, temperature 15 (TC and the resistive sensors) | comparator on PB1 against the rows' own R21 = 0xdd window (+-20.8 mV = 0.44 mA on 47.5 ohm); CTSTA CMPHO / CMPLO sampled on every poll | R20 = 0x90 (SCMPI = PB1, ENCMP; no counter, no CMP pin) |
//! | AC V 0, AC mV 2 | AD1 (off in these rows) on PB1 - PB3, IG 3.6, OSR 256 unchopped (1.56 ms), peak hold of AD1<23:5>; PKHMAX / PKHMIN come with poll_ac's register read, ENPKH is toggled to restart the hold | R22 = 0x00, R23 \|= ENAD1, R24 = 0x95, R25 AD1IG = 11, R29 ENPKH + PKHSEL = 01 |
//! | DC A 7..9 ("V-JACK?": voltage on V/ohm while measuring current) | comparator on SENSE with the /1000 divider tap (SS5 only: the A rows run the power bus as SGND, PS5 would ground the tap): trips from ~21 V | R20 = 0x10 (SCMPI = SENSE, ENCMP), R2C \|= 0x10 (SS5) |
//!
//! Nothing else in the rows changes (host tests: byte diff against the
//! stock rows). AC+DC is left alone: `freq_secondary` rewrites R20 every
//! 350 ms in both phases. Diode / continuity / capacitance / frequency / AC A
//! keep the stock rows (their comparator / converters are busy).
//! "LIVE?" (a source on the circuit in ohm / continuity / diode) needs no
//! hardware: a negative ohm / continuity result or a negative diode voltage
//! is something only an external source produces (a positive one cannot be
//! told from "open": section 4.3).
//!
//! Never switches the shunt MOSFETs (section 4.3: a voltage source on A-COM
//! would then see 10 mohm instead of 48 ohm); it only warns.

use crate::hy3131::CFG_LEN;

/// Alert enable mask bits (SYST:ALER:ENAB, Settings "Jack alerts").
pub const M_AJACK: u8 = 1;
pub const M_VJACK: u8 = 2;
pub const M_LIVE: u8 = 4;
pub const M_ALL: u8 = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alert {
    None,
    /// The uA / mA shunt overloaded: the meter moved to the 10 A shunt
    /// (`dmm.rs` OVERLOAD latch, doc 4.3); red, not maskable.
    Overload,
    /// Current in the A jack while not in a current mode (red).
    AJack,
    /// Voltage on V/ohm, no current, in a current mode (amber).
    VJack,
    /// A source on the circuit in ohm / continuity / diode (amber).
    Live,
}

impl Alert {
    /// SYST:ALER? name.
    pub fn name(self) -> &'static str {
        match self {
            Alert::None => "NONE",
            Alert::Overload => "OVERLOAD",
            Alert::AJack => "AJACK",
            Alert::VJack => "VJACK",
            Alert::Live => "LIVE",
        }
    }
    /// Badge text (DMM status slot, combined-view strip).
    pub fn badge(self) -> &'static str {
        match self {
            Alert::None => "",
            // "OVERLOAD" does not fit the 83 px slot in TEXT22.
            Alert::Overload => "OVLD",
            Alert::AJack => "A-JACK",
            Alert::VJack => "V-JACK?",
            Alert::Live => "LIVE?",
        }
    }
    /// Combined-view strip badge (its pill holds 6 characters).
    pub fn strip_badge(self) -> &'static str {
        match self {
            Alert::VJack => "V-JACK",
            Alert::Live => "LIVE",
            a => a.badge(),
        }
    }
    /// Badge colour: red for A-JACK / OVERLOAD, the stock amber for the
    /// hints.
    pub fn color(self) -> u16 {
        match self {
            Alert::AJack | Alert::Overload => 0xF800,
            _ => 0xFC40,
        }
    }
    /// A red (safety) alert: white on red in the status slot, alternates
    /// with HOLD (decision #5), STAT:QUES bit 9.
    pub fn red(self) -> bool {
        matches!(self, Alert::AJack | Alert::Overload)
    }
    /// Toast shown once per assertion.
    pub fn toast(self) -> &'static str {
        match self {
            Alert::None => "",
            Alert::Overload => "Overload: 10 A shunt selected",
            Alert::AJack => "Current in A jack: check leads",
            Alert::VJack => "Voltage on V jack, no current",
            Alert::Live => "Voltage on circuit - power off",
        }
    }
}

/// STATus:QUEStionable bits of an alert (doc 4.4): bit 9 (SCPI-99: free
/// for the instrument) while a red alert (A-JACK, OVERLOAD) is on, plus bit
/// 1 (SCPI CURRent / the 34401A "current overload") for OVERLOAD.
pub fn ques_bits(a: Alert) -> u16 {
    (if a.red() { 1 << 9 } else { 0 }) | (if a == Alert::Overload { 1 << 1 } else { 0 })
}

/// STATus:QUEStionable:EVENt: the bits seen since the last read / *CLS.
pub static QUES_EVENT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Which sentinel the current row carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Off,
    /// Comparator on PB1 (A-JACK).
    CmpA,
    /// AD1 peak hold on PB1 - PB3 (A-JACK).
    PeakA,
    /// Comparator on SENSE via the /1000 tap (V-JACK?).
    CmpV,
}

impl Kind {
    /// What the kind detects: 0 nothing, 1 A-JACK, 2 V-JACK?.
    pub fn family(self) -> u8 {
        match self {
            Kind::Off => 0,
            Kind::CmpA | Kind::PeakA => 1,
            Kind::CmpV => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Off => "OFF",
            Kind::CmpA => "CMP_PB1",
            Kind::PeakA => "AD1PK_PB1",
            Kind::CmpV => "CMP_SENSE",
        }
    }
}

/// R20 with SCMPI = PB1 (100), ENCMP.
pub const R20_PB1: u8 = 0x90;
/// R20 with SCMPI = SENSE (000), ENCMP.
pub const R20_SENSE: u8 = 0x10;
/// R2C bit 4: SS5, the /1000 tap to SENSE.
pub const R2C_SS5: u8 = 0x10;
/// AD1 sentinel input: SAD1FP = PB1 (1001), SAD1FN = PB3 (101).
pub const R24_PB1_PB3: u8 = 0x95;
/// R25 AD1IG = 11 (3.6).
pub const R25_AD1IG_36: u8 = 0x30;
/// R29: ENPKH, PKHSEL = 01 (AD1<23:5>).
pub const R29_PKH_AD1: u8 = 0x05;
/// R29 bit 2.
pub const R29_ENPKH: u8 = 0x04;
/// CTSTA CMPHO | CMPLO.
pub const CTSTA_CMP: u8 = 0x30;

/// R33 bits 7:6: OP1CHOP (00 = CHOP_CLK held at 0).
pub const R33_OP1CHOP: u8 = 0xC0;

/// Device-check variants (DBG:SENTinel:VARiant, RAM only, 0 = the build's
/// choice): bit 0 keeps the row's OP1 chopper in the AC V sentinel row (the
/// 9dd1222 row), bit 1 drops the AC V sentinel (stock AC V row), bit 2 puts
/// the comparator sentinel back on DC V 60 / 600 / 1000 V (the 9dd1222 rows).
pub const VAR_ACV_CHOP: u8 = 1;
pub const VAR_ACV_OFF: u8 = 2;
pub const VAR_DCV_HI: u8 = 4;
static VARIANT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn set_variant(v: u8) {
    VARIANT.store(v & 7, core::sync::atomic::Ordering::Relaxed);
}

pub fn variant() -> u8 {
    VARIANT.load(core::sync::atomic::Ordering::Relaxed)
}

/// The stock row of `mode` / `range` with the sentinel overrides (module
/// table) when the alert that uses it is enabled in `mask`, and the
/// sentinel kind.
///
/// Device check of 9dd1222 (docs/device-check-2026-10-21.md):
/// * DC V 60 / 600 / 1000 V (r1..r3) carry no comparator sentinel: there
///   the comparator on PB1 tripped for as long as 10 V was on V-COM with
///   the A jack open (r0 at 1 V: never). Those rows drive the power bus
///   from OP3 (R2F SMODE 0100, PS6 + PS8 closed; r0 leaves the bus off), so
///   PB1 picks up an input-dependent level on the board. Leads in A/COM
///   with the function on DC V leave V-COM open: the autorange then sits on
///   6 V (r0), where the sentinel stays.
/// * AC V: the stock row keeps OP1CHOP = 10 (2 kHz) with AD1 off. OP1's
///   chopper clock evidently runs only with AD1 converting (the stock pairs
///   them in DC V 6 V / DC A), so turning AD1 on for the peak hold started a
///   2 kHz square of OP1's offset (x10 in 6 V) at OP1O -> PB4 -> AD2 / AD3:
///   a constant ~37 mV RMS floor for any input (AC mV reads OP2: clean).
///   The sentinel row holds the chopper off (OP1CHOP = 00); AC V is
///   AC-coupled, so OP1's static offset does not reach the reading.
pub fn sentinel_row(mode: u8, range: u8, acdc: bool, mask: u8, row: &[u8; CFG_LEN]) -> ([u8; CFG_LEN], Kind) {
    row_for(mode, range, acdc, mask, variant(), row)
}

/// `sentinel_row` with an explicit variant (host tests).
pub fn row_for(mode: u8, range: u8, acdc: bool, mask: u8, var: u8, row: &[u8; CFG_LEN]) -> ([u8; CFG_LEN], Kind) {
    let mut c = *row;
    if acdc {
        return (c, Kind::Off);
    }
    let kind = match mode {
        1 if range > 0 && var & VAR_DCV_HI == 0 => Kind::Off,
        1 | 3 | 10 | 15 if mask & M_AJACK != 0 => {
            c[0] = R20_PB1;
            Kind::CmpA
        }
        0 if var & VAR_ACV_OFF != 0 => Kind::Off,
        0 | 2 if mask & M_AJACK != 0 => {
            c[2] = 0x00;
            c[3] |= 0x80;
            c[4] = R24_PB1_PB3;
            c[5] |= R25_AD1IG_36;
            c[9] = (c[9] & 0xF8) | R29_PKH_AD1;
            if mode == 0 && var & VAR_ACV_CHOP == 0 {
                c[19] &= !R33_OP1CHOP;
            }
            Kind::PeakA
        }
        7..=9 if mask & M_VJACK != 0 => {
            c[0] = R20_SENSE;
            c[12] |= R2C_SS5;
            Kind::CmpV
        }
        _ => Kind::Off,
    };
    (c, kind)
}

/// A-JACK on the AD1 peak: |PKH| above 0.5 mA on 47.5 ohm (23.75 mV at
/// PB1): IG 3.6 * 23.75 mV / 1.25 V * 2^23 / 2^5 = 17 932.
pub const PK_THR: i32 = 17_900;
/// Window length (ms) of the comparator samples; a window with any trip is
/// a "hit".
pub const WIN_MS: u64 = 100;
/// Hit windows in a row before A-JACK is asserted (2 polls >= 100 ms apart).
pub const AJACK_WINDOWS: u8 = 2;
/// A-JACK clears this long after the last hit.
pub const AJACK_CLEAR_MS: u64 = 1000;
/// V-JACK?: voltage seen (and the current small) this long; clears 0.5 s
/// after the last hit.
pub const VJACK_ASSERT_MS: u64 = 1000;
pub const VJACK_CLEAR_MS: u64 = 500;
/// LIVE?: a live result this long; clears 1 s after the last one.
pub const LIVE_ASSERT_MS: u64 = 300;
pub const LIVE_CLEAR_MS: u64 = 1000;

// ------------------------------------------------------------- OVERLOAD
//
// Doc 4.3 / 4.7 and the manual (one 10 A ceramic fuse in front of all
// shunts: the 47.5 ohm uA resistor and the 0.5 ohm mA shunt have no fuse of
// their own). `dmm.rs` applies the rule; the pure parts are here.

/// Shunt of a current mode: 0 = A (10 mohm), 1 = mA (0.5 ohm), 2 = uA
/// (47.5 ohm chain); None outside the current modes (V / ohm / ... never
/// switch the shunt MOSFETs, doc 4.3).
pub fn shunt_of(mode: u8) -> Option<u8> {
    match mode {
        4 | 7 => Some(0),
        5 | 8 => Some(1),
        6 | 9 => Some(2),
        _ => None,
    }
}

/// Full scale (A) of a current range: unit (`shunt_of`) and range 0 / 1
/// (DM40C: 6 / 10 A, 60 / 600 mA, 600 / 6000 uA).
pub fn current_fs(unit: u8, range: u8) -> f32 {
    match (unit, range) {
        (0, 0) => 6.0,
        (0, _) => 10.0,
        (1, 0) => 0.06,
        (1, _) => 0.6,
        (_, 0) => 600e-6,
        _ => 6000e-6,
    }
}

/// The rule: on the uA / mA shunt (`unit` 2 / 1) a reading over its range
/// (`over`: |I| above 1.03 x the range, i.e. OL) moves the meter to the
/// 10 A shunt when the range is manual or the top range of that shunt
/// (r1). In auto at r0 the stock range step (r0 -> r1, the same shunt, 10x)
/// comes first; if r1 is over too the rule trips there. Never on the A
/// shunt, never outside the current modes.
pub fn overload_trips(unit: u8, range: u8, manual: bool, over: bool) -> bool {
    unit >= 1 && unit <= 2 && over && (manual || range >= 1)
}

/// The latch lets go after the current stayed below the release threshold
/// for this long (on whatever range the latch left the meter on).
pub const OVL_RELEASE_MS: u64 = 5000;

/// The 10 A range cannot tell less than this from zero (1 mA digits, a few
/// counts of noise): the release threshold is never below it.
pub const OVL_ZERO_A: f32 = 2e-3;

/// A trip this soon after a timed release makes the new latch sticky (no
/// timed release: only a user choice ends it). Covers the one case where
/// the threshold exceeds the range it returns to (manual 600 uA: 0.6..2 mA
/// would otherwise cycle trip / release every ~5 s).
pub const OVL_RETRIP_MS: u64 = 30_000;

/// Release threshold (A): half the full scale the meter goes back to - the
/// manual range it came from (`back`: unit, range), else (auto) the top
/// range of the barred shunt - so the return does not overload again at
/// once; at least OVL_ZERO_A.
pub fn overload_release_thr(barred: u8, back: Option<(u8, u8)>) -> f32 {
    let t = match back {
        Some((u, r)) => 0.5 * current_fs(u, r),
        None => 0.5 * current_fs(barred, 1),
    };
    t.max(OVL_ZERO_A)
}

pub struct Sentinel {
    pub kind: Kind,
    pub mask: u8,
    pub alert: Alert,
    /// Assertions so far (the app toasts on a change).
    pub seq: u32,
    win_t0: u64,
    win_hit: bool,
    streak: u8,
    /// Time of the last hit window (comparator / peak) and of its first one
    /// in the current run of hits.
    last_hit: u64,
    run_t0: u64,
    /// V-JACK?: current small since (None: not small).
    low_since: Option<u64>,
    /// LIVE?: first / last live reading.
    live_t0: Option<u64>,
    live_last: u64,
    /// Diagnostics (DBG:SENT?): last CTSTA, last peak pair, hit windows.
    pub ctsta: u8,
    pub pk: (i32, i32),
    pub hits: u32,
    pub samples: u32,
    /// OVERLOAD latch in force (set by `dmm.rs`, `set_overload`).
    pub ovl: bool,
}

impl Sentinel {
    pub const fn new() -> Self {
        Sentinel {
            kind: Kind::Off,
            mask: M_ALL,
            alert: Alert::None,
            seq: 0,
            win_t0: 0,
            win_hit: false,
            streak: 0,
            last_hit: 0,
            run_t0: 0,
            low_since: None,
            live_t0: None,
            live_last: 0,
            ctsta: 0,
            pk: (0, 0),
            hits: 0,
            samples: 0,
            ovl: false,
        }
    }

    /// A new row was written (`write_config`). Another kind or a function
    /// change (`reset`) drops the hit history and the alert; a range change
    /// of the same kind keeps it (an autoranging DC V must not restart it).
    ///
    /// Kinds of one family (the A-jack ones: comparator and AD1 peak) keep
    /// the history too: AC+DC alternates them every phase. OVERLOAD is the
    /// latch's (`set_overload`) and survives a row change (the latch steps
    /// through ranges of its own).
    pub fn configured(&mut self, kind: Kind, now: u64, reset: bool) {
        if kind.family() != self.kind.family() || reset {
            self.streak = 0;
            self.low_since = None;
            self.live_t0 = None;
            if !matches!(self.alert, Alert::None | Alert::Overload) {
                self.alert = Alert::None;
            }
        }
        self.kind = kind;
        self.win_t0 = now;
        self.win_hit = false;
    }

    /// OVERLOAD latch on / off (`dmm.rs`; not maskable: it reports a
    /// protective range change, not a guess).
    pub fn set_overload(&mut self, now: u64, on: bool) {
        self.ovl = on;
        self.decide(now);
    }

    /// Alerts masked off by the user are dropped at once.
    pub fn set_mask(&mut self, mask: u8) {
        self.mask = mask & M_ALL;
        let bit = match self.alert {
            Alert::AJack => M_AJACK,
            Alert::VJack => M_VJACK,
            Alert::Live => M_LIVE,
            Alert::None | Alert::Overload => 0,
        };
        if bit != 0 && self.mask & bit == 0 {
            self.alert = Alert::None;
        }
    }

    /// One CTSTA sample (comparator kinds), taken on every poll.
    pub fn sample_cmp(&mut self, now: u64, ctsta: u8) {
        self.ctsta = ctsta;
        self.samples = self.samples.wrapping_add(1);
        self.win_hit |= ctsta & CTSTA_CMP != 0;
        if now.saturating_sub(self.win_t0) >= WIN_MS {
            let hit = self.win_hit;
            self.win_t0 = now;
            self.win_hit = false;
            self.window(now, hit);
        }
        self.decide(now);
    }

    /// One peak-hold harvest (AD1 kind; every AC result, ~107 ms apart).
    /// `pkmin` / `pkmax`: the 19-bit PKHMIN / PKHMAX; an empty hold (no
    /// conversion since the restart: max < min) is no hit.
    pub fn sample_peak(&mut self, now: u64, pkmin: i32, pkmax: i32) {
        self.pk = (pkmin, pkmax);
        self.samples = self.samples.wrapping_add(1);
        let hit = pkmax >= pkmin && (pkmax > PK_THR || pkmin < -PK_THR);
        self.window(now, hit);
        self.decide(now);
    }

    fn window(&mut self, now: u64, hit: bool) {
        if hit {
            if self.streak == 0 {
                self.run_t0 = now;
            }
            self.streak = self.streak.saturating_add(1);
            self.last_hit = now;
            self.hits = self.hits.wrapping_add(1);
        } else {
            self.streak = 0;
        }
    }

    /// Current-mode reading: the current is below 0.5 % of the range (the
    /// V-JACK? precondition), or not.
    pub fn current_small(&mut self, now: u64, small: bool) {
        if !small {
            self.low_since = None;
        } else if self.low_since.is_none() {
            self.low_since = Some(now);
        }
        self.decide(now);
    }

    /// Ohm / continuity / diode reading: `live` = a result only an external
    /// source gives.
    pub fn live_reading(&mut self, now: u64, live: bool) {
        if live {
            if self.live_t0.is_none() {
                self.live_t0 = Some(now);
            }
            self.live_last = now;
        } else if now.saturating_sub(self.live_last) >= LIVE_CLEAR_MS {
            self.live_t0 = None;
        }
        self.decide(now);
    }

    /// The alert from the hit history (priority OVERLOAD > A-JACK > V-JACK?
    /// > LIVE?).
    fn decide(&mut self, now: u64) {
        let ajack = match self.kind {
            Kind::CmpA | Kind::PeakA => {
                let on = matches!(self.alert, Alert::AJack);
                if on {
                    now.saturating_sub(self.last_hit) < AJACK_CLEAR_MS
                } else {
                    self.streak >= AJACK_WINDOWS
                }
            }
            _ => false,
        } && self.mask & M_AJACK != 0;
        let vjack = self.kind == Kind::CmpV && self.mask & M_VJACK != 0 && {
            let on = matches!(self.alert, Alert::VJack);
            let low = self.low_since.map_or(false, |t| now.saturating_sub(t) >= VJACK_ASSERT_MS);
            if on {
                self.low_since.is_some() && now.saturating_sub(self.last_hit) < VJACK_CLEAR_MS
            } else {
                low && self.streak > 0 && now.saturating_sub(self.run_t0) + WIN_MS >= VJACK_ASSERT_MS
            }
        };
        let live = self.mask & M_LIVE != 0 && {
            let on = matches!(self.alert, Alert::Live);
            match self.live_t0 {
                Some(_) if on => now.saturating_sub(self.live_last) < LIVE_CLEAR_MS,
                Some(t0) => self.live_last.saturating_sub(t0) >= LIVE_ASSERT_MS,
                None => false,
            }
        };
        let a = if self.ovl {
            Alert::Overload
        } else if ajack {
            Alert::AJack
        } else if vjack {
            Alert::VJack
        } else if live {
            Alert::Live
        } else {
            Alert::None
        };
        if a != self.alert {
            if !matches!(a, Alert::None) {
                self.seq = self.seq.wrapping_add(1);
            }
            self.alert = a;
            QUES_EVENT.fetch_or(ques_bits(a), core::sync::atomic::Ordering::Relaxed);
        }
    }

    /// DBG:SENT? text: kind,alert,mask,ctsta,pkmin,pkmax,streak,hits,samples.
    pub fn diag_text(&self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        let mut put = |s: &[u8]| {
            let k = s.len().min(buf.len() - n);
            buf[n..n + k].copy_from_slice(&s[..k]);
            n += k;
        };
        let mut nb = [0u8; 12];
        put(self.kind.name().as_bytes());
        put(b",");
        put(self.alert.name().as_bytes());
        for v in [self.mask as i32, self.ctsta as i32, self.pk.0, self.pk.1, self.streak as i32, self.hits as i32, self.samples as i32] {
            put(b",");
            put(itoa(&mut nb, v));
        }
        n
    }
}

fn itoa(b: &mut [u8; 12], v: i32) -> &[u8] {
    let mut i = b.len();
    let neg = v < 0;
    let mut u = v.unsigned_abs();
    loop {
        i -= 1;
        b[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        b[i] = b'-';
    }
    &b[i..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hy3131_tables::*;

    /// Bytes of `got` that differ from `stock`, as (index, stock, got).
    fn diff(stock: &[u8; CFG_LEN], got: &[u8; CFG_LEN]) -> Vec<(usize, u8, u8)> {
        (0..CFG_LEN).filter(|&i| stock[i] != got[i]).map(|i| (i, stock[i], got[i])).collect()
    }

    #[test]
    fn comparator_rows_change_only_r20() {
        // DC V r0..r3, DC mV, ohm (both variants: also the resistive
        // temperature sensors), thermocouple.
        let mut rows: Vec<(u8, u8, &[u8; CFG_LEN])> = vec![(1, 0, &CFG_F1[0])];
        rows.extend(CFG_F3.iter().enumerate().map(|(i, r)| (3u8, i as u8, r)));
        rows.extend(CFG_F10_P8SET.iter().enumerate().map(|(i, r)| (10u8, i as u8, r)));
        rows.extend(CFG_F10_P8CLR.iter().enumerate().map(|(i, r)| (10u8, i as u8, r)));
        rows.extend(CFG_F10_P8CLR.iter().enumerate().map(|(i, r)| (15u8, i as u8, r)));
        rows.extend(CFG_F15.iter().map(|r| (15u8, 0, r)));
        // DC V 60 V+ only with the device-check variant.
        rows.extend(CFG_F1[1..].iter().map(|r| (1u8, 0x80, r)));
        for (m, rr, r) in rows {
            let var = if rr & 0x80 != 0 { VAR_DCV_HI } else { 0 };
            let (c, k) = row_for(m, rr & 0x7F | (rr >> 7), false, M_ALL, var, r);
            assert_eq!(k, Kind::CmpA);
            assert_eq!(diff(r, &c), vec![(0, r[0], 0x90)], "mode {m}");
            // The stock R20 of these rows: SCMPI = PB4 (0xf0) or ENCMP off
            // (0xe0, 0xc0): no counter, no CMP pin either way.
            assert_eq!(r[0] & 0x0E, 0, "mode {m}");
            // R21 = 0xdd: AGNDP1 / AGNDN1 = +-20.8 mV (ohm r7 0xcf is the
            // only exception; it is not used in fw-rs autorange).
        }
        for r in CFG_F1.iter().chain(CFG_F3.iter()).chain(CFG_F15.iter()) {
            assert_eq!(r[1], 0xdd);
        }
    }

    #[test]
    fn dcv_high_ranges_have_no_comparator() {
        // 9dd1222: A-JACK for as long as 10 V sat on the 60 V range (open A
        // jack). DC V r1..r3 keep the stock row; r0 keeps the sentinel.
        for (i, r) in CFG_F1.iter().enumerate() {
            let (c, k) = row_for(1, i as u8, false, M_ALL, 0, r);
            if i == 0 {
                assert_eq!((k, c[0]), (Kind::CmpA, R20_PB1));
            } else {
                assert_eq!((c, k), (*r, Kind::Off), "r{i}");
                // What sets them apart: OP3 drives the power bus (SMODE 0100).
                assert_eq!(r[15] & 0x0F, 0x04);
            }
        }
        assert_eq!(CFG_F1[0][15] & 0x0F, 0);
    }

    #[test]
    fn peak_rows_change_only_the_ad1_fields() {
        for (m, rows) in [(0u8, &CFG_F0[..]), (2, &CFG_F2[..])] {
            for (i, r) in rows.iter().enumerate() {
                let (c, k) = row_for(m, i as u8, false, M_ALL, 0, r);
                assert_eq!(k, Kind::PeakA);
                let d = diff(r, &c);
                let idx: Vec<usize> = d.iter().map(|x| x.0).collect();
                // R22 OSR, R23 ENAD1, R24 inputs, R25 AD1IG, R29 PKH; AC V
                // also R33 OP1CHOP held off (the 9dd1222 37 mV floor).
                if m == 0 {
                    assert_eq!(idx, vec![2, 3, 4, 5, 9, 19], "mode {m}");
                    assert_eq!(c[19], r[19] & 0x3F);
                    assert_eq!(r[19] & 0xC0, 0x80, "stock: OP1CHOP 2 kHz");
                    let (c1, _) = row_for(m, i as u8, false, M_ALL, VAR_ACV_CHOP, r);
                    assert_eq!(c1[19], r[19]);
                    assert_eq!(row_for(m, i as u8, false, M_ALL, VAR_ACV_OFF, r), (*r, Kind::Off));
                } else {
                    assert_eq!(idx, vec![2, 3, 4, 5, 9], "mode {m}");
                    assert_eq!(row_for(m, i as u8, false, M_ALL, VAR_ACV_OFF | VAR_ACV_CHOP, r).0, c);
                }
                assert_eq!(r[3] & 0x80, 0, "AD1 off in the stock AC row");
                assert_eq!(c[3], r[3] | 0x80);
                assert_eq!(c[4], 0x95);
                assert_eq!(c[5] & 0xCF, r[5] & 0xCF, "AD2IG / SACM / OPS kept");
                assert_eq!(c[5] & 0x30, 0x30);
                assert_eq!(c[9] & 0xF8, r[9] & 0xF8, "RMS / LPF / LPFBW kept");
                assert_eq!(c[9] & 7, 5);
                assert_eq!(c[2], 0x00);
                // R28 refs PB6 / AGND as the row (VR = 1.25 V).
                assert_eq!(c[8], 0x31);
            }
        }
    }

    #[test]
    fn current_rows_close_ss5_only() {
        for (m, rows) in [(7u8, &CFG_F7[..]), (8, &CFG_F8_9[..]), (9, &CFG_F8_9[..])] {
            for (i, r) in rows.iter().enumerate() {
                let (c, k) = row_for(m, i as u8, false, M_ALL, 0, r);
                assert_eq!(k, Kind::CmpV);
                assert_eq!(diff(r, &c), vec![(0, r[0], 0x10), (12, r[12], r[12] | 0x10)]);
                // The A rows: power bus as SGND (R2F 0x81), PS5 never closed.
                assert_eq!(r[15], 0x81);
                assert_eq!(c[12] & 0x80, 0);
                assert_eq!(r[12], 0);
            }
        }
    }

    #[test]
    fn other_rows_masks_and_acdc_untouched() {
        let all: Vec<(u8, &[u8; CFG_LEN])> = vec![(4, &CFG_F4[0]), (5, &CFG_F5_6[1]), (11, &CFG_F11[0]), (12, &CFG_F12[0]), (13, &CFG_F13[0]), (13, &CFG_F13[6]), (14, &CFG_F14[0])];
        for (m, r) in all {
            assert_eq!(row_for(m, 0, false, M_ALL, 0, r), (*r, Kind::Off));
        }
        assert_eq!(row_for(1, 0, true, M_ALL, 0, &CFG_F1[0]), (CFG_F1[0], Kind::Off));
        assert_eq!(row_for(0, 0, true, M_ALL, 0, &CFG_F0[1]), (CFG_F0[1], Kind::Off));
        assert_eq!(row_for(1, 0, false, M_VJACK | M_LIVE, 0, &CFG_F1[0]), (CFG_F1[0], Kind::Off));
        assert_eq!(row_for(0, 0, false, 0, 0, &CFG_F0[0]), (CFG_F0[0], Kind::Off));
        assert_eq!(row_for(7, 0, false, M_AJACK, 0, &CFG_F7[0]), (CFG_F7[0], Kind::Off));
    }

    #[test]
    fn ajack_asserts_after_two_windows_and_clears_after_1s() {
        let mut s = Sentinel::new();
        s.configured(Kind::CmpA, 0, true);
        let mut t = 0;
        // Quiet for a while: no alert.
        while t < 1000 {
            s.sample_cmp(t, 0);
            t += 10;
        }
        assert_eq!(s.alert, Alert::None);
        // Current: one window is not enough, two are.
        let start = t;
        while s.alert == Alert::None {
            s.sample_cmp(t, 0x20);
            t += 10;
            assert!(t - start <= 300);
        }
        // Hits seen >= 100 ms apart (two window closes).
        assert!(t - start >= 100, "{}", t - start);
        assert_eq!(s.alert, Alert::AJack);
        assert_eq!(s.seq, 1);
        // An AC current toggling between the bits and the dead band: stays.
        for i in 0..300 {
            s.sample_cmp(t, if i % 3 == 0 { 0 } else { 0x10 });
            t += 10;
        }
        assert_eq!(s.alert, Alert::AJack);
        // Gone: clears 1 s later, not before.
        let off = t;
        while s.alert == Alert::AJack {
            s.sample_cmp(t, 0);
            t += 10;
        }
        assert!(t - off >= 1000 && t - off <= 1200, "{}", t - off);
        assert_eq!(s.seq, 1);
        // A single glitch window does not assert.
        for i in 0..100 {
            s.sample_cmp(t, if i == 50 { 0x20 } else { 0 });
            t += 10;
        }
        assert_eq!(s.alert, Alert::None);
    }

    #[test]
    fn ajack_masked_and_reset() {
        let mut s = Sentinel::new();
        s.configured(Kind::CmpA, 0, true);
        for t in (0..500).step_by(10) {
            s.sample_cmp(t, 0x20);
        }
        assert_eq!(s.alert, Alert::AJack);
        s.set_mask(M_VJACK);
        assert_eq!(s.alert, Alert::None);
        s.set_mask(M_ALL);
        s.sample_cmp(510, 0x20);
        assert_eq!(s.alert, Alert::AJack);
        // A function change drops it; a same-kind range change keeps it.
        s.configured(Kind::CmpA, 520, false);
        assert_eq!(s.alert, Alert::AJack);
        s.configured(Kind::Off, 530, true);
        assert_eq!(s.alert, Alert::None);
    }

    #[test]
    fn peak_sentinel() {
        let mut s = Sentinel::new();
        s.configured(Kind::PeakA, 0, true);
        // Empty hold (preset) and noise: nothing.
        s.sample_peak(107, 0x3FFFF, -0x40000);
        s.sample_peak(214, -40, 35);
        s.sample_peak(321, -40, 35);
        assert_eq!(s.alert, Alert::None);
        // 1 mA AC: +-~50 k.
        s.sample_peak(428, -50_000, 50_000);
        assert_eq!(s.alert, Alert::None);
        s.sample_peak(535, -50_000, 50_000);
        assert_eq!(s.alert, Alert::AJack);
        // DC of the other sign only.
        s.sample_peak(642, -30_000, -29_000);
        assert_eq!(s.alert, Alert::AJack);
        for k in 0..12 {
            s.sample_peak(749 + k * 107, -20, 20);
        }
        assert_eq!(s.alert, Alert::None);
    }

    #[test]
    fn vjack_needs_voltage_and_no_current_for_1s() {
        let mut s = Sentinel::new();
        s.configured(Kind::CmpV, 0, true);
        let mut t = 0;
        // Voltage present, but a current flows: nothing.
        while t < 3000 {
            s.sample_cmp(t, 0x20);
            if t % 100 == 0 {
                s.current_small(t, false);
            }
            t += 10;
        }
        assert_eq!(s.alert, Alert::None);
        let t0 = t;
        while s.alert == Alert::None {
            s.sample_cmp(t, 0x20);
            if t % 100 == 0 {
                s.current_small(t, true);
            }
            t += 10;
            assert!(t - t0 < 2000);
        }
        assert!(t - t0 >= 1000);
        assert_eq!(s.alert, Alert::VJack);
        // Voltage gone: clears within ~0.5 s.
        let t1 = t;
        while s.alert == Alert::VJack {
            s.sample_cmp(t, 0);
            t += 10;
        }
        assert!(t - t1 >= 500 && t - t1 <= 700, "{}", t - t1);
        // Masked: never.
        s.set_mask(M_AJACK);
        for _ in 0..300 {
            s.sample_cmp(t, 0x20);
            s.current_small(t, true);
            t += 10;
        }
        assert_eq!(s.alert, Alert::None);
    }

    #[test]
    fn live_from_readings() {
        let mut s = Sentinel::new();
        s.configured(Kind::CmpA, 0, true);
        s.live_reading(0, true);
        s.live_reading(100, true);
        assert_eq!(s.alert, Alert::None);
        s.live_reading(300, true);
        assert_eq!(s.alert, Alert::Live);
        s.live_reading(700, false);
        assert_eq!(s.alert, Alert::Live);
        s.live_reading(1400, false);
        assert_eq!(s.alert, Alert::None);
        // A-JACK wins over LIVE?.
        s.live_reading(1500, true);
        s.live_reading(1900, true);
        assert_eq!(s.alert, Alert::Live);
        for t in (1900..2300).step_by(10) {
            s.sample_cmp(t, 0x10);
        }
        assert_eq!(s.alert, Alert::AJack);
    }

    #[test]
    fn overload_rule() {
        // Only the current modes have a shunt; V / ohm / cont / diode / cap /
        // Hz / temperature never trip (doc 4.3: no MOSFET switching there).
        for m in (0..=3).chain(10..=17) {
            assert_eq!(shunt_of(m), None, "mode {m}");
        }
        assert_eq!([4, 5, 6, 7, 8, 9].map(shunt_of), [Some(0), Some(1), Some(2), Some(0), Some(1), Some(2)]);
        // A shunt: never.
        for (r, man) in [(0, false), (1, false), (0, true), (1, true)] {
            assert!(!overload_trips(0, r, man, true));
        }
        for u in [1, 2] {
            // Auto r0 over: the range step first; r1 over: trip.
            assert!(!overload_trips(u, 0, false, true));
            assert!(overload_trips(u, 1, false, true));
            // Manual: any OL.
            assert!(overload_trips(u, 0, true, true));
            assert!(overload_trips(u, 1, true, true));
            // In range: never.
            assert!(!overload_trips(u, 1, true, false));
        }
        assert!(!overload_trips(3, 1, true, true));
        // Release below half the scale it returns to.
        assert_eq!(overload_release_thr(2, None), 3e-3);
        assert_eq!(overload_release_thr(1, None), 0.3);
        assert_eq!(overload_release_thr(1, Some((2, 0))), OVL_ZERO_A);
        assert_eq!(overload_release_thr(2, Some((2, 1))), 3e-3);
        assert_eq!(overload_release_thr(2, Some((1, 1))), 0.3);
        // Below the range it returns to, except manual 600 uA (OVL_RETRIP_MS).
        for u in 0..3 {
            for r in 0..2 {
                let t = overload_release_thr(u, Some((u, r)));
                assert_eq!(t < current_fs(u, r) / 1.03, (u, r) != (2, 0), "{u} {r}");
            }
        }
    }

    #[test]
    fn overload_alert_latch() {
        let mut s = Sentinel::new();
        s.configured(Kind::CmpV, 0, true);
        s.set_overload(10, true);
        assert_eq!((s.alert, s.seq), (Alert::Overload, 1));
        // A range / shunt change of the latch (a new row, also a function
        // reset) keeps it and does not count a new assertion.
        s.configured(Kind::CmpV, 20, true);
        s.configured(Kind::Off, 30, true);
        s.sample_cmp(140, 0x20);
        assert_eq!((s.alert, s.seq), (Alert::Overload, 1));
        // Not maskable.
        s.set_mask(0);
        assert_eq!(s.alert, Alert::Overload);
        s.set_mask(M_ALL);
        // Over V-JACK?.
        s.configured(Kind::CmpV, 200, true);
        for t in (200..2000).step_by(10) {
            s.sample_cmp(t, 0x20);
            s.current_small(t, true);
        }
        assert_eq!(s.alert, Alert::Overload);
        s.set_overload(2000, false);
        assert_eq!((s.alert, s.seq), (Alert::VJack, 2));
        assert!(Alert::Overload.red() && Alert::AJack.red() && !Alert::VJack.red() && !Alert::Live.red());
    }

    #[test]
    fn family_keeps_history_across_ajack_kinds() {
        // AC+DC alternates the comparator (DC phase) and the AD1 peak (AC
        // phase) sentinels: an asserted A-JACK survives the phase switch.
        let mut s = Sentinel::new();
        s.configured(Kind::CmpA, 0, true);
        for t in (0..300).step_by(10) {
            s.sample_cmp(t, 0x20);
        }
        assert_eq!(s.alert, Alert::AJack);
        s.configured(Kind::PeakA, 300, false);
        assert_eq!(s.alert, Alert::AJack);
        s.sample_peak(400, -50_000, 50_000);
        assert_eq!(s.alert, Alert::AJack);
        // Another family: dropped.
        s.configured(Kind::CmpV, 500, false);
        assert_eq!(s.alert, Alert::None);
    }

    #[test]
    fn diag_text_format() {
        let mut s = Sentinel::new();
        s.configured(Kind::PeakA, 0, true);
        s.sample_peak(100, -5, 7);
        let mut b = [0u8; 96];
        let n = s.diag_text(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "AD1PK_PB1,NONE,7,0,-5,7,0,0,1");
    }
}
