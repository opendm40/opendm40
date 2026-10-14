//! Touch gestures from the point track: tap, long press, swipes and drag, plus
//! the finger-count-aware global gestures (docs/scope-ui-plan.md §2.6):
//!
//! * One finger: `Down` at touch-down, `Drag` once it moved more than SLOP
//!   and its axis is locked, `LongPress` held still, and on release `Tap`
//!   (did not move) or a swipe.
//! * Axis lock (owner feedback: a horizontal drag must not also nudge the
//!   vertical): once past SLOP the drag is horizontal if |dx| >= 1.5 |dy|,
//!   vertical if |dy| >= 1.5 |dx|; a diagonal start waits until LOCK_MAX and
//!   then takes the larger component. The axis is kept until release; no
//!   `Drag` is sent before it is known. Consumers move only along it.
//! * Hold-to-grab (runtime: `set_grab_ms`, Settings "Scope drag", DBG:GRAB;
//!   0 = direct drag, the default): a drag must first be held within
//!   GRAB_SLOP for the grab time (`Grab` is sent then, the app beeps); a
//!   finger that moves earlier never drags (it can still swipe). The long
//!   press then needs LONG_HOLD_MS.
//! * A captured touch ends with `Up` (its release is never a tap / swipe),
//!   so the app can close a drag (gesture log, DBG:GEST?).
//!   A swipe needs >= SWIPE_MIN px within SWIPE_MAX_MS and a dominant axis
//!   (the travel along it more than twice the travel across). A touch the app
//!   captured (`capture()`, called when it took the `Down`/`Drag`, e.g. a drag
//!   that starts in the scope graph) is never a swipe or a tap: dragging a
//!   cursor or the trigger quickly no longer switches the view on release
//!   (docs/ui-review.md §2.3, the old classifier fired on any >= 60 px release).
//! * Two or more fingers: never a one-finger gesture. When a second finger
//!   lands during a one-finger touch the tracker emits `Cancel` once (the app
//!   undoes whatever the first finger's Down/Drag changed) and nothing else
//!   for the rest of that touch, until the release classifies it by the most
//!   fingers seen: the centroid of the points must move down (dy) at least
//!   MULTI_MIN[n] px, more than sideways, between the frame where n fingers
//!   were first seen and the last frame that still had n fingers (fingers
//!   lift one by one, and the slot order can change as they do). 2 fingers
//!   >= 60 px = `MultiSwipeDown(2)` (power-off confirm), 3 fingers >= 30 px
//!   (the stock threshold, gui_v2_6.h "Screenshot") = `MultiSwipeDown(3)`
//!   (screenshot). 4-5 points (a palm) are ignored.
//!
//! * Raw moves (the stock scope drag model, docs/stock-scope-ui.md sec. 10.2):
//!   when the app captures a touch-down after `raw_next(true)`, every later
//!   one-finger report that moved is sent as `Move(prev, p)` (no slop, no
//!   axis lock) instead of `Drag`; the app attributes each event itself.
//! * Pinch (two fingers, finger identity by nearest match between frames, the
//!   controller's slot order is not stable): once the separation along the
//!   fingers' dominant axis changed by PINCH_MIN px before the centroid moved
//!   PINCH_SWIPE px down, the touch is a pinch: `Pinch(axis, span0, span)`
//!   per two-finger frame (H = fingers side by side), `PinchEnd(true)` on
//!   release (`PinchEnd(false)` when a third finger lands); a pinch never
//!   ends as `MultiSwipeDown(2)`.
//!
//! The controller only reports points; the stock firmware has no one-finger
//! gestures.

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use crate::touch::{Event, Point, MAX_POINTS};

