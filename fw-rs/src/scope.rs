//! Oscilloscope: acquisition (scope_gen_v2_6.h) with the stock timebase,
//! vertical and offset maps (scope_gen_ui_v2_6.h).
//!
//! ADC 3PA1030 (10 bit) on PB0..PB9, clocked by TIM8 CH1 on PC6 at 50 MHz.
//! fs >= 50 MS/s: DMA1 stream 4 ch 6 (TIM1_CH4) GPIOB->IDR -> record;
//! fs <  50 MS/s: stream 3 ch 7 (TIM8_CH2) latches IDR at 50 MHz, stream 4 copies
//! the latch into the record at fs (as the stock firmware).
//! One record per frame; the trigger is found in software in the record
//! (screen/2 samples of pre-trigger, then a search window, then screen/2).
//! The analog front end is powered by PC4 (the stock raises it on the scope /
//! generator pages and lowers PE2, the DMM side; see rails()). While PC4 is
//! off poll() returns nothing (the scope is paused, the last frame stays).

use crate::hwtrig::{self, HwTrig, Status};
use crate::mmio::{modify32, r32, w32};

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const RCU_APB2EN: u32 = 0x4002_3844;
const GPIOB: u32 = 0x4002_0400;
const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const GPIOE: u32 = 0x4002_1000;
const GPIOB_IDR: u32 = GPIOB + 0x10;
const TIM1: u32 = 0x4001_0000;
const TIM8: u32 = 0x4001_0400;
const TIM3: u32 = 0x4000_0400;
const DMA1: u32 = 0x4002_6400;
const S3: u32 = DMA1 + 0x10 + 0x18 * 3;
const S4: u32 = DMA1 + 0x10 + 0x18 * 4;
const LIFCR: u32 = DMA1 + 0x08;
const HIFCR: u32 = DMA1 + 0x0C;

/// Settings record (read only): scope calibration pairs.
const REC: u32 = 0x080C_0000;

/// Screen = 400 columns, 10 divisions; graph height 280 rows (8 x 35).
pub const COLS: usize = 400;
pub const ROWS: i32 = 280;
/// Trigger search window (samples), as the stock software refinement.
const WIN: usize = 800;
/// Record buffer (samples) in SRAM0.
pub const CAP: usize = 26 * 1024;

#[link_section = ".dmabuf"]
static mut BUF: [u16; CAP] = [0; CAP];
#[link_section = ".dmabuf"]
static mut LATCH: [u32; 2] = [0; 2];

/// Hardware timer setups (flash 0x08049a54): fs, tim1 psc, tim1 arr, ccr4.
const HW: [(u32, u32, u32, u32); 31] = [
    (50_000_000, 0, 3, 2),
    (25_000_000, 0, 7, 4),
    (12_500_000, 0, 15, 8),
    (10_000_000, 0, 19, 10),
    (6_250_000, 0, 31, 16),
    (5_000_000, 0, 39, 20),
    (2_500_000, 0, 79, 40),
    (1_250_000, 0, 159, 80),
    (1_000_000, 0, 199, 100),
    (625_000, 0, 319, 160),
    (500_000, 0, 399, 200),
    (250_000, 0, 799, 400),
    (125_000, 0, 1599, 800),
    (100_000, 0, 1999, 1000),
    (62_500, 0, 3199, 1600),
    (50_000, 0, 3999, 2000),
    (25_000, 0, 7999, 4000),
    (12_500, 0, 15999, 8000),
    (10_000, 0, 19999, 10000),
    (6_250, 0, 31999, 16000),
    (5_000, 1, 19999, 10000),
    (2_500, 3, 19999, 10000),
    (1_250, 7, 19999, 10000),
    (1_000, 9, 19999, 10000),
    (625, 15, 19999, 10000),
    (500, 19, 19999, 10000),
    (250, 39, 19999, 10000),
    (125, 79, 19999, 10000),
    (100, 99, 19999, 10000),
    (50, 199, 19999, 10000),
    (25, 399, 19999, 10000),
];

/// Timebases (s/div) and the stock Auto entries {fs, num, den, ppp}: each
/// column covers num/den points of the ppp-times upsampled stream.
pub const SDIV: [f32; 30] = [
    10e-9, 20e-9, 50e-9, 100e-9, 200e-9, 500e-9, 1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6, 100e-6, 200e-6, 500e-6,
    1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0,
];
const TB: [(u32, u32, u32, u32); 30] = [
    (200_000_000, 1, 1, 20),
    (200_000_000, 1, 1, 10),
    (200_000_000, 1, 1, 4),
    (50_000_000, 1, 1, 8),
    (50_000_000, 1, 1, 4),
    (50_000_000, 5, 4, 2),
    (50_000_000, 5, 4, 1),
    (50_000_000, 5, 2, 1),
    (50_000_000, 25, 4, 1),
    (50_000_000, 25, 2, 1),
    (50_000_000, 25, 1, 1),
    (50_000_000, 125, 2, 1),
    // From 100 us/div: fs halved, 2500 samples/div (stock Normal table), so
    // a screen + trigger window fits the 26 K record.
    (25_000_000, 125, 2, 1),
    (12_500_000, 125, 2, 1),
    (5_000_000, 125, 2, 1),
    (2_500_000, 125, 2, 1),
    (1_250_000, 125, 2, 1),
    (500_000, 125, 2, 1),
    (250_000, 125, 2, 1),
    (125_000, 125, 2, 1),
    (50_000, 125, 2, 1),
    (25_000, 125, 2, 1),
    (12_500, 125, 2, 1),
    (5_000, 125, 2, 1),
    (2_500, 125, 2, 1),
    (1_250, 125, 2, 1),
    (500, 125, 2, 1),
    (250, 125, 2, 1),
    (125, 125, 2, 1),
    (50, 125, 2, 1),
];
/// User timebase range. Stock `SCOPE_TB_USER_MIN`/`MAX` = 3..29 (100 ns/div ..
/// 50 s/div, `scope_gen_ui_v2_6.h:522`/`523`; label list `scope_sdiv[30]`
/// `:494`). fw-rs now matches the full range: 3..29. In YT (non-roll) mode the
/// stock caps the index at 24 (1 s/div, `SCOPE_TB_YT_MAX`, `:524`); 22..29
/// (200 ms .. 50 s/div) run in roll mode when Roll is on.
pub const TB_MIN: usize = 3;
pub const TB_MAX: usize = 29;
/// Highest index usable in YT (non-roll) mode: 1 s/div (stock `SCOPE_TB_YT_MAX`).
pub const TB_YT_MAX: usize = 24;

