//! Modal overlays drawn over any view: the power-off confirm (2-finger swipe
//! down, docs/user-feedback.md UF-18, ../docs/history/scope-ui-plan.md §2.6: never an instant
//! off) and short toasts (screenshot / CSV recording results). There is no frame buffer: while
//! an overlay is up the app keeps its full redraw pending and draws nothing
//! else, and when it goes the view is redrawn from scratch.
//!
//! Layout (stock confirm dialog 0x08074ce0: box (104,92,292,150), title bar
//! 40 px, colours below), all in screen coordinates for DBG:TAP:
//!   box    (94, 80) 292 x 160
//!   No     (106, 180) 128 x 48   -> cancel
//!   Yes    (246, 180) 128 x 48   -> power off
//! Toast: (40, 132) 400 x 56, centred text at glyph spacing 2 (ASCII26
//! "f" touches its neighbour below 2; "Disk in use by PC - eject first" is
//! 388 px). While the data-logger panel is up (`LOG_BOX` (40, 64) 400 x
//! 146, the toast's width; the GUI frozen: nothing repaints the view until
//! the session ends) a toast takes the panel's body instead, (40, 102) 400
//! x 108 under its title (`LOG_TOAST`); the panel's refresh waits while it
//! is up and the whole panel is drawn again after it (`toast_gone`), which
//! covers it exactly.

use crate::lcd::{self, Color};
use crate::stockfont::{self as sf, rect};

/// Stock dialog colours (gui_v2_6.h: title 0x04fc, body 0x10a3).
pub const TITLE_BG: Color = 0x04FC;
const BODY_BG: Color = 0x10A3;
const FG: Color = 0xF79E;
const BTN: Color = 0x2966;
const BTN_RED: Color = 0xB0A2;
const FRAME: Color = 0x94D3;
const OK_BG: Color = 0x0340;
const ERR_BG: Color = 0x9800;

pub const BOX: (u16, u16, u16, u16) = (94, 80, 292, 160);
pub const NO: (u16, u16, u16, u16) = (106, 180, 128, 48);
pub const YES: (u16, u16, u16, u16) = (246, 180, 128, 48);
pub const TOAST: (u16, u16, u16, u16) = (40, 132, 400, 56);

/// Auto-cancel of the power-off confirm, ms.
pub const CONFIRM_MS: u64 = 3000;
/// Toast duration, ms.
pub const TOAST_MS: u64 = 1500;
/// Auto-cancel of the energy reset confirm, ms. It was 10 s: on the device
/// (c50425f) a Yes that came ~13 s after the long press (a screenshot read in
/// between) landed on the bare power page and reset nothing; a question
/// that answers itself while it is being read is worse than one that stays
/// (the wiring dialog / the firmware question: 60 s).
pub const ENERGY_MS: u64 = 60_000;
/// Scope AUTO-set fell back from AC to DC coupling (UF-04, app.rs autoset).
pub const AUTO_DC: &str = "AUTO: DC (no AC signal)";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Power-off confirm, cancelled at `until` (ms).
    PowerOff { until: u64 },
    /// Toast, gone at `until` (ms).
    Toast { until: u64 },
    /// Power-meter wiring dialog (ui::pw::entry_dialog), cancelled at
    /// `until` (ms).
    PowerEntry { until: u64 },
    /// Power-meter energy reset confirm (`energy_reset`, the power-off
    /// confirm's box and buttons), cancelled at `until` (ms).
    EnergyReset { until: u64 },
    /// A firmware file was copied onto the USB disk (vwrite.rs): install?
    /// (`fw_install`, the power-off confirm's box and buttons); No at
    /// `until` (ms).
    FwInstall { until: u64 },
    /// Fuse check (`fuse_check`; the Current Settings page): `stage` 0
    /// instructions, 1 measuring, 2 the verdict; closed at `until` (ms).
    FuseCheck { until: u64, stage: u8 },
    /// Power Settings Zero... / V cal... (`ui::pw::job_dialog`, the fuse
    /// check's box): what the job needs, Cancel / Start; `job` pw::JOB_*.
    PowJob { until: u64, job: u8 },
}

impl Overlay {
    pub fn until(&self) -> u64 {
        match *self {
            Overlay::PowerOff { until }
            | Overlay::Toast { until }
            | Overlay::PowerEntry { until }
            | Overlay::EnergyReset { until }
            | Overlay::FwInstall { until }
            | Overlay::FuseCheck { until, .. }
            | Overlay::PowJob { until, .. } => until,
        }
    }