/// Minimum travel for a swipe, pixels.
const SWIPE_MIN: i32 = 60;
/// Swipes must finish within this time, ms.
const SWIPE_MAX_MS: u64 = 800;
/// A press without movement longer than this is a long press, ms.
const LONG_MS: u64 = 700;
/// Movement tolerated for a tap / long press, pixels.
const SLOP: i32 = 15;
/// A drag whose start is diagonal (neither component 1.5x the other) locks
/// to its larger component once it is this far from the touch-down, pixels.
const LOCK_MAX: i32 = 2 * SLOP;
/// A touch the app did not capture is still a tap when it never got farther
/// than this from the touch-down, pixels. Only captured touches (the scope
/// graph, sliders) use what a drag past SLOP does; for a button the drag
/// events go nowhere, and a fingertip that rolls or jitters by 16..30 px
/// (pressing harder, lifting off) used to lose the tap altogether.
const TAP_SLOP: i32 = LOCK_MAX;
/// A long press the app did not capture (it returned false for the
/// `LongPress`) still taps on release, so a slow press on a button that has
/// no long-press action is not lost. Needs app.rs to capture the long presses
/// it acts on (else those would also tap): it does (history clear, statistics
/// reset, strip -> DMM).
const TAP_AFTER_LONG: bool = true;
/// Press-and-hold-then-drag (owner A/B experiment, docs/combined-view-design.md
/// §6.2): hold time before a drag is accepted, ms; 0 = direct drag (default).
static GRAB_MS: AtomicU16 = AtomicU16::new(0);
/// Grab time the Settings toggle switches to.
pub const GRAB_DEFAULT_MS: u16 = 300;
/// Hold-to-grab: the finger must stay within this many px until the grab.
const GRAB_SLOP: i32 = 8;
/// Long press while hold-to-grab is on (after any grab), ms.
const LONG_HOLD_MS: u64 = 1200;

/// Hold-to-grab time in ms, 0 = direct drag.
pub fn grab_ms() -> u16 {
    GRAB_MS.load(Ordering::Relaxed)
}

pub fn set_grab_ms(ms: u16) {
    GRAB_MS.store(ms.min(5000), Ordering::Relaxed);
}
/// The next `capture` makes the touch a raw-move touch (`raw_next`).
static RAW_NEXT: AtomicBool = AtomicBool::new(false);

/// Ask for raw `Move` events on the touch the app is about to capture (call
/// it from the `Down` handler before returning true; any later `capture`
/// clears it).
pub fn raw_next(on: bool) {
    RAW_NEXT.store(on, Ordering::Relaxed);
}

/// Pinch: change of the finger separation that makes a two-finger touch a
/// pinch, pixels, and the downward centroid travel that makes it a swipe
/// instead (whichever comes first).
const PINCH_MIN: i32 = 20;
const PINCH_SWIPE: i32 = 30;

/// Minimum downward centroid travel of an n-finger swipe, pixels (index = n;
/// 0 = not a gesture).
const MULTI_MIN: [i32; MAX_POINTS + 1] = [0, 0, 60, 30, 0, 0];
/// An n-finger swipe must be done within this time of the n-th finger landing.
const MULTI_MAX_MS: u64 = 2000;

/// The locked axis of a one-finger drag.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Horizontal (x).
    H,
    /// Vertical (y).
    V,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Finger down (for immediate button feedback).
    Down(Point),
    Tap(Point),
    LongPress(Point),
    /// Finger moved while down (start, current, locked axis). Move only
    /// along the axis.
    Drag(Point, Point, Axis),
    /// Hold-to-grab only: the finger was held still for the grab time, a drag
    /// may follow.
    Grab(Point),
    /// Release of a captured touch (the app owned it: no tap / swipe).
    Up(Point),
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    /// A second finger landed during a one-finger touch: undo what its
    /// Down / Drag did and drop anything pending for it.
    Cancel,
    /// n >= 2 fingers swiped down (n = the most fingers seen in the touch).
    MultiSwipeDown(u8),
    /// Side key went down / up: code 0..3.
    KeyDown(u8),
    KeyUp(u8),
    /// Raw-move touch (`raw_next`): the finger moved from the previous
    /// report's point to this one.
    Move(Point, Point),
    /// Two-finger pinch: axis of the finger pair (H = side by side), the
    /// separation along it at the start and now, pixels.
    Pinch(Axis, u16, u16),
    /// Pinch over: released (true) or abandoned for a third finger (false).
    PinchEnd(bool),
}

/// Two-finger tracking for the pinch: the fingers (in identity order), the
/// start separation and centroid, the classification.
#[derive(Clone, Copy)]
struct Pair {
    a: Point,
    b: Point,
    axis: Axis,
    span0: i32,
    c0: (i32, i32),
    /// 0 undecided, 1 pinch, 2 not a pinch (swipe / third finger).
    state: u8,
}

#[derive(Clone, Copy)]
struct Multi {
    /// Most fingers seen in this touch.
    n: u8,
    /// Centroid and time when `n` fingers were first seen.
    c0: (i32, i32),
    t0: u64,
    /// Centroid and time of the last frame with `n` fingers.
    c1: (i32, i32),
    t1: u64,
}

