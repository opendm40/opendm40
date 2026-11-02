//! Loader mode: file list, keys, touch, main loop. All state is on this stack.
use crate::fat::{self, Vol};
use crate::hw::*;
use crate::touch::Touch;
use crate::usb::Usb;
use crate::{boot, install, lcd, spi, tail, touch};

/// Key arrows at x 0..8, their actions at x 14..98, list and status right of them.
pub const LIST_X: u16 = 102;
const ROW_H: u16 = 24;
const LIST_Y: u16 = 24;
/// Rows 24..288: eleven files at a time; the count itself is not limited.
const VISIBLE: u32 = 11;
pub const STATUS_Y: u16 = 292;
/// The INSTALL label is the button; the RUN key's arrow height.
const INST_Y: u16 = 124;
const LABEL_H: u16 = 50;
const SEL_BG: u16 = 0x001F;
const BTN_BG: u16 = 0x0400;
const BTN_OFF: u16 = 0x4208;

/// Vertical centres from emu/panel_draw.py: STRIP y 15, 4 cells of 13 mm; DISP
/// y 15.5, 49 mm = 320 px. Key code order: emu/peripherals/CST_Touch.cs + touch.rs.
const LABELS: [(u16, &[u8], u16); 4] = [
    (39, b"RESCAN", 0x0318),
    (INST_Y, b"INSTALL", BTN_BG),
    (209, b"PREV", 0x8200),
    (294, b"NEXT", 0x6010),
];

/// Decimal, right-aligned to `x`; returns its left edge.
fn num(mut x: u16, mut v: u32) -> u16 {
    loop {
        x -= 12;
        lcd::glyph(x, 4, b'0' + (v % 10) as u8, 0);
        v /= 10;
        if v == 0 {
            return x;
        }
    }
}

/// A key's coloured label with its arrow.
fn label(y: u16, s: &[u8], bg: u16) {
    lcd::fill(0, y - LABEL_H / 2, LIST_X - 2, LABEL_H, bg);
    for i in 0..9u16 {
        lcd::fill(i, y - i, 1, 2 * i + 1, 0xFFFF);
    }
    lcd::text(14, y - 7, s, bg);
}

struct Ui {
    vol: Option<Vol>,
    sel: u32,
    top: u32,
    count: u32,
    cl: u16,
    size: u32,
    sel_inst: bool,
    app_ok: bool,
    btn: Option<bool>,
}

pub fn status(s: &[u8]) {
    lcd::row(LIST_X, STATUS_Y, s, 0);
}

impl Ui {
    fn installed(&self, v: &Vol, cl: u16, size: u32) -> bool {
        cfg!(feature = "installed") && self.app_ok && install::is_marker(v, cl, size)
    }

    /// Walks the root once: draws the visible matches, counts them all, keeps
    /// the selected one's chain start, size and installed state.
    fn draw(&mut self) {
        let mut n = 0u32;
        if let Some(v) = self.vol {
            #[cfg(feature = "lfn")]
            let mut lfn = fat::Lfn::new();
            let mut i = 0;
            while i < v.entries {
                let mut e = [0u8; 32];
                spi::read(v.root + i * 32, &mut e);
                i += 1;
                if e[0] == 0 {
                    break;
                }
                #[cfg(feature = "lfn")]
                if e[0] != 0xE5 && e[11] == 0x0F {
                    lfn.part(&e);
                    continue;
                }
                if fat::matches(&e) {
                    let seen = n >= self.top && n < self.top + VISIBLE;
                    if seen || n == self.sel {
                        let cl = u16::from_le_bytes([e[26], e[27]]);
                        let size = u32::from_le_bytes([e[28], e[29], e[30], e[31]]);
                        let inst = self.installed(&v, cl, size);
                        if n == self.sel {
                            self.cl = cl;
                            self.size = size;
                            self.sel_inst = inst;
                        }
                        if seen {
                            let mut l = [b' '; 31];
                            if inst {
                                l[0] = b'*';
                            }
                            #[cfg(feature = "lfn")]
                            let long = lfn.valid(&e);
                            #[cfg(not(feature = "lfn"))]
                            let long = false;
                            #[cfg(feature = "lfn")]
                            if long {
                                l[1..].copy_from_slice(&lfn.name);
                            }
                            if !long {
                                let mut p = 1;
                                let mut k = 0;
                                while k < 11 {
                                    if k == 8 {
                                        l[p] = b'.';
                                        p += 1;
                                    }
                                    if e[k] != b' ' {
                                        l[p] = e[k];
                                        p += 1;
                                    }
                                    k += 1;
                                }
                            }
                            let y = LIST_Y + (n - self.top) as u16 * ROW_H;
                            let bg = if n == self.sel { SEL_BG } else { 0 };
                            // The mark in its own 16 px, the name after it.
                            lcd::fill(LIST_X, y, 16, ROW_H, bg);
                            lcd::text(LIST_X, y + 4, &l[..1], bg);
                            lcd::row(LIST_X + 16, y, &l[1..], bg);
                        }
                    }
                    n += 1;
                }
                #[cfg(feature = "lfn")]
                lfn.reset();
            }
        }
        self.count = n;
        let mut r = n.saturating_sub(self.top);
        while r < VISIBLE {
            let msg: &[u8] = if r + n != 0 {
                b""
            } else if self.vol.is_some() {
                b"NO FILES"
            } else {
                b"NO FILESYSTEM"
            };
            lcd::row(LIST_X, LIST_Y + r as u16 * ROW_H, msg, 0);
            r += 1;
        }
        self.counter();
        self.button();
    }

