//! Application: all instruments run all the time (DMM, scope, generator);
//! only the view changes. Swipe left/right cycles the views, MENU opens the
//! menu, the bottom row holds the per-view buttons.
//!
//! Side keys (touch controller key codes 0..3, stock events 0x10 << code):
//!   0 MENU: menu.  1 RUN: DMM HOLD / scope run-stop.
//!   2 AUTO: DMM auto range / scope single re-arm.  3 MF: DMM REL toggle.

use crate::clock::Clock;
use crate::dmm::{self, Button, Dmm, Reading};
use crate::gen::{self, Gen, Wave};
use crate::gesture::Gesture;
use crate::lcd;
use crate::meter::Meter;
use crate::mmio::{modify32, w32};
use crate::power::Power;
use crate::scope::{self, Scope};
use crate::ui::{self, View};

const TIM13_CCR1: u32 = 0x4000_1C34;
/// Offered stock options: dim 5/10/30 min/never, shutdown 10/30/60 min/never.
const DIM_MS: [u64; 4] = [300_000, 600_000, 1_800_000, 0];
const OFF_MS: [u64; 4] = [600_000, 1_800_000, 3_600_000, 0];
const DIM_TXT: [&str; 4] = ["5 min", "10 min", "30 min", "Never"];
const OFF_TXT: [&str; 4] = ["10 min", "30 min", "1 hour", "Never"];
const START: [View; 4] = [View::Dmm, View::Scope, View::Gen, View::Menu];
const TIM14: u32 = 0x4000_2000;
const BUZZER_ARR: u32 = 11200;