pub struct Tracker {
    /// A touch is in progress (any number of fingers).
    active: bool,
    /// One-finger touch: start point and time.
    down: Option<(Point, u64)>,
    last: Point,
    moved: bool,
    /// Farthest distance from the touch-down so far (dist), pixels.
    far: i32,
    long_sent: bool,
    /// Drag axis, decided once past SLOP.
    axis: Option<Axis>,
    /// Hold-to-grab: held still long enough, drags are allowed.
    grabbed: bool,
    /// Captured with `capture_direct`: no hold-to-grab for this touch.
    direct: bool,
    /// Hold-to-grab: moved past GRAB_SLOP before the grab (never drags).
    early: bool,
    /// The app owns this touch (no swipe / tap on release).
    captured: bool,
    /// Set once the touch had two or more fingers.
    multi: Option<Multi>,
    /// Two-finger pinch tracking.
    pair: Option<Pair>,
    /// Raw-move touch (`raw_next` at the capture).
    raw: bool,
    keys: u8,
}

impl Tracker {
    pub const fn new() -> Self {
        Tracker {
            active: false,
            down: None,
            last: Point { x: 0, y: 0 },
            moved: false,
            far: 0,
            long_sent: false,
            axis: None,
            grabbed: false,
            direct: false,
            early: false,
            captured: false,
            multi: None,
            pair: None,
            raw: false,
            keys: 0,
        }
    }

    /// The app took the current touch (its `Down` or `Drag`): on release it
    /// is not classified as a swipe or a tap. No effect without a touch.
    pub fn capture(&mut self) {
        let raw = RAW_NEXT.swap(false, Ordering::Relaxed);
        if self.active && !self.captured {
            self.captured = true;
            self.raw = raw;
            crate::touch::log(b'G', 0, b'c', raw as u8, 0, self.last);
        }
    }

    /// As `capture`, and this touch drags at once even with hold-to-grab on
    /// (a slider, not the scope: the grab setting is the scope's).
    pub fn capture_direct(&mut self) {
        RAW_NEXT.store(false, Ordering::Relaxed);
        if self.active {
            self.captured = true;
            self.direct = true;
        }
    }

    /// Feed a touch state change. At most one gesture per call (keys first).
    pub fn event(&mut self, ev: &Event, now: u64) -> Option<Gesture> {
        let first_drag = self.axis.is_none();
        let g = self.step(ev, now);
        // DBG:TOUCH?: every decision, but only the first Drag of a touch (no
        // Move / Pinch events: one per report).
        if matches!(g, Some(Gesture::Move(..) | Gesture::Pinch(..))) {
            return g;
        }
        if !matches!(g, Some(Gesture::Drag(..))) || first_drag {
            log_gesture(g, now);
        }
        g
    }