    /// "3/17" at the right of the title, only when the list does not fit.
    fn counter(&self) {
        lcd::fill(396, 0, 84, 24, 0);
        if self.count > VISIBLE {
            let x = num(480, self.count) - 12;
            lcd::glyph(x, 4, b'/', 0);
            num(x, self.sel + 1);
        }
    }

    /// Green when there is something to install, grey otherwise.
    fn button(&mut self) {
        if !cfg!(feature = "touch_ui") {
            return;
        }
        let on = self.count > 0 && !self.sel_inst;
        if self.btn == Some(on) {
            return;
        }
        self.btn = Some(on);
        let bg = if on { BTN_BG } else { BTN_OFF };
        label(INST_Y, b"INSTALL", bg);
    }

    /// Selects a file; the window follows, one row of context at its edges, so
    /// that tapping the last visible row also scrolls.
    fn select(&mut self, s: u32) {
        status(b"");
        self.sel = s;
        if s < self.top + 1 {
            self.top = s.saturating_sub(1);
        }
        if s + 2 > self.top + VISIBLE {
            self.top = s + 2 - VISIBLE;
        }
        self.top = self.top.min(self.count.saturating_sub(VISIBLE));
        self.draw();
    }

    fn install(&mut self, usb: &mut Usb) {
        let Some(v) = self.vol else { return };
        if self.count == 0 {
            return;
        }
        if self.sel_inst {
            return status(b"ALREADY INSTALLED");
        }
        usb.connect(false);
        let e = install::run(&v, self.cl, self.size);
        usb.connect(true);
        // A failure after the erase leaves no application.
        self.app_ok = boot::app_valid();
        self.draw();
        status(e);
    }

    /// One look at the volume. A single candidate installs without asking,
    /// once: only another rescan can try again.
    fn rescan(&mut self, usb: &mut Usb) {
        status(b"");
        self.vol = Vol::mount();
        self.app_ok = cfg!(feature = "installed") && boot::app_valid();
        self.sel = 0;
        self.top = 0;
        self.draw();
        if self.count == 1 && !self.sel_inst {
            self.install(usb);
        }
    }
}

pub fn run() -> ! {
    // PE2 is the power hold (the stock powers off by dropping it).
    pins_all();
    timer_init();
    lcd::init();
    lcd::text(LIST_X, 4, if tail::ok() { b"DM40 LOADER  *=INSTALLED" } else { b"NO CAL COPY" }, 0);
    if cfg!(feature = "labels") {
        for (y, act, bg) in LABELS {
            label(y, act, bg);
        }
    }
    spi::init();
    touch::init();
    let mut usb = Usb::new();
    // Only here, never on a rescan: a host may be rewriting the disk later.
    if cfg!(feature = "format") && Vol::mount().is_none() && fat::blank() {
        status(b"FORMATTING");
        fat::format();
    }
    let mut ui = Ui { vol: None, sel: 0, top: 0, count: 0, cl: 0, size: 0, sel_inst: false, btn: None, app_ok: false };
    ui.rescan(&mut usb);
    usb.connect(true);
    let mut t = Touch { held: false, last: 0 };
    loop {
        usb.poll();
        if cfg!(feature = "idle") && usb.idle_rescan() {
            ui.rescan(&mut usb);
        }
        // A key arrives as a tap on its label: 4 zones, halfway between the arrows.
        if let Some((x, y)) = t.poll() {
            if x < LIST_X {
                match (y + 4) / 85 {
                    0 => ui.rescan(&mut usb),
                    1 => ui.install(&mut usb),
                    2 => ui.select(if ui.sel == 0 { ui.count.saturating_sub(1) } else { ui.sel - 1 }),
                    _ => ui.select(if ui.sel + 1 >= ui.count { 0 } else { ui.sel + 1 }),
                }
            } else if y >= LIST_Y && y < LIST_Y + VISIBLE as u16 * ROW_H {
                let s = ui.top + ((y - LIST_Y) / ROW_H) as u32;
                if s < ui.count {
                    ui.select(s);
                }
            }
        }
    }
}
