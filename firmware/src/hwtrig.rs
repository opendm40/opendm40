//! Hardware trigger: the stock comparator path, without interrupts.
//!
//! Board (stock `FUN_08012260`, `scope_gen_v2_6.h` "Trigger"): DAC channel 2
//! on PA5 sets the level of an external comparator (chip not traced) whose
//! output reaches PA15 = AF1 TIM2_CH1. The comparator sits on the ADC input
//! node (after the offset PWM and the gain stage): the stock programs the DAC
//! from an ADC-code-domain level with a two-point map, so 1024 ADC codes span
//! about 621 DAC codes = 0.45 V (3.0 V reference), the full-scale span of the
//! 3PA1030. The comparator output is inverted with respect to "signal > level"
//! (stock `FUN_0801230c`: a rising-edge trigger captures a *falling* PA15
//! edge).
//!
//! How the stock uses it (`FUN_08010d22`, `FUN_08012370`, `FUN_080123ce`):
//! TIM2 is a one-pulse post-trigger counter started by the PA15 edge (slave
//! trigger mode, TS = TI1FP1, IC filter 3). Its update interrupt, `post`
//! samples later, disables DMA stream 4 and reads NDTR; the trigger position is
//! `pre + post` arithmetic (CCR1 is never read), refined by a software search
//! over +-400 samples with the +-2 code hysteresis. The subtraction
//! `trig_delay` (2 us at every fast rate) is the comparator latency: the data
//! crossing sits about 2 us *before* the PA15 edge.
//!
//! opendm40 has no peripheral interrupts and must not stop the DMA from an ISR,
//! so the stop is moved into hardware. PA15 reaches only TIM2 (GD32F427
//! datasheet pin table: TIMER1_CH0 / TIMER1_ETI), and TIM2 also feeds the
//! frequency counter, so TIM2 only time-stamps the edges and TIM4 counts the
//! post-trigger samples (GD32F4xx User Manual rev 3.3, the ITI table of the
//! slave mode controller; GD TIMERn = TIM(n+1) here):
//!   * TIM2 runs free (32 bit, PSC 0, 200 MHz) with CH1 capturing the PA15
//!     edge of the trigger polarity into CCR1; its TRGO is the CC1 pulse
//!     (MMS = 3), one pulse per captured edge.
//!   * TIM5 (32 bit) counts those pulses: external clock mode 1 from ITR0 =
//!     TIM2_TRGO. Edge count and the last edge's time stamp give a
//!     reciprocal frequency counter (`FreqCounter`), independent of the
//!     timebase and of the record.
//!   * TIM4 is the post-trigger counter: OC2 "inactive on match" with CCR2 =
//!     post count, OC2REF is its TRGO (MMS = 5). It is forced high while
//!     prepared; `arm` puts TIM4 in trigger mode from ITR1 = TIM2_TRGO, the
//!     next captured edge starts it, CCR2 ticks later OC2REF drops and stays
//!     low (the 16-bit counter may wrap, the level holds).
//!   * TIM1 (the sample-request timer, `scope.rs`) is put in gated slave mode
//!     from ITR3 = TIM4_TRGO: it counts while OC2REF is high, so the DMA
//!     (circular over the whole record) freezes with the trigger `post`
//!     samples before the write pointer `CAP - NDTR`.
//!   * The record is then linearised and the stock software search runs in a
//!     window around that estimate; the display and Single/stored records use
//!     the refined index. TIM8 (ADC clock, latch stream) is not affected.
//! One TIM4 tick is 1/m sample (`tim4_div`: TIM1's period in core ticks over
//! the smallest m that fits the 16-bit PSC), the post count is in those ticks.
//!
//! Only DAC bits 16..31 are touched (channel 2); `gen.rs` owns bits 0..15.
//! Nothing here allocates: the module is a thin register driver plus the
//! calibration maths.