    fn step(&mut self, ev: &Event, now: u64) -> Option<Gesture> {
        if ev.keys != self.keys {
            let changed = ev.keys ^ self.keys;
            let code = changed.trailing_zeros() as u8;
            let down = ev.keys & changed != 0;
            self.keys = ev.keys;
            return Some(if down { Gesture::KeyDown(code) } else { Gesture::KeyUp(code) });
        }
        let n = (ev.points as usize).min(MAX_POINTS) as u8;
        if n == 0 {
            if !self.active {
                return None;
            }
            self.active = false;
            let single = self.down.take();
            let pinch = self.pair.take().is_some_and(|p| p.state == 1);
            if let Some(m) = self.multi.take() {
                if pinch {
                    return Some(Gesture::PinchEnd(true));
                }
                return multi_gesture(&m);
            }
            let (p0, t0) = single?;
            return self.release(p0, t0, now);
        }
        if n >= 2 {
            let c = centroid(&ev.pts[..n as usize]);
            let had_single = self.active && self.down.is_some();
            // Pinch tracking (two fingers only).
            if n == 2 && !had_single {
                if let Some(g) = self.pinch_step(ev.pts[0], ev.pts[1], c) {
                    self.active = true;
                    return Some(g);
                }
            } else if n > 2 {
                if let Some(p) = self.pair.as_mut() {
                    let was = p.state;
                    p.state = 2;
                    if was == 1 {
                        self.active = true;
                        self.down = None;
                        let m = self.multi.get_or_insert(Multi { n, c0: c, t0: now, c1: c, t1: now });
                        if n > m.n {
                            *m = Multi { n, c0: c, t0: now, c1: c, t1: now };
                        }
                        return Some(Gesture::PinchEnd(false));
                    }
                }
            }
            match self.multi.as_mut() {
                // More fingers than before: the gesture (re)starts here.
                Some(m) if n <= m.n => {
                    if n == m.n {
                        m.c1 = c;
                        m.t1 = now;
                    } // else a finger lifted: keep the last full-count frame
                }
                _ => self.multi = Some(Multi { n, c0: c, t0: now, c1: c, t1: now }),
            }
            self.active = true;
            self.down = None;
            if had_single && n == 2 {
                // The first finger's touch becomes a two-finger one: start the
                // pair tracking on this frame.
                self.pair = None;
                let _ = self.pinch_step(ev.pts[0], ev.pts[1], c);
            }
            return if had_single { Some(Gesture::Cancel) } else { None };
        }
        // One finger.
        if self.multi.is_some() {
            return None; // fingers lifting after a multi-finger touch
        }
        let p = ev.p;
        match self.down {
            None => {
                self.active = true;
                self.down = Some((p, now));
                self.last = p;
                self.moved = false;
                self.far = 0;
                self.long_sent = false;
                self.axis = None;
                self.grabbed = false;
                self.direct = false;
                self.early = false;
                self.captured = false;
                self.raw = false;
                self.pair = None;
                Some(Gesture::Down(p))
            }
            Some((p0, _)) => {
                let prev = self.last;
                self.last = p;
                self.far = self.far.max(dist(p0, p));
                if dist(p0, p) > SLOP {
                    self.moved = true;
                }
                let hold = if self.direct { 0 } else { grab_ms() };
                if hold != 0 && !self.grabbed && dist(p0, p) > GRAB_SLOP {
                    self.early = true;
                }
                // The controller reports jitter as moves, so tick() may not run
                // while the finger rests: check the hold here too.
                if let Some(g) = self.grab(now) {
                    return Some(g);
                }
                // Raw-move touch: every report that moved, from the first.
                if self.raw {
                    return if p != prev { Some(Gesture::Move(prev, p)) } else { None };
                }
                // Hold-to-grab: moving before the grab never drags.
                if !self.moved || (hold != 0 && !self.grabbed) {
                    return None;
                }
                if self.axis.is_none() {
                    self.axis = lock_axis(p.x as i32 - p0.x as i32, p.y as i32 - p0.y as i32);
                }
                self.axis.map(|a| Gesture::Drag(p0, p, a))
            }
        }
    }

    /// Two-finger frame: keep the finger identity (nearest match to the last
    /// frame), classify pinch vs swipe, report the pinch.
    fn pinch_step(&mut self, p: Point, q: Point, c: (i32, i32)) -> Option<Gesture> {
        let pr = match self.pair.as_mut() {
            None => {
                let (dx, dy) = (q.x as i32 - p.x as i32, q.y as i32 - p.y as i32);
                let axis = if dx.abs() >= dy.abs() { Axis::H } else { Axis::V };
                let span0 = if axis == Axis::H { dx.abs() } else { dy.abs() };
                self.pair = Some(Pair { a: p, b: q, axis, span0, c0: c, state: 0 });
                return None;
            }
            Some(pr) => pr,
        };
        // Identity: the assignment with the smaller total travel.
        let (a, b) = if dist2(pr.a, p) + dist2(pr.b, q) <= dist2(pr.a, q) + dist2(pr.b, p) { (p, q) } else { (q, p) };
        pr.a = a;
        pr.b = b;
        let span = if pr.axis == Axis::H { (b.x as i32 - a.x as i32).abs() } else { (b.y as i32 - a.y as i32).abs() };
        if pr.state == 0 {
            if (span - pr.span0).abs() >= PINCH_MIN {
                pr.state = 1;
            } else if c.1 - pr.c0.1 >= PINCH_SWIPE {
                pr.state = 2;
            }
        }
        if pr.state == 1 {
            Some(Gesture::Pinch(pr.axis, pr.span0.clamp(1, 999) as u16, span.clamp(1, 999) as u16))
        } else {
            None
        }
    }