/// V/div of hardware range r (x1 probe), front-end GPIO row {PC13, PB15, PD12,
/// PD13}, calibration voltage X_r and default gain point.
pub const VDIV: [f32; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];
const GPIO_ROW: [[u8; 4]; 10] = [
    [1, 1, 1, 0],
    [1, 1, 1, 1],
    [1, 0, 1, 1],
    [1, 0, 0, 1],
    [1, 0, 1, 0],
    [0, 1, 1, 0],
    [0, 1, 1, 1],
    [0, 1, 0, 1],
    [0, 0, 0, 1],
    [0, 0, 1, 0],
];
const XR: [f32; 10] = [0.04, 0.04, 0.04, 0.4, 0.4, 0.4, 4.0, 4.0, 4.0, 4.0];
const G_DEFAULT: [f32; 10] = [944.0, 727.0, 598.0, 944.0, 727.0, 598.0, 944.0, 727.0, 598.0, 556.0];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrigMode {
    Auto,
    Normal,
    Single,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub run: bool,
    pub tb: usize,
    pub range: usize,
    pub ac: bool,
    /// Trigger level and zero position in screen rows (0..280, 140 = centre).
    pub trig_row: i32,
    pub pos: i32,
    pub rising: bool,
    pub mode: TrigMode,
    /// Probe factor (x1, x10, x100): scales readouts and V/div labels only.
    pub probe: f32,
    /// Trigger position: screen column (0..=400) of the trigger point, 200 =
    /// centre (stock: always the centre). Sets the pre-trigger part of each
    /// record; ignored in roll mode.
    pub hpos: i32,
}

impl Settings {
    /// Stock defaults: 200 us/div, 1 V/div, DC, rising, Auto, level row 105.
    pub const DEFAULT: Settings =
        Settings { run: true, tb: 13, range: 6, ac: false, trig_row: 105, pos: 0, rising: true, mode: TrigMode::Auto, probe: 1.0, hpos: 200 };
}

/// A finished frame: the visible window of the record, or (roll mode, empty
/// `data`) per-column min/max codes in `cols` (u16::MAX = no data yet).
pub struct Frame<'a> {
    pub data: &'a [u16],
    pub cols: Option<&'a [(u16, u16); COLS]>,
    pub trig: Option<usize>,
    pub fs: u32,
    pub seq: u32,
}

fn cal_pair(off: u32, i: usize, dflt: (f32, f32)) -> (f32, f32) {
    let a = REC + off + 8 * i as u32;
    let rd = |x: u32| unsafe { core::ptr::read_volatile(x as *const f32) };
    let (p0, p1) = (rd(a), rd(a + 4));
    let ok = |v: f32| v.is_finite() && v > 0.0 && v < 1024.0;
    if ok(p0) && ok(p1) && (p0 - p1).abs() > 10.0 { (p0, p1) } else { dflt }
}

fn pin(port: u32, n: u32, high: bool) {
    unsafe { w32(port + 0x18, if high { 1 << n } else { 1 << (n + 16) }) };
}

fn out(port: u32, n: u32) {
    unsafe {
        modify32(port + 0x0C, 3 << (2 * n), 0);
        modify32(port + 0x08, 0, 3 << (2 * n));
        modify32(port, 3 << (2 * n), 1 << (2 * n));
    }
}

#[link_section = ".dmabuf"]
static mut ROLL_COLS: [(u16, u16); COLS] = [(0, 0); COLS];

/// Roll mode (stock: from 200 ms/div): continuous circular capture.
pub const TB_ROLL: usize = 22;

pub struct Scope {
    pub s: Settings,
    /// Roll display enabled (used from TB_ROLL up).
    pub roll: bool,
    /// DMM rail (PE2) on; `powered` is the scope rail (PC4).
    dmm_on: bool,
    rolling: bool,
    roll_written: u64,
    roll_last_wr: usize,
    roll_next: u64,
    busy: bool,
    deadline: u64,
    seq: u32,
    relay_until: u64,
    last_pc13: u8,
    powered: bool,
    screen: usize,
    rec: usize,
    // Stop-and-zoom: the last captured record, kept in BUF while stopped so a
    // timebase change re-renders it at the new s/div (stock §3 "Zoom", state 6).
    stored_valid: bool,
    stored_len: usize,
    stored_trig: Option<usize>,
    stored_anchor: usize,
    stored_fs: u32,
    /// Hardware range the stored codes were captured with (the code -> volts
    /// gain); the current range only sets the display scale while stopped.
    stored_range: usize,
    /// Stored record came from roll mode: newest sample at the right edge.
    stored_roll: bool,
    /// Emit one re-render of the stored record on the next poll (set on any
    /// settings change while stopped and by `refresh`); no periodic re-render.
    zoom_pending: bool,
    /// Comparator trigger engine (hwtrig.rs): DAC2 level, TIM2 post count
    /// gating TIM1. `hw_mode`: the acquisition in flight uses it (circular
    /// record over CAP); `hw_t0`: its start time (health check).
    pub hw: HwTrig,
    hw_mode: bool,
    hw_t0: u64,
}