use crate::mmio::{modify32, r32, w32};

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const GPIOA: u32 = 0x4002_0000;
const DAC: u32 = 0x4000_7400;
const DAC_CR: u32 = DAC + 0x00;
const DAC_DHR12R2: u32 = DAC + 0x14;
const DAC_DOR2: u32 = DAC + 0x30;
const TIM1: u32 = 0x4001_0000;
const TIM1_SMCR: u32 = TIM1 + 0x08;
const TIM2: u32 = 0x4000_0000;
const TIM4: u32 = 0x4000_0800;
const TIM4_CR1: u32 = TIM4 + 0x00;
const TIM4_CR2: u32 = TIM4 + 0x04;
const TIM4_SMCR: u32 = TIM4 + 0x08;
const TIM4_DIER: u32 = TIM4 + 0x0C;
const TIM4_SR: u32 = TIM4 + 0x10;
const TIM4_EGR: u32 = TIM4 + 0x14;
const TIM4_CCMR1: u32 = TIM4 + 0x18;
const TIM4_CCER: u32 = TIM4 + 0x20;
const TIM4_CNT: u32 = TIM4 + 0x24;
const TIM4_PSC: u32 = TIM4 + 0x28;
const TIM4_ARR: u32 = TIM4 + 0x2C;
const TIM4_CCR2: u32 = TIM4 + 0x38;
const TIM5: u32 = 0x4000_0C00;
const TIM5_CR1: u32 = TIM5 + 0x00;
const TIM5_CR2: u32 = TIM5 + 0x04;
const TIM5_SMCR: u32 = TIM5 + 0x08;
const TIM5_DIER: u32 = TIM5 + 0x0C;
const TIM5_SR: u32 = TIM5 + 0x10;
const TIM5_EGR: u32 = TIM5 + 0x14;
const TIM5_CNT: u32 = TIM5 + 0x24;
const TIM5_PSC: u32 = TIM5 + 0x28;
const TIM5_ARR: u32 = TIM5 + 0x2C;
const TIM2_CR1: u32 = TIM2 + 0x00;
const TIM2_CR2: u32 = TIM2 + 0x04;
const TIM2_SMCR: u32 = TIM2 + 0x08;
const TIM2_DIER: u32 = TIM2 + 0x0C;
const TIM2_SR: u32 = TIM2 + 0x10;
const TIM2_EGR: u32 = TIM2 + 0x14;
const TIM2_CCMR1: u32 = TIM2 + 0x18;
const TIM2_CCER: u32 = TIM2 + 0x20;
const TIM2_CNT: u32 = TIM2 + 0x24;
const TIM2_PSC: u32 = TIM2 + 0x28;
const TIM2_ARR: u32 = TIM2 + 0x2C;
const TIM2_CCR1: u32 = TIM2 + 0x34;
/// Offset PWM compare register (scope.rs owns TIM3; written here only during
/// `calibrate`, the caller re-applies the scope settings afterwards).
const TIM3_CCR3: u32 = 0x4000_0400 + 0x3C;

/// Settings record: per-edge trigger map `REC_CH1_TRIG_CAL` (stock
/// `scope_gen_ui_v2_6.h:469`): 2 x {code at DAC 1780, code at DAC 2401}.
const REC_TRIG: u32 = 0x080C_0000 + 0x148;
/// Offset calibration pairs `REC_CH1_OFFSET_CAL`: 10 x {code at CCR3 11636,
/// code at CCR3 8800}.
const REC_OFFSET: u32 = 0x080C_0000 + 0xA8;

/// Stock two-point map constants (`SCOPE_TRIG_DAC_LO/HI`, flash 0x0806d090).
pub const DAC_LO: f32 = 1780.0;
pub const DAC_HI: f32 = 2401.0;
/// Stock default map: code 300 at DAC 1780, code 780 at DAC 2401 (1.294
/// DAC counts per ADC code).
pub const MAP_DEFAULT: (f32, f32) = (300.0, 780.0);
/// Offset PWM points of the stock self-calibration.
pub const CCR3_LO: u32 = 8800;
pub const CCR3_HI: u32 = 11636;

/// Timer clock (TIMPRE = 1: every timer runs at the 200 MHz core clock).
pub const TIM_CLK: u32 = 200_000_000;
/// TIM2 CCMR1: CH1 input capture on TI1, no prescaler, filter 3 (fCK_INT,
/// N = 8: an edge must hold 40 ns).
const TIM2_CCMR1_CAPTURE: u32 = (3 << 4) | 1;
/// TIM2 CR2: MMS = 011, TRGO = the CC1 (capture) pulse.
const TIM2_CR2_CC1_PULSE: u32 = 3 << 4;
/// TIM5 SMCR: external clock mode 1 (SMS = 111) from ITR0 = TIM2_TRGO.
const TIM5_SMCR_COUNT_TIM2: u32 = (0b000 << 4) | 0b111;
/// TIM4 CCMR1: CH2 output compare, OC2M = 101 forces OC2REF active (TIM1
/// gate open).
const TIM4_CCMR1_FORCED: u32 = 0b101 << 12;
/// OC2M = 010: OC2REF goes inactive at CNT = CCR2 and stays there.
const TIM4_CCMR1_LOW_ON_MATCH: u32 = 0b010 << 12;
/// TIM4 CR2: MMS = 101, TRGO = OC2REF.
const TIM4_CR2_OC2REF: u32 = 5 << 4;
/// TIM4 SMCR: trigger mode (SMS = 110) from ITR1 = TIM2_TRGO (TS = 001).
const TIM4_SMCR_TRIG_TIM2: u32 = (0b001 << 4) | 0b110;
/// TIM1 SMCR: gated mode (SMS = 101) from ITR3 = TIM4_TRGO (TS = 011).
const TIM1_SMCR_GATED_BY_TIM4: u32 = (0b011 << 4) | 0b101;