    /// A confirm / dialog (every overlay but a toast): it takes every
    /// gesture, and a toast does not replace it (`App::toast` keeps the
    /// toast for after it).
    pub fn modal(&self) -> bool {
        !matches!(self, Overlay::Toast { .. })
    }
}

fn inside(r: (u16, u16, u16, u16), x: u16, y: u16) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

fn frame(r: (u16, u16, u16, u16), c: Color) {
    lcd::fill_rect(r.0, r.1, r.2, 1, c);
    lcd::fill_rect(r.0, r.1 + r.3 - 1, r.2, 1, c);
    lcd::fill_rect(r.0, r.1, 1, r.3, c);
    lcd::fill_rect(r.0 + r.2 - 1, r.1, 1, r.3, c);
}

/// `s` centred in `r` (the stock 26 px ASCII font; fills `r` with `bg`).
fn centred(r: (u16, u16, u16, u16), s: &str, fg: Color, bg: Color) {
    centred_sp(r, s, fg, bg, 0);
}

fn centred_sp(r: (u16, u16, u16, u16), s: &str, fg: Color, bg: Color, spacing: i8) {
    sf::label(sf::ASCII26, rect(r.0 as i16, r.1 as i16, r.2 as i16, r.3 as i16), s, fg, bg, 8, spacing);
}

fn button(r: (u16, u16, u16, u16), label: &str, bg: Color) {
    centred(r, label, FG, bg);
    frame(r, FRAME);
}

/// Draw the power-off confirm.
pub fn power_off() {
    let b = BOX;
    centred((b.0, b.1, b.2, 40), "Power off?", FG, TITLE_BG);
    centred((b.0, b.1 + 40, b.2, 60), "Cancels in 3 s", FG, BODY_BG);
    lcd::fill_rect(b.0, b.1 + 100, b.2, b.3 - 100, BODY_BG);
    frame(b, FRAME);
    button(NO, "No", BTN);
    button(YES, "Yes", BTN_RED);
}

/// Draw the energy reset confirm (power meter: a long press on the energy
/// readout / the side column); hit test = `power_off_hit`.
pub fn energy_reset() {
    let b = BOX;
    centred((b.0, b.1, b.2, 40), "Reset energy?", FG, TITLE_BG);
    centred((b.0, b.1 + 40, b.2, 60), "Wh, VAh and the time", FG, BODY_BG);
    lcd::fill_rect(b.0, b.1 + 100, b.2, b.3 - 100, BODY_BG);
    frame(b, FRAME);
    button(NO, "No", BTN);
    button(YES, "Yes", BTN_RED);
}

/// The firmware install question (vwrite.rs: a valid update file copied
/// onto the USB disk is staged as DM40FW.BIN); hit test = `power_off_hit`.
pub const FW_TITLE: &str = "Firmware received";
pub const FW_BODY: &str = "Install it now?";
/// Auto-answer No, ms.
pub const FW_MS: u64 = 60_000;
/// The toast for a DM40*.BIN that is no valid update file.
pub const FW_BAD: &str = "Not a DM40 update file";

pub fn fw_install() {
    let b = BOX;
    centred((b.0, b.1, b.2, 40), FW_TITLE, FG, TITLE_BG);
    centred((b.0, b.1 + 40, b.2, 60), FW_BODY, FG, BODY_BG);
    lcd::fill_rect(b.0, b.1 + 100, b.2, b.3 - 100, BODY_BG);
    frame(b, FRAME);
    button(NO, "No", BTN);
    button(YES, "Yes", BTN_RED);
}

/// Power-off confirm hit test: Some(true) = Yes, Some(false) = No, None =
/// elsewhere.
pub fn power_off_hit(x: u16, y: u16) -> Option<bool> {
    if inside(YES, x, y) {
        Some(true)
    } else if inside(NO, x, y) {
        Some(false)
    } else {
        None
    }
}

/// A touch at (x, y) is on the toast's box (`TOAST`): a tap there dismisses
/// the toast; elsewhere a toast takes no touch (app.rs `gesture`).
pub fn toast_hit(x: u16, y: u16) -> bool {
    inside(toast_rect(), x, y)
}