    /// One-finger release: swipe (not captured, dominant axis), tap or nothing.
    /// "Nothing" is logged for DBG:TOUCH? as `N` with the reason: 1 moved
    /// farther than TAP_SLOP (argument 100 + the farthest px, max 99), 2 after
    /// a long press (200), 3 diagonal swipe (logged as 255).
    fn release(&mut self, p0: Point, t0: u64, now: u64) -> Option<Gesture> {
        let p1 = self.last;
        let dx = p1.x as i32 - p0.x as i32;
        let dy = p1.y as i32 - p0.y as i32;
        let dt = now.saturating_sub(t0);
        let none = |why: u8, arg: i32| {
            // u16, saturated into the u8 log field: 1xx / 200 / 255 (= 300,
            // diagonal swipe); `why * 100` overflowed u8 for reason 3.
            crate::touch::log(b'G', now, b'N', (why as u16 * 100 + arg.clamp(0, 99) as u16).min(255) as u8, 0, p1);
            None
        };
        if self.captured {
            Some(Gesture::Up(p1))
        } else if self.long_sent {
            if TAP_AFTER_LONG && self.far <= TAP_SLOP { Some(Gesture::Tap(p0)) } else { none(2, 0) }
        } else if dt <= SWIPE_MAX_MS && (dx.abs() >= SWIPE_MIN || dy.abs() >= SWIPE_MIN) {
            if dx.abs() > 2 * dy.abs() {
                Some(if dx > 0 { Gesture::SwipeRight } else { Gesture::SwipeLeft })
            } else if dy.abs() > 2 * dx.abs() {
                Some(if dy > 0 { Gesture::SwipeDown } else { Gesture::SwipeUp })
            } else {
                none(3, 0) // diagonal: no swipe
            }
        } else if self.far <= TAP_SLOP {
            Some(Gesture::Tap(p0))
        } else {
            none(1, self.far)
        }
    }

    /// Call periodically: reports a long press while the finger is still down.
    pub fn tick(&mut self, now: u64) -> Option<Gesture> {
        let g = self.tick_step(now);
        log_gesture(g, now);
        g
    }

    fn tick_step(&mut self, now: u64) -> Option<Gesture> {
        let (p0, t0) = self.down?;
        if let Some(g) = self.grab(now) {
            return Some(g);
        }
        let long = if grab_ms() != 0 { LONG_HOLD_MS } else { LONG_MS };
        if !self.moved && !self.long_sent && now.saturating_sub(t0) >= long {
            self.long_sent = true;
            return Some(Gesture::LongPress(p0));
        }
        None
    }

    /// Hold-to-grab: `Grab` once the finger stayed within GRAB_SLOP for the
    /// grab time.
    fn grab(&mut self, now: u64) -> Option<Gesture> {
        let (p0, t0) = self.down?;
        let hold = if self.direct { 0 } else { grab_ms() as u64 };
        if hold != 0 && !self.early && !self.grabbed && now.saturating_sub(t0) >= hold {
            self.grabbed = true;
            return Some(Gesture::Grab(p0));
        }
        None
    }

    /// Test hook (DBG:SWIPE D,n): run a synthetic `fingers`-finger downward
    /// swipe (120 px in 200 ms, fingers 60 px apart) through the tracker and
    /// return what the release yields (`MultiSwipeDown(n)` for 2 or 3 fingers,
    /// `SwipeDown` for one). Anything in progress is dropped first; the
    /// intermediate Down / Drag / Cancel are not reported.
    pub fn synth_swipe_down(&mut self, fingers: u8, now: u64) -> Option<Gesture> {
        let keys = self.keys;
        *self = Tracker::new();
        self.keys = keys;
        let n = fingers.clamp(1, MAX_POINTS as u8);
        let mut ev = Event { keys, ..Event::default() };
        for step in 0..=8u16 {
            ev.points = n;
            for i in 0..n as usize {
                ev.pts[i] = Point { x: 120 + 60 * i as u16, y: 100 + 15 * step };
            }
            ev.p = ev.pts[0];
            self.event(&ev, now + step as u64 * 25);
        }
        self.event(&Event { keys, ..Event::default() }, now + 225)
    }

    /// Test hook (DBG:DRAG x0,y0,x1,y1,ms[,hold]): run a synthetic one-finger
    /// touch through the tracker: down at p0, held still `hold` ms, moved to
    /// p1 in `ms` ms (a point every 20 ms), released. Every gesture goes to
    /// `app` at once (timestamps are synthetic, from `now`); when it returns
    /// true the touch is captured, as in the main loop. Anything in progress
    /// is dropped first.
    pub fn synth_drag(&mut self, p0: Point, p1: Point, ms: u16, hold: u16, now: u64, mut app: impl FnMut(Gesture) -> bool) {
        let keys = self.keys;
        *self = Tracker::new();
        self.keys = keys;
        let mut feed = |tr: &mut Tracker, g: Option<Gesture>| {
            if let Some(g) = g {
                if app(g) {
                    tr.capture();
                }
            }
        };
        let ev = |p: Point| {
            let mut ev = Event { keys, points: 1, p, ..Event::default() };
            ev.pts[0] = p;
            ev
        };
        let g = self.event(&ev(p0), now);
        feed(self, g);
        let t1 = now + hold as u64;
        let g = self.tick(t1);
        feed(self, g);
        let n = (ms / 20).max(1) as i32;
        for i in 1..=n {
            let lerp = |a: u16, b: u16| (a as i32 + (b as i32 - a as i32) * i / n) as u16;
            let g = self.event(&ev(Point { x: lerp(p0.x, p1.x), y: lerp(p0.y, p1.y) }), t1 + (ms as i32 * i / n) as u64);
            feed(self, g);
        }
        let g = self.event(&Event { keys, ..Event::default() }, t1 + ms as u64 + 10);
        feed(self, g);
    }
}