/// Trigger engine state as seen in the TIM2 registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Not prepared: TIM1 free-running, TIM4 idle.
    Off,
    /// Prepared (level set, gate open), waiting for `arm` (pre-trigger fill).
    Prepared,
    /// Armed: the next PA15 edge starts the post-trigger count.
    Armed,
    /// Edge seen, post-trigger samples still being written.
    Fired,
    /// Post-trigger count elapsed: TIM1 is gated off, the record is frozen.
    Done,
}

#[derive(Clone, Copy, Debug)]
pub enum CalError {
    /// PA15 does not differ between DAC 0 and DAC 4095: no comparator output
    /// (scope rail PC4 off, or no comparator on this board revision).
    NoComparator,
    /// A sweep found no clean flip (noise, or the input is not quiet).
    NoFlip,
    /// Moving the offset PWM did not move the flip point: the comparator is
    /// not on the offset-corrected node (the "after the offset stage"
    /// inference would be wrong) or the input is driven.
    OffsetNoEffect,
    /// Slope outside 0.8..2.0 DAC codes per ADC code (stock default 1.294).
    Slope,
}

/// Result of `calibrate`: two offset points, each swept up and down.
#[derive(Clone, Copy, Debug)]
pub struct CalResult {
    /// DAC flip points `[point][0 = up-sweep, 1 = down-sweep]`, point 0 at
    /// CCR3 8800 (code ~280), point 1 at CCR3 11636 (code ~730).
    pub flip: [[u16; 2]; 2],
    /// ADC codes of the flat input at the two offset points (from the
    /// settings record or the caller).
    pub codes: [f32; 2],
    /// Resulting per-edge map in the stock format {code at 1780, code at 2401}.
    pub map: [(f32, f32); 2],
    /// Comparator hysteresis, DAC codes (up-sweep minus down-sweep flip).
    pub hyst: f32,
    /// DAC codes per ADC code.
    pub slope: f32,
}

/// Trigger statistics since the last `reset_stats` (DBG query / jitter test).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Frames where the comparator fired and a crossing was found.
    pub hits: u32,
    /// Frames where it fired but no crossing was within the window.
    pub misses: u32,
    /// Refined crossing minus the hardware estimate (samples): last, min, max,
    /// and the sum for a mean. The spread is the comparator latency jitter in
    /// samples at the current rate.
    pub delta_last: i32,
    pub delta_min: i32,
    pub delta_max: i32,
    pub delta_sum: i64,
}

pub struct HwTrig {
    /// Map from `calibrate` (RAM only, lost at reset; a settings-record
    /// write-back is a follow-up).
    cal: Option<[(f32, f32); 2]>,
    pub stats: Stats,
    /// Policy switch: `false` leaves the scope on its software trigger.
    /// Starts as `DEFAULT_ENABLED`.
    enabled: bool,
    /// Consecutive frames where the comparator fired but no crossing was in
    /// the search window (bad map), or a timeout where software found one:
    /// after `MAX_MISSES` the engine suspends itself.
    misses: u8,
    /// Suspended by `MAX_MISSES` misses or a stuck gate (`suspend`): the
    /// software trigger runs until `resume`, which the scope calls on any
    /// change of the trigger-relevant settings. Not a permanent switch-off
    /// (device 2026-10-21: AUTO's sweep through overdriven ranges collected
    /// three misses, and the comparator stayed off for the rest of the
    /// session: ~9 fps at 1 ms/div after AUTO vs ~36 fps before).
    suspended: bool,
    prepared: bool,
    armed: bool,
    post_ticks: u32,
    pub last_cal: Option<CalResult>,
    /// Comparator frequency counter (TIM2 time stamps, TIM5 edge count).
    pub freq: FreqCounter,
    /// TIM2 CC1P as last written (the captured edge: true = falling PA15 =
    /// a rising signal).
    edge: Option<bool>,
    /// DAC2 code as last written by `track_level`.
    dac: u16,
}

const MAX_MISSES: u8 = 3;
/// Off until the device test (report "Device test plan") has passed: with the
/// stock pair at REC+0x148 present `available()` would otherwise be true from
/// the first boot and every acquisition would take the comparator path.
/// `set_enabled(true)` (DBG command) switches it on at run time.
pub const DEFAULT_ENABLED: bool = false;

impl HwTrig {
    pub const fn new() -> Self {
        HwTrig {
            cal: None,
            stats: Stats { hits: 0, misses: 0, delta_last: 0, delta_min: i32::MAX, delta_max: i32::MIN, delta_sum: 0 },
            enabled: DEFAULT_ENABLED,
            misses: 0,
            suspended: false,
            prepared: false,
            armed: false,
            post_ticks: 0,
            last_cal: None,
            freq: FreqCounter::new(),
            edge: None,
            dac: 0xFFFF,
        }
    }