/// Fuse-check dialog box (wider than the confirm: two body lines of up to
/// ~30 characters): title bar 40, lines at +40 / +80, buttons at +130.
pub const FBOX: (u16, u16, u16, u16) = (40, 56, 400, 192);
pub const FLEFT: (u16, u16, u16, u16) = (56, 186, 160, 48);
pub const FRIGHT: (u16, u16, u16, u16) = (264, 186, 160, 48);
/// Title colours of the fuse verdicts.
pub const FUSE_OK_BG: Color = OK_BG;
pub const FUSE_BAD_BG: Color = ERR_BG;
pub const FUSE_ODD_BG: Color = 0xA3C0;

/// Draw the fuse-check dialog: a title on `title_bg`, two lines, the left /
/// right buttons when given (hit test `fuse_hit`).
#[inline(never)] // .cold (link-native.x): dialog draw
pub fn fuse_check(title: &str, title_bg: Color, l1: &str, l2: &str, left: Option<&str>, right: Option<&str>) {
    // Glyph spacing 2 as the toasts (ASCII26 letters touch at 0).
    let b = FBOX;
    centred_sp((b.0, b.1, b.2, 40), title, FG, title_bg, 2);
    centred_sp((b.0, b.1 + 40, b.2, 40), l1, FG, BODY_BG, 2);
    centred_sp((b.0, b.1 + 80, b.2, 40), l2, FG, BODY_BG, 2);
    lcd::fill_rect(b.0, b.1 + 120, b.2, b.3 - 120, BODY_BG);
    frame(b, FRAME);
    for (r, t) in [(FLEFT, left), (FRIGHT, right)] {
        if let Some(t) = t {
            centred_sp(r, t, FG, BTN, 2);
            frame(r, FRAME);
        }
    }
}

/// Fuse-check hit test: Some(false) = left, Some(true) = right.
pub fn fuse_hit(x: u16, y: u16) -> Option<bool> {
    if inside(FRIGHT, x, y) {
        Some(true)
    } else if inside(FLEFT, x, y) {
        Some(false)
    } else {
        None
    }
}

/// The toast's box: `TOAST`, or the logger panel's body while the panel is
/// up (`LOG_TOAST`; see the module header).
pub fn toast_rect() -> (u16, u16, u16, u16) {
    if log_state().shown { LOG_TOAST } else { TOAST }
}

/// Draw a toast (`ok` = green, else red). Over the logger panel: in its
/// body, the panel's refresh held until `toast_gone`.
pub fn toast(s: &str, ok: bool) {
    let r = toast_rect();
    let st = log_state();
    st.hold |= st.shown;
    let bg = if ok { OK_BG } else { ERR_BG };
    centred_sp(r, s, FG, bg, 2);
    frame(r, FRAME);
}

/// The toast went (expired or dismissed; app.rs): over the logger panel,
/// draw the whole panel again (it covers `LOG_TOAST`).
pub fn toast_gone() {
    let st = log_state();
    if st.hold {
        st.hold = false;
        st.part = 0;
        st.up = false;
        st.next = 0;
    }
}

// --- Data-logger panel (FR-38) ------------------------------------------------

/// Logger panel over any view while LOG:STATe ON: the GUI is frozen then (the
/// HY3131 belongs to the logger, the DMM reading is not updated), so the
/// panel says so instead of leaving a stale reading up. Covers the main
/// reading of the DMM view. As wide as `TOAST` (a toast during a session
/// goes into its body, `LOG_TOAST`).
const LOG_BOX: (u16, u16, u16, u16) = (TOAST.0, 64, TOAST.2, 146);
const LOG_ROW: [u16; 3] = [102, 138, 174];
/// A toast while the panel is up: the panel's body (its three rows, under
/// the title), so the panel's next full draw covers it.
pub const LOG_TOAST: (u16, u16, u16, u16) = (LOG_BOX.0, LOG_ROW[0], LOG_BOX.2, LOG_BOX.1 + LOG_BOX.3 - LOG_ROW[0]);
/// Refresh of the rate / count rows, ms.
const LOG_REFRESH_MS: u64 = 1000;

/// Panel progress: `part` = next part to draw (0 title, 1 rate, 2 count,
/// 3 hint, 4 frame, 5 = nothing pending); `up` = the whole panel has been
/// drawn once (later passes redraw only 1 and 2); `next` = next refresh, ms;
/// `shown` = main.rs draws the panel (a host / disk session, `log_panel_on`);
/// `hold` = a toast is over it (`toast`, `toast_gone`): nothing drawn.
struct LogPanel {
    part: u8,
    up: bool,
    next: u64,
    shown: bool,
    hold: bool,
    /// `log_panel_room`: the ring's fill last pass, when it last went down
    /// (ms, low 16 bits: compared every pass, wrapping; keeps the static
    /// at 16 bytes).
    w: u16,
    drain_t: u16,
}