/// DBG:TOUCH? gesture line: code, argument, point. Codes: D down, T tap,
/// L long press, M first drag of a touch (argument 0 H / 1 V; x,y = the
/// current point), G grab, U up (captured release), < > ^ v swipes left /
/// right / up / down, C cancel, W n-finger swipe down (argument n), K / k side
/// key down / up (argument code), c the app captured the touch (logged by
/// `capture`, time 0), N release that is nothing (see `release`).
fn log_gesture(g: Option<Gesture>, now: u64) {
    let z = Point { x: 0, y: 0 };
    let (c, a, p) = match g {
        None => return,
        Some(Gesture::Down(p)) => (b'D', 0, p),
        Some(Gesture::Tap(p)) => (b'T', 0, p),
        Some(Gesture::LongPress(p)) => (b'L', 0, p),
        Some(Gesture::Drag(_, p, ax)) => (b'M', (ax == Axis::V) as u8, p),
        Some(Gesture::Grab(p)) => (b'G', 0, p),
        Some(Gesture::Up(p)) => (b'U', 0, p),
        Some(Gesture::SwipeLeft) => (b'<', 0, z),
        Some(Gesture::SwipeRight) => (b'>', 0, z),
        Some(Gesture::SwipeUp) => (b'^', 0, z),
        Some(Gesture::SwipeDown) => (b'v', 0, z),
        Some(Gesture::Cancel) => (b'C', 0, z),
        Some(Gesture::MultiSwipeDown(n)) => (b'W', n, z),
        Some(Gesture::KeyDown(k)) => (b'K', k, z),
        Some(Gesture::KeyUp(k)) => (b'k', k, z),
        Some(Gesture::Move(_, p)) => (b'm', 0, p),
        Some(Gesture::Pinch(ax, _, s)) => (b'P', (ax == Axis::V) as u8, Point { x: s, y: 0 }),
        Some(Gesture::PinchEnd(ok)) => (b'E', ok as u8, z),
    };
    crate::touch::log(b'G', now, c, a, 0, p);
}

/// Axis of a drag that moved (dx, dy) past SLOP, None while undecided.
fn lock_axis(dx: i32, dy: i32) -> Option<Axis> {
    let (ax, ay) = (dx.abs(), dy.abs());
    if 2 * ax >= 3 * ay {
        Some(Axis::H)
    } else if 2 * ay >= 3 * ax {
        Some(Axis::V)
    } else if ax.max(ay) > LOCK_MAX {
        Some(if ax >= ay { Axis::H } else { Axis::V })
    } else {
        None
    }
}

/// Release of a touch that had n >= 2 fingers.
fn multi_gesture(m: &Multi) -> Option<Gesture> {
    let min = MULTI_MIN[m.n as usize];
    let dx = m.c1.0 - m.c0.0;
    let dy = m.c1.1 - m.c0.1;
    if min > 0 && dy >= min && dy > dx.abs() && m.t1.saturating_sub(m.t0) <= MULTI_MAX_MS {
        Some(Gesture::MultiSwipeDown(m.n))
    } else {
        None
    }
}

fn centroid(pts: &[Point]) -> (i32, i32) {
    let n = pts.len().max(1) as i32;
    let (sx, sy) = pts.iter().fold((0i32, 0i32), |(sx, sy), p| (sx + p.x as i32, sy + p.y as i32));
    (sx / n, sy / n)
}

fn dist2(a: Point, b: Point) -> i32 {
    let (dx, dy) = (a.x as i32 - b.x as i32, a.y as i32 - b.y as i32);
    dx * dx + dy * dy
}

fn dist(a: Point, b: Point) -> i32 {
    (a.x as i32 - b.x as i32).abs().max((a.y as i32 - b.y as i32).abs())
}