    pub fn reset_stats(&mut self) {
        self.stats = Stats { hits: 0, misses: 0, delta_last: 0, delta_min: i32::MAX, delta_max: i32::MIN, delta_sum: 0 };
    }

    /// One-time hardware setup (stock `FUN_08012260`): GPIOA / DAC / TIM2 /
    /// TIM4 / TIM5 clocks, PA5 analog, PA15 AF1 with pull-down, DAC channel 2
    /// enabled (buffer on, no trigger, no wave), TIM2 time-stamping and TIM5
    /// counting the edges (they run from here on), TIM4 idle with the TIM1
    /// gate open. TIM1 SMCR is left at 0 here: `prepare` sets the gate.
    pub fn init(&mut self) {
        unsafe {
            modify32(RCU_AHB1EN, 0, 1 << 0);
            modify32(RCU_APB1EN, 0, (1 << 29) | (1 << 3) | (1 << 2) | (1 << 0));
            // PA5: analog, no pull.
            modify32(GPIOA + 0x0C, 3 << 10, 0);
            modify32(GPIOA + 0x00, 3 << 10, 3 << 10);
            // PA15: AF1 (TIM2_CH1), push-pull, very high speed, pull-down.
            modify32(GPIOA + 0x24, 0xF << 28, 1 << 28);
            modify32(GPIOA + 0x04, 1 << 15, 0);
            modify32(GPIOA + 0x08, 3 << 30, 3 << 30);
            modify32(GPIOA + 0x0C, 3 << 30, 2 << 30);
            modify32(GPIOA + 0x00, 3 << 30, 2 << 30);
            // DAC channel 2: EN2 = 1, BOFF2 = 0, TEN2 = 0, WAVE2 = 0. Channel 1
            // bits (generator) untouched.
            modify32(DAC_CR, 0xFFFF_0000, 1 << 16);
            w32(DAC_DHR12R2, 2048);
            // TIM5: edge counter (no interrupts), started before TIM2 so it
            // sees every pulse.
            w32(TIM5_CR1, 0);
            w32(TIM5_DIER, 0);
            w32(TIM5_CR2, 0);
            w32(TIM5_SMCR, TIM5_SMCR_COUNT_TIM2);
            w32(TIM5_PSC, 0);
            w32(TIM5_ARR, 0xFFFF_FFFF);
            w32(TIM5_EGR, 1);
            w32(TIM5_CNT, 0);
            w32(TIM5_SR, 0);
            w32(TIM5_CR1, 1);
            // TIM2: free-running time base, CH1 capture, TRGO = CC1 pulse.
            w32(TIM2_CR1, 0);
            w32(TIM2_DIER, 0);
            w32(TIM2_SMCR, 0);
            w32(TIM2_CCER, 0);
            w32(TIM2_CCMR1, TIM2_CCMR1_CAPTURE);
            w32(TIM2_CR2, TIM2_CR2_CC1_PULSE);
            w32(TIM2_PSC, 0);
            w32(TIM2_ARR, 0xFFFF_FFFF);
            w32(TIM2_EGR, 1);
            w32(TIM2_SR, 0);
            w32(TIM2_CCER, 1 | (1 << 1));
            w32(TIM2_CR1, 1);
            // TIM4 quiet: no interrupts, gate open, counter stopped.
            w32(TIM4_CR1, 0);
            w32(TIM4_DIER, 0);
            w32(TIM4_SMCR, 0);
            w32(TIM4_CCER, 0);
            w32(TIM4_CCMR1, TIM4_CCMR1_FORCED);
            w32(TIM4_CR2, TIM4_CR2_OC2REF);
            w32(TIM4_ARR, 0xFFFF);
            w32(TIM4_SR, 0);
        }
        self.edge = Some(true);
        self.dac = 0xFFFF;
        self.freq.restart();
        self.prepared = false;
        self.armed = false;
    }

    /// Captured edge: a rising signal is a falling PA15 edge (CC1P = 1).
    /// TIM2 keeps running; a new polarity restarts the frequency count.
    pub fn set_edge(&mut self, rising: bool) {
        if self.edge == Some(rising) {
            return;
        }
        unsafe { w32(TIM2_CCER, 1 | if rising { 1 << 1 } else { 0 }) };
        self.edge = Some(rising);
        self.freq.restart();
    }

    /// Keep the comparator at the trigger level and edge also while the
    /// software trigger runs (the frequency counter uses it). Writes only on
    /// a change.
    pub fn track_level(&mut self, code: f32, rising: bool) {
        self.set_edge(rising);
        let d = self.level_dac(code, rising);
        if d != self.dac {
            self.set_level(code, rising);
        }
    }

