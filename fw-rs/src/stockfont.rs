//! Text with the stock firmware's own bitmap fonts, read in place from the
//! internal flash (the stock app stays installed, we only read it).
//! Format and placement rules: firmware/re/analysis/gui_v2_6.h section 5.
//!
//! Font object F = { D*, vtable*, u16 line_h, i16 descent }.
//!   class A: D = { u8 *bits, entry *tab, u32 n, u8 fmt }, entry 12 B =
//!            { u32 code, u32 offset, i8 yoff, u8 w, u8 h, u8 adv }
//!   class B: entry 8 B = { u32 code, u32 off << 8 | w }, h = line_h, adv = w
//! fmt 4 = 4 bpp, else 1 bpp; bitmap is one bit stream, column-major
//! (pixel p -> x = p / h, y = p % h), MSB first.
//! glyph top = line_top + line_h - descent - h - yoff; pen += adv + spacing.

use crate::lcd::{self, Color};

const VT_A: u32 = 0x0802_3509;
const VT_B: u32 = 0x0802_36F5;

fn r32(a: u32) -> u32 {
    unsafe { core::ptr::read_volatile(a as *const u32) }
}
fn r8(a: u32) -> u8 {
    unsafe { core::ptr::read_volatile(a as *const u8) }
}

#[derive(Clone, Copy)]
pub struct Font(pub u32);

#[derive(Clone, Copy)]
pub struct Glyph {
    bits: u32,
    four: bool,
    pub w: u8,
    pub h: u8,
    pub adv: u8,
    pub yoff: i8,
}

impl Glyph {
    /// Coverage 0..15 of pixel (x, y) inside the glyph box.
    fn alpha(&self, x: u32, y: u32) -> u8 {
        let p = x * self.h as u32 + y;
        if self.four {
            let b = r8(self.bits + p / 2);
            if p & 1 == 0 { b >> 4 } else { b & 0xF }
        } else {
            let b = r8(self.bits + p / 8);
            if b & (0x80 >> (p & 7)) != 0 { 15 } else { 0 }
        }
    }
}

// Fonts used by the multimeter page (addresses from gui_v2_6.h).
pub const F78: Font = Font(0x0806_5070); // main readout
pub const UNIT: Font = Font(0x0805_ECB8); // main unit
pub const LETTERS: Font = Font(0x0805_DAB4); // button letters V/A, digits/units
pub const F21: Font = Font(0x0806_67F8); // status bar
pub const F14: Font = Font(0x0805_B138); // small labels, 1 bpp
pub const HIST: Font = Font(0x0806_C3E8); // history boxes, 1 bpp
pub const ASCII26: Font = Font(0x0805_92F0); // scope / generator
pub const AUX_TAG: Font = Font(0x0805_965C);
pub const MODE: Font = Font(0x0805_9D00); // 0x04 DC .. 0x08 diode, 32 px
pub const BTN_GLYPH: Font = Font(0x0805_A9FC); // 04..08, 0x10 Hz, 0x11 cap, ohm, C
pub const BATTERY: Font = Font(0x0804_FA4C); // 0x22..0x27
pub const LOCK: Font = Font(0x0806_BF24); // 0x20 closed, 0x21 open
pub const SAVE_BT: Font = Font(0x0805_B2E8); // 0x21 Bluetooth
pub const ON_TAG: Font = Font(0x0806_C724);
pub const LIGHTNING: Font = Font(0x0805_A0E0);
pub const TEXT22: Font = Font(0x0806_B13C); // menu text, class B, 1 bpp

impl Font {
    /// The stock app is present and this looks like a font object.
    pub fn valid(&self) -> bool {
        let vt = r32(self.0 + 4);
        (0x0800_4000..0x0808_0000).contains(&vt) && matches!(r32(vt), VT_A | VT_B)
    }

    pub fn line_h(&self) -> u16 {
        (r32(self.0 + 8) & 0xFFFF) as u16
    }

    pub fn descent(&self) -> i16 {
        (r32(self.0 + 8) >> 16) as i16
    }

