//! Anti-aliased text straight to the panel (no framebuffer).
//!
//! Every character is drawn as a full cell (advance x ascent+descent) so text
//! overwrites what was there without a separate clear, i.e. without flicker.
//! Glyph data format: see tools/mkfont.py.

use crate::lcd::{self, Color};

pub struct Glyph {
    pub ch: char,
    pub off: u32,
    pub w: u16,
    pub h: u16,
    pub x_off: u16,
    pub top: i16,
    pub adv: u16,
}

pub struct Font {
    pub ascent: u16,
    pub descent: u16,
    pub glyphs: &'static [Glyph],
    pub data: &'static [u8],
}

impl Font {
    fn glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.iter().find(|g| g.ch == ch)
    }

    pub fn height(&self) -> u16 {
        self.ascent + self.descent
    }

    pub fn width(&self, s: &str) -> u16 {
        s.chars().filter_map(|c| self.glyph(c)).map(|g| g.adv).sum()
    }
}

/// 16-step blend table from `bg` (alpha 0) to `fg` (alpha 15), RGB565.
fn ramp(fg: Color, bg: Color) -> [u16; 16] {
    let split = |c: Color| (((c >> 11) & 0x1F) as i32, ((c >> 5) & 0x3F) as i32, (c & 0x1F) as i32);
    let (fr, fgr, fb) = split(fg);
    let (br, bgr, bb) = split(bg);
    let mut t = [0u16; 16];
    for (a, v) in t.iter_mut().enumerate() {
        let a = a as i32;
        let mix = |f: i32, b: i32| (b + (f - b) * a / 15) as u16;
        *v = (mix(fr, br) << 11) | (mix(fgr, bgr) << 5) | mix(fb, bb);
    }
    t
}

/// Draw `s` with its baseline at `y`, starting at `x`. Returns the end x.
/// Characters missing from the font are skipped. Clips at the right edge.
pub fn draw(font: &Font, x: u16, y: u16, s: &str, fg: Color, bg: Color) -> u16 {
    let lut = ramp(fg, bg);
    let top = y.saturating_sub(font.ascent);
    let rows = font.height();
    let mut x = x;
    for ch in s.chars() {
        let Some(g) = font.glyph(ch) else { continue };
        if x + g.adv > lcd::W {
            break;
        }
        draw_cell(font, g, x, top, rows, &lut);
        x += g.adv;
    }
    x
}

/// Draw `s` right-aligned to `x_end` and clear `[x_start, start of text)` to
/// `bg`, so a shorter string leaves no trace of a longer previous one.
pub fn draw_right(font: &Font, x_start: u16, x_end: u16, y: u16, s: &str, fg: Color, bg: Color) {
    let w = font.width(s);
    let x = x_end.saturating_sub(w).max(x_start);
    if x > x_start {
        lcd::fill_rect(x_start, y.saturating_sub(font.ascent), x - x_start, font.height(), bg);
    }
    draw(font, x, y, s, fg, bg);
}

fn draw_cell(font: &Font, g: &Glyph, x: u16, top: u16, rows: u16, lut: &[u16; 16]) {
    lcd::set_window(x, top, x + g.adv - 1, top + rows - 1);
    // Glyph box rows inside the cell (cell row 0 = ascent above the baseline).
    let gy0 = font.ascent as i16 - g.top;
    let mut runs = font.data[g.off as usize..].iter();
    let (mut run_left, mut run_alpha) = (0u8, 0u8);
    // Glyph rows above the cell (taller than the ascent) are skipped.
    let mut skip = (-gy0).max(0) as u32 * g.w as u32;
    while skip > 0 {
        if run_left == 0 {
            let b = *runs.next().unwrap_or(&0);
            run_left = (b >> 4) + 1;
            run_alpha = b & 0xF;
        }
        let n = (run_left as u32).min(skip);
        run_left -= n as u8;
        skip -= n;
    }
    for r in 0..rows as i16 {
        let in_rows = r >= gy0 && r < gy0 + g.h as i16;
        for c in 0..g.adv {
            let mut a = 0u8;
            if in_rows && c >= g.x_off && c < g.x_off + g.w {
                if run_left == 0 {
                    let b = *runs.next().unwrap_or(&0);
                    run_left = (b >> 4) + 1;
                    run_alpha = b & 0xF;
                }
                run_left -= 1;
                a = run_alpha;
            }
            lcd::data(lut[a as usize]);
        }
    }
}