    /// Policy: use the comparator at all (default on; `note_miss` may turn it
    /// off; a DBG command may turn it back on).
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if on {
            self.misses = 0;
            self.suspended = false;
        }
    }

    /// Hand the scope to the software trigger until `resume`.
    #[cfg_attr(not(test), allow(dead_code))] // tools/touch-test
    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    /// Try the comparator again (new settings): clears a suspension and the
    /// miss count. No effect while disabled.
    pub fn resume(&mut self) {
        self.suspended = false;
        self.misses = 0;
    }

    #[cfg_attr(not(test), allow(dead_code))] // tools/touch-test
    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The comparator path is usable: enabled, not suspended, and a plausible
    /// map exists.
    pub fn available(&self) -> bool {
        self.enabled && !self.suspended && (self.cal.is_some() || rec_pair(REC_TRIG, 0).is_some())
    }

    /// Per-edge map {code at DAC 1780, code at DAC 2401}: RAM calibration,
    /// else the settings record, else the stock defaults.
    pub fn map(&self, rising: bool) -> (f32, f32) {
        let e = if rising { 0 } else { 1 };
        if let Some(c) = self.cal {
            return c[e];
        }
        rec_pair(REC_TRIG, e).unwrap_or(MAP_DEFAULT)
    }

    /// Trigger level in ADC codes -> DAC2 code (stock `FUN_08021032` /
    /// `FUN_0801241a`: `1780 + (code - t0) (2401 - 1780) / (t1 - t0)`,
    /// truncated), clamped to the 12-bit range.
    pub fn level_dac(&self, code: f32, rising: bool) -> u16 {
        let (t0, t1) = self.map(rising);
        let d = DAC_LO + (code - t0) * (DAC_HI - DAC_LO) / (t1 - t0);
        if !d.is_finite() {
            return 2048;
        }
        d.clamp(0.0, 4095.0) as u16
    }

    /// Write the level: DAC2 = `level_dac(code)`. Re-asserts EN2 (cheap and
    /// immune to any future whole-register write on the generator side).
    pub fn set_level(&mut self, code: f32, rising: bool) {
        let d = self.level_dac(code, rising);
        self.dac = d;
        unsafe {
            modify32(DAC_CR, 0xFFFF_0000, 1 << 16);
            w32(DAC_DHR12R2, d as u32);
        }
    }

    /// Raw DAC2 code (calibration / diagnostics).
    pub fn set_dac(&mut self, code: u16) {
        self.dac = 0xFFFF;
        unsafe {
            modify32(DAC_CR, 0xFFFF_0000, 1 << 16);
            w32(DAC_DHR12R2, (code & 0xFFF) as u32);
        }
    }

    /// Comparator output as seen on PA15 (IDR works in AF mode).
    pub fn pa15(&self) -> bool {
        unsafe { r32(GPIOA + 0x10) & (1 << 15) != 0 }
    }

    /// Configure the engine for the acquisition about to start. Call before
    /// TIM1 is enabled (TIM1 CR1 CEN); `tim1_psc` / `tim1_arr` are the row of
    /// the HW table in use, `post` the number of samples to keep after the
    /// trigger, `code` the trigger level in ADC codes.
    /// After this TIM1 only counts while TIM4's OC2REF is high (it is, until
    /// a trigger has run its post count), the DAC holds the level, and TIM4
    /// waits for `arm`. False (nothing changed, TIM1 not gated): the post
    /// count does not fit TIM4 at this rate; use the software trigger.
    pub fn prepare(&mut self, tim1_psc: u32, tim1_arr: u32, post: u32, code: f32, rising: bool) -> bool {
        let Some((div, ticks)) = tim4_div((tim1_psc + 1) * (tim1_arr + 1), post.max(1)) else {
            self.prepared = false;
            self.armed = false;
            return false;
        };
        self.set_edge(rising);
        self.set_level(code, rising);
        self.post_ticks = ticks;
        unsafe {
            w32(TIM4_CR1, 0);
            w32(TIM4_SMCR, 0);
            w32(TIM4_CCMR1, TIM4_CCMR1_FORCED);
            w32(TIM4_CR2, TIM4_CR2_OC2REF);
            w32(TIM4_PSC, div - 1);
            w32(TIM4_ARR, 0xFFFF);
            w32(TIM4_CCR2, ticks);
            w32(TIM4_CNT, 0);
            // Load PSC (takes effect at an update event), then clear the flags.
            w32(TIM4_EGR, 1);
            w32(TIM4_SR, 0);
            w32(TIM1_SMCR, TIM1_SMCR_GATED_BY_TIM4);
        }
        self.prepared = true;
        self.armed = false;
        true
    }

    /// Enable the trigger once the pre-trigger part of the record is written
    /// (the stock arms from TIM5 CC1 at `pre`). From here the next captured
    /// PA15 edge starts the post-trigger count.
    pub fn arm(&mut self) {
        if !self.prepared || self.armed {
            return;
        }
        unsafe {
            w32(TIM4_CNT, 0);
            w32(TIM4_SR, 0);
            w32(TIM4_CCMR1, TIM4_CCMR1_LOW_ON_MATCH);
            w32(TIM4_SMCR, TIM4_SMCR_TRIG_TIM2);
        }
        self.armed = true;
    }

    /// Where the engine is.
    pub fn status(&self) -> Status {
        if !self.prepared {
            return Status::Off;
        }
        if !self.armed {
            return Status::Prepared;
        }
        let (sr, cr1) = unsafe { (r32(TIM4_SR), r32(TIM4_CR1)) };
        let fired = sr & (1 << 6) != 0 || cr1 & 1 != 0;
        if !fired {
            return Status::Armed;
        }
        // CC2IF: the match that dropped OC2REF (CNT may have wrapped since).
        if sr & (1 << 2) != 0 {
            Status::Done
        } else {
            Status::Fired
        }
    }

    /// Post-trigger count of the current preparation in TIM4 ticks.
    pub fn post_ticks(&self) -> u32 {
        self.post_ticks
    }

    /// Release: TIM4 stopped and reset, gate forced open, TIM1 back to its
    /// free-running (software trigger) configuration. TIM2 / TIM5 (the
    /// frequency counter) keep running. Safe to call any time; `Scope::halt`
    /// must call it so the plain path is never left gated.
    pub fn disarm(&mut self) {
        unsafe {
            w32(TIM4_SMCR, 0);
            w32(TIM4_CR1, 0);
            w32(TIM4_CCMR1, TIM4_CCMR1_FORCED);
            w32(TIM4_CNT, 0);
            w32(TIM4_SR, 0);
            w32(TIM1_SMCR, 0);
        }
        self.prepared = false;
        self.armed = false;
    }

    /// The comparator fired but the software search found no crossing near
    /// the estimate: the map is off (or the signal is too fast for the
    /// comparator, or the front end is overdriven / still settling). Three in
    /// a row suspend the engine until the next settings change (`resume`).
    pub fn note_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
        self.stats.misses = self.stats.misses.wrapping_add(1);
        if self.misses >= MAX_MISSES {
            self.suspended = true;
        }
    }

    /// A crossing was found `delta` samples from the hardware estimate.
    pub fn note_hit(&mut self, delta: i32) {
        self.misses = 0;
        let s = &mut self.stats;
        s.hits = s.hits.wrapping_add(1);
        s.delta_last = delta;
        s.delta_min = s.delta_min.min(delta);
        s.delta_max = s.delta_max.max(delta);
        s.delta_sum += delta as i64;
    }

    /// Wire-free calibration (docs/dac-adc-paths.md section 5): with an open
    /// input the node the comparator sees is the offset level, i.e. the ADC
    /// code of 0 V at the current CCR3. For CCR3 = 8800 and 11636 the codes
    /// are the stock offset pair {o1, o0} of the current range (settings
    /// record +0xA8, calibrated by the stock offset self-cal; `codes` can
    /// override them with measured record means). At each point DAC2 is swept
    /// up and down and the PA15 flip is recorded: the down-sweep flip is the
    /// threshold a rising signal meets, the up-sweep flip the one a falling
    /// signal meets (comparator hysteresis), so each edge gets its own map
    /// like the stock pairs.
    ///
    /// Preconditions (caller): scope rail PC4 on and settled, acquisition
    /// halted and `disarm`ed, range 6 (1 V/div) set, DC coupling, nothing on
    /// the MCX. Blocks for about 0.5 s. Afterwards re-apply the scope settings
    /// (CCR3 is left at 11636).
    #[inline(never)] // .cold (link-native.x): SCPI calibration
    pub fn calibrate(&mut self, range: usize, codes: Option<(f32, f32)>) -> Result<CalResult, CalError> {
        let (lo_code, hi_code) = match codes {
            Some(c) => c,
            None => {
                let (o0, o1) = rec_pair(REC_OFFSET, range.min(9)).unwrap_or((730.0, 280.0));
                (o1, o0)
            }
        };
        let mut flip = [[0u16; 2]; 2];
        for (i, ccr) in [CCR3_LO, CCR3_HI].iter().enumerate() {
            unsafe { w32(TIM3_CCR3, *ccr) };
            crate::clock::delay_us(200_000);
            // Static levels at both DAC rails must differ.
            self.set_dac(0);
            crate::clock::delay_us(50);
            let at0 = self.pa15();
            self.set_dac(4095);
            crate::clock::delay_us(50);
            let at4095 = self.pa15();
            if at0 == at4095 {
                self.set_dac(2048);
                return Err(CalError::NoComparator);
            }
            flip[i][0] = self.sweep(true).ok_or(CalError::NoFlip)?;
            flip[i][1] = self.sweep(false).ok_or(CalError::NoFlip)?;
        }
        self.set_dac(2048);
        // The sweeps flipped PA15 thousands of times: TIM5 counted them.
        self.freq.restart();
        let codes = [lo_code, hi_code];
        // The offset step (~450 codes) must move the flip by a few hundred
        // DAC codes (default slope 1.294 -> ~580).
        let d_up = flip[1][0] as f32 - flip[0][0] as f32;
        let d_dn = flip[1][1] as f32 - flip[0][1] as f32;
        if d_up.abs() < 100.0 || d_dn.abs() < 100.0 {
            return Err(CalError::OffsetNoEffect);
        }
        let dcode = hi_code - lo_code;
        if !(dcode.abs() > 50.0) {
            return Err(CalError::Slope);
        }
        let slope = (d_up + d_dn) * 0.5 / dcode;
        if !(0.8..=2.0).contains(&slope) {
            return Err(CalError::Slope);
        }
        // Per edge: rising uses the down-sweep flips, falling the up-sweep
        // flips. Re-express as {code at DAC 1780, code at DAC 2401}.
        let line = |f0: f32, f1: f32| -> (f32, f32) {
            let s = (f1 - f0) / dcode; // DAC per code
            let code_at = |d: f32| lo_code + (d - f0) / s;
            (code_at(DAC_LO), code_at(DAC_HI))
        };
        let map = [line(flip[0][1] as f32, flip[1][1] as f32), line(flip[0][0] as f32, flip[1][0] as f32)];
        let hyst = ((flip[0][0] as f32 - flip[0][1] as f32) + (flip[1][0] as f32 - flip[1][1] as f32)) * 0.5;
        let r = CalResult { flip, codes, map, hyst, slope };
        self.cal = Some(map);
        self.last_cal = Some(r);
        self.misses = 0;
        Ok(r)
    }

    /// Sweep DAC2 over the full range (step 1, ~4 us settle per step, about
    /// 16 ms) and return the first code where PA15 differs from its level at
    /// the sweep start and holds for 8 further codes.
    fn sweep(&mut self, up: bool) -> Option<u16> {
        let start = if up { 0u16 } else { 4095 };
        self.set_dac(start);
        crate::clock::delay_us(50);
        let base = self.pa15();
        let mut run = 0u8;
        let mut first = 0u16;
        for k in 0..4096u16 {
            let d = if up { k } else { 4095 - k };
            self.set_dac(d);
            crate::clock::delay_us(4);
            if self.pa15() != base {
                if run == 0 {
                    first = d;
                }
                run += 1;
                if run >= 8 {
                    return Some(first);
                }
            } else {
                run = 0;
            }
        }
        None
    }

    /// Debug: TIM4 SR, TIM4 CNT, TIM2 CCR1, (PA15 << 16 | DAC DOR2).
    pub fn debug(&self) -> [u32; 4] {
        unsafe { [r32(TIM4_SR), r32(TIM4_CNT), r32(TIM2_CCR1), ((self.pa15() as u32) << 16) | (r32(DAC_DOR2) & 0xFFF)] }
    }

    /// Debug: TIM5 CNT (edges), TIM2 CNT (now), TIM2 CCR1 (last edge).
    pub fn freq_regs(&self) -> [u32; 3] {
        unsafe { [r32(TIM5_CNT), r32(TIM2_CNT), r32(TIM2_CCR1)] }
    }

    /// Advance the frequency counter (call every main-loop pass while the
    /// scope front end is on).
    pub fn freq_poll(&mut self, now_ms: u64) {
        let (n, t, now) = read_edges();
        self.freq.update(n, t, now, now_ms);
    }
}

