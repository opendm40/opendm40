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

fn buzzer(on: bool) {
    // Default volume 0.1 -> duty 0.1/3*0.5 of the period (stock formula).
    let duty = if on { BUZZER_ARR / 60 } else { 0 };
    unsafe { w32(TIM14 + 0x34, duty) };
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
        backlight(a.backlight);
        a.gen.apply(gen::Settings::DEFAULT);
        a
    }

    fn beep(&mut self, now: u64, ms: u64) {
        if self.beep_on {
            buzzer(true);
            self.beep_until = now + ms;
        }
    }

    fn show(&mut self, v: View, clk: &mut Clock) {
        if v == View::Menu && self.view != View::Menu {
            self.back_view = self.view;
        }
        // Analog power as the stock pages: scope / generator side or DMM side
        // (scope.rs power()). The menu keeps whatever is on.
        match v {
            View::Scope | View::Gen => self.scope.power(true, clk),
            View::Dmm | View::DmmScope => self.scope.power(false, clk),
            View::Menu => {}
        }
        self.view = v;
        self.redraw = true;
    }

    fn draw_static(&mut self) {
        match self.view {
            View::Dmm => self.dmm_view.draw_static(self.dmm.button(), self.dmm.mode()),
            View::DmmScope => {
                self.dmm_view.draw_static(self.dmm.button(), self.dmm.mode());
                lcd::fill_rect(0, ui::BAR_H, lcd::W, ui::BTN_Y - ui::BAR_H, ui::BG);
                ui::SCOPE_COMPACT.draw_static(&self.scope, false);
            }
            View::Scope => ui::SCOPE_FULL.draw_static(&self.scope, true),
            View::Gen => ui::gen_static(&self.gen),
            View::Menu => ui::menu(ui::view_name(self.back_view), self.bt_on, self.backlight, self.beep_on),
        }
        if let (Some(r), true) = (self.meter.shown().copied(), matches!(self.view, View::Dmm | View::DmmScope)) {
            self.dmm_view.reading(&r, self.meter.hold, self.meter.rel_active(), self.view == View::DmmScope);
        }
    }

    /// Periodic work. `can_draw` is false while a screenshot is being read.
    pub fn tick(&mut self, clk: &mut Clock, can_draw: bool) {
        let now = clk.now_ms();
        if self.beep_until != 0 && now >= self.beep_until {
            buzzer(false);
            self.beep_until = 0;
        }
        if !can_draw {
            return;
        }
        if self.redraw {
            self.redraw = false;
            self.draw_static();
            self.status_next = 0;
            self.chart_dirty = true;
        }
        self.power.poll(now);
        if now >= self.status_next {
            self.status_next = now + 1000;
            match self.view {
                View::Dmm | View::DmmScope => ui::status_right(
                    (now / 1000) as u32,
                    self.power.bt_connected(),
                    self.power.bars,
                    self.power.charging(),
                ),
                View::Scope => ui::scope_battery(self.power.bars, self.power.charging()),
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

    /// New DMM reading (always measured, whatever the view).
    pub fn reading(&mut self, raw: Reading, can_draw: bool) -> Option<Reading> {
        self.last_raw = Some(raw);
        let r = self.meter.update(&raw)?;
        if can_draw && !self.redraw && matches!(self.view, View::Dmm | View::DmmScope) {
            self.dmm_view.reading(&r, self.meter.hold, self.meter.rel_active(), self.view == View::DmmScope);
        }
        Some(r)
    }

    /// Poll the scope; draws the frame in scope views.
    pub fn scope_poll(&mut self, now: u64, can_draw: bool) {
        if let Some(f) = self.scope.poll(now) {
            if !can_draw || self.redraw {
                return;
            }
            match self.view {
                View::Scope => {
                    ui::SCOPE_FULL.frame(&f, &self.scope);
                    if now >= self.meas_next {
                        self.meas_next = now + 300;
                        let (vpp, mean, rms, freq) = ui::measure(&f, &self.scope);
                        ui::scope_measurements(vpp, mean, rms, freq);
                    }
                }
                View::DmmScope => ui::SCOPE_COMPACT.frame(&f, &self.scope),
                _ => {}
            }
        }
    }

    fn scope_set(&mut self, s: scope::Settings, now: u64) {
        self.scope.apply(s, now);
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
        match g {
            Gesture::KeyDown(c) => {
                self.beep(now, 50);
                self.key(clk, c);
            }
            Gesture::SwipeLeft | Gesture::SwipeRight if self.view != View::Menu => {
                let i = View::CYCLE.iter().position(|&v| v == self.view).unwrap_or(0);
                let n = View::CYCLE.len();
                let j = if g == Gesture::SwipeLeft { (i + 1) % n } else { (i + n - 1) % n };
                self.show(View::CYCLE[j], clk);
            }
            Gesture::Tap(p) => {
                self.beep(now, 30);
                self.tap(clk, p.x, p.y);
            }
            Gesture::Drag(p0, p) if self.view == View::Scope && p0.x >= 13 && p0.x < 414 && p0.y >= 20 && p0.y < 301 => {
                let mut st = self.scope.s;
                st.trig_row = p.y as i32 - 20;
                self.scope_set(st, now);
            }
            _ => {}
        }
    }

    fn key(&mut self, clk: &mut Clock, code: u8) {
        let now = clk.now_ms();
        match code {
            0 => {
                let v = if self.view == View::Menu { self.back_view } else { View::Menu };
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
                View::Scope => {
                    let mut s = self.scope.s;
                    s.run = true;
                    self.scope_set(s, now);
                }
                _ => {
                    self.meter.rel_off();
                    self.meter.set_hold(false);
                    self.dmm.auto_key(clk);
                    self.dmm_view.invalidate();
                }
            },
            _ => {
                if self.meter.rel_active() {
                    self.meter.rel_off();
                    self.dmm.hold_range(false);
                } else if self.meter.rel_on(self.last_raw.as_ref()) {
                    self.dmm.hold_range(true);
                }
                self.dmm_view.invalidate();
            }
        }
    }

    fn tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let now = clk.now_ms();
        match self.view {
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
                    st.freq_hz = if plus {
                        *steps.iter().find(|&&f| f > st.freq_hz).unwrap_or(&st.freq_hz)
                    } else {
                        *steps.iter().rev().find(|&&f| f < st.freq_hz).unwrap_or(&1)
                    };
                } else if x >= 246 && (133..223).contains(&y) {
                    st.amp_v = if plus { (st.amp_v + 0.1).min(3.0) } else { (st.amp_v - 0.1).max(0.5) };
                } else if x >= 246 && (226..316).contains(&y) {
                    st.duty = if plus { (st.duty + 500).min(10000) } else { st.duty.saturating_sub(500) };
                } else {
                    return;
                }
                st.freq_hz = gen::snap_freq(st.wave, st.freq_hz);
                self.gen_set(st);
            }
            View::Menu => match ui::menu_hit(y) {
                Some(0) => {
                    let i = View::CYCLE.iter().position(|&v| v == self.back_view).unwrap_or(0);
                    self.back_view = View::CYCLE[(i + 1) % View::CYCLE.len()];
                    self.redraw = true;
                }
                Some(1) => {
                    self.bt_on = !self.bt_on;
                    self.redraw = true;
                }
                Some(2) => {
                    self.backlight = if self.backlight >= 100 { 10 } else { self.backlight + 15 }.min(100);
                    backlight(self.backlight);
                    self.redraw = true;
                }
                Some(3) => {
                    self.beep_on = !self.beep_on;
                    self.redraw = true;
                }
                Some(5) => {
                    let v = self.back_view;
                    self.show(v, clk);
                }
                _ => {}
            },
        }
        let _ = dmm::button_of;
    }
}