static mut LOG: LogPanel = LogPanel { part: 0, up: false, next: 0, shown: false, hold: false, w: 0, drain_t: 0 };

/// A panel part draws only while the logger's ring holds fewer records
/// than this: a quarter of logger::CAP (main.rs asserts it), so a part of
/// up to ~20 ms at 76.8 kS/s (1536 samples) still fits before the next
/// drain.
pub const LOG_PANEL_ROOM: u32 = 512;
/// ... or when the ring has not gone down for this long (no host drains
/// it: the panel must still come up).
const LOG_PANEL_STALE_MS: u16 = 200;

/// main.rs, every pass: may a panel part be drawn now? `waiting` = the
/// ring's fill during a session (None outside one). A part costs a few ms
/// of LCD writes (a ~400 x 36 label); device 2f56ff9: at 76.8 kS/s the
/// first draw's parts, one per pass between the host's first queries, kept
/// LOG:DATA? from the ring for ~29 ms and 113-275 samples were overwritten
/// after the first 2048 of every session.
#[inline(never)]
pub fn log_panel_room(now: u64, waiting: Option<u32>) -> bool {
    let st = log_state();
    let t = now as u16;
    let Some(w) = waiting else {
        (st.w, st.drain_t) = (0, t);
        return false;
    };
    let w = w.min(u16::MAX as u32) as u16;
    if w < st.w {
        st.drain_t = t;
    }
    st.w = w;
    (w as u32) < LOG_PANEL_ROOM || t.wrapping_sub(st.drain_t) >= LOG_PANEL_STALE_MS
}

fn log_state() -> &'static mut LogPanel {
    unsafe { &mut *core::ptr::addr_of_mut!(LOG) }
}

/// What the logger panel shows (main.rs fills it from the Logger).
#[derive(Clone, Copy)]
pub struct LogInfo {
    /// Measured rate, S/s (0 until two samples: the nominal one is shown
    /// with "~").
    pub hz: u32,
    /// Samples captured this session.
    pub n: u32,
    /// Nominal (chip) rate, S/s (Logger::nominal_mhz, rounded).
    pub nominal_hz: u32,
    /// AD2 source (FR-38 B): the rate row reads "AD2 <measured>/<nominal>".
    pub ad2: bool,
    /// Conversions not captured (Logger::lost); 0 = the "Display paused" hint.
    pub lost: u32,
    /// A fast-logger session to the disk (fastlog.rs): its file / progress.
    pub disk: Option<LogDisk>,
}

/// The disk part of the logger panel.
#[derive(Clone, Copy)]
pub struct LogDisk {
    /// Allocating / erasing (no samples yet): `pct` done.
    pub prep: bool,
    pub pct: u8,
    /// FASTnnn.DLG, KiB written.
    pub file: u16,
    pub kb: u32,
}

/// The three text rows of the logger panel (rate, count, hint / loss).
pub fn log_panel_text(i: &LogInfo, row: usize, t: &mut [u8; 32]) -> usize {
    let nominal = i.nominal_hz;
    let mut k = 0;
    let mut put = |t: &mut [u8; 32], s: &[u8]| {
        let n = s.len().min(32 - k);
        t[k..k + n].copy_from_slice(&s[..n]);
        k += n;
    };
    let mut d = [0u8; 10];
    if let Some(dk) = i.disk.filter(|x| x.prep) {
        match row {
            0 => put(t, b"Preparing disk"),
            1 => {
                let n = dec(&mut d, dk.pct as u32);
                put(t, &d[..n]);
                put(t, b" %");
            }
            _ => put(t, b"Tap to cancel"),
        }
        return k;
    }
    match row {
        0 => {
            if i.ad2 {
                put(t, b"AD2 ");
            }
            if i.hz == 0 && nominal != 0 {
                put(t, b"~");
                let n = dec(&mut d, nominal);
                put(t, &d[..n]);
            } else {
                let n = dec(&mut d, i.hz);
                put(t, &d[..n]);
                if i.ad2 {
                    put(t, b"/");
                    let n = dec(&mut d, nominal);
                    put(t, &d[..n]);
                }
            }
            put(t, b" Sa/s");
        }
        1 => {
            let n = dec(&mut d, i.n);
            put(t, &d[..n]);
            put(t, b" samples");
        }
        _ => {
            if let Some(dk) = i.disk.filter(|_| i.lost == 0) {
                // "FAST003 1234 KB" (tap stops: the first row's job)
                put(t, b"FAST");
                let n = dec(&mut d, 1000 + dk.file as u32);
                put(t, &d[1..n]);
                put(t, b" ");
                let n = dec(&mut d, dk.kb);
                put(t, &d[..n]);
                put(t, b" KB");
            } else if i.lost == 0 {
                put(t, b"Display paused");
            } else {
                put(t, b"lost ");
                let n = dec(&mut d, i.lost);
                put(t, &d[..n]);
                // per mille of the conversions due, one decimal of a percent
                let pm = (i.lost as u64 * 1000 / (i.lost as u64 + i.n as u64).max(1)) as u32;
                put(t, b" (");
                let n = dec(&mut d, pm / 10);
                put(t, &d[..n]);
                put(t, b".");
                let n = dec(&mut d, pm % 10);
                put(t, &d[..n]);
                put(t, b"%)");
            }
        }
    }
    k
}

