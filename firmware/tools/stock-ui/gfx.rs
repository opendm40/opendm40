//! Stock-exact drawing primitives for the host renderer (tools/stock-ui).
//!
//! Everything here reproduces the stock GUI's pixel rules
//! (stock/re/analysis/gui_v2_6.h sec. 1/5, gui_render_v2_6.h sec. 3/4):
//!   * fonts are read live from the stock image mapped at 0x08000000 (all
//!     classes: A, B, B+ with the 0x0805471c ASCII fallback, composite);
//!   * glyph pixels: 4 bpp coverage v -> alpha v * 17; with anti-aliasing
//!     OFF (the scope page) a pixel is ink when alpha > 0x7f (v >= 8); with
//!     AA ON (popups) it is blend565(fg, dst, alpha) in 8-bit channels;
//!   * text placement: text_w = sum(adv) of all but the last glyph + w of
//!     the last + (n-1)*spacing; glyph top = line_top + line_h - descent -
//!     g.h - g.yoff; align 0..8 as the stock (centering uses >> 1).
//! Pixels go straight into the emulator frame buffer (host.rs), which is
//! what the PNG is made from; no LCD protocol is needed for a reference.
#![allow(dead_code)]

use crate::host::{emu, H, W};

pub type Color = u16;

const BASE: u32 = 0x0800_0000;
const VT_A: u32 = 0x0802_3509;
const VT_B: u32 = 0x0802_36F5;
const VT_BPLUS: u32 = 0x0802_3425;
const VT_COMP: u32 = 0x0802_37A5;
const ASCII12: u32 = 0x0805_471C;
/// Class C font (vtable 0x08054820): fixed 10x16 bitmaps ' R U N S T O P W A I D H.
pub const FONT_C: u32 = 0x0805_4828;
const FONT_C_CODES: &[u8] = b"'RUNSTOPWAIDH";
/// 16 px wrapper font (vtable 0x0804f258): ASCII 0x080534c4 then CJK 0x08052ba0.
pub const FONT16: u32 = 0x0804_F260;
const ASCII16: u32 = 0x0805_34C4;
const CJK16: u32 = 0x0805_2BA0;
const COMP_FIRST: u32 = 0x0805_965C;
const COMP_SECOND: u32 = 0x0805_DAB4;

fn r32(a: u32) -> u32 {
    unsafe { core::ptr::read_volatile(a as *const u32) }
}
fn r8(a: u32) -> u8 {
    unsafe { core::ptr::read_volatile(a as *const u8) }
}

/// FUN_08019038: `fg` over `bg` with alpha `a` (0..255), RGB565 channels
/// expanded to 8 bits ((c << 3) | 7 etc.), 16-bit truncation as the code.
pub fn blend565(fg: Color, bg: Color, a: u8) -> Color {
    if a > 252 {
        return fg;
    }
    if a < 3 {
        return bg;
    }
    let (a, na) = (a as u32, 255 - a as u32);
    let b = ((((fg as u32 & 0x1f) << 3 | 7) * a + ((bg as u32 & 0x1f) << 3 | 7) * na) & 0xffff) >> 11;
    let g = (((((fg as u32 >> 3) & 0xfc) | 3) * a + (((bg as u32 >> 3) & 0xfc) | 3) * na) & 0xffff) >> 10;
    let r = ((((fg as u32 >> 8) & 0xf8) | 7) * a + (((bg as u32 >> 8) & 0xf8) | 7) * na) & 0xf800;
    (r | g << 5 | b) as Color
}

// ------------------------------------------------------------------ pixels

pub fn put(x: i32, y: i32, c: Color) {
    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
        return;
    }
    let e = emu();
    let i = y as usize * W + x as usize;
    e.fb[i] = c;
    // Layout-check bookkeeping (host.rs probe): which call last inked here.
    e.owner[i] = match e.call {
        Some((id, bg, g)) if c != bg => id << 8 | g as u32,
        _ => 0,
    };
}

/// Probe kinds for the layout check (same codes as tools/ui-render).
pub const P_TEXT: u8 = 0;
pub const P_SHAPE: u8 = 1;
pub const P_ICON: u8 = 2;

/// Mark the following draws as one probed call of `kind` over `bg`.
pub fn probe(kind: u8, bg: Color) {
    crate::host::begin(kind, bg)
}
pub fn probe_end() {
    crate::host::end()
}