/// A consistent (edge count, time of that count's last edge, TIM2 now):
/// TIM5 read before and after CCR1 must agree (an edge between the reads
/// moves both). After four tries (edges denser than the reads, a few MHz):
/// the count with the current time, at most one period off over the gate.
fn read_edges() -> (u32, u32, u32) {
    unsafe {
        for _ in 0..4 {
            let a = r32(TIM5_CNT);
            let c = r32(TIM2_CCR1);
            let b = r32(TIM5_CNT);
            if a == b {
                return (a, c, r32(TIM2_CNT));
            }
        }
        let now = r32(TIM2_CNT);
        (r32(TIM5_CNT), now, now)
    }
}

/// Reciprocal frequency counter on the comparator edges: whole edge-to-edge
/// intervals of at least `GATE` TIM2 ticks, f = edges x TIM_CLK / ticks.
/// The resolution is one TIM2 tick (5 ns) per gate, the accuracy that of the
/// MCU crystal; it needs the signal to cross the trigger level (comparator
/// hysteresis and chatter on slow edges: not measured on the device).
pub struct FreqCounter {
    /// Gate start: edge count and its edge's TIM2 time; None until an edge
    /// is seen after `restart`.
    anchor: Option<(u32, u32)>,
    /// Edge count at `restart` (an anchor needs a newer edge: CCR1 is stale
    /// before it).
    seen: Option<u32>,
    /// Last result (Hz; NaN: none) and when it was taken (ms).
    f: f32,
    at_ms: u64,
}