pub struct App {
    pub dmm: Dmm,
    pub meter: Meter,
    pub scope: Scope,
    pub gen: Gen,
    pub view: View,
    back_view: View,
    dmm_view: ui::DmmView,
    pub bt_on: bool,
    backlight: u8,
    beep_on: bool,
    /// Stock settings (gui_v2_6.h section 2): dim / shutdown index into
    /// IDLE_MS, volume 0..100 %, start page, combined view for the menu row.
    dim_idx: usize,
    off_idx: usize,
    volume: u8,
    start_page: usize,
    combo: View,
    last_input: u64,
    dimmed: bool,
    pd6_since: Option<u64>,
    /// PD6 level at boot = released (polarity not known for sure).
    pd6_idle: bool,
    /// Last scope frame for SCPI: 400 columns (ADC code / 4) and measurements.
    pub wave: [u8; 400],
    /// Debug: max duration (us) of scope poll / measurements / draw.
    pub prof: [u32; 3],
    /// Drawn trace segments per scope column (see ScopeView::frame).
    scope_prev: [(u16, u16); 401],
    persist_next: u64,
    /// Keypad target (0 frequency, 1 Vpp, 2 duty) and the typed text.
    kp_target: u8,
    kp_buf: [u8; 8],
    kp_n: usize,
    kp_mult: u8,
    cur: ui::Cursors,
    /// Cursor being dragged (0/1 vertical, 2/3 horizontal).
    cur_drag: Option<u8>,
    /// Opt-in: keep both analog rails on so all instruments measure at once.
    analog_both: bool,
    persist_trace: bool,
    /// Anti-drop: re-assert the latching V/A relay periodically (stock
    /// setting; the relay can flip on a mechanical shock).
    anti_drop: bool,
    relay_next: u64,
    meas_sel: [u8; 4],
    meas_n: usize,
    pub meas_res: crate::scope_meas::Results,
    scope_tab: usize,
    roll: bool,
    /// AUTO-set: (phase 1/2, candidate index, round, settings to restore).
    autoset: Option<(u8, usize, u8, scope::Settings)>,
    locked: bool,
    pd6_presses: u8,
    pd6_last_release: u64,
    hist: [(f32, u8, &'static str); 5],
    hist_n: usize,
    hist_new: usize,
    hv_shown: bool,
    pub meas: [f32; 4],
    beep_until: u64,
    pub last_raw: Option<Reading>,
    redraw: bool,
    meas_next: u64,
    pub power: Power,
    status_next: u64,
    /// Strip chart: one point per `chart_rate` x 10 ms (1/2/4 = 160/320/640 ms/div).
    chart: [f32; ui::CHART_W as usize],
    chart_n: usize,
    chart_rate: u8,
    chart_next: u64,
    chart_unit: &'static str,
    chart_dec: u8,
    chart_dirty: bool,
}

fn backlight(pct: u8) {
    unsafe { w32(TIM13_CCR1, pct.min(100) as u32) };
}

fn buzzer_init() {
    unsafe {
        modify32(0x4002_3840, 0, 1 << 8); // TIM14 clock
        // PA7: AF9 (TIM14_CH1).
        modify32(0x4002_0000 + 0x20, 0xF << 28, 9 << 28);
        modify32(0x4002_0000, 3 << 14, 2 << 14);
        w32(TIM14 + 0x28, 4);
        w32(TIM14 + 0x2C, BUZZER_ARR - 1);
        w32(TIM14 + 0x18, (6 << 4) | (1 << 3));
        w32(TIM14 + 0x34, 0);
        w32(TIM14 + 0x20, 1);
        w32(TIM14 + 0x14, 1);
        w32(TIM14, 1 | (1 << 7));
    }
}

/// Buzzer on/off at `vol` 0..100 % (stock: duty = min(v/3, 0.99) * 0.5).
fn buzzer(on: bool, vol: u8) {
    let v = vol as f32 / 100.0;
    let k = if v < 0.9 { v / 3.0 } else { v.min(0.99) } * 0.5;
    let duty = if on { (BUZZER_ARR as f32 * k) as u32 } else { 0 };
    unsafe { w32(TIM14 + 0x34, duty) };
}

/// Power off as the stock: both analog rails / power hold low.
fn power_off() -> ! {
    unsafe {
        w32(0x4002_1000 + 0x18, 1 << (2 + 16)); // PE2 = 0
        w32(0x4002_0800 + 0x18, 1 << (4 + 16)); // PC4 = 0
    }
    loop {}
}

impl App {
    pub fn new(clk: &mut Clock) -> Self {
        let mut dmm = Dmm::new();
        dmm.select(clk, 1);
        buzzer_init();
        let mut a = App {
            dmm,
            meter: Meter::new(),
            scope: Scope::new(),
            gen: Gen::new(),
            view: View::Dmm,
            back_view: View::Dmm,
            dmm_view: ui::DmmView::new(),
            bt_on: true,
            backlight: 55,
            beep_on: true,
            dim_idx: 0,
            off_idx: 1,
            volume: 50,
            start_page: 0,
            combo: View::DmmScope,
            last_input: 0,
            dimmed: false,
            pd6_since: None,
            wave: [0; 400],
            prof: [0; 3],
            scope_prev: [(u16::MAX, 0); 401],
            persist_next: 0,
            kp_target: 0,
            kp_buf: [0; 8],
            kp_n: 0,
            kp_mult: b'H',
            cur: ui::Cursors { x_on: false, y_on: false, x: [100, 300], y: [70, 210] },
            cur_drag: None,
            analog_both: true,
            persist_trace: false,
            anti_drop: true,
            relay_next: 0,
            meas_sel: crate::scope_meas::DEFAULT_SEL,
            meas_n: 4,
            meas_res: crate::scope_meas::Results { v: [f32::NAN; 20] },
            scope_tab: 0,
            roll: true,
            autoset: None,
            locked: false,
            pd6_presses: 0,
            pd6_last_release: 0,
            hist: [(0.0, 0, ""); 5],
            hist_n: 0,
            hist_new: 0,
            hv_shown: false,
            meas: [0.0; 4],
            pd6_idle: unsafe { core::ptr::read_volatile(0x4002_0C10 as *const u32) } & (1 << 6) != 0,
            beep_until: 0,
            last_raw: None,
            redraw: true,
            meas_next: 0,
            power: Power::new(),
            status_next: 0,
            chart: [0.0; ui::CHART_W as usize],
            chart_n: 0,
            chart_rate: 2,
            chart_next: 0,
            chart_unit: "",
            chart_dec: 0,
            chart_dirty: false,
        };
        crate::persist::init();
        if let Some(p) = crate::persist::load() {
            a.restore(&p, clk);
        }
        backlight(a.backlight);
        let g = a.gen.s;
        a.gen.apply(gen::Settings { on: false, ..g });
        a.view = START[a.start_page];
        a.back_view = View::Dmm;
        let v = a.view;
        a.show(v, clk);
        a
    }

    fn beep(&mut self, now: u64, ms: u64) {
        if self.beep_on {
            buzzer(true, self.volume);
            self.beep_until = now + ms;
        }
    }

    fn show(&mut self, v: View, clk: &mut Clock) {
        if matches!(v, View::Menu | View::Settings | View::Range | View::ScopeMenu | View::Keypad) && !matches!(self.view, View::Menu | View::Settings | View::Range | View::ScopeMenu | View::Keypad) {
            self.back_view = self.view;
        }
        // Analog power as the stock pages: scope / generator side or DMM side
        // (scope.rs power()), or both when enabled in the settings. The menu
        // keeps whatever is on.
        if self.analog_both {
            self.scope.power_both(clk);
        } else {
        match v {
            View::Scope | View::Gen | View::ScopeMenu | View::Keypad => self.scope.power(true, clk),
            View::Dmm | View::DmmScope => self.scope.power(false, clk),
            View::Menu | View::Settings | View::Range => {}
        }
        }
        self.view = v;
        self.redraw = true;
    }

    fn draw_static(&mut self) {
        match self.view {
            View::Dmm => {
                self.dmm_view.draw_static(self.dmm.button(), self.dmm.mode());
                ui::history(&self.hist, self.hist_n, self.hist_new);
                self.hv_shown = false;
            }
            View::DmmScope => {
                self.dmm_view.draw_static(self.dmm.button(), self.dmm.mode());
                lcd::fill_rect(0, ui::BAR_H, lcd::W, ui::BTN_Y - ui::BAR_H, ui::BG);
                ui::SCOPE_COMPACT.draw_static(&self.scope, false);
            }
            View::Scope => {
                ui::SCOPE_FULL.draw_static(&self.scope, true);
                ui::scope_measurements(&self.meas_sel[..self.meas_n], &self.meas_res);
            }
            View::ScopeMenu => {
                ui::SCOPE_FULL.draw_static(&self.scope, true);
                ui::scope_menu(self.scope_tab, &self.scope, &self.meas_sel[..self.meas_n], self.roll, self.persist_trace);
            }
            View::Gen => ui::gen_static(&self.gen),
            View::Keypad => {
                ui::gen_static(&self.gen);
                let (title, unit) = self.kp_labels();
                let n = self.kp_n;
                let t = core::str::from_utf8(&self.kp_buf[..n]).unwrap_or("");
                ui::keypad(self.kp_target == 0, title, t, unit);
            }
            View::Menu => ui::main_menu(ui::view_name(self.combo)),
            View::Range => {
                let b = self.dmm.button();
                let ac = matches!(self.dmm.mode(), 0 | 2 | 4..=6);
                let sel = self.range_sel();
                ui::range_page(b, sel, ac);
            }
            View::Settings => {
                let mut b = [[0u8; 8]; 2];
                let pct = |b: &mut [u8; 8], v: u8| -> usize {
                    let mut t = [0u8; 16];
                    let s = crate::dmm::format(&mut t, v as f32, 0);
                    b[..s.len()].copy_from_slice(s.as_bytes());
                    b[s.len()] = b'%';
                    s.len() + 1
                };
                let [b0, b1] = &mut b;
                let n0 = pct(b0, self.backlight);
                let n1 = pct(b1, self.volume);
                let vals: [&str; 8] = [
                    DIM_TXT[self.dim_idx],
                    OFF_TXT[self.off_idx],
                    core::str::from_utf8(&b0[..n0]).unwrap_or(""),
                    core::str::from_utf8(&b1[..n1]).unwrap_or(""),
                    if self.bt_on { "ON" } else { "OFF" },
                    ui::view_name(START[self.start_page]),
                    if self.analog_both { "Both (exp.)" } else { "Per page" },
                    "",
                ];
                ui::settings(&vals);
            }
        }
        if let (Some(r), true) = (self.meter.shown().copied(), matches!(self.view, View::Dmm | View::DmmScope)) {
            self.dmm_view.reading(&r, self.meter.hold, self.meter.rel_active(), self.view == View::DmmScope);
        }
    }

    /// Periodic work. `can_draw` is false while a screenshot is being read.
    pub fn tick(&mut self, clk: &mut Clock, can_draw: bool) {
        let now = clk.now_ms();
        if self.beep_until != 0 && now >= self.beep_until {
            buzzer(false, self.volume);
            self.beep_until = 0;
        }
        if !can_draw {
            return;
        }
        if self.redraw {
            self.redraw = false;
            self.scope_prev = [(u16::MAX, 0); 401];
            self.draw_static();
            self.status_next = 0;
            self.chart_dirty = true;
        }
        self.power.poll(now);
        self.persist(now);
        if self.anti_drop && now >= self.relay_next {
            self.relay_next = now + 30_000;
            if self.relay_next > 30_000 {
                self.dmm.reassert_relay(clk);
            }
        }
        // Idle dim / shutdown (stock defaults 5 min / 30 min) and the power
        // key PD6 (hold 1.5 s -> off).
        let idle = now.saturating_sub(self.last_input);
        if DIM_MS[self.dim_idx] != 0 && idle > DIM_MS[self.dim_idx] && !self.dimmed {
            self.dimmed = true;
            backlight(5);
        }
        if OFF_MS[self.off_idx] != 0 && idle > OFF_MS[self.off_idx] {
            power_off();
        }
        let pd6 = (unsafe { core::ptr::read_volatile(0x4002_0C10 as *const u32) } & (1 << 6) != 0) != self.pd6_idle;
        match (pd6, self.pd6_since) {
            (true, None) => self.pd6_since = Some(now),
            (true, Some(t)) if now - t > 1500 => power_off(),
            (false, Some(_)) => {
                // Release: two releases within 1 s toggle the key / touch lock.
                self.pd6_since = None;
                if now - self.pd6_last_release < 1000 && self.pd6_presses >= 1 {
                    self.locked = !self.locked;
                    self.pd6_presses = 0;
                    self.status_next = 0;
                    self.beep(now, 150);
                } else {
                    self.pd6_presses = 1;
                }
                self.pd6_last_release = now;
            }
            _ => {}
        }
        if now >= self.status_next {
            self.status_next = now + 1000;
            match self.view {
                View::Dmm | View::DmmScope => ui::status_right(
                    (now / 1000) as u32,
                    self.power.bt_connected(),
                    self.power.bars,
                    self.power.charging(),
                    self.locked,
                ),
                View::Scope => {
                    ui::scope_battery(self.power.bars, self.power.charging());
                    ui::SCOPE_FULL.gen_state(&self.gen);
                }
                _ => {}
            }
        }
        // Strip chart point.
        if now >= self.chart_next {
            self.chart_next = now + self.chart_rate as u64 * 10;
            if let Some(r) = self.meter.shown().copied() {
                if !self.meter.hold && !r.main.ol {
                    if r.main.unit != self.chart_unit {
                        self.chart_unit = r.main.unit;
                        self.chart_n = 0;
                    }
                    self.chart_dec = r.main.dec;
                    let n = self.chart.len();
                    if self.chart_n == n {
                        self.chart.copy_within(1.., 0);
                        self.chart[n - 1] = r.main.v;
                    } else {
                        self.chart[self.chart_n] = r.main.v;
                        self.chart_n += 1;
                    }
                    self.chart_dirty = true;
                }
            }
        }
        // Redraw the chart at most every 100 ms (it is ~30k pixels).
        if self.chart_dirty && self.view == View::Dmm && now >= self.meas_next {
            self.meas_next = now + 100;
            self.chart_dirty = false;
            ui::chart(
                &self.chart[..self.chart_n],
                self.chart_dec,
                self.chart_rate,
                self.meter.max,
                self.meter.min,
                self.meter.rel_value(),
                self.chart_unit,
            );
        }
    }

    /// Make sure the DMM side is powered (SCPI / BT measurement requests).
    pub fn dmm_power(&mut self, clk: &mut Clock) {
        if !self.analog_both {
            self.scope.power(false, clk);
        }
    }

    /// X2-1 (s), Xf (Hz), Y2-1 (V) from the cursor positions (stock formulas).
    fn cursor_values(&mut self) {
        let sc = &self.scope.s;
        let t_px = scope::SDIV[sc.tb] / 40.0;
        let dx = (self.cur.x[1] as f32 - self.cur.x[0] as f32) * t_px;
        self.meas_res.v[7] = if self.cur.x_on { dx.abs() } else { f32::NAN };
        self.meas_res.v[8] = if self.cur.x_on && dx.abs() > 0.0 { 1.0 / dx.abs() } else { f32::NAN };
        let dy = -(self.cur.y[1] - self.cur.y[0]) as f32 * 4.0 * scope::VDIV[sc.range] * sc.probe / 141.0;
        self.meas_res.v[17] = if self.cur.y_on { dy } else { f32::NAN };
    }

    fn cursors_from_selection(&mut self) {
        let sel = &self.meas_sel[..self.meas_n];
        self.cur.x_on = sel.contains(&7) || sel.contains(&8);
        self.cur.y_on = sel.contains(&17);
        self.scope_prev = [(u16::MAX, 0); 401];
    }

    fn kp_open(&mut self, target: u8, clk: &mut Clock) {
        self.kp_target = target;
        self.kp_n = 0;
        self.kp_mult = b'K';
        self.show(View::Keypad, clk);
    }

    fn kp_labels(&self) -> (&'static str, &'static str) {
        match self.kp_target {
            0 => ("Frequency", match self.kp_mult {
                b'M' => "MHz",
                b'K' => "KHz",
                _ => "Hz",
            }),
            1 => ("Vpp", "V"),
            _ => ("Duty", "%"),
        }
    }

    fn kp_apply(&mut self) {
        let n = self.kp_n;
        let Some(v) = crate::scpi::parse_num(&self.kp_buf[..n]) else { return };
        let mut st = self.gen.s;
        match self.kp_target {
            0 => {
                let m = match self.kp_mult {
                    b'M' => 1e6,
                    b'K' => 1e3,
                    _ => 1.0,
                };
                st.freq_hz = gen::snap_freq(st.wave, (v * m).max(1.0) as u32);
            }
            1 => st.amp_v = v.clamp(0.5, 3.0),
            _ => st.duty = (v.clamp(0.0, 100.0) * 100.0) as u16,
        }
        self.gen.apply(st);
    }

    /// Settings -> backup SRAM words.
    fn persist_words(&self) -> [u32; crate::persist::WORDS] {
        let g = &self.gen.s;
        let sc = &self.scope.s;
        let mut p = [0u32; crate::persist::WORDS];
        p[0] = self.dim_idx as u32
            | (self.off_idx as u32) << 4
            | (self.start_page as u32) << 8
            | (self.bt_on as u32) << 12
            | (self.analog_both as u32) << 13;
        p[1] = self.backlight as u32 | (self.volume as u32) << 8 | (self.roll as u32) << 16;
        p[2] = self.dmm.mode() as u32 | (self.dmm.v_auto() as u32) << 8 | (self.dmm.a_auto() as u32) << 16 | (self.dmm.ohm_var() as u32) << 24;
        p[3] = Wave::ALL.iter().position(|&w| w == g.wave).unwrap_or(0) as u32 | (g.duty as u32) << 8;
        p[4] = g.freq_hz;
        p[5] = g.amp_v.to_bits();
        p[6] = sc.tb as u32 | (sc.range as u32) << 8 | (sc.ac as u32) << 16 | (sc.rising as u32) << 17;
        p[7] = (sc.trig_row as u32 & 0xFFFF) | (sc.pos as u32) << 16;
        p[8] = sc.probe.to_bits();
        for i in 0..4 {
            p[9] |= (self.meas_sel[i] as u32) << (8 * i);
        }
        p[10] = self.meas_n as u32;
        p
    }

    fn restore(&mut self, p: &[u32; crate::persist::WORDS], clk: &mut Clock) {
        self.dim_idx = (p[0] & 3) as usize;
        self.off_idx = (p[0] >> 4 & 3) as usize;
        self.start_page = (p[0] >> 8 & 3) as usize;
        self.bt_on = p[0] >> 12 & 1 != 0;
        self.analog_both = p[0] >> 13 & 1 != 0;
        self.backlight = (p[1] & 0xFF).clamp(10, 100) as u8;
        self.volume = (p[1] >> 8 & 0xFF).min(100) as u8;
        self.roll = p[1] >> 16 & 1 != 0;
        self.scope.roll = self.roll;
        let mode = (p[2] & 0xFF) as u8;
        if mode <= 15 {
            self.dmm.select(clk, mode);
        }
        let mut g = self.gen.s;
        g.wave = Wave::ALL[(p[3] & 3) as usize];
        g.duty = ((p[3] >> 8) & 0xFFFF).min(10000) as u16;
        g.freq_hz = p[4].clamp(1, 10_000_000);
        let a = f32::from_bits(p[5]);
        g.amp_v = if a.is_finite() { a.clamp(0.5, 3.0) } else { 3.0 };
        self.gen.s = g;
        let mut sc = self.scope.s;
        sc.tb = (p[6] & 0xFF) as usize;
        sc.range = (p[6] >> 8 & 0xFF) as usize;
        sc.ac = p[6] >> 16 & 1 != 0;
        sc.rising = p[6] >> 17 & 1 != 0;
        sc.trig_row = (p[7] & 0xFFFF) as u16 as i16 as i32;
        sc.pos = (p[7] >> 16) as u16 as i16 as i32;
        let pr = f32::from_bits(p[8]);
        sc.probe = if pr == 1.0 || pr == 10.0 || pr == 100.0 { pr } else { 1.0 };
        self.scope.apply(sc, 0);
        let n = (p[10] as usize).min(4);
        for i in 0..n {
            self.meas_sel[i] = (p[9] >> (8 * i)) as u8;
        }
        self.meas_n = n;
        self.cursors_from_selection();
    }

    /// Save the settings now and then (cheap: 18 words of backup SRAM).
    pub fn persist(&mut self, now: u64) {
        if now >= self.persist_next {
            self.persist_next = now + 2000;
            let w = self.persist_words();
            crate::persist::save(&w);
        }
    }

    /// AUTO-set (stock mode 4), phase 1: DC, 5 us/div, V/div candidates
    /// 10 V, 1 V, 100 mV, 10 mV until a signal with > 19 codes swing is seen.
    fn autoset_start(&mut self, now: u64) {
        let saved = self.scope.s;
        let mut st = saved;
        st.ac = false;
        st.tb = 8;
        st.range = 9;
        st.pos = 0;
        st.mode = scope::TrigMode::Auto;
        st.run = true;
        self.autoset = Some((1, 0, 0, saved));
        self.scope.apply(st, now);
    }

    fn autoset_step(&mut self, f: &scope::Frame, now: u64) {
        const CAND: [usize; 4] = [9, 6, 3, 0];
        let Some((phase, idx, round, saved)) = self.autoset else { return };
        let mut st = self.scope.s;
        let (mut lo, mut hi) = (1023u16, 0u16);
        for &v in f.data {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let pp = (hi - lo) as f32;
        // Period from rising crossings of the 30 % / 70 % levels.
        let (l30, l70) = (lo as f32 + 0.3 * pp, lo as f32 + 0.7 * pp);
        let (mut armed, mut first, mut last, mut n) = (false, 0usize, 0usize, 0u32);
        for (i, &v) in f.data.iter().enumerate() {
            let v = v as f32;
            if v < l30 {
                armed = true;
            } else if armed && v > l70 {
                armed = false;
                if n == 0 {
                    first = i;
                }
                last = i;
                n += 1;
            }
        }
        let period = if n >= 2 { (last - first) as f32 / (n - 1) as f32 / f.fs as f32 } else { 0.0 };
        let tb_for = |p: f32| -> usize {
            scope::SDIV.iter().position(|&d| p / 4.0 < d).unwrap_or(scope::TB_MAX).clamp(scope::TB_MIN, scope::TB_MAX)
        };
        if phase == 1 {
            if l70 - l30 > 19.0 * 0.4 && pp > 19.0 {
                if period > 0.0 {
                    st.tb = tb_for(period);
                }
                if lo == 0 || hi > 1022 {
                    st.range = (st.range + 3).min(9);
                }
                st.trig_row = 140;
                self.autoset = Some((2, idx, round, saved));
            } else if idx + 1 < CAND.len() {
                st.range = CAND[idx + 1];
                self.autoset = Some((1, idx + 1, round, saved));
            } else if round == 0 {
                st.tb = 19;
                st.range = CAND[0];
                self.autoset = Some((1, 0, 1, saved));
            } else {
                self.autoset = None;
                self.scope_set(saved, now);
                return;
            }
            self.scope.apply(st, now);
            return;
        }
        // Phase 2: V/div so that the signal fits +-4 div within 6.4 div, centred.
        let (vmax, vmin) = (self.scope.volts(hi as f32), self.scope.volts(lo as f32));
        let probe = st.probe;
        let mut r = 9;
        for (i, &d) in scope::VDIV.iter().enumerate() {
            let d = d * probe;
            if vmin >= -4.0 * d && vmax <= 4.0 * d && (vmax - vmin) <= 6.4 * d {
                r = i;
                break;
            }
        }
        st.range = r;
        let mid = (vmax + vmin) / 2.0;
        st.pos = (141.0 * mid / (4.0 * scope::VDIV[r] * probe)) as i32;
        st.trig_row = 140;
        if period > 0.0 {
            st.tb = tb_for(period);
        }
        self.autoset = None;
        self.scope_set(st, now);
        self.redraw = true;
    }

    /// Index of the current range in the MF page list.
    fn range_sel(&self) -> Option<usize> {
        let b = self.dmm.button();
        let items = ui::range_items(b);
        let want: Option<u8> = if self.dmm.is_auto() {
            match b {
                Button::Volt if self.dmm.v_auto() == 1 => Some(255),
                Button::Amp if self.dmm.a_auto() == 1 => Some(255),
                _ => None,
            }
        } else {
            let (m, r) = (self.dmm.mode(), self.dmm.range() as u8);
            Some(match m {
                2 | 3 => 0,
                0 | 1 => r + 1,
                4..=9 => (2 - (m - 4) % 3) * 2 + r,
                _ => r,
            })
        };
        items.iter().position(|it| it.1 == want)
    }

    /// New DMM reading (always measured, whatever the view).
    pub fn reading(&mut self, raw: Reading, can_draw: bool) -> Option<Reading> {
        self.last_raw = Some(raw);
        let r = self.meter.update(&raw)?;
        if can_draw && !self.redraw && matches!(self.view, View::Dmm | View::DmmScope) {
            self.dmm_view.reading(&r, self.meter.hold, self.meter.rel_active(), self.view == View::DmmScope);
            let hv = self.view == View::Dmm && r.mode <= 1 && !r.main.ol && r.si.abs() >= 100.0;
            if hv != self.hv_shown {
                self.hv_shown = hv;
                ui::hv_warning(hv);
            }
        }
        Some(r)
    }

    /// Poll the scope; draws the frame in scope views.
    pub fn scope_poll(&mut self, now: u64, can_draw: bool) {
        let cyc = || unsafe { core::ptr::read_volatile(0xE000_1004 as *const u32) };
        let t0 = cyc();
        let polled = self.scope.poll(now);
        self.prof[0] = self.prof[0].max(cyc().wrapping_sub(t0) / 200);
        if let Some(f) = polled {
            let n = f.data.len();
            for (i, w) in self.wave.iter_mut().enumerate().take(if n > 0 { 400 } else { 0 }) {
                let a = i * n / 400;
                let b = ((i + 1) * n / 400).max(a + 1).min(n);
                let sum: u32 = f.data[a..b].iter().map(|&v| v as u32).sum();
                *w = ((sum / (b - a) as u32) >> 2) as u8;
            }
            if self.autoset.is_some() {
                self.autoset_step(&f, now);
                return;
            }
            let fresh = now >= self.meas_next && !f.data.is_empty();
            if fresh {
                self.meas_next = now + 300;
                let t1 = cyc();
                self.meas_res = crate::scope_meas::compute(f.data, f.fs as f32, &self.scope);
                self.cursor_values();
                self.prof[1] = self.prof[1].max(cyc().wrapping_sub(t1) / 200);
                let r = &self.meas_res.v;
                self.meas = [r[10], r[19], r[11], r[0]];
            }
            if !can_draw || self.redraw {
                return;
            }
            match self.view {
                View::Scope => {
                    let t2 = cyc();
                    ui::SCOPE_FULL.frame(&f, &self.scope, &mut self.scope_prev, &self.cur, self.persist_trace);
                    self.prof[2] = self.prof[2].max(cyc().wrapping_sub(t2) / 200);
                    if fresh {
                        ui::scope_measurements(&self.meas_sel[..self.meas_n], &self.meas_res);
                    }
                }
                View::DmmScope => ui::SCOPE_COMPACT.frame(&f, &self.scope, &mut self.scope_prev, &self.cur, self.persist_trace),
                _ => {}
            }
        }
    }

    fn scope_set(&mut self, s: scope::Settings, now: u64) {
        self.scope.apply(s, now);
        self.scope_prev = [(u16::MAX, 0); 401];
        match self.view {
            View::Scope => ui::SCOPE_FULL.labels(&self.scope, true),
            View::DmmScope => ui::SCOPE_COMPACT.labels(&self.scope, false),
            _ => {}
        }
    }

    fn gen_set(&mut self, s: gen::Settings) {
        self.gen.apply(s);
        if self.view == View::Gen {
            ui::gen_values(&self.gen);
        }
    }

    pub fn gesture(&mut self, clk: &mut Clock, g: Gesture) {
        let now = clk.now_ms();
        self.last_input = now;
        if self.locked {
            return;
        }
        if self.dimmed {
            self.dimmed = false;
            backlight(self.backlight);
        }
        match g {
            Gesture::KeyDown(c) => {
                self.beep(now, 50);
                self.key(clk, c);
            }
            Gesture::SwipeLeft | Gesture::SwipeRight if !matches!(self.view, View::Menu | View::Settings | View::Range | View::ScopeMenu | View::Keypad) => {
                let i = View::CYCLE.iter().position(|&v| v == self.view).unwrap_or(0);
                let n = View::CYCLE.len();
                let j = if g == Gesture::SwipeLeft { (i + 1) % n } else { (i + n - 1) % n };
                self.show(View::CYCLE[j], clk);
            }
            Gesture::Tap(p) => {
                self.beep(now, 30);
                self.tap(clk, p.x, p.y);
            }
            Gesture::LongPress(p) if self.view == View::Dmm && (153..172).contains(&p.y) => {
                self.hist_n = 0;
                self.beep(now, 300);
                ui::history(&self.hist, 0, 0);
            }
            Gesture::Down(p) if self.view == View::Scope => {
                // Grab a cursor within 8 px.
                let (cx, cy) = (p.x as i32 - 13, p.y as i32 - 20);
                self.cur_drag = None;
                for k in 0..2 {
                    if self.cur.x_on && (cx - self.cur.x[k] as i32).abs() <= 8 {
                        self.cur_drag = Some(k as u8);
                    }
                    if self.cur.y_on && (cy - self.cur.y[k]).abs() <= 8 {
                        self.cur_drag = Some(2 + k as u8);
                    }
                }
            }
            Gesture::Drag(p0, p) if self.view == View::Scope && p0.x >= 13 && p0.x < 414 && p0.y >= 20 && p0.y < 301 => {
                match self.cur_drag {
                    Some(k) if k < 2 => {
                        self.cur.x[k as usize] = (p.x.saturating_sub(13)).clamp(1, 399);
                        self.scope_prev = [(u16::MAX, 0); 401];
                        self.cursor_values();
                    }
                    Some(k) => {
                        self.cur.y[k as usize - 2] = (p.y as i32 - 20).clamp(1, 279);
                        self.scope_prev = [(u16::MAX, 0); 401];
                        self.cursor_values();
                    }
                    None => {
                        let mut st = self.scope.s;
                        st.trig_row = p.y as i32 - 20;
                        self.scope_set(st, now);
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, clk: &mut Clock, code: u8) {
        let now = clk.now_ms();
        match code {
            0 => {
                let v = if matches!(self.view, View::Menu | View::Settings | View::Range | View::ScopeMenu | View::Keypad) { self.back_view } else { View::Menu };
                self.show(v, clk);
            }
            1 => match self.view {
                View::Gen => {
                    let mut st = self.gen.s;
                    st.on = !st.on;
                    self.gen_set(st);
                }
                View::Scope => {
                    let mut s = self.scope.s;
                    s.run = !s.run;
                    self.scope_set(s, now);
                }
                _ => {
                    let h = !self.meter.hold;
                    self.meter.set_hold(h);
                    self.dmm_view.invalidate();
                }
            },
            2 => match self.view {
                View::Scope => self.autoset_start(now),
                _ => {
                    self.meter.rel_off();
                    self.meter.set_hold(false);
                    self.dmm.auto_key(clk);
                    self.dmm_view.invalidate();
                }
            },
            _ => match self.view {
                View::Dmm | View::DmmScope if matches!(self.dmm.button(), Button::Volt | Button::Amp | Button::Ohm | Button::Cap) => {
                    self.show(View::Range, clk)
                }
                View::Range => {
                    let v = self.back_view;
                    self.show(v, clk)
                }
                View::Scope => self.show(View::ScopeMenu, clk),
                View::ScopeMenu => self.show(View::Scope, clk),
                _ => {}
            },
        }
    }

    fn tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let now = clk.now_ms();
        match self.view {
            View::Dmm | View::DmmScope if self.view == View::Dmm && x >= 402 && (205..243).contains(&y) => {
                // REL button (stock (402,205,76,38)).
                if self.meter.rel_active() {
                    self.meter.rel_off();
                    self.dmm.hold_range(false);
                    self.beep(now, 300);
                } else if self.meter.rel_on(self.last_raw.as_ref()) {
                    self.dmm.hold_range(self.dmm.mode() <= 10 || self.dmm.mode() == 14);
                }
                self.dmm_view.invalidate();
                self.chart_dirty = true;
            }
            View::Dmm if (153..172).contains(&y) => {
                // History row: record the shown value (not HOLD, not OL).
                if let Some(r) = self.meter.shown().copied() {
                    if !self.meter.hold && !r.main.ol {
                        let i = if self.hist_n < 5 { self.hist_n } else { (self.hist_new + 1) % 5 };
                        self.hist[i] = (r.main.v, r.main.dec, r.main.unit);
                        self.hist_n = (self.hist_n + 1).min(5);
                        self.hist_new = i;
                        ui::history(&self.hist, self.hist_n, self.hist_new);
                    }
                }
            }
            View::Keypad => {
                let freq = self.kp_target == 0;
                match ui::keypad_hit(freq, x, y) {
                    Some(b'=') => {
                        self.kp_apply();
                        self.show(View::Gen, clk);
                    }
                    Some(b'<') => self.kp_n = self.kp_n.saturating_sub(1),
                    Some(c @ (b'H' | b'K' | b'M')) => self.kp_mult = c,
                    Some(c) => {
                        let max = if freq { 3 } else { 5 };
                        if self.kp_n < max && !(c == b'.' && self.kp_buf[..self.kp_n].contains(&b'.')) {
                            self.kp_buf[self.kp_n] = c;
                            self.kp_n += 1;
                        }
                    }
                    None => {
                        // Outside the keypad: cancel.
                        if !(x >= 224 && y >= if freq { 142 } else { 100 }) {
                            self.show(View::Gen, clk);
                        }
                        return;
                    }
                }
                let (_, unit) = self.kp_labels();
                let n = self.kp_n;
                let t = core::str::from_utf8(&self.kp_buf[..n]).unwrap_or("");
                ui::keypad_input(freq, t, unit);
            }
            View::ScopeMenu => {
                let mut st = self.scope.s;
                match ui::scope_menu_hit(self.scope_tab, x, y) {
                    None => {
                        self.show(View::Scope, clk);
                        return;
                    }
                    Some((t, 255)) => self.scope_tab = t,
                    Some((0, k)) => {
                        let id = crate::scope_meas::ITEMS[k].0;
                        if let Some(p) = self.meas_sel[..self.meas_n].iter().position(|&i| i == id) {
                            self.meas_sel.copy_within(p + 1..self.meas_n, p);
                            self.meas_n -= 1;
                        } else if self.meas_n < 4 {
                            self.meas_sel[self.meas_n] = id;
                            self.meas_n += 1;
                        }
                        self.cursors_from_selection();
                    }
                    Some((1, 0)) => {
                        self.roll = !self.roll;
                        self.scope.roll = self.roll;
                    }
                    Some((1, _)) => {
                        self.persist_trace = !self.persist_trace;
                        self.scope_prev = [(u16::MAX, 0); 401];
                    }
                    Some((2, k)) => match k {
                        0 => st.rising = true,
                        1 => st.rising = false,
                        3 => st.mode = scope::TrigMode::Auto,
                        4 => st.mode = scope::TrigMode::Normal,
                        5 => st.mode = scope::TrigMode::Single,
                        _ => {}
                    },
                    Some((_, k)) => match k {
                        0 => st.ac = false,
                        1 => st.ac = true,
                        3..=5 => st.probe = [1.0, 10.0, 100.0][k - 3],
                        _ => {}
                    },
                }
                self.scope.apply(st, now);
                ui::scope_menu(self.scope_tab, &self.scope, &self.meas_sel[..self.meas_n], self.roll, self.persist_trace);
            }
            View::Keypad => {
                let freq = self.kp_target == 0;
                match ui::keypad_hit(freq, x, y) {
                    Some(b'=') => {
                        self.kp_apply();
                        self.show(View::Gen, clk);
                    }
                    Some(b'<') => self.kp_n = self.kp_n.saturating_sub(1),
                    Some(c @ (b'H' | b'K' | b'M')) => self.kp_mult = c,
                    Some(c) => {
                        let max = if freq { 3 } else { 5 };
                        if self.kp_n < max && !(c == b'.' && self.kp_buf[..self.kp_n].contains(&b'.')) {
                            self.kp_buf[self.kp_n] = c;
                            self.kp_n += 1;
                        }
                    }
                    None => {
                        // Outside the keypad: cancel.
                        if !(x >= 224 && y >= if freq { 142 } else { 100 }) {
                            self.show(View::Gen, clk);
                        }
                        return;
                    }
                }
                let (_, unit) = self.kp_labels();
                let n = self.kp_n;
                let t = core::str::from_utf8(&self.kp_buf[..n]).unwrap_or("");
                ui::keypad_input(freq, t, unit);
            }
            View::ScopeMenu => {
                let mut st = self.scope.s;
                match ui::scope_menu_hit(self.scope_tab, x, y) {
                    None => {
                        self.show(View::Scope, clk);
                        return;
                    }
                    Some((t, 255)) => self.scope_tab = t,
                    Some((0, k)) => {
                        let id = crate::scope_meas::ITEMS[k].0;
                        if let Some(p) = self.meas_sel[..self.meas_n].iter().position(|&i| i == id) {
                            self.meas_sel.copy_within(p + 1..self.meas_n, p);
                            self.meas_n -= 1;
                        } else if self.meas_n < 4 {
                            self.meas_sel[self.meas_n] = id;
                            self.meas_n += 1;
                        }
                        self.cursors_from_selection();
                    }
                    Some((1, 0)) => {
                        self.roll = !self.roll;
                        self.scope.roll = self.roll;
                    }
                    Some((1, _)) => {
                        self.persist_trace = !self.persist_trace;
                        self.scope_prev = [(u16::MAX, 0); 401];
                    }
                    Some((2, k)) => match k {
                        0 => st.rising = true,
                        1 => st.rising = false,
                        3 => st.mode = scope::TrigMode::Auto,
                        4 => st.mode = scope::TrigMode::Normal,
                        5 => st.mode = scope::TrigMode::Single,
                        _ => {}
                    },
                    Some((_, k)) => match k {
                        0 => st.ac = false,
                        1 => st.ac = true,
                        3..=5 => st.probe = [1.0, 10.0, 100.0][k - 3],
                        _ => {}
                    },
                }
                self.scope.apply(st, now);
                ui::scope_menu(self.scope_tab, &self.scope, &self.meas_sel[..self.meas_n], self.roll, self.persist_trace);
            }
            View::Range => {
                let b = self.dmm.button();
                match ui::range_hit(b, x, y) {
                    Some(Ok(i)) => {
                        let item = ui::range_items(b)[i].1;
                        self.dmm.set_range(clk, item);
                        self.meter.rel_off();
                        self.redraw = true;
                    }
                    Some(Err(2)) => {
                        let v = self.back_view;
                        self.show(v, clk);
                    }
                    Some(Err(k)) => {
                        let ac = matches!(self.dmm.mode(), 0 | 2 | 4..=6);
                        if (k == 1) != ac {
                            self.dmm.press(clk, b);
                            self.meter.reset();
                        }
                        self.redraw = true;
                    }
                    None => {}
                }
            }
            View::Dmm | View::DmmScope => {
                if self.view == View::Dmm && x < ui::CHART_X && (176..280).contains(&y) {
                    self.chart_rate = match self.chart_rate {
                        1 => 2,
                        2 => 4,
                        _ => 1,
                    };
                    self.chart_n = 0;
                    self.chart_dirty = true;
                } else if let Some(i) = ui::button_hit(6, x, y) {
                    self.dmm.press(clk, Button::ALL[i]);
                    self.meter.reset();
                    self.redraw = true;
                }
            }
            View::Scope => {
                let mut st = self.scope.s;
                if x >= 414 {
                    // Right panel: V/div + / label / -, time + / label / -, TRIG.
                    match y {
                        20..=60 => st.range = st.range.saturating_sub(1),
                        61..=101 => st.ac = !st.ac,
                        102..=146 => st.range = (st.range + 1).min(9),
                        150..=190 => st.tb = st.tb.saturating_sub(1),
                        191..=231 => {
                            st.mode = match st.mode {
                                scope::TrigMode::Auto => scope::TrigMode::Normal,
                                scope::TrigMode::Normal => scope::TrigMode::Single,
                                scope::TrigMode::Single => scope::TrigMode::Auto,
                            };
                            st.run = true;
                        }
                        232..=272 => st.tb = (st.tb + 1).min(scope::TB_MAX),
                        _ => st.rising = !st.rising,
                    }
                } else if y < 20 && x < 75 {
                    st.run = !st.run;
                } else if x < 13 {
                    st.pos = 140 - (y as i32 - 20);
                } else if y >= 20 && y < 301 {
                    st.trig_row = y as i32 - 20;
                }
                self.scope_set(st, now);
            }
            View::Gen => {
                let mut st = self.gen.s;
                let steps: [u32; 22] = [
                    1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10_000, 20_000, 50_000, 100_000, 200_000,
                    500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000,
                ];
                let plus = x >= 361;
                if x < 243 && (40..82).contains(&y) {
                    let k = Wave::ALL.iter().position(|&w| w == st.wave).unwrap_or(0);
                    st.wave = Wave::ALL[(k + 1) % 4];
                } else if x >= 246 && (40..130).contains(&y) {
                    self.kp_open(0, clk);
                    return;
                } else if x >= 246 && (133..223).contains(&y) {
                    self.kp_open(1, clk);
                    return;
                } else if x >= 246 && (226..316).contains(&y) {
                    if st.wave == Wave::Square && st.freq_hz <= 50_000 {
                        self.kp_open(2, clk);
                    }
                    return;
                } else {
                    return;
                }
                st.freq_hz = gen::snap_freq(st.wave, st.freq_hz);
                self.gen_set(st);
            }
            View::Menu => match ui::main_menu_hit(x, y) {
                Some(0) => self.show(View::Dmm, clk),
                Some(1) => self.show(View::Scope, clk),
                Some(2) => self.show(View::Gen, clk),
                Some(3) => self.show(View::Settings, clk),
                Some(_) => {
                    let c = self.combo;
                    self.show(c, clk);
                }
                None => {}
            },
            View::Settings => {
                match ui::settings_hit(y) {
                    Some(0) => self.dim_idx = (self.dim_idx + 1) % 4,
                    Some(1) => self.off_idx = (self.off_idx + 1) % 4,
                    Some(2) => {
                        self.backlight = if self.backlight >= 100 { 10 } else { (self.backlight + 10).min(100) };
                        backlight(self.backlight);
                    }
                    Some(3) => self.volume = if self.volume >= 100 { 0 } else { self.volume + 10 },
                    Some(4) => self.bt_on = !self.bt_on,
                    Some(5) => self.start_page = (self.start_page + 1) % 4,
                    Some(6) => {
                        self.analog_both = !self.analog_both;
                        if self.analog_both {
                            self.scope.power_both(clk);
                        } else {
                            let scope_side = matches!(self.back_view, View::Scope | View::Gen);
                            self.scope.power(scope_side, clk);
                        }
                    }
                    Some(_) => {
                        self.show(View::Menu, clk);
                        return;
                    }
                    None => return,
                }
                self.redraw = true;
            }
        }
        let _ = dmm::button_of;
    }

    /// SCPI scope / generator command. Writes the reply (if any) to `out` and
    /// returns true when there is one.
    pub fn scpi(&mut self, clk: &mut Clock, cmd: crate::scpi::Cmd, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Cmd as C;
        let now = clk.now_ms();
        let mut g = self.gen.s;
        let mut sc = self.scope.s;
        let scope_cmd =
            !matches!(cmd, C::GenFunc(_) | C::GenFreq(_) | C::GenVolt(_) | C::GenDuty(_) | C::GenOut(_) | C::DiagDac | C::DiagPwm);
        if scope_cmd && !self.scope.powered() && !self.analog_both {
            self.scope.power(true, clk);
        }
        let reply = match cmd {
            C::GenFunc(Some(w)) => {
                g.wave = Wave::ALL[w as usize % 4];
                None
            }
            C::GenFunc(None) => {
                out.put_bytes(match g.wave {
                    Wave::Sine => b"SIN",
                    Wave::Square => b"SQU",
                    Wave::Triangle => b"TRI",
                    Wave::Ramp => b"RAMP",
                });
                Some(())
            }
            C::GenFreq(Some(f)) => {
                g.freq_hz = gen::snap_freq(g.wave, f.max(1.0) as u32);
                None
            }
            C::GenFreq(None) => {
                out.put_f32(g.freq_hz as f32);
                Some(())
            }
            C::GenVolt(Some(v)) => {
                g.amp_v = v.clamp(0.5, 3.0);
                None
            }
            C::GenVolt(None) => {
                out.put_f32(g.amp_v);
                Some(())
            }
            C::GenDuty(Some(d)) => {
                g.duty = (d.clamp(0.0, 100.0) * 100.0) as u16;
                None
            }
            C::GenDuty(None) => {
                out.put_f32(g.duty as f32 / 100.0);
                Some(())
            }
            C::GenOut(Some(on)) => {
                g.on = on;
                None
            }
            C::GenOut(None) => {
                out.put_bytes(if g.on { b"1" } else { b"0" });
                Some(())
            }
            C::TimScale(Some(t)) => {
                sc.tb = scope::SDIV
                    .iter()
                    .position(|&d| d >= t * 0.999)
                    .unwrap_or(scope::TB_MAX)
                    .clamp(scope::TB_MIN, scope::TB_MAX);
                None
            }
            C::TimScale(None) => {
                out.put_f32(scope::SDIV[sc.tb]);
                Some(())
            }
            C::ChScale(Some(v)) => {
                sc.range = scope::VDIV.iter().position(|&d| d >= v * 0.999).unwrap_or(9);
                None
            }
            C::ChScale(None) => {
                out.put_f32(scope::VDIV[sc.range]);
                Some(())
            }
            C::ChCoup(Some(ac)) => {
                sc.ac = ac;
                None
            }
            C::ChCoup(None) => {
                out.put_bytes(if sc.ac { b"AC" } else { b"DC" });
                Some(())
            }
            C::TrigLevel(Some(v)) => {
                let r = scope::VDIV[sc.range];
                sc.trig_row = (140.0 + sc.pos as f32 - v * 141.0 / (4.0 * r)) as i32;
                None
            }
            C::TrigLevel(None) => {
                out.put_f32(self.scope.trig_volts());
                Some(())
            }
            C::TrigSlope(Some(r)) => {
                sc.rising = r;
                None
            }
            C::TrigSlope(None) => {
                out.put_bytes(if sc.rising { b"POS" } else { b"NEG" });
                Some(())
            }
            C::TrigSweep(Some(m)) => {
                sc.mode = [scope::TrigMode::Auto, scope::TrigMode::Normal, scope::TrigMode::Single][m as usize % 3];
                None
            }
            C::TrigSweep(None) => {
                out.put_bytes(match sc.mode {
                    scope::TrigMode::Auto => b"AUTO",
                    scope::TrigMode::Normal => b"NORM",
                    scope::TrigMode::Single => b"SING",
                });
                Some(())
            }
            C::Run => {
                sc.run = true;
                None
            }
            C::Stop => {
                sc.run = false;
                None
            }
            C::Single => {
                sc.mode = scope::TrigMode::Single;
                sc.run = true;
                None
            }
            C::Meas(k) => {
                out.put_f32(self.meas[k as usize % 4]);
                Some(())
            }
            C::WavData => {
                out.put_bytes(b"#3400");
                let w = self.wave;
                out.put_bytes(&w);
                Some(())
            }
            C::DiagDac => {
                // min,max (V, 3.0 V reference), mid crossings, window (s)
                let (lo, hi, cross, cyc) = self.power.sample(4, 512);
                out.put_f32(lo as f32 * 3.0 / 4096.0);
                out.put_bytes(b",");
                out.put_f32(hi as f32 * 3.0 / 4096.0);
                out.put_bytes(b",");
                out.put_f32(cross as f32);
                out.put_bytes(b",");
                out.put_f32(cyc as f32 / 200e6);
                Some(())
            }
            C::DiagPwm => {
                // PB14 rising edges over 2 ms (tight loop on GPIOB->IDR), and
                // the PB12 output-path select level.
                let idr = 0x4002_0410 as *const u32;
                let cyc = || unsafe { core::ptr::read_volatile(0xE000_1004 as *const u32) };
                let t0 = cyc();
                let mut last = false;
                let mut edges = 0u32;
                while cyc().wrapping_sub(t0) < 400_000 {
                    let v = unsafe { core::ptr::read_volatile(idr) } & (1 << 14) != 0;
                    if v && !last {
                        edges += 1;
                    }
                    last = v;
                }
                let pb12 = unsafe { core::ptr::read_volatile(idr) } & (1 << 12) != 0;
                out.put_f32(edges as f32 / 0.002);
                out.put_bytes(if pb12 { b",PB12=1" } else { b",PB12=0" });
                Some(())
            }
            C::WavPre => {
                // points, x increment (s), y: volts = (byte*4 - zero) * k.
                out.put_bytes(b"400,");
                out.put_f32(scope::SDIV[sc.tb] * 10.0 / 400.0);
                out.put_bytes(b",");
                out.put_f32(self.scope.volts(4.0) - self.scope.volts(0.0));
                out.put_bytes(b",");
                out.put_f32(self.scope.volts(0.0));
                Some(())
            }
        };
        if matches!(cmd, C::DiagDac | C::DiagPwm) {
        } else if !scope_cmd {
            self.gen_set(g);
        } else if !matches!(cmd, C::Meas(_) | C::WavData | C::WavPre) && reply.is_none() {
            self.scope_set(sc, now);
        }
        reply.is_some()
    }
}