pub fn get(x: i32, y: i32) -> Color {
    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
        return 0;
    }
    emu().fb[y as usize * W + x as usize]
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub const fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

impl Rect {
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        rect(self.x + dx, self.y + dy, self.w, self.h)
    }
    pub fn inset(&self, d: i32) -> Rect {
        rect(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Clip rectangle for everything drawn (the stock clips every primitive to
/// the widget / band; here: to the current container).
static mut CLIP: Rect = Rect { x: 0, y: 0, w: W as i32, h: H as i32 };

pub fn set_clip(r: Rect) {
    unsafe { CLIP = r }
}
pub fn clip() -> Rect {
    unsafe { CLIP }
}
pub fn clip_screen() {
    set_clip(rect(0, 0, W as i32, H as i32))
}

fn visible(x: i32, y: i32) -> bool {
    clip().contains(x, y)
}

pub fn fill(r: Rect, c: Color) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            if visible(x, y) {
                put(x, y, c);
            }
        }
    }
}

/// Corner shape of a rounded box (FUN_0801d754 circle mask, FUN_0801d0e4
/// coverage, thresholded a > 0x7f as the scope page draws with AA off):
/// row j (0 = outermost) of a corner of radius r starts `INSET[r][j]` px in.
/// Tables from the literal port notes/tools/rounded_mask.py (radius clamped
/// to min(w, h) / 2 by the stock before this applies).
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

/// Painted corner pixels (k, j) of a 1 px rounded border (FUN_0801e194:
/// outer mask x inverted inner mask of radius r-1 inset 1, thresholded),
/// inside the (r+1) x (r+1) corner square; same source as INSET.
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

pub fn corner_inside(r: i32, k: i32, j: i32) -> bool {
    if r <= 0 || k >= r || j >= r {
        return true;
    }
    if r <= 10 {
        return k >= INSET[r as usize][j as usize] as i32;
    }
    let dx = 2 * (r - k) - 1;
    let dy = 2 * (r - j) - 1;
    dx * dx + dy * dy <= 4 * r * r
}

fn clamp_r(r: Rect, radius: i32) -> i32 {
    radius.min(r.w.min(r.h) >> 1).max(0)
}

/// Rounded box, corner mask bit0 TL, bit1 TR, bit2 BL, bit3 BR (stock style
/// +5; 0x0f = all four).
pub fn rbox(r: Rect, radius: i32, mask: u8, c: Color) {
    probe(P_SHAPE, get(r.x - 1, r.y - 1));
    rbox_np(r, radius, mask, c);
    probe_end();
}

fn rbox_np(r: Rect, radius: i32, mask: u8, c: Color) {
    let radius = clamp_r(r, radius);
    for y in 0..r.h {
        for x in 0..r.w {
            let tl = mask & 1 != 0 && corner_inside(radius, x, y);
            let tr = mask & 2 != 0 && corner_inside(radius, r.w - 1 - x, y);
            let bl = mask & 4 != 0 && corner_inside(radius, x, r.h - 1 - y);
            let br = mask & 8 != 0 && corner_inside(radius, r.w - 1 - x, r.h - 1 - y);
            let inside = (mask & 1 == 0 || tl) && (mask & 2 == 0 || tr) && (mask & 4 == 0 || bl) && (mask & 8 == 0 || br);
            if inside && visible(r.x + x, r.y + y) {
                put(r.x + x, r.y + y, c);
            }
        }
    }
}

/// 1 px rounded border (FUN_0801e194 width 1) with the stock corner pixels.
pub fn rborder(r: Rect, radius: i32, mask: u8, c: Color) {
    probe(P_SHAPE, get(r.x - 1, r.y - 1));
    let radius = clamp_r(r, radius);
    let ring = if radius <= 10 { RING[radius as usize] } else { RING[10] };
    let corner = |k: i32, j: i32| ring.iter().any(|&(a, b)| a as i32 == k && b as i32 == j);
    for y in 0..r.h {
        for x in 0..r.w {
            let (rx, ry) = (r.w - 1 - x, r.h - 1 - y);
            let in_tl = x <= radius && y <= radius;
            let in_tr = rx <= radius && y <= radius;
            let in_bl = x <= radius && ry <= radius;
            let in_br = rx <= radius && ry <= radius;
            let on = if in_tl && mask & 1 != 0 {
                corner(x, y)
            } else if in_tr && mask & 2 != 0 {
                corner(rx, y)
            } else if in_bl && mask & 4 != 0 {
                corner(x, ry)
            } else if in_br && mask & 8 != 0 {
                corner(rx, ry)
            } else {
                x == 0 || y == 0 || rx == 0 || ry == 0
            };
            if on && visible(r.x + x, r.y + y) {
                put(r.x + x, r.y + y, c);
            }
        }
    }
    probe_end();
}