impl Scope {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_AHB1EN, 0, (1 << 22) | 0x1F);
            modify32(RCU_APB2EN, 0, 0b11);
            modify32(RCU_APB1EN, 0, 1 << 1);
            // PB0..PB9 inputs with pull-down.
            modify32(GPIOB, 0xF_FFFF, 0);
            modify32(GPIOB + 0x0C, 0xF_FFFF, 0xA_AAAA);
            // PC6: TIM8_CH1 (AF3). PC8: TIM3_CH3 (AF2) offset PWM.
            modify32(GPIOC + 0x20, 0xF << 24, 3 << 24);
            modify32(GPIOC + 0x08, 3 << 12, 3 << 12);
            modify32(GPIOC, 3 << 12, 2 << 12);
            modify32(GPIOC + 0x24, 0xF, 2);
            modify32(GPIOC, 3 << 16, 2 << 16);
            w32(TIM3 + 0x28, 0);
            w32(TIM3 + 0x2C, 20000);
            w32(TIM3 + 0x1C, (6 << 4) | (1 << 3));
            w32(TIM3 + 0x20, 1 << 8);
            w32(TIM3 + 0x14, 1);
            w32(TIM3, 1 | (1 << 7));
        }
        // Range / coupling GPIOs; PC4 / PE2 keep their current levels.
        for (p, n) in [(GPIOC, 13), (GPIOB, 15), (GPIOD, 12), (GPIOD, 13), (GPIOE, 3)] {
            out(p, n);
        }
        let mut sc = Scope {
            s: Settings::DEFAULT,
            busy: false,
            deadline: 0,
            seq: 0,
            relay_until: 0,
            last_pc13: 2,
            // Unknown levels after the stock / boot: claim "scope on, DMM off"
            // so the first rails() call drives both pins to a known state.
            powered: true,
            screen: 0,
            rec: 0,
            roll: true,
            dmm_on: false,
            rolling: false,
            roll_written: 0,
            roll_last_wr: 0,
            roll_next: 0,
            stored_valid: false,
            stored_len: 0,
            stored_trig: None,
            stored_anchor: 0,
            stored_fs: 0,
            stored_range: 0,
            stored_roll: false,
            zoom_pending: false,
            hw: HwTrig::new(),
            hw_mode: false,
            hw_t0: 0,
        };
        sc.hw.init();
        sc.apply(Settings::DEFAULT, 0);
        sc
    }

    /// Analog rails: DMM side PE2, scope / generator side PC4 (the stock
    /// raises PC4 on its scope / generator pages and PE2 on the DMM page, never
    /// both). A rail that comes up does so 10 ms before one goes down, as the
    /// stock page switch; at least one rail stays on. Cheap when nothing
    /// changes (called every tick by App::update_rails).
    pub fn rails(&mut self, dmm: bool, scope: bool, clk: &mut crate::clock::Clock) {
        let dmm = dmm || !scope;
        if dmm == self.dmm_on && scope == self.powered {
            return;
        }
        if scope && !self.powered {
            pin(GPIOC, 4, true);
            out(GPIOC, 4);
            self.relay_until = clk.now_ms() + 200;
        }
        if dmm && !self.dmm_on {
            pin(GPIOE, 2, true);
            out(GPIOE, 2);
        }
        if (!scope && self.powered) || (!dmm && self.dmm_on) {
            clk.delay_ms(10);
        }
        if !scope && self.powered {
            self.halt();
            pin(GPIOC, 4, false);
            out(GPIOC, 4);
        }
        if !dmm && self.dmm_on {
            pin(GPIOE, 2, false);
            out(GPIOE, 2);
        }
        self.powered = scope;
        self.dmm_on = dmm;
    }

    /// DMM rail (PE2) on.
    pub fn dmm_rail(&self) -> bool {
        self.dmm_on
    }

    pub fn powered(&self) -> bool {
        self.powered
    }

    fn gain(&self, r: usize) -> (f32, f32) {
        cal_pair(0x08, r, (G_DEFAULT[r], 512.0))
    }

    /// Hardware range of the codes currently being served: the capture range
    /// of the stored record while stopped, else the live range. A V/div change
    /// while stopped then rescales the old codes with the gain they were taken
    /// at (a digital vertical zoom) instead of re-mapping them with the new
    /// range's gain. Right after STOP the two are equal, so the re-render maps
    /// every code to the same row as the live frame did.
    fn code_range(&self) -> usize {
        if !self.s.run && self.stored_valid { self.stored_range } else { self.s.range }
    }

    /// ADC code -> volts (FUN_080301a0, without the position). The probe is the
    /// current one: it only relabels (stock: "P scales the sample map and the
    /// displayed V/div only"), so a probe change while stopped relabels the
    /// stored trace without moving it.
    pub fn volts(&self, code: f32) -> f32 {
        let r = self.code_range();
        let (g0, g1) = self.gain(r);
        (code - g1) * XR[r] / (g0 - g1) * self.s.probe
    }

    /// Volts -> screen row (FUN_08030146): 140 + pos - 141 v / (4 V/div).
    pub fn row_of_volts(&self, v: f32) -> f32 {
        140.0 + self.s.pos as f32 - 141.0 * v / (4.0 * VDIV[self.s.range] * self.s.probe)
    }

    pub fn row_of_code(&self, code: f32) -> f32 {
        self.row_of_volts(self.volts(code))
    }

    pub fn trig_volts(&self) -> f32 {
        (140.0 + self.s.pos as f32 - self.s.trig_row as f32) * 4.0 * VDIV[self.s.range] * self.s.probe / 141.0
    }

    fn trig_code(&self) -> i32 {
        let r = self.s.range;
        let (g0, g1) = self.gain(r);
        (g1 + self.trig_volts() / self.s.probe * (g0 - g1) / XR[r]) as i32
    }

    pub fn apply(&mut self, s: Settings, now: u64) {
        // A user STOP (run -> stop) freezes what is on screen; any other change
        // just aborts the acquisition in flight (it restarts in poll()).
        if self.s.run && !s.run {
            self.freeze(now);
        } else {
            self.halt();
        }
        let mut s = s;
        // Roll on: full 3..29 (22.. rolls). Roll off: YT only, capped at 24
        // (1 s/div). Single never rolls (one linear record), so it is capped at
        // the YT maximum too: at 2..50 s/div a full linear record would take
        // 21..532 s with nothing drawn until it completes.
        let top = if self.roll && s.mode != TrigMode::Single { TB_MAX } else { TB_YT_MAX };
        s.tb = s.tb.clamp(TB_MIN, top);
        // Stop-and-zoom: while stopped any change re-renders the stored record
        // at the new settings. The timebase is never clamped to the captured
        // extent: zooming out past it shows the record at the true scale with
        // empty columns around it (zoom_frame), so neither the STOP press nor a
        // later change silently picks a different s/div.
        if !s.run && self.stored_valid {
            self.zoom_pending = true;
        }
        s.range = s.range.min(9);
        s.trig_row = s.trig_row.clamp(-10, 290);
        s.pos = s.pos.clamp(-392, 392);
        s.hpos = s.hpos.clamp(0, COLS as i32);
        self.s = s;
        let g = GPIO_ROW[s.range];
        if g[0] != self.last_pc13 {
            self.last_pc13 = g[0];
            self.relay_until = now + 200;
        }
        pin(GPIOC, 13, g[0] != 0);
        pin(GPIOB, 15, g[1] != 0);
        pin(GPIOD, 12, g[2] != 0);
        pin(GPIOD, 13, g[3] != 0);
        pin(GPIOE, 3, !s.ac);
        // Offset PWM (FUN_080302d6): ADC code of 0 V at this position.
        let r = s.range;
        let (g0, g1) = self.gain(r);
        let code_t = g1 - s.pos as f32 * (4.0 * VDIV[r] / XR[r]) * (g0 - g1) / 141.0;
        let (o0, o1) = cal_pair(0xA8, r, (730.0, 280.0));
        let ccr = 8800.0 + (code_t - o1) * (11636.0 - 8800.0) / (o0 - o1);
        let ccr = if ccr.is_finite() && ccr > 0.0 { ccr.min(20000.0) as u32 } else { 0 };
        unsafe { w32(TIM3 + 0x3C, ccr) };
        let (_, num, den, ppp) = TB[s.tb];
        self.screen = ((num * COLS as u32 / den / ppp) as usize).max(2);
        // Single captures the full record (stock Single = 65520 samples) so the
        // stored-record zoom can pan/zoom out; Auto/Normal keep one screen + the
        // trigger window per frame.
        self.rec = if s.mode == TrigMode::Single { CAP } else { (self.screen + WIN).min(CAP) };
        self.deadline = 0;
    }

    /// Stop the acquisition hardware (timers + DMA). Leaves BUF and the stored
    /// record state alone.
    fn halt(&mut self) {
        unsafe {
            modify32(TIM1, 1, 0);
            modify32(TIM8, 1, 0);
            w32(S4, 0);
            w32(S3, 0);
            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.hw.disarm();
        self.hw_mode = false;
        self.busy = false;
        self.rolling = false;
    }

    /// User STOP: keep the frame the user is looking at.
    ///  * Roll: the roll display is the newest data in BUF, so linearise it.
    ///  * Nothing in flight: the stored record is the last frame poll() showed
    ///    (or nothing, if poll() discarded the last record).
    ///  * A linear acquisition in flight has already overwritten (part of) the
    ///    displayed frame. If it is complete, treat it exactly as poll() would:
    ///    trigger search and the Auto/Normal/Single show rule; a frame poll()
    ///    would have shown becomes the stored record and is drawn. Otherwise
    ///    (partial, or a record poll() would have discarded) there is no record
    ///    to zoom: the screen stays frozen as drawn, as the stock STOP does.
    fn freeze(&mut self, now: u64) {
        let was_busy = self.busy;
        let was_rolling = self.rolling;
        let was_hw = self.hw_mode;
        let ndtr = unsafe { r32(S4 + 0x04) };
        self.halt();
        if !was_busy {
            return;
        }
        self.stored_valid = false;
        if was_rolling {
            self.snapshot_roll();
        } else if was_hw {
            // Circular hardware-trigger record: only a frozen (Done) one is
            // a complete frame; poll() would have shown it on the next tick,
            // so leave the screen as drawn (as a partial linear record).
        } else if ndtr == 0 {
            if self.finish(now).is_some() {
                self.zoom_pending = true;
            }
        }
    }

    /// Mask BUF[0..len] to 10 bits and mark it as the stored record, captured
    /// with the current settings (fs, range).
    fn snapshot_mark(&mut self, len: usize, anchor: usize, trig: Option<usize>, roll: bool) {
        if len < 2 {
            self.stored_valid = false;
            return;
        }
        let buf = unsafe { &mut (&mut *core::ptr::addr_of_mut!(BUF))[..len] };
        for v in buf.iter_mut() {
            *v &= 0x3FF;
        }
        self.stored_len = len;
        self.stored_fs = self.sample_rate();
        self.stored_range = self.s.range;
        self.stored_roll = roll;
        self.stored_anchor = anchor.min(len - 1);
        self.stored_trig = trig;
        self.stored_valid = true;
    }

    /// A complete linear record (NDTR = 0, hardware halted): trigger search and
    /// the stock show rule. Returns the start of the visible window and the
    /// trigger when the frame is to be shown, and makes it the stored record
    /// (anchored on the on-screen centre); a discarded record (Normal / Single
    /// without trigger, Auto before its timeout) invalidates the stored record,
    /// since its samples have replaced the frame that was shown before.
    fn finish(&mut self, now: u64) -> Option<(usize, Option<usize>)> {
        let n = self.rec;
        // Pre-trigger samples: the trigger point lands on column hpos. The
        // search window starts there, so start = trig - pre >= 0 and
        // start + screen <= pre + win + screen <= n.
        let pre = self.screen * self.s.hpos.clamp(0, COLS as i32) as usize / COLS;
        let win = WIN.min(n - self.screen);
        // Mask once here (snapshot_mark), then search in the masked record.
        self.snapshot_mark(n, 0, None, false);
        let data: &[u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..n] };
        let trig = self.find_trigger(&data[pre..pre + win]).map(|i| i + pre);
        let show = match (trig, self.s.mode) {
            (Some(_), _) => true,
            (None, TrigMode::Auto) => now >= self.deadline,
            (None, _) => false,
        };
        if !show {
            self.stored_valid = false;
            return None;
        }
        let start = trig.map(|t| t - pre).unwrap_or(win / 2);
        if self.stored_valid {
            self.stored_trig = trig;
            self.stored_anchor = (start + self.screen / 2).min(self.stored_len - 1);
        }
        Some((start, trig))
    }

    /// Roll record: linearise the circular buffer so the stored record runs
    /// oldest -> newest, anchored at the newest sample (the right edge the user
    /// was watching). Matches the stock 65520-sample roll record (§3), capped at
    /// the fw-rs 26 K capacity.
    fn snapshot_roll(&mut self) {
        let wr = (CAP - unsafe { r32(S4 + 0x04) } as usize % (CAP + 1)) % CAP;
        let d = (wr + CAP - self.roll_last_wr) % CAP;
        self.roll_written += d as u64;
        self.roll_last_wr = wr;
        let len = if self.roll_written >= CAP as u64 {
            unsafe { (&mut (&mut *core::ptr::addr_of_mut!(BUF))[..]).rotate_left(wr) };
            CAP
        } else {
            wr.min(CAP)
        };
        self.snapshot_mark(len, len.saturating_sub(1), None, true);
    }

    fn start(&mut self) {
        let fs = TB[self.s.tb].0;
        let (_, psc, arr, ccr4) = *HW.iter().find(|h| h.0 == fs).unwrap_or(&HW[0]);
        let buf = core::ptr::addr_of!(BUF) as u32;
        unsafe {
            w32(LIFCR, 0x3D << 22);
            w32(HIFCR, 0x3D);
            w32(TIM8, 0);
            w32(TIM8 + 0x28, 0);
            w32(TIM8 + 0x2C, 3);
            w32(TIM8 + 0x34, 2);
            w32(TIM8 + 0x38, 0);
            w32(TIM8 + 0x18, 6 << 4);
            w32(TIM8 + 0x20, 1);
            w32(TIM8 + 0x44, 1 << 15);
            w32(TIM1, 0);
            w32(TIM1 + 0x28, psc);
            w32(TIM1 + 0x2C, arr);
            w32(TIM1 + 0x40, ccr4);
            w32(TIM1 + 0x1C, 6 << 12);
            w32(TIM1 + 0x20, 1 << 12);
            w32(TIM1 + 0x44, 1 << 15);
            w32(TIM1 + 0x0C, 1 << 12);
            w32(TIM1 + 0x14, 1);
            let src = if fs >= 50_000_000 {
                w32(TIM8 + 0x0C, 0);
                GPIOB_IDR
            } else {
                w32(TIM8 + 0x0C, 1 << 10);
                w32(S3 + 0x08, GPIOB_IDR);
                w32(S3 + 0x0C, core::ptr::addr_of!(LATCH) as u32);
                w32(S3 + 0x04, 2);
                w32(S3, (7 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 8) | 1);
                core::ptr::addr_of!(LATCH) as u32
            };
            w32(S4 + 0x08, src);
            w32(S4 + 0x0C, buf);
            // Roll from 200 ms/div (TB_ROLL) when Roll is on, except in Single,
            // which captures a full linear record like the stock Single mode.
            self.rolling = self.roll && self.s.tb >= TB_ROLL && self.s.mode != TrigMode::Single;
            // BUF is about to be overwritten: the previous stored record is gone
            // until this acquisition completes (or is snapshotted in stop()).
            self.stored_valid = false;
            // Comparator trigger: circular record over CAP, frozen in hardware
            // `post` samples after the edge (hwtrig.rs). Roll never triggers.
            let hw = self.hw.available() && !self.rolling;
            let (len, circ) = if self.rolling || hw { (CAP as u32, 1 << 8) } else { (self.rec as u32, 0) };
            self.roll_written = 0;
            self.roll_last_wr = 0;
            w32(S4 + 0x04, len);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | circ | 1);
            if hw {
                let (pre, _) = self.hw_split();
                let code = self.trig_code() as f32;
                self.hw.prepare(psc, arr, hwtrig::post_for(self.screen - pre, WIN), code, self.s.rising);
            }
            self.hw_mode = hw;
            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
    }

    /// Screen samples left / right of the trigger point (hpos column).
    fn hw_split(&self) -> (usize, usize) {
        let pre = self.screen * self.s.hpos.clamp(0, COLS as i32) as usize / COLS;
        (pre, self.screen - pre)
    }

    /// Linearise the circular record after a hardware stop: with the ring
    /// wrapped (TCIF4) every sample is valid, rotate so it runs oldest ->
    /// newest; otherwise samples 0..wr are the record. Returns its length.
    fn hw_linearise(&mut self, wr: usize, wrapped: bool) -> usize {
        if wrapped {
            unsafe { (&mut (&mut *core::ptr::addr_of_mut!(BUF))[..]).rotate_left(wr) };
            CAP
        } else {
            wr
        }
    }

    /// Hardware-trigger acquisition in flight: arm after the pre-trigger part,
    /// then wait for TIM2 to gate TIM1 off (Done) or for the Auto timeout.
    fn hw_poll(&mut self, now: u64) -> Option<Frame<'static>> {
        let wr = CAP - (unsafe { r32(S4 + 0x04) } as usize).min(CAP);
        // HISR TCIF4: the ring has wrapped at least once.
        let wrapped = unsafe { r32(DMA1 + 0x04) } & (1 << 5) != 0;
        let st = self.hw.status();
        // Health: nothing written 100 ms after the start means the TIM1 gate
        // is stuck: give up on the comparator, the software path restarts.
        if wr == 0 && !wrapped && now >= self.hw_t0 + 100 {
            self.halt();
            self.hw.set_enabled(false);
            return None;
        }
        let (pre, post) = self.hw_split();
        match st {
            Status::Off => None,
            Status::Prepared => {
                if wrapped || wr >= hwtrig::pre_for(pre, WIN) {
                    self.hw.arm();
                }
                None
            }
            Status::Armed | Status::Fired => {
                // Timeout without an edge (Auto: the stock timeout; Normal /
                // Single: the same period, as a comparator health check): stop
                // and look for a crossing in software. One found while the
                // comparator stayed quiet is a miss (no comparator on this
                // board, or the level map is off); three in a row hand the
                // scope back to the software trigger (hwtrig::note_miss).
                if st != Status::Armed || now < self.deadline || !(wrapped || wr >= self.screen) {
                    return None;
                }
                self.halt();
                // Re-read after the stop: the DMA was still running above.
                let wr = CAP - (unsafe { r32(S4 + 0x04) } as usize).min(CAP);
                let wrapped = unsafe { r32(DMA1 + 0x04) } & (1 << 5) != 0;
                let len = self.hw_linearise(wr, wrapped);
                if len < self.screen {
                    self.stored_valid = false;
                    return None;
                }
                self.snapshot_mark(len, len - self.screen / 2, None, false);
                let data: &[u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..len] };
                let found = if len - post > pre { self.find_trigger(&data[pre..len - post]).map(|i| i + pre) } else { None };
                if found.is_some() {
                    self.hw.note_miss();
                }
                let start = match (found, self.s.mode) {
                    (Some(t), _) => t - pre,
                    // Auto: the newest screen, untriggered (stock TIM5 UIF
                    // "record done without trigger").
                    (None, TrigMode::Auto) => len - self.screen,
                    (None, _) => {
                        self.stored_valid = false;
                        return None;
                    }
                };
                self.stored_trig = found;
                self.stored_anchor = start + pre;
                self.deadline = 0;
                if found.is_some() && self.s.mode == TrigMode::Single {
                    self.s.run = false;
                }
                let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[start..start + self.screen] };
                self.seq = self.seq.wrapping_add(1);
                Some(Frame { data, cols: None, trig: found, fs: self.sample_rate(), seq: self.seq })
            }
            Status::Done => {
                self.halt();
                let len = self.hw_linearise(wr, wrapped);
                let post_n = hwtrig::post_for(post, WIN) as usize;
                if len < self.screen + WIN || len < post_n + pre {
                    // Cannot happen after `arm` (pre + post samples written),
                    // kept as a guard against a stale TCIF.
                    self.stored_valid = false;
                    return None;
                }
                // Hardware estimate: the edge is post_n samples before the
                // newest sample; the stock software search refines it.
                let th = len - post_n;
                let lo = th.saturating_sub(WIN / 2).max(pre);
                let hi = (th + WIN / 2).min(len - post);
                self.snapshot_mark(len, th, None, false);
                let data: &[u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..len] };
                let found = if lo < hi { self.find_trigger(&data[lo..hi]).map(|i| i + lo) } else { None };
                let t = match found {
                    Some(t) => {
                        self.hw.note_hit(t as i32 - th as i32);
                        t
                    }
                    None => {
                        // Fired, but no crossing near the estimate: the map
                        // is off. Auto shows the frame at the estimate;
                        // Normal / Single wait for the next one.
                        self.hw.note_miss();
                        if self.s.mode != TrigMode::Auto {
                            self.stored_valid = false;
                            return None;
                        }
                        th.clamp(pre, len - post)
                    }
                };
                self.stored_trig = found;
                self.stored_anchor = t;
                self.deadline = 0;
                if found.is_some() && self.s.mode == TrigMode::Single {
                    self.s.run = false;
                }
                let start = t - pre;
                let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[start..start + self.screen] };
                self.seq = self.seq.wrapping_add(1);
                Some(Frame { data, cols: None, trig: found, fs: self.sample_rate(), seq: self.seq })
            }
        }
    }

    /// Wire-free comparator calibration (hwtrig::calibrate) at range 6 / DC
    /// with the scope's own offset points; the current settings are restored
    /// (acquisition restarts from poll). Needs PC4 on and nothing on the MCX.
    pub fn hw_calibrate(&mut self, now: u64, clk: &mut crate::clock::Clock) -> Result<hwtrig::CalResult, hwtrig::CalError> {
        if !self.powered {
            return Err(hwtrig::CalError::NoComparator);
        }
        let saved = self.s;
        let mut s = saved;
        s.range = 6;
        s.ac = false;
        self.apply(s, now);
        // Relay settle (PC13 changes when coming from r0..4) + rail.
        clk.delay_ms(250);
        let r = self.hw.calibrate(6, None);
        self.apply(saved, now);
        r
    }

    pub fn sample_rate(&self) -> u32 {
        TB[self.s.tb].0
    }

    /// Display points per sample (> 1 below 1 us/div).
    pub fn ppp(&self) -> u32 {
        TB[self.s.tb].3
    }

    /// Zoom anchor of the stored record (sample index at the screen centre),
    /// None while running or without a stored record. Taken at touch-down so
    /// a pan is relative to it (`pan_from`).
    pub fn anchor(&self) -> Option<usize> {
        if self.s.run || !self.stored_valid || self.stored_len < 2 {
            return None;
        }
        Some(if self.stored_roll {
            // Roll record: newest sample at the right edge.
            (self.stored_len as u64).saturating_sub(self.zoom_screen(self.s.tb) / 2) as usize
        } else {
            self.stored_anchor
        })
    }

    /// Stopped: show the stored record panned by `cols` screen columns from
    /// anchor `a0` (positive = the trace follows the finger to the right, so
    /// earlier samples come into view); re-rendered on the next poll.
    pub fn pan_from(&mut self, a0: usize, cols: i32) {
        if self.anchor().is_none() {
            return;
        }
        let per = self.zoom_screen(self.s.tb) as i64;
        let a = a0 as i64 - cols as i64 * per / COLS as i64;
        self.stored_anchor = a.clamp(0, self.stored_len as i64 - 1) as usize;
        self.stored_roll = false;
        self.zoom_pending = true;
    }

    /// Ask for one re-render of the stored record while stopped (a scope view
    /// was (re)drawn, or its trace cache was invalidated). No-op while running.
    pub fn refresh(&mut self) {
        if !self.s.run && self.stored_valid {
            self.zoom_pending = true;
        }
    }

    /// Poll: start / finish frames. Returns the visible window when ready.
    pub fn poll(&mut self, now: u64) -> Option<Frame<'static>> {
        if !self.s.run {
            // Stopped: re-render the stored record (stock STOP "keeps redrawing
            // the stored record", §4) once per change (zoom_pending: a settings
            // change, or `refresh` from a view redraw). Nothing is recomputed
            // while nothing changes, whatever view is shown.
            if self.stored_valid && self.zoom_pending {
                self.zoom_pending = false;
                return self.zoom_frame();
            }
            return None;
        }
        if !self.powered || now < self.relay_until {
            return None;
        }
        if !self.busy {
            self.start();
            self.hw_t0 = now;
            if self.deadline == 0 {
                // Auto: untriggered frame after the stock timeout.
                let t = match self.s.tb {
                    0..=15 => 100,
                    16 => 200,
                    17 => 500,
                    18 => 1000,
                    _ => 2000,
                };
                self.deadline = now + t;
            }
            return None;
        }
        if self.rolling {
            return self.roll_poll(now);
        }
        if self.hw_mode {
            return self.hw_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
            return None;
        }
        self.halt();
        // The shown frame becomes the stored record (trigger + on-screen centre
        // remembered so a later zoom stays on this feature).
        let (start, trig) = self.finish(now)?;
        self.deadline = 0;
        if trig.is_some() && self.s.mode == TrigMode::Single {
            self.s.run = false;
        }
        let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..self.rec] };
        self.seq = self.seq.wrapping_add(1);
        Some(Frame { data: &data[start..start + self.screen], cols: None, trig, fs: self.sample_rate(), seq: self.seq })
    }

    /// Roll: every 100 ms, min/max of 400 columns over the latest screen.
    fn roll_poll(&mut self, now: u64) -> Option<Frame<'static>> {
        if now < self.roll_next {
            return None;
        }
        self.roll_next = now + 100;
        let wr = CAP - unsafe { r32(S4 + 0x04) } as usize % (CAP + 1);
        let wr = wr % CAP;
        let d = (wr + CAP - self.roll_last_wr) % CAP;
        self.roll_written += d as u64;
        self.roll_last_wr = wr;
        let per = (self.screen / COLS).max(1);
        let buf = unsafe { &*core::ptr::addr_of!(BUF) };
        let cols = unsafe { &mut *core::ptr::addr_of_mut!(ROLL_COLS) };
        for i in 0..COLS {
            // Column i ends (COLS - 1 - i) columns before the write position.
            let back = (COLS - i) * per;
            if back as u64 > self.roll_written {
                cols[i] = (u16::MAX, u16::MAX);
                continue;
            }
            let start = (wr + CAP - back) % CAP;
            let (mut lo, mut hi) = (u16::MAX, 0u16);
            for k in 0..per {
                let v = buf[(start + k) % CAP] & 0x3FF;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            cols[i] = (lo, hi);
        }
        self.seq = self.seq.wrapping_add(1);
        Some(Frame { data: &[], cols: Some(unsafe { &*core::ptr::addr_of!(ROLL_COLS) }), trig: None, fs: self.sample_rate(), seq: self.seq })
    }

    /// Samples of the stored record that fill the 400 px screen at timebase
    /// `tb`, using the sample rate the record was captured at. Equivalent to the
    /// stock stored-record re-render (§3): the s/div changes while fs is fixed,
    /// so the window = screen(tb) * stored_fs / fs(tb) (integer, no f32).
    /// Saturates at u32::MAX samples (zooming a 50 MS/s record out to 50 s/div
    /// asks for ~2.5e10); zoom_frame works in u64 with any value.
    fn zoom_screen(&self, tb: usize) -> u64 {
        let (fs_tb, num, den, ppp) = TB[tb];
        let screen = (num * COLS as u32 / den / ppp).max(2) as u64;
        (screen * self.stored_fs as u64 / fs_tb.max(1) as u64).clamp(2, u32::MAX as u64)
    }

    /// Re-render a window of the stored record at the current timebase, anchored
    /// on the feature that was on screen when acquisition stopped (a roll record
    /// on its newest sample at the right edge, as it was displayed).
    fn zoom_frame(&mut self) -> Option<Frame<'static>> {
        if !self.stored_valid || self.stored_len < 2 || self.stored_fs == 0 {
            return None;
        }
        let len = self.stored_len;
        let want = self.zoom_screen(self.s.tb);
        self.seq = self.seq.wrapping_add(1);
        if want <= len as u64 {
            let want = want as usize;
            let start = if self.stored_roll {
                len - want
            } else {
                self.stored_anchor.saturating_sub(want / 2).min(len - want)
            };
            let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[start..start + want] };
            let trig = self.stored_trig.and_then(|t| t.checked_sub(start).filter(|_| t < start + want));
            return Some(Frame { data, cols: None, trig, fs: self.stored_fs, seq: self.seq });
        }
        // The screen spans more time than was captured: draw the record at its
        // true scale as per-column min/max (the roll-frame format) with empty
        // columns where there is no data, instead of stretching it to 400 px.
        // f32 column edges: exact up to 2^24 samples, and beyond that a column
        // spans >= 40 K samples (> the whole record), so rounding is invisible.
        let step = want as f32 / COLS as f32;
        let w0 = if self.stored_roll { len as f32 - want as f32 } else { self.stored_anchor as f32 - (want / 2) as f32 };
        let buf = unsafe { &*core::ptr::addr_of!(BUF) };
        let cols = unsafe { &mut *core::ptr::addr_of_mut!(ROLL_COLS) };
        for (i, c) in cols.iter_mut().enumerate() {
            let af = w0 + i as f32 * step;
            let bf = af + step;
            *c = (u16::MAX, u16::MAX);
            if bf <= 0.0 || af >= len as f32 {
                continue; // no data in this column
            }
            // A negative left edge saturates to 0 in the cast.
            let a = af as usize;
            let b = (bf as usize).max(a + 1).min(len);
            let (mut lo, mut hi) = (u16::MAX, 0u16);
            for &v in &buf[a..b] {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            *c = (lo, hi);
        }
        Some(Frame { data: &[], cols: Some(unsafe { &*core::ptr::addr_of!(ROLL_COLS) }), trig: None, fs: self.stored_fs, seq: self.seq })
    }

    /// Stock software trigger: beyond the hysteresis band (code -+ 2) on the
    /// far side, then the first sample across the level.
    fn find_trigger(&self, d: &[u16]) -> Option<usize> {
        let l = self.trig_code();
        let mut armed = false;
        for (i, &v) in d.iter().enumerate() {
            let v = v as i32;
            if self.s.rising {
                if v < l - 2 {
                    armed = true;
                } else if armed && v > l {
                    return Some(i);
                }
            } else if v > l + 2 {
                armed = true;
            } else if armed && v < l {
                return Some(i);
            }
        }
        None
    }
}

/// Debug: latch word, record DMA count left, first sample, GPIOB IDR.
pub fn debug() -> [u32; 4] {
    unsafe {
        [
            core::ptr::read_volatile(core::ptr::addr_of!(LATCH[0])),
            r32(S4 + 0x04),
            core::ptr::read_volatile(core::ptr::addr_of!(BUF[0])) as u32,
            r32(GPIOB_IDR),
        ]
    }
}