/// Gate: 50 M ticks = 250 ms.
const GATE: u32 = TIM_CLK / 4;
/// Longest wait for a second edge: 10 s (TIM2 wraps after 21.5 s).
const MAX_TICKS: u32 = 10 * TIM_CLK;

impl FreqCounter {
    pub const fn new() -> Self {
        FreqCounter { anchor: None, seen: None, f: f32::NAN, at_ms: 0 }
    }

    /// Forget the gate and the result (new polarity, range, coupling, a
    /// front end coming up).
    pub fn restart(&mut self) {
        self.anchor = None;
        self.seen = None;
        self.f = f32::NAN;
    }

    /// One reading: `n` edges counted, the last one at TIM2 time `t`, TIM2
    /// now `now`.
    pub fn update(&mut self, n: u32, t: u32, now: u32, now_ms: u64) {
        let Some((n0, t0)) = self.anchor else {
            match self.seen {
                Some(s) if s != n => self.anchor = Some((n, t)),
                _ => self.seen = Some(n),
            }
            return;
        };
        let (dn, dt) = (n.wrapping_sub(n0), t.wrapping_sub(t0));
        if dn >= 1 && dt >= GATE {
            self.f = (dn as f64 * TIM_CLK as f64 / dt as f64) as f32;
            self.at_ms = now_ms;
            self.anchor = Some((n, t));
        } else if now.wrapping_sub(t0) > MAX_TICKS {
            // No second edge within 10 s: start over before TIM2 wraps.
            self.restart();
            self.seen = Some(n);
        }
    }