/// 1 px rectangle outline.
pub fn frame(r: Rect, c: Color) {
    fill(rect(r.x, r.y, r.w, 1), c);
    fill(rect(r.x, r.y + r.h - 1, r.w, 1), c);
    fill(rect(r.x, r.y, 1, r.h), c);
    fill(rect(r.x + r.w - 1, r.y, 1, r.h), c);
}

/// Dotted vertical line (FUN_0801a596 / FUN_0801d7b4): pixels y0, y0+step,
/// ... while < y0 + len.
pub fn dots_v(x: i32, y0: i32, len: i32, step: i32, c: Color) {
    let mut y = y0;
    while y < y0 + len {
        if visible(x, y) {
            put(x, y, c);
        }
        y += step;
    }
}

/// Dotted horizontal line (FUN_0801a67a / FUN_0801d81e).
pub fn dots_h(x0: i32, y: i32, len: i32, step: i32, c: Color) {
    let mut x = x0;
    while x < x0 + len {
        if visible(x, y) {
            put(x, y, c);
        }
        x += step;
    }
}

// ------------------------------------------------------------------ fonts

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
    /// Coverage 0..15 at (x, y) of the glyph box (column-major stream).
    pub fn cov(&self, x: u32, y: u32) -> u8 {
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
    fn vt(&self) -> u32 {
        r32(r32(self.0 + 4))
    }
    pub fn valid(&self) -> bool {
        if self.0 == FONT_C || self.0 == FONT16 {
            return true;
        }
        let vt = r32(self.0 + 4);
        (0x0800_4000..0x0808_0000).contains(&vt) && matches!(r32(vt), VT_A | VT_B | VT_BPLUS | VT_COMP)
    }
    pub fn line_h(&self) -> i32 {
        (r32(self.0 + 8) & 0xFFFF) as i32
    }
    /// Class C (FUN_0802362c / FUN_080235bc): ' is 4 px wide at 0x08054728,
    /// the 12 letters 10 px wide, 20 bytes each from 0x08054730.
    fn glyph_c(&self, code: u32) -> Option<Glyph> {
        let i = FONT_C_CODES.iter().position(|&c| c as u32 == code)?;
        Some(if i == 0 {
            Glyph { bits: 0x0805_4728, four: false, w: 4, h: 16, adv: 4, yoff: 0 }
        } else {
            Glyph { bits: 0x0805_4730 + 20 * (i as u32 - 1), four: false, w: 10, h: 16, adv: 10, yoff: 0 }
        })
    }
    pub fn descent(&self) -> i32 {
        ((r32(self.0 + 8) >> 16) as u16 as i16) as i32
    }
    /// Glyph lookup with the stock fallback chains (B+ asks 0x0805471c
    /// first; the composite asks 0x0805965c then 0x0805dab4).
    pub fn glyph(&self, code: u32) -> Option<Glyph> {
        if self.0 == FONT_C {
            return self.glyph_c(code);
        }
        if self.0 == FONT16 {
            return Font(ASCII16).glyph(code).or_else(|| Font(CJK16).glyph(code));
        }
        match self.vt() {
            VT_BPLUS => Font(ASCII12).glyph_own(code, false).or_else(|| self.glyph_own(code, false)),
            VT_COMP => Font(COMP_FIRST).glyph(code).or_else(|| Font(COMP_SECOND).glyph(code)),
            VT_A => self.glyph_own(code, true),
            _ => self.glyph_own(code, false),
        }
    }
    fn glyph_own(&self, code: u32, class_a: bool) -> Option<Glyph> {
        let d = r32(self.0);
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
                    Glyph { bits: bits + r32(e + 4), four, yoff: r8(e + 8) as i8, w: r8(e + 9), h: r8(e + 10), adv: r8(e + 11) }
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

/// How glyph coverage reaches the screen.
#[derive(Clone, Copy, PartialEq)]
pub enum Aa {
    /// Scope page: mgr+0x918 = 0, alpha > 0x7f -> fg (coverage >= 8).
    Off,
    /// Popups / DMM: blend565(fg, dst, v * 17).
    On,
}

pub struct Placed {
    pub g: Glyph,
    pub x: i32,
    pub y: i32,
}

/// Lay `text` out in `r` (align, spacing as the stock label); returns the
/// placed glyphs and the text width.
pub fn layout(font: Font, r: Rect, text: &str, align: u8, spacing: i32) -> (Vec<Placed>, i32) {
    let lh = font.line_h();
    let mut xs = vec![];
    let mut gs = vec![];
    let mut pen = 0;
    let mut last_w = 0;
    for ch in text.chars() {
        let g = font.glyph(ch as u32);
        xs.push(pen);
        match g {
            Some(g) => {
                pen += g.adv as i32 + spacing;
                last_w = g.w as i32;
            }
            None => {
                pen += lh;
                last_w = lh;
            }
        }
        gs.push(g);
    }
    let tw = if gs.is_empty() { 0 } else { xs[gs.len() - 1] + last_w };
    let ox = match align {
        0..=2 => 0,
        3..=5 => r.w - tw,
        _ => (r.w - tw) >> 1,
    };
    let oy = match align {
        0 | 3 | 6 => 0,
        2 | 5 | 7 => r.h - lh,
        _ => (r.h - lh) >> 1,
    };
    let desc = font.descent();
    let mut out = vec![];
    for (i, g) in gs.iter().enumerate() {
        if let Some(g) = g {
            out.push(Placed { g: *g, x: r.x + ox + xs[i], y: r.y + oy + lh - desc - g.h as i32 - g.yoff as i32 });
        }
    }
    (out, tw)
}

pub fn text_width(font: Font, text: &str, spacing: i32) -> i32 {
    layout(font, rect(0, 0, 0, 0), text, 0, spacing).1
}

/// Draw text (glyph boxes only; the caller fills the background as the
/// owning widget does in the stock).
pub fn text(font: Font, r: Rect, s: &str, fg: Color, align: u8, spacing: i32, aa: Aa) {
    let (pl, _) = layout(font, r, s, align, spacing);
    let bg = get(r.x + r.w / 2, r.y + r.h / 2);
    probe(P_TEXT, bg);
    for (i, p) in pl.iter().enumerate() {
        crate::host::sub(i as u8);
        glyph(&p.g, p.x, p.y, fg, aa);
    }
    probe_end();
}

pub fn glyph(g: &Glyph, x0: i32, y0: i32, fg: Color, aa: Aa) {
    for gy in 0..g.h as i32 {
        for gx in 0..g.w as i32 {
            let (x, y) = (x0 + gx, y0 + gy);
            if !visible(x, y) {
                continue;
            }
            let v = g.cov(gx as u32, gy as u32);
            match aa {
                Aa::Off => {
                    if v >= 8 {
                        put(x, y, fg);
                    }
                }
                Aa::On => {
                    let a = v * 17;
                    if a >= 3 {
                        put(x, y, blend565(fg, get(x, y), a));
                    }
                }
            }
        }
    }
}

/// Single glyph by code (icons), placed like a 1-character label.
pub fn icon(font: Font, r: Rect, code: u32, fg: Color, align: u8, aa: Aa) {
    probe(P_ICON, get(r.x + r.w / 2, r.y + r.h / 2));
    icon_np(font, r, code, fg, align, aa);
    probe_end();
}

fn icon_np(font: Font, r: Rect, code: u32, fg: Color, align: u8, aa: Aa) {
    if let Some(g) = font.glyph(code) {
        let lh = font.line_h();
        let tw = g.w as i32;
        let ox = match align {
            0..=2 => 0,
            3..=5 => r.w - tw,
            _ => (r.w - tw) >> 1,
        };
        let oy = match align {
            0 | 3 | 6 => 0,
            2 | 5 | 7 => r.h - lh,
            _ => (r.h - lh) >> 1,
        };
        glyph(&g, r.x + ox, r.y + oy + lh - font.descent() - g.h as i32 - g.yoff as i32, fg, aa);
    }
}
