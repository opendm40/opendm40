//! Pressed feedback (ui-review #11) on the menu tiles, the list pages'
//! rows / stepper keys and the generator boxes: the item under the finger
//! drawn pressed from touch-down until the touch ends or rolls off it (the
//! keypads' rule), one item redrawn per change. A child of app.rs; runs per
//! gesture, not per main-loop pass: linked into .cold (link-native.x).

use super::*;

impl App {
    /// Track the pressed item through gesture `g` (before the tap handlers).
    /// The guard is inlined into App::gesture (region B): every touch
    /// report of a drag on an instrument view (scope trigger / cursor drags
    /// while a stream runs) passes here and must not fetch from region C.
    #[inline(always)]
    pub(super) fn press_gesture(&mut self, g: &Gesture) {
        if self.pressed.is_some() || matches!(self.view, View::Menu | View::Gen | View::Settings | View::Limits | View::Record) {
            self.press_track(g);
        }
    }

    #[inline(never)]
    fn press_track(&mut self, g: &Gesture) {
        match g {
            Gesture::Down(p) => {
                self.press_end();
                if let Some(t) = self.press_hit(p.x, p.y) {
                    self.pressed = Some((self.view, t));
                    self.press_draw(t);
                }
            }
            Gesture::Drag(_, p, _) | Gesture::Move(_, p) if self.pressed.map(|q| q.1) == self.press_hit(p.x, p.y) => {}
            // A long press with no action of its own still taps on release.
            Gesture::Grab(_) | Gesture::LongPress(_) => {}
            _ => self.press_end(),
        }
    }

    /// The item a touch at (x, y) would act on (the tap handlers' hit
    /// tests), as pressed feedback (ui-review #11).
    fn press_hit(&self, x: u16, y: u16) -> Option<ui::RowPress> {
        let r = |row: usize, part: i8| Some(ui::RowPress { row: row as u8, part, on: true });
        match self.view {
            View::Menu => r(ui::main_menu_hit(x, y)?, 0),
            View::Gen if ui::gen_wave_hit(x, y) => r(4, 0),
            View::Gen => match ui::gen_box_hit(x, y)? {
                3 if self.gen.s.wave != Wave::Square => None,
                k => r(k, 0),
            },
            View::Settings | View::Limits | View::Record if ui::title_back_hit(y) => None,
            View::Settings => {
                let k = ui::settings_hit(self.set_page, y)?;
                r(k, if self.set_page == 0 && ui::SET_STEP_ROWS.contains(&k) { ui::settings_step(x) } else { 0 })
            }
            View::Limits => r(ui::list_hit(y, ui::LIMIT_ROWS.len())?, 0),
            View::Record => match ui::list_hit(y, ui::RECORD_ROWS.len())? {
                k if ui::REC_STEP_ROWS.contains(&k) => r(k, ui::record_step(x)),
                ui::REC_START_ROW => r(ui::REC_START_ROW, 0),
                _ => None,
            },
            _ => None,
        }
    }

    /// Draw item `t` of the current view pressed / released: the page's
    /// own draw call limited to it (ui::set_row_only), no full repaint.
    fn press_draw(&mut self, t: ui::RowPress) {
        match self.view {
            View::Menu => ui::menu_item(t.row as usize, ui::view_name(self.combo), t.on),
            View::Gen => ui::gen_press(&self.gen, t.row as usize, t.on),
            _ => {
                ui::set_row_only(Some(t));
                self.draw_static();
                ui::set_row_only(None);
            }
        }
    }

    /// Release the pressed item (still on its view: drawn normal again).
    fn press_end(&mut self) {
        if let Some((v, mut t)) = self.pressed.take() {
            if v == self.view {
                t.on = false;
                self.press_draw(t);
            }
        }
    }
}