/// Call every main-loop pass while logging and drawing is otherwise
/// allowed. Draws at most ONE part per call (one label, a ~400 x 36 fill +
/// glyphs, roughly 0.5 ms) so the logger's sampling loses at most a sample
/// or two per part at AD1 rates (at AD2 rates a part costs several
/// conversions: LOG:GAPs? shows it); the rate, sample count and loss rows
/// are refreshed once per second (three passes).
pub fn log_panel(now: u64, info: LogInfo) {
    let st = log_state();
    if st.hold {
        return;
    }
    let b = LOG_BOX;
    let row = |i: usize| (b.0 + 1, LOG_ROW[i], b.2 - 2, 36);
    let mut t = [0u8; 32];
    match st.part {
        // Letter spacing 2 as the toasts (ASCII26 glyphs touch at 0).
        0 => centred_sp((b.0, b.1, b.2, 38), "DATA LOGGING", FG, ERR_BG, 2),
        p @ 1..=3 => {
            let k = log_panel_text(&info, p as usize - 1, &mut t);
            let fg = if p == 3 && info.lost == 0 { FRAME } else { FG };
            centred_sp(row(p as usize - 1), core::str::from_utf8(&t[..k]).unwrap_or(""), fg, BODY_BG, 2);
        }
        4 => frame(b, FRAME),
        _ => {
            if now >= st.next {
                st.part = 1; // rate now, count and loss on the next passes
            }
            return;
        }
    }
    // The frame is static: a refresh ends after the loss row.
    st.part = if st.part == 3 && st.up { 5 } else { st.part + 1 };
    if st.part == 5 {
        st.up = true;
        st.next = now + LOG_REFRESH_MS;
    }
}

/// Between two refreshes: `log_panel` would draw nothing (main.rs skips
/// building its `LogInfo`: the loss and rate need 64-bit divisions, ~10 %
/// of the main loop's CPU at 76.8 kS/s on the device model when done every
/// pass).
#[inline(always)]
pub fn log_panel_idle(now: u64) -> bool {
    let st = log_state();
    st.hold || (st.part >= 5 && now < st.next)
}

/// main.rs, every pass: the panel is drawn (a session freezes the GUI) or
/// not. A toast then goes into the panel (`toast_rect`).
#[inline(always)]
pub fn log_panel_on(on: bool) {
    log_state().shown = on;
}

/// The panel is up (`log_panel_on`).
pub fn log_panel_shown() -> bool {
    log_state().shown
}

/// The logger stopped: the next session draws the whole panel again (the
/// caller repaints the view). A toast over a panel still shown keeps it
/// held (DBG:AD2:DRAW 2's repaint resets the panel every 100 ms).
pub fn log_panel_reset() {
    let st = log_state();
    *st = LogPanel { part: 0, up: false, next: 0, shown: st.shown, hold: st.hold && st.shown, w: st.w, drain_t: st.drain_t };
}

/// `v` in decimal at the start of `t`; returns the length.
fn dec(t: &mut [u8], v: u32) -> usize {
    let mut d = [0u8; 10];
    let (mut k, mut v) = (d.len(), v);
    loop {
        k -= 1;
        d[k] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let n = d.len() - k;
    t[..n].copy_from_slice(&d[k..]);
    n
}
