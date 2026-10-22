//! Glyph matrix (tools/font-matrix/run.sh): every glyph (codes below
//! U+2E80) of every embedded font through stockfont's label, glyphs,
//! label_half, cells_diff and Ink::at at several aligns, clips and spacings,
//! the synthesised W / own k included; prints one hash of the whole screen
//! per case. Two trees printing the same list draw every glyph to the same
//! pixels (docs/fonts.md: the font storage change was checked this way).
#![allow(dead_code, unused, static_mut_refs)]
#[path = "../ui-render/host.rs"]
mod host;
pub use host::mmio;
#[path = "../../src/lcd.rs"]
mod lcd;
#[path = "../../src/stockfont.rs"]
mod stockfont;
pub mod probe {
    /// ui-render's clip rule (stockfont probe_fit): not checked here.
    pub fn fit(_r: (i32, i32, i32, i32), _ink: (i32, i32, i32, i32), _text: &str) {}
    pub fn open(kind: u8, bg: u16) -> u32 { crate::host::open(kind, bg) }
    pub fn set(id: u32, bg: u16, g: u8) { crate::host::set(id, bg, g) }
    pub const TEXT: u8 = 0;
    pub const SHAPE: u8 = 1;
    pub const ICON: u8 = 2;
    pub fn begin(kind: u8, bg: u16) { crate::host::begin(kind, bg) }
    pub fn sub(g: u8) { crate::host::sub(g) }
    pub fn end() { crate::host::end() }
    pub fn group(on: bool) { crate::host::group(on) }
}
use stockfont as sf;
use sf::{rect, Font};

fn hash() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in host::emu().fb.chunks(4) {
        let v = c[0] as u64 | (c[1] as u64) << 16 | (c[2] as u64) << 32 | (c[3] as u64) << 48;
        h = (h ^ v).wrapping_mul(0x100_0000_01b3);
        h ^= h >> 29;
    }
    h
}

/// Every embedded font (stock font-object addresses, src/stockfont_data.rs).
const FONTS: &[u32] = &[
    0x08065070, 0x0805ECB8, 0x0805DAB4, 0x080667F8, 0x0805B138, 0x0806C3E8, 0x080592F0, 0x0805965C, 0x08059D00,
    0x0805A9FC, 0x0804FA4C, 0x0806BF24, 0x0805B2E8, 0x0806C724, 0x0805A0E0, 0x0806B13C, 0x08059428, 0x0805C5DC,
    0x0805592C, 0x08054828, 0x0804F24C, 0x08054148, 0x08054FC0, 0x08055014, 0x08065558, 0x0806BD44, 0x0804F260,
];

fn main() {
    let mut n = 0u64;
    let mut out = String::new();
    let (fg, bg) = (0xFFE0u16, 0x2966u16);
    let mut list = Vec::new();
    for &a in FONTS {
        for c in 0..0x2E80u32 {
            if Font(a).glyph(c).is_some() || c == 'W' as u32 || c == 'k' as u32 {
                list.push((a, c));
            }
        }
    }
    for (a, c) in list {
        let f = Font(a);
        let ch = char::from_u32(c).unwrap();
        let mut b = [0u8; 4];
        let s1 = ch.encode_utf8(&mut b).to_string();
        let lh = f.line_h() as i16;
        let texts = [s1.clone(), format!("{}{}{}", s1, s1, '0'), format!("A{}W", s1)];
        let mut case = |tag: &str, out: &mut String| {
            n += 1;
            out.push_str(&format!("{:08x} {} {} {:016x}\n", a, c, tag, hash()));
        };
        for (ti, t) in texts.iter().enumerate() {
            for align in [0u8, 4, 7, 8] {
                for &(rx, ry, rw, rh) in &[(10i16, 10i16, 300i16, lh + 40), (-7, -5, lh / 2 + 3, lh / 2 + 2), (470, 300, 40, 40)] {
                    for sp in [-3i8, 1] {
                        host::reset(0x1234);
                        sf::label(f, rect(rx, ry, rw, rh), t, fg, bg, align, sp);
                        case(&format!("label{} a{} r{},{} s{}", ti, align, rx, ry, sp), &mut out);
                        if rx >= 0 {
                            host::reset(0x1234);
                            sf::glyphs(f, rect(rx, ry, rw, rh), t, fg, bg, align, sp);
                            case(&format!("glyphs{} a{} r{},{} s{}", ti, align, rx, ry, sp), &mut out);
                        }
                        host::reset(0x1234);
                        let k = sf::Ink::new(f, rect(rx, ry, rw, rh), t, align, sp);
                        let e = host::emu();
                        for y in 0..host::H as i16 {
                            for x in 0..host::W as i16 {
                                if let Some(g) = k.at(x, y) { e.fb[y as usize * host::W + x as usize] = 0xF000 | g as u16; }
                            }
                        }
                        case(&format!("ink{} a{} r{},{} s{}", ti, align, rx, ry, sp), &mut out);
                    }
                }
            }
            let _ = sf::text_width(f, t, 1);
        }
        for base in [lh + 10, lh / 2 + 3, 5] {
            host::reset(0x1234);
            sf::label_half(f, rect(20, 7, 40, lh), ch, base, fg, bg);
            case(&format!("half b{}", base), &mut out);
        }
        host::reset(0x1234);
        sf::cells_diff(f, rect(5, 5, 200, lh + 4), &format!("{}{}", s1, s1), "", &[30, 40], fg, bg);
        case("cells", &mut out);
    }
    print!("{}", out);
    eprintln!("cases {}", n);
}