    /// The last result while fresh: within 1.5 s or three periods of it.
    pub fn hz(&self, now_ms: u64) -> Option<f32> {
        if !(self.f > 0.0) {
            return None;
        }
        let keep = (3000.0 / self.f).max(1500.0) as u64;
        if now_ms.saturating_sub(self.at_ms) <= keep { Some(self.f) } else { None }
    }
}

/// TIM4 prescaler and post count for a sample period of `period` core ticks
/// and `post` samples: the smallest m dividing `period` with period / m <=
/// 65536 (the 16-bit PSC); the count is post x m ticks of period / m, which
/// must stay below the 16-bit ARR. None: does not fit (below ~2.5 kS/s with a
/// long post part).
pub fn tim4_div(period: u32, post: u32) -> Option<(u32, u32)> {
    let mut m = period.div_ceil(65536).max(1);
    while m <= period && period % m != 0 {
        m += 1;
    }
    let ticks = post.checked_mul(m)?;
    if m > period || ticks > 0xFFFE { None } else { Some((period / m, ticks)) }
}

/// A {f0, f1} pair from the settings record if plausible (finite, within the
/// 10-bit code range, at least 10 codes apart), as `scope.rs::cal_pair`.
/// From the record's copy (scope.rs `rec_f32`): the record is in the slow
/// flash bank, and `available` / `map` run per frame, also while an AD2
/// stream runs.
fn rec_pair(base: u32, i: usize) -> Option<(f32, f32)> {
    let a = base - 0x080C_0000 + 8 * i as u32;
    let (p0, p1) = (crate::scope::rec_f32(a), crate::scope::rec_f32(a + 4));
    let ok = |v: f32| v.is_finite() && v > 0.0 && v < 1024.0;
    if ok(p0) && ok(p1) && (p0 - p1).abs() > 10.0 { Some((p0, p1)) } else { None }
}

/// Post-trigger samples to keep: the screen part right of the trigger
/// (`screen - pre`), the search slack and a little for the comparator
/// latency.
pub fn post_for(post_screen: usize, win: usize) -> u32 {
    (post_screen + win / 2 + 16) as u32
}

/// Samples that must be written before `arm`: the screen part left of the
/// trigger plus the search slack (mirrors `post_for`).
pub fn pre_for(pre_screen: usize, win: usize) -> usize {
    pre_screen + win / 2 + 16
}

/// Post-trigger samples for a triggered record of `rec` samples: armed once
/// `pre_for` samples are written, the stop `rec - pre_for` samples after the
/// edge leaves exactly `rec` samples with the edge `pre_for` from the start
/// (the stock post count `len - pre - trig_delay`); never less than
/// `post_for` (the screen right of the trigger and the search slack).
pub fn post_rec(rec: usize, pre_screen: usize, post_screen: usize, win: usize) -> u32 {
    (rec.saturating_sub(pre_for(pre_screen, win)) as u32).max(post_for(post_screen, win))
}
