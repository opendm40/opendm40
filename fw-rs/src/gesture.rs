//! Touch gestures from the point track: tap, long press, swipes and drag.
//! The controller only reports points; the stock firmware has no gestures.

use crate::touch::{Event, Point};

/// Minimum travel for a swipe, pixels.
const SWIPE_MIN: i32 = 60;
/// Swipes must finish within this time, ms.
const SWIPE_MAX_MS: u64 = 800;
/// A press without movement longer than this is a long press, ms.
const LONG_MS: u64 = 700;
/// Movement tolerated for a tap / long press, pixels.
const SLOP: i32 = 15;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Finger down (for immediate button feedback).
    Down(Point),
    Tap(Point),
    LongPress(Point),
    /// Finger moved while down (start, current).
    Drag(Point, Point),
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    /// Side key went down / up: code 0..3.
    KeyDown(u8),
    KeyUp(u8),
}

pub struct Tracker {
    down: Option<(Point, u64)>,
    last: Point,
    moved: bool,
    long_sent: bool,
    keys: u8,
}

impl Tracker {
    pub const fn new() -> Self {
        Tracker { down: None, last: Point { x: 0, y: 0 }, moved: false, long_sent: false, keys: 0 }
    }

    /// Feed a touch state change. At most one gesture per call (keys first).
    pub fn event(&mut self, ev: &Event, now: u64) -> Option<Gesture> {
        if ev.keys != self.keys {
            let changed = ev.keys ^ self.keys;
            let code = changed.trailing_zeros() as u8;
            let down = ev.keys & changed != 0;
            self.keys = ev.keys;
            return Some(if down { Gesture::KeyDown(code) } else { Gesture::KeyUp(code) });
        }
        match (self.down, ev.points > 0) {
            (None, true) => {
                self.down = Some((ev.p, now));
                self.last = ev.p;
                self.moved = false;
                self.long_sent = false;
                Some(Gesture::Down(ev.p))
            }
            (Some((p0, _)), true) => {
                self.last = ev.p;
                if dist(p0, ev.p) > SLOP {
                    self.moved = true;
                }
                if self.moved { Some(Gesture::Drag(p0, ev.p)) } else { None }
            }
            (Some((p0, t0)), false) => {
                self.down = None;
                let p1 = self.last;
                let dx = p1.x as i32 - p0.x as i32;
                let dy = p1.y as i32 - p0.y as i32;
                let dt = now.saturating_sub(t0);
                if self.long_sent {
                    None
                } else if dt <= SWIPE_MAX_MS && (dx.abs() >= SWIPE_MIN || dy.abs() >= SWIPE_MIN) {
                    Some(if dx.abs() > dy.abs() {
                        if dx > 0 { Gesture::SwipeRight } else { Gesture::SwipeLeft }
                    } else if dy > 0 {
                        Gesture::SwipeDown
                    } else {
                        Gesture::SwipeUp
                    })
                } else if !self.moved {
                    Some(Gesture::Tap(p0))
                } else {
                    None
                }
            }
            (None, false) => None,
        }
    }

    /// Call periodically: reports a long press while the finger is still down.
    pub fn tick(&mut self, now: u64) -> Option<Gesture> {
        let (p0, t0) = self.down?;
        if !self.moved && !self.long_sent && now.saturating_sub(t0) >= LONG_MS {
            self.long_sent = true;
            return Some(Gesture::LongPress(p0));
        }
        None
    }
}

fn dist(a: Point, b: Point) -> i32 {
    (a.x as i32 - b.x as i32).abs().max((a.y as i32 - b.y as i32).abs())
}