    pub fn glyph(&self, code: u32) -> Option<Glyph> {
        let d = r32(self.0);
        let class_a = r32(r32(self.0 + 4)) == VT_A;
        let (bits, tab, n, fmt) = (r32(d), r32(d + 4), r32(d + 8), r8(d + 12));
        let es = if class_a { 12 } else { 8 };
        let (mut lo, mut hi) = (0u32, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let e = tab + mid * es;
            let c = r32(e);
            if c == code {
                let four = fmt == 4;
                return Some(if class_a {
                    Glyph {
                        bits: bits + r32(e + 4),
                        four,
                        yoff: r8(e + 8) as i8,
                        w: r8(e + 9),
                        h: r8(e + 10),
                        adv: r8(e + 11),
                    }
                } else {
                    let v = r32(e + 4);
                    let w = (v & 0xFF) as u8;
                    Glyph { bits: bits + (v >> 8), four, yoff: 0, w, h: self.line_h() as u8, adv: w }
                });
            } else if c < code {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        None
    }
}

/// Stock alignment 0..8: 0 left/top, 1 left/vcenter, 2 left/bottom, 3 right/top,
/// 4 right/vcenter, 5 right/bottom, 6 hcenter/top, 7 hcenter/bottom, 8 center.
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

pub const fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect { x, y, w, h }
}

const MAXG: usize = 48;

struct Placed {
    g: Glyph,
    x: i16,
    y: i16,
}

fn layout(font: Font, r: Rect, text: &str, align: u8, spacing: i8, out: &mut [Option<Placed>; MAXG]) -> usize {
    // Measure.
    let mut n = 0;
    let mut tw: i32 = 0;
    let mut last_w = 0i32;
    let mut pen = 0i32;
    let lh = font.line_h() as i32;
    let mut xs = [0i32; MAXG];
    let mut gs: [Option<Glyph>; MAXG] = [None; MAXG];
    for ch in text.chars() {
        if n == MAXG {
            break;
        }
        let g = font.glyph(ch as u32);
        xs[n] = pen;
        gs[n] = g;
        match g {
            Some(g) => {
                pen += g.adv as i32 + spacing as i32;
                last_w = g.w as i32;
            }
            None => {
                pen += lh;
                last_w = lh;
            }
        }
        n += 1;
    }
    if n > 0 {
        tw = xs[n - 1] + last_w;
    }
    let th = lh;
    let ox = match align {
        0..=2 => 0,
        3..=5 => r.w as i32 - tw,
        _ => (r.w as i32 - tw) >> 1,
    };
    let oy = match align {
        0 | 3 | 6 => 0,
        2 | 5 | 7 => r.h as i32 - th,
        _ => (r.h as i32 - th) >> 1,
    };
    let desc = font.descent() as i32;
    for i in 0..n {
        out[i] = gs[i].map(|g| Placed {
            g,
            x: (r.x as i32 + ox + xs[i]) as i16,
            y: (r.y as i32 + oy + lh - desc - g.h as i32 - g.yoff as i32) as i16,
        });
    }
    n
}

fn blend(fg: Color, bg: Color, a: u8) -> Color {
    if a == 0 {
        return bg;
    }
    if a >= 15 {
        return fg;
    }
    let a = a as i32;
    let ch = |c: Color, s: u32, m: u32| ((c as u32 >> s) & m) as i32;
    let mix = |s: u32, m: u32| ((ch(bg, s, m) + (ch(fg, s, m) - ch(bg, s, m)) * a / 15) as u32 & m) << s;
    (mix(11, 0x1F) | mix(5, 0x3F) | mix(0, 0x1F)) as Color
}

/// Draw `text` in `r` (background `bg` fills the whole rect, so a redraw
/// replaces the old text without flicker).
pub fn label(font: Font, r: Rect, text: &str, fg: Color, bg: Color, align: u8, spacing: i8) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let mut pl: [Option<Placed>; MAXG] = [const { None }; MAXG];
    let n = layout(font, r, text, align, spacing, &mut pl);
    let x1 = (r.x + r.w - 1).min(lcd::W as i16 - 1);
    let y1 = (r.y + r.h - 1).min(lcd::H as i16 - 1);
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    // Per column: the glyph covering it (last one wins on 1-2 px overlaps of
    // negative spacing), so each pixel needs one lookup instead of a scan.
    let mut cover = [u8::MAX; 480];
    for (gi, p) in pl[..n].iter().enumerate() {
        if let Some(p) = p {
            for gx in 0..p.g.w as i16 {
                let x = p.x + gx;
                if x >= x0 && x <= x1 {
                    cover[x as usize] = gi as u8;
                }
            }
        }
    }
    lcd::set_window(x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let gi = cover[x as usize];
            let mut c = bg;
            if gi != u8::MAX {
                if let Some(p) = &pl[gi as usize] {
                    let gy = y - p.y;
                    if gy >= 0 && gy < p.g.h as i16 {
                        c = blend(fg, bg, p.g.alpha((x - p.x) as u32, gy as u32));
                    }
                }
            }
            lcd::data(c);
        }
    }
}

