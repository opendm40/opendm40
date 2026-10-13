//! Text with the stock firmware's bitmap fonts. Two data sources, chosen by the
//! `embedded-fonts` Cargo feature; the drawing code below is shared.
//!   default (feature off): read the stock app's fonts in place from the
//!     internal flash (the stock app stays installed, we only read it) — small
//!     enough for the RAM-loaded sector-8 image.
//!   `embedded-fonts`: use our own copy of the glyph tables + bitmaps, embedded
//!     from stockfont_data.rs (extracted by tools/mkstockfont.py), so no stock
//!     app in flash is needed — for the future native-flash layout.
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

/// A font, keyed by its original stock font-object address.
#[derive(Clone, Copy)]
pub struct Font(pub u32);

// The glyph struct and the per-font data-access layer (Font::valid/line_h/
// descent/glyph and Glyph::alpha) are the only parts that differ between the
// two sources, so only they are cfg-gated; one `mod data` survives, and its
// `Glyph` is re-exported below. Everything from `Rect` on down is shared.
pub use data::Glyph;

// ---- Default: read the stock fonts in place from internal flash ----
#[cfg(not(feature = "embedded-fonts"))]
mod data {
    use super::Font;

    const VT_A: u32 = 0x0802_3509;
    const VT_B: u32 = 0x0802_36F5;
    const VT_BPLUS: u32 = 0x0802_3425;
    const FONT_C: u32 = 0x0805_4828;
    const FONT12: u32 = 0x0804_F24C;
    const ASCII12: u32 = 0x0805_471C;
    const FONT16: u32 = 0x0804_F260;
    const ASCII16: u32 = 0x0805_34C4;

    fn r32(a: u32) -> u32 {
        unsafe { core::ptr::read_volatile(a as *const u32) }
    }
    fn r8(a: u32) -> u8 {
        unsafe { core::ptr::read_volatile(a as *const u8) }
    }

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
        pub const EMPTY: Glyph = Glyph { bits: 0, four: false, w: 0, h: 0, adv: 0, yoff: 0 };

