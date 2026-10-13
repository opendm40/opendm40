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

    fn show(&mut self, v: View) {
        if v == View::Menu && self.view != View::Menu {
            self.back_view = self.view;
        }
        self.view = v;
        self.redraw = true;
    }

    fn draw_static(&mut self) {
        match self.view {
            View::Dmm => self.dmm_view.draw_static(self.dmm.button()),
            View::DmmScope => {
                self.dmm_view.draw_static(self.dmm.button());
                lcd::fill_rect(0, ui::BAR_H, lcd::W, ui::BTN_Y - ui::BAR_H, ui::BG);
                ui::SCOPE_COMPACT.draw_static(&self.scope.s, false);
            }
            View::Scope => ui::SCOPE_FULL.draw_static(&self.scope.s, true),
            View::Gen => {
                ui::gen_static();
                ui::gen_values(&self.gen);
            }
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
                    ui::SCOPE_FULL.frame(&f, &self.scope.s);
                    if now >= self.meas_next {
                        self.meas_next = now + 300;
                        let (vpp, mean, freq) = ui::measure(&f, &self.scope);
                        ui::scope_measurements(vpp, mean, freq, ui::SCOPE_FULL.y0);
                    }
                }
                View::DmmScope => ui::SCOPE_COMPACT.frame(&f, &self.scope.s),
                _ => {}
            }
        }
    }

    fn scope_set(&mut self, s: scope::Settings, now: u64) {
        self.scope.apply(s, now);
        match self.view {
            View::Scope => ui::SCOPE_FULL.labels(&self.scope.s),
            View::DmmScope => ui::SCOPE_COMPACT.labels(&self.scope.s),
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
                self.show(View::CYCLE[j]);
            }
            Gesture::Tap(p) => {
                self.beep(now, 30);
                self.tap(clk, p.x, p.y);
            }
            _ => {}
        }
    }

    fn key(&mut self, clk: &mut Clock, code: u8) {
        let now = clk.now_ms();
        match code {
            0 => {
                let v = if self.view == View::Menu { self.back_view } else { View::Menu };
                self.show(v);
            }
            1 => match self.view {
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
                if let Some(i) = ui::button_hit(6, x, y) {
                    self.dmm.press(clk, Button::ALL[i]);
                    self.meter.reset();
                    self.redraw = true;
                }
            }
            View::Scope => {
                if let Some(i) = ui::button_hit(6, x, y) {
                    let mut s = self.scope.s;
                    match i {
                        0 => s.tb = (s.tb + 1).min(20),
                        1 => s.tb = s.tb.saturating_sub(1),
                        2 => s.range = (s.range + 1).min(10),
                        3 => s.range = s.range.saturating_sub(1),
                        4 => s.rising = !s.rising,
                        _ => {
                            s.mode = match s.mode {
                                scope::TrigMode::Auto => scope::TrigMode::Normal,
                                scope::TrigMode::Normal => scope::TrigMode::Single,
                                scope::TrigMode::Single => scope::TrigMode::Auto,
                            };
                            s.run = true;
                        }
                    }
                    self.scope_set(s, now);
                } else if x < ui::SCOPE_FULL.w && y >= ui::SCOPE_FULL.y0 {
                    // Tap in the plot: trigger level there.
                    let v = &ui::SCOPE_FULL;
                    let r = scope::RANGES[self.scope.s.range];
                    let px_per_count = (v.h / v.divs_y) as f32 / (r.2 * r.0);
                    let code = 512.0 + (v.h as f32 / 2.0 - (y - v.y0) as f32) / px_per_count;
                    let mut s = self.scope.s;
                    s.trig_level = code.clamp(0.0, 1023.0) as u16;
                    self.scope_set(s, now);
                }
            }
            View::Gen => {
                if let Some(i) = ui::button_hit(6, x, y) {
                    let mut s = self.gen.s;
                    let steps: [u32; 23] = [
                        1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10_000, 20_000, 50_000, 100_000,
                        200_000, 500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000, 10_000_000,
                    ];
                    match i {
                        0 => {
                            let k = Wave::ALL.iter().position(|&w| w == s.wave).unwrap_or(0);
                            s.wave = Wave::ALL[(k + 1) % 4];
                        }
                        1 => s.freq_hz = *steps.iter().rev().find(|&&f| f < s.freq_hz).unwrap_or(&1),
                        2 => s.freq_hz = *steps.iter().find(|&&f| f > s.freq_hz).unwrap_or(&s.freq_hz),
                        3 => s.amp_v = (s.amp_v - 0.1).max(0.5),
                        4 => s.amp_v = (s.amp_v + 0.1).min(3.0),
                        _ => s.on = !s.on,
                    }
                    s.freq_hz = gen::snap_freq(s.wave, s.freq_hz);
                    self.gen_set(s);
                } else if y > 180 && y < 220 {
                    // Duty row: left half -, right half +.
                    let mut s = self.gen.s;
                    s.duty = if x < 240 { s.duty.saturating_sub(500) } else { (s.duty + 500).min(10000) };
                    self.gen_set(s);
                }
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
                Some(5) => self.show(self.back_view),
                _ => {}
            },
        }
        let _ = dmm::button_of;
    }
}