/// Fixed-cell label: character i centred (align 8) in cell i, cells from r.x.
pub fn cells(font: Font, r: Rect, text: &str, widths: &[u8], fg: Color, bg: Color) {
    let mut x = r.x;
    let mut it = text.chars();
    for &w in widths {
        let c = it.next().unwrap_or(' ');
        let mut b = [0u8; 4];
        label(font, rect(x, r.y, w as i16, r.h), c.encode_utf8(&mut b), fg, bg, 8, 0);
        x += w as i16;
    }
    let end = r.x + r.w;
    if x < end {
        lcd::fill_rect(x as u16, r.y as u16, (end - x) as u16, r.h as u16, bg);
    }
}

/// Rounded box (corners cut at `radius`, no anti-aliasing).
pub fn rbox(r: Rect, radius: i16, color: Color, outside: Color) {
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    lcd::set_window(x0 as u16, y0 as u16, (r.x + r.w - 1) as u16, (r.y + r.h - 1) as u16);
    for y in 0..r.h {
        for x in 0..r.w {
            let dx = if x < radius { radius - x } else if x >= r.w - radius { x - (r.w - radius - 1) } else { 0 };
            let dy = if y < radius { radius - y } else if y >= r.h - radius { y - (r.h - radius - 1) } else { 0 };
            let inside = (dx as i32 * dx as i32 + dy as i32 * dy as i32) <= radius as i32 * radius as i32;
            lcd::data(if inside { color } else { outside });
        }
    }
}

/// Draw only the glyph boxes of `text` placed in `r` (pixels outside the
/// glyphs are left alone): for text over shapes such as rounded buttons.
pub fn glyphs(font: Font, r: Rect, text: &str, fg: Color, bg: Color, align: u8, spacing: i8) {
    let mut pl: [Option<Placed>; MAXG] = [const { None }; MAXG];
    let n = layout(font, r, text, align, spacing, &mut pl);
    for p in pl[..n].iter().flatten() {
        if p.g.w == 0 || p.g.h == 0 || p.x < 0 || p.y < 0 {
            continue;
        }
        let (x, y) = (p.x as u16, p.y as u16);
        lcd::set_window(x, y, x + p.g.w as u16 - 1, y + p.g.h as u16 - 1);
        for gy in 0..p.g.h as u32 {
            for gx in 0..p.g.w as u32 {
                lcd::data(blend(fg, bg, p.g.alpha(gx, gy)));
            }
        }
    }
}

/// Map our unit text to the stock font's code points (no 'k' / 'µ' glyphs:
/// the stock writes "K" and "u").
pub fn unit_text<'a>(u: &str, buf: &'a mut [u8; 16]) -> &'a str {
    let mut n = 0;
    for c in u.chars() {
        let c = match c {
            'k' => 'K',
            '\u{b5}' => 'u',
            '\u{b0}' => continue,
            'C' if u.contains('\u{b0}') => '\u{2103}',
            'F' if u.contains('\u{b0}') => '\u{2109}',
            c => c,
        };
        let mut t = [0u8; 4];
        let s = c.encode_utf8(&mut t);
        if n + s.len() > buf.len() {
            break;
        }
        buf[n..n + s.len()].copy_from_slice(s.as_bytes());
        n += s.len();
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}