        /// Coverage 0..15 of pixel (x, y) inside the glyph box.
        pub fn alpha(&self, x: u32, y: u32) -> u8 {
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

    impl Font {
        /// The stock app is present and this looks like a font object.
        pub fn valid(&self) -> bool {
            if self.0 == FONT_C || self.0 == FONT16 {
                return Font(ASCII16).valid();
            }
            let vt = r32(self.0 + 4);
            (0x0800_4000..0x0808_0000).contains(&vt) && matches!(r32(vt), VT_A | VT_B | VT_BPLUS)
        }

        pub fn line_h(&self) -> u16 {
            match self.0 {
                FONT_C => 16,
                FONT16 => Font(ASCII16).line_h(),
                _ => (r32(self.0 + 8) & 0xFFFF) as u16,
            }
        }

        pub fn descent(&self) -> i16 {
            match self.0 {
                FONT_C => 0,
                FONT16 => Font(ASCII16).descent(),
                _ => (r32(self.0 + 8) >> 16) as i16,
            }
        }

        /// Glyph lookup with the stock chains of the scope page fonts: class C
        /// (fixed bitmaps, FUN_0802362c), the 12 px B+ font (0x0805471c first,
        /// then its own table, FUN_08023425) and the 16 px wrapper (its ASCII
        /// part 0x080534c4; the CJK part is not used).
        pub fn glyph(&self, code: u32) -> Option<Glyph> {
            match self.0 {
                FONT_C => {
                    let i = b"'RUNSTOPWAIDH".iter().position(|&c| c as u32 == code)? as u32;
                    let (bits, w) = if i == 0 { (0x0805_4728, 4) } else { (0x0805_4730 + 20 * (i - 1), 10) };
                    return Some(Glyph { bits, four: false, w, h: 16, adv: w, yoff: 0 });
                }
                FONT16 => return Font(ASCII16).glyph(code),
                FONT12 => {
                    if let Some(g) = Font(ASCII12).glyph_own(code) {
                        return Some(g);
                    }
                }
                _ => {}
            }
            self.glyph_own(code)
        }

        fn glyph_own(&self, code: u32) -> Option<Glyph> {
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
}

// ---- Feature: use our embedded copy of the stock fonts ----
#[cfg(feature = "embedded-fonts")]
mod data {
    use super::Font;

    #[derive(Clone, Copy)]
    pub struct Entry {
        pub code: u32,
        pub off: u32,
        pub w: u8,
        pub h: u8,
        pub adv: u8,
        pub yoff: i8,
    }

    #[derive(Clone, Copy)]
    pub struct FontData {
        pub line_h: u16,
        pub descent: i16,
        pub four: bool,
        pub entries: &'static [Entry],
        pub blob: &'static [u8],
    }

    // Embedded glyph tables + bitmap blobs (generated by tools/mkstockfont.py
    // from the stock DM40 V2.6 app; our own copy, so no stock app in flash is
    // needed). Pulled in with include! so stockfont stays a single module; the
    // path resolves relative to stockfont.rs even inside this inline module.
    include!("stockfont_data.rs");

    #[derive(Clone, Copy)]
    pub struct Glyph {
        blob: &'static [u8],
        off: u32,
        four: bool,
        pub w: u8,
        pub h: u8,
        pub adv: u8,
        pub yoff: i8,
    }

    impl Glyph {
        pub const EMPTY: Glyph = Glyph { blob: &[], off: 0, four: false, w: 0, h: 0, adv: 0, yoff: 0 };

        /// Coverage 0..15 of pixel (x, y) inside the glyph box. Column-major,
        /// MSB-first; 4 bpp packs two pixels per byte (high nibble first).
        pub fn alpha(&self, x: u32, y: u32) -> u8 {
            let p = x * self.h as u32 + y;
            if self.four {
                let b = self.blob[(self.off + p / 2) as usize];
                if p & 1 == 0 { b >> 4 } else { b & 0xF }
            } else {
                let b = self.blob[(self.off + p / 8) as usize];
                if b & (0x80 >> (p & 7)) != 0 { 15 } else { 0 }
            }
        }
    }

    impl Font {
        /// True if this address has embedded font data.
        pub fn valid(&self) -> bool {
            font_data(self.0).is_some()
        }

        pub fn line_h(&self) -> u16 {
            font_data(self.0).map_or(0, |f| f.line_h)
        }

        pub fn descent(&self) -> i16 {
            font_data(self.0).map_or(0, |f| f.descent)
        }

        pub fn glyph(&self, code: u32) -> Option<Glyph> {
            let fd = font_data(self.0)?;
            let e = fd.entries;
            let (mut lo, mut hi) = (0usize, e.len());
            while lo < hi {
                let mid = (lo + hi) / 2;
                let c = e[mid].code;
                if c == code {
                    let g = &e[mid];
                    return Some(Glyph {
                        blob: fd.blob,
                        off: g.off,
                        four: fd.four,
                        w: g.w,
                        h: g.h,
                        adv: g.adv,
                        yoff: g.yoff,
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
}

// Fonts used by the multimeter page (stock font-object addresses).
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
                // A space advances at least a third of the line height: the
                // stock F14 space glyph advances 1 px ("40rows").
                pen += (if ch == ' ' { (g.adv as i32).max(lh / 3) } else { g.adv as i32 }) + spacing as i32;
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

/// 16-step colour ramp bg..fg for one label (avoids per-pixel arithmetic).
pub fn ramp(fg: Color, bg: Color) -> [Color; 16] {
    let mut t = [0; 16];
    for (a, v) in t.iter_mut().enumerate() {
        *v = blend(fg, bg, a as u8);
    }
    t
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
    let lut = ramp(fg, bg);
    #[cfg(test)]
    crate::probe::begin(crate::probe::TEXT, bg);
    lcd::set_window(x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let gi = cover[x as usize];
            #[cfg(test)]
            crate::probe::sub(gi);
            let mut c = bg;
            if gi != u8::MAX {
                if let Some(p) = &pl[gi as usize] {
                    let gy = y - p.y;
                    if gy >= 0 && gy < p.g.h as i16 {
                        c = lut[p.g.alpha((x - p.x) as u32, gy as u32) as usize];
                    }
                }
            }
            lcd::data(c);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Glyph `c` of `font` at half size, each pixel the mean coverage of a 2x2
/// block of the full-size glyph (same face, no extra font data). The glyph is
/// centred horizontally in `r`, which is painted entirely (`bg` outside the
/// glyph, so a redraw leaves nothing behind), and sits on `base` (exclusive
/// bottom y) exactly as the full-size glyph sits on its baseline: full-size
/// rows 2k, 2k + 1 below/above the baseline become half-size row k, so odd
/// glyph heights do not shift it. Used for the small +1 digit after the F78
/// readout.
pub fn label_half(font: Font, r: Rect, c: char, base: i16, fg: Color, bg: Color) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let g = font.glyph(c as u32);
    // Full-size row of glyph row 0, relative to the baseline; left x of the
    // half-size glyph.
    let (top, ox) = match &g {
        Some(g) => (-(g.h as i32 + g.yoff as i32), r.x as i32 + ((r.w as i32 - (g.w as i32 + 1) / 2) >> 1)),
        None => (0, 0),
    };
    let x1 = (r.x + r.w - 1).min(lcd::W as i16 - 1);
    let y1 = (r.y + r.h - 1).min(lcd::H as i16 - 1);
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    let lut = ramp(fg, bg);
    #[cfg(test)]
    crate::probe::begin(crate::probe::TEXT, bg);
    lcd::set_window(x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    for y in y0..=y1 {
        let gy = 2 * (y as i32 - base as i32) - top;
        for x in x0..=x1 {
            let mut c = bg;
            if let Some(g) = &g {
                let gx = 2 * (x as i32 - ox);
                let (w, h) = (g.w as i32, g.h as i32);
                if gx >= 0 && gx < w && gy > -2 && gy < h {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = (gx + dx, gy + dy);
                        if px < w && py >= 0 && py < h {
                            s += g.alpha(px as u32, py as u32) as u32;
                        }
                    }
                    c = lut[((s + 2) / 4) as usize];
                }
            }
            lcd::data(c);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Mean coverage (0..15) of the 2x2 block of full-size glyph pixels behind
/// half-size pixel (hx, hy) of `g`; pixels outside the glyph count as 0.
fn alpha_half(g: &Glyph, hx: i32, hy: i32) -> u8 {
    let (w, h) = (g.w as i32, g.h as i32);
    let mut s = 0u32;
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (px, py) = (2 * hx + dx, hy + dy);
        if px >= 0 && px < w && py >= 0 && py < h {
            s += g.alpha(px as u32, py as u32) as u32;
        }
    }
    ((s + 2) / 4) as u8
}

const MAXH: usize = 12;

/// Half-size glyphs of `text` and their x offsets from the text start:
/// proportional spacing, each glyph advances by its half-size width plus
/// `gap` (the stock digit advances are tabular, which at half size left a
/// visible hole after a narrow '1'). Returns the count and the total width.
fn half_layout(font: Font, text: &str, gap: i16, gs: &mut [Option<Glyph>; MAXH], xs: &mut [i16; MAXH]) -> (usize, i16) {
    let (mut n, mut pen, mut end) = (0usize, 0i16, 0i16);
    for ch in text.chars().take(MAXH) {
        let g = font.glyph(ch as u32);
        xs[n] = pen;
        gs[n] = g;
        let w = g.map_or(font.line_h() as i16 / 4, |g| (g.w as i16 + 1) / 2);
        if g.is_some() {
            end = pen + w;
        }
        pen += w + gap;
        n += 1;
    }
    (n, end)
}


/// `text` in `font` at half size (each pixel the mean of a 2x2 block of the
/// full-size glyph, as `label_half`), proportionally spaced (`half_layout`),
/// sitting on `base` (exclusive bottom y of the full-size baseline, halved)
/// and aligned in `r` (align 3..5 right, 6..8 centred, else left). Paints all
/// of `r`, so one call replaces the previous text without flicker. Used for
/// the meter-strip reading of the combined view (F78 at half size = "F39").
pub fn half_text(font: Font, r: Rect, text: &str, base: i16, gap: i16, align: u8, fg: Color, bg: Color) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let mut gs = [None; MAXH];
    let mut xs = [0i16; MAXH];
    let (n, tw) = half_layout(font, text, gap, &mut gs, &mut xs);
    let ox = r.x
        + match align {
            3..=5 => r.w - tw,
            6..=8 => (r.w - tw) / 2,
            _ => 0,
        };
    let x1 = (r.x + r.w - 1).min(lcd::W as i16 - 1);
    let y1 = (r.y + r.h - 1).min(lcd::H as i16 - 1);
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    // Glyph per column, as in `label`.
    let mut cover = [u8::MAX; 480];
    for i in 0..n {
        if let Some(g) = &gs[i] {
            for gx in 0..(g.w as i16 + 1) / 2 {
                let x = ox + xs[i] + gx;
                if x >= x0 && x <= x1 {
                    cover[x as usize] = i as u8;
                }
            }
        }
    }
    let lut = ramp(fg, bg);
    #[cfg(test)]
    crate::probe::begin(crate::probe::TEXT, bg);
    lcd::set_window(x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let gi = cover[x as usize];
            #[cfg(test)]
            crate::probe::sub(gi);
            let mut c = bg;
            if let Some(Some(g)) = gs.get(gi as usize) {
                // Full-size glyph row of half-size row y (see label_half).
                let gy = 2 * (y as i32 - base as i32) + g.h as i32 + g.yoff as i32;
                if gy > -2 && gy < g.h as i32 {
                    c = lut[alpha_half(g, (x - ox - xs[gi as usize]) as i32, gy) as usize];
                }
            }
            lcd::data(c);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

mod logo_data {
    include!("stockfont_logo.rs");
}

/// Side of the round Wiren Board logo (tools/mklogo.py), pixels.
pub const LOGO: i16 = logo_data::LOGO_N as i16;

/// The round Wiren Board logo at (x, y), anti-aliased against `bg` (the
/// identity mark of this firmware at the left of every status bar).
pub fn logo(x: i16, y: i16, bg: Color) {
    use logo_data::{LOGO_C, LOGO_M, LOGO_N};
    let green = ramp(0x5DA0, bg); // #5cb400
    #[cfg(test)]
    crate::probe::begin(crate::probe::ICON, bg);
    lcd::set_window(x as u16, y as u16, (x + LOGO - 1) as u16, (y + LOGO - 1) as u16);
    for p in 0..LOGO_N * LOGO_N {
        let nib = |t: &[u8]| if p & 1 == 0 { t[p / 2] >> 4 } else { t[p / 2] & 0xF };
        let c = green[nib(&LOGO_C) as usize];
        lcd::data(blend(0xFFFF, c, nib(&LOGO_M)));
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Fixed-cell label, redrawing only the cells whose character differs from
/// `prev` (same cell layout assumed; pass an empty `prev` for a full draw).
pub fn cells_diff(font: Font, r: Rect, text: &str, prev: &str, widths: &[u8], fg: Color, bg: Color) {
    #[cfg(test)]
    crate::probe::group(true);
    let mut x = r.x;
    let mut it = text.chars();
    let mut ip = prev.chars();
    let full = prev.is_empty();
    for &w in widths {
        let c = it.next().unwrap_or(' ');
        let pc = ip.next();
        if full || pc != Some(c) {
            let mut b = [0u8; 4];
            label(font, rect(x, r.y, w as i16, r.h), c.encode_utf8(&mut b), fg, bg, 8, 0);
        }
        x += w as i16;
    }
    let end = r.x + r.w;
    if full && x < end {
        lcd::fill_rect(x as u16, r.y as u16, (end - x) as u16, r.h as u16, bg);
    }
    #[cfg(test)]
    crate::probe::group(false);
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
    #[cfg(test)]
    crate::probe::begin(crate::probe::SHAPE, outside);
    lcd::set_window(x0 as u16, y0 as u16, (r.x + r.w - 1) as u16, (r.y + r.h - 1) as u16);
    for y in 0..r.h {
        for x in 0..r.w {
            let dx = if x < radius { radius - x } else if x >= r.w - radius { x - (r.w - radius - 1) } else { 0 };
            let dy = if y < radius { radius - y } else if y >= r.h - radius { y - (r.h - radius - 1) } else { 0 };
            let inside = (dx as i32 * dx as i32 + dy as i32 * dy as i32) <= radius as i32 * radius as i32;
            lcd::data(if inside { color } else { outside });
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Draw only the glyph boxes of `text` placed in `r` (pixels outside the
/// glyphs are left alone): for text over shapes such as rounded buttons.
pub fn glyphs(font: Font, r: Rect, text: &str, fg: Color, bg: Color, align: u8, spacing: i8) {
    let mut pl: [Option<Placed>; MAXG] = [const { None }; MAXG];
    let n = layout(font, r, text, align, spacing, &mut pl);
    #[cfg(test)]
    crate::probe::begin(crate::probe::TEXT, bg);
    for (_i, p) in pl[..n].iter().enumerate() {
        let Some(p) = p else { continue };
        if p.g.w == 0 || p.g.h == 0 || p.x < 0 || p.y < 0 {
            continue;
        }
        #[cfg(test)]
        crate::probe::sub(_i as u8);
        let (x, y) = (p.x as u16, p.y as u16);
        let lut = ramp(fg, bg);
        lcd::set_window(x, y, x + p.g.w as u16 - 1, y + p.g.h as u16 - 1);
        for gy in 0..p.g.h as u32 {
            for gx in 0..p.g.w as u32 {
                lcd::data(lut[p.g.alpha(gx, gy) as usize]);
            }
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Rows of `text`'s glyph boxes relative to its line top: (first, last),
/// None for no glyphs (to fit text whose glyphs reach above the caps ('i',
/// '%') or below the baseline ('y', 'q') into a tight bar).
pub fn glyph_rows(font: Font, text: &str) -> Option<(i16, i16)> {
    let base = font.line_h() as i16 - font.descent();
    let (mut t, mut b) = (i16::MAX, i16::MIN);
    for ch in text.chars() {
        if let Some(g) = font.glyph(ch as u32) {
            let top = base - g.h as i16 - g.yoff as i16;
            t = t.min(top);
            b = b.max(top + g.h as i16 - 1);
        }
    }
    if t > b { None } else { Some((t, b)) }
}

/// Width in pixels of `text` as `label` lays it out (pen advance plus the
/// last glyph's width), e.g. to place a tag right before right-aligned text.
pub fn text_width(font: Font, text: &str, spacing: i8) -> i16 {
    let (mut pen, mut last) = (0i32, 0i32);
    let lh = font.line_h() as i32;
    for ch in text.chars() {
        let (adv, w) = match font.glyph(ch as u32) {
            // As `layout`: a space advances at least lh / 3.
            Some(g) if ch == ' ' => ((g.adv as i32).max(lh / 3), g.w as i32),
            Some(g) => (g.adv as i32, g.w as i32),
            None => (font.line_h() as i32, font.line_h() as i32),
        };
        last = pen + w;
        pen += adv + spacing as i32;
    }
    last as i16
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

// ---------------------------------------------------------------------------
// Stock-exact drawing for the replica scope page (docs/stock-scope-ui.md sec. 0,
// the port of tools/stock-ui/gfx.rs): anti-aliasing OFF (a glyph pixel is ink
// iff its coverage >= 8 of 15), the stock text placement without our
// "space >= line_h / 3" rule, rounded boxes and 1 px rings with the stock
// corner tables. Drawing is by composition: a widget is one LCD window whose
// every pixel is computed from its layers (`paint`), so nothing flickers and
// a repaint of any sub-rectangle is exact.

/// Fonts of the stock scope page (stock font-object addresses; embedded by
/// tools/mkstockfont.py, or read in place on the RAM image).
pub const SC_C: Font = Font(0x0805_4828); // 10x16 status letters ' R U N S T O P W A I D H
pub const SC_F12: Font = Font(0x0804_F24C); // 12 px texts (ASCII of 0x0805471c + own table)
pub const SC_F16: Font = Font(0x0804_F260); // 16 px popup texts (ASCII 0x080534c4)
pub const SC_F20: Font = Font(0x0805_4148); // 20 px panel buttons
pub const SC_F22: Font = TEXT22; // 22 px tab labels, toasts
pub const SC_PROBE: Font = Font(0x0805_4FC0); // coupling / probe glyph 0x2c..0x45
pub const SC_EDGE: Font = Font(0x0805_5014); // trigger edge 0x20 rising / 0x21 falling
pub const SC_MARK: Font = Font(0x0806_5558); // 12x12 marker glyphs 0x20..0x4b
pub const SC_BATT: Font = Font(0x0806_BD44); // battery 0x22 outline, 0x23.. level
pub const SC_DIGITS: Font = Font(0x0805_C5DC); // 20 px digits (generator / keypad)

/// Row j (0 = outermost) of a rounded corner of radius r starts INSET[r][j]
/// px in (FUN_0801d754 / FUN_0801d0e4 thresholded a > 0x7f; port
/// tools/stock-ui/notes/tools/rounded_mask.py, radius 1..10).
const INSET: [[u8; 10]; 11] = [
    [0; 10],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 2, 1, 0, 0, 0, 0, 0, 0, 0],
    [4, 2, 1, 1, 0, 0, 0, 0, 0, 0],
    [5, 3, 2, 1, 0, 0, 0, 0, 0, 0],
    [5, 3, 2, 1, 1, 0, 0, 0, 0, 0],
    [6, 4, 3, 2, 1, 1, 0, 0, 0, 0],
    [7, 5, 3, 2, 2, 1, 1, 0, 0, 0],
];

/// Painted corner pixels (k, j) of a 1 px rounded border (FUN_0801e194 width
/// 1), per radius, inside the (r+1) x (r+1) corner square; same source.
const RING: [&[(u8, u8)]; 11] = [
    &[],
    &[(0, 0)],
    &[(1, 0), (2, 0), (0, 1), (0, 2)],
    &[(2, 0), (3, 0), (0, 1), (1, 1), (0, 2), (0, 3)],
    &[(2, 0), (3, 0), (4, 0), (1, 1), (0, 2), (0, 3), (0, 4)],
    &[(3, 0), (4, 0), (5, 0), (2, 1), (1, 2), (0, 3), (0, 4), (0, 5)],
    &[(4, 0), (5, 0), (6, 0), (2, 1), (3, 1), (1, 2), (1, 3), (0, 4), (0, 5), (0, 6)],
    &[(5, 0), (6, 0), (7, 0), (3, 1), (4, 1), (2, 2), (1, 3), (0, 4), (1, 4), (0, 5), (0, 6), (0, 7)],
    &[(5, 0), (6, 0), (7, 0), (8, 0), (3, 1), (4, 1), (2, 2), (3, 2), (1, 3), (2, 3), (1, 4), (0, 5), (0, 6), (0, 7), (0, 8)],
    &[(6, 0), (7, 0), (8, 0), (9, 0), (4, 1), (5, 1), (3, 2), (2, 3), (1, 4), (1, 5), (0, 6), (0, 7), (0, 8), (0, 9)],
    &[(7, 0), (8, 0), (9, 0), (10, 0), (5, 1), (6, 1), (3, 2), (4, 2), (2, 3), (3, 3), (2, 4), (1, 5), (1, 6), (0, 7), (0, 8), (0, 9), (0, 10)],
];

fn clamp_radius(r: Rect, radius: i16) -> i16 {
    radius.min(r.w.min(r.h) >> 1).max(0).min(10)
}

fn corner_inside(r: i16, k: i16, j: i16) -> bool {
    r <= 0 || k >= r || j >= r || k >= INSET[r as usize][j as usize] as i16
}

impl Rect {
    pub const fn has(&self, x: i16, y: i16) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub const fn inset(&self, d: i16) -> Rect {
        rect(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

/// (x, y) inside the stock rounded box `r` (FUN_0801b272: corner mask bit0
/// TL, bit1 TR, bit2 BL, bit3 BR; radius clamped to min(w, h) / 2).
pub fn in_rbox(r: Rect, radius: i16, mask: u8, x: i16, y: i16) -> bool {
    if !r.has(x, y) {
        return false;
    }
    let rad = clamp_radius(r, radius);
    let (kx, ky) = (x - r.x, y - r.y);
    let (rx, ry) = (r.w - 1 - kx, r.h - 1 - ky);
    (mask & 1 == 0 || corner_inside(rad, kx, ky))
        && (mask & 2 == 0 || corner_inside(rad, rx, ky))
        && (mask & 4 == 0 || corner_inside(rad, kx, ry))
        && (mask & 8 == 0 || corner_inside(rad, rx, ry))
}

/// (x, y) on the 1 px stock rounded border of `r` (FUN_0801e194).
pub fn on_ring(r: Rect, radius: i16, mask: u8, x: i16, y: i16) -> bool {
    if !r.has(x, y) {
        return false;
    }
    let rad = clamp_radius(r, radius);
    let ring = RING[rad as usize];
    let corner = |k: i16, j: i16| ring.iter().any(|&(a, b)| a as i16 == k && b as i16 == j);
    let (kx, ky) = (x - r.x, y - r.y);
    let (rx, ry) = (r.w - 1 - kx, r.h - 1 - ky);
    if kx <= rad && ky <= rad && mask & 1 != 0 {
        corner(kx, ky)
    } else if rx <= rad && ky <= rad && mask & 2 != 0 {
        corner(rx, ky)
    } else if kx <= rad && ry <= rad && mask & 4 != 0 {
        corner(kx, ry)
    } else if rx <= rad && ry <= rad && mask & 8 != 0 {
        corner(rx, ry)
    } else {
        kx == 0 || ky == 0 || rx == 0 || ry == 0
    }
}

/// Most glyphs of one `Ink` text.
const INK_N: usize = 24;

/// A text laid out as the stock label does (FUN_0801ae50), for threshold
/// compositing: `at(x, y)` is Some(glyph index) where a glyph has ink.
#[derive(Clone, Copy)]
pub struct Ink {
    g: [(Glyph, i16, i16); INK_N],
    n: u8,
    /// Ink bounds (glyph boxes), for a quick reject.
    pub bx: Rect,
}

impl Ink {
    pub const fn none() -> Ink {
        // A zero-size glyph box never has ink.
        Ink { g: [(Glyph::EMPTY, 0, 0); INK_N], n: 0, bx: rect(0, 0, 0, 0) }
    }

    /// Lay `text` out in `r` with the stock rules: text_w = sum(adv) of all
    /// but the last glyph + w(last) + (n-1) * spacing; an unknown code
    /// advances line_h; align 0..8 as `label`; glyph top = line top + line_h
    /// - descent - h - yoff.
    pub fn new(font: Font, r: Rect, text: &str, align: u8, spacing: i8) -> Ink {
        let mut k = Ink::none();
        let lh = font.line_h() as i32;
        let mut xs = [0i32; INK_N];
        let mut gs: [Option<Glyph>; INK_N] = [None; INK_N];
        let (mut pen, mut last_w, mut n) = (0i32, 0i32, 0usize);
        for ch in text.chars() {
            if n == INK_N {
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
                    pen += lh + spacing as i32;
                    last_w = lh;
                }
            }
            n += 1;
        }
        let tw = if n > 0 { xs[n - 1] + last_w } else { 0 };
        let ox = match align {
            0..=2 => 0,
            3..=5 => r.w as i32 - tw,
            _ => (r.w as i32 - tw) >> 1,
        };
        let oy = match align {
            0 | 3 | 6 => 0,
            2 | 5 | 7 => r.h as i32 - lh,
            _ => (r.h as i32 - lh) >> 1,
        };
        let desc = font.descent() as i32;
        let (mut x0, mut y0, mut x1, mut y1) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
        for i in 0..n {
            if let Some(g) = gs[i] {
                if g.w == 0 || g.h == 0 {
                    continue;
                }
                let x = (r.x as i32 + ox + xs[i]) as i16;
                let y = (r.y as i32 + oy + lh - desc - g.h as i32 - g.yoff as i32) as i16;
                k.g[k.n as usize] = (g, x, y);
                k.n += 1;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + g.w as i16);
                y1 = y1.max(y + g.h as i16);
            }
        }
        if k.n > 0 {
            k.bx = rect(x0, y0, x1 - x0, y1 - y0);
        }
        k
    }

    /// One glyph placed like a 1-character label (icons, markers).
    pub fn icon(font: Font, r: Rect, code: u32, align: u8) -> Ink {
        let mut b = [0u8; 4];
        match char::from_u32(code) {
            Some(c) => Ink::new(font, r, c.encode_utf8(&mut b), align, 0),
            None => Ink::none(),
        }
    }

    /// Glyph index with ink (coverage >= 8 of 15, the AA-off threshold) at
    /// (x, y), or None.
    #[inline]
    pub fn at(&self, x: i16, y: i16) -> Option<u8> {
        if !self.bx.has(x, y) {
            return None;
        }
        for i in 0..self.n as usize {
            let (g, gx, gy) = &self.g[i];
            let (dx, dy) = (x - gx, y - gy);
            if dx >= 0 && dy >= 0 && dx < g.w as i16 && dy < g.h as i16 && g.alpha(dx as u32, dy as u32) >= 8 {
                return Some(i as u8);
            }
        }
        None
    }

    /// Ink width as laid out (glyph boxes), 0 for none.
    pub fn width(&self) -> i16 {
        self.bx.w
    }
}

/// Stream the window `r` (clipped to the screen) computing every pixel with
/// `f(x, y) -> (colour, tag)`. The tag only matters to the host layout check
/// (cfg(test)): 0 = background, else `layer << 8 | glyph`, layer 1.. indexing
/// `layers` = (probe kind, background colour) of each drawn element, so one
/// composite window still reports its shapes, texts and icons separately.
#[inline(never)]
pub fn paint(r: Rect, layers: &[(u8, Color)], mut f: impl FnMut(i16, i16) -> (Color, u16)) {
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w - 1).min(lcd::W as i16 - 1);
    let y1 = (r.y + r.h - 1).min(lcd::H as i16 - 1);
    if x1 < x0 || y1 < y0 {
        return;
    }
    #[cfg(test)]
    let ids: [u32; 8] = {
        let mut ids = [0u32; 8];
        for (i, l) in layers.iter().take(8).enumerate() {
            ids[i] = crate::probe::open(l.0, l.1);
        }
        ids
    };
    #[cfg(not(test))]
    let _ = layers;
    lcd::set_window(x0 as u16, y0 as u16, x1 as u16, y1 as u16);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (c, _tag) = f(x, y);
            #[cfg(test)]
            {
                let l = (_tag >> 8) as usize;
                if l == 0 || l > layers.len().min(8) {
                    crate::probe::set(0, 0, 0);
                } else {
                    crate::probe::set(ids[l - 1], layers[l - 1].1, _tag as u8);
                }
            }
            lcd::data(c);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}
