//! The second UI language of a localized image (features `zh`, `ru`, `it`,
//! `fr`; the default image is English only and this module compiles to
//! nothing). Settings > Display > Language switches between English and it.
//!
//! The draw layer translates: `label`, `glyphs`, `text_width` and `Ink::new`
//! (stockfont.rs `ui_text`) look each text up by the hash of the English
//! string, so call sites, DBG:VIEW? and SCPI keep the English text. A string
//! stays English when it has no entry, its font lacks a glyph or it does not
//! fit its box.
//!
//! i18n/<lang>.tsv -> tools/mklang.py -> lang_<lang>.rs. Chinese: each hanzi is
//! U+0400 + its glyph index in one 14 px 1 bpp set (`bits`, every font).
//! Other languages: glyphs missing from a stock font come per font (`extra`).

#[cfg(any(all(feature = "zh", any(feature = "ru", feature = "it", feature = "fr")), all(feature = "ru", any(feature = "it", feature = "fr")), all(feature = "it", feature = "fr")))]
compile_error!("one UI language per image");

#[cfg(any(feature = "zh", feature = "ru", feature = "it", feature = "fr"))]
mod imp {
    #[allow(unused_imports)]
    use super::super::data::{Entry, FontData, BIT1, E, HUF};
    use core::sync::atomic::{AtomicBool, Ordering::Relaxed};

    #[cfg(feature = "zh")]
    include!("lang_zh.rs");
    #[cfg(feature = "ru")]
    include!("lang_ru.rs");
    #[cfg(feature = "it")]
    include!("lang_it.rs");
    #[cfg(feature = "fr")]
    include!("lang_fr.rs");

    const BASE: u32 = 0x400;
    static ON: AtomicBool = AtomicBool::new(false);

    pub fn on() -> bool {
        ON.load(Relaxed)
    }

    pub fn set(on: bool) {
        ON.store(on, Relaxed);
    }

    /// The translation of `text` (`text` itself if none); `line_h`: the line
    /// height of its font.
    pub fn tr(line_h: impl FnOnce() -> u16, text: &str) -> &str {
        if !on() || line_h() < MIN_LINE {
            return text;
        }
        let h = text.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
        let Ok(i) = H.binary_search(&h) else {
            return text;
        };
        // SAFETY: mklang.py writes each slice as one encoded str.
        unsafe { core::str::from_utf8_unchecked(&T[O[i] as usize..O[i + 1] as usize]) }
    }

    /// Chinese: the bitmap of `code` (column-major, MSB first), its size.
    pub fn bits(code: u32) -> Option<(&'static [u8], u8)> {
        let i = code.wrapping_sub(BASE) as usize;
        (i < N).then(|| (&G[i * GB..(i + 1) * GB], SIZE))
    }

    /// The glyphs this language adds to the stock font at `addr`.
    pub fn extra(addr: u32) -> Option<&'static FontData> {
        EXTRA.iter().find(|e| e.0 == addr).map(|e| &e.1)
    }
}

#[cfg(not(any(feature = "zh", feature = "ru", feature = "it", feature = "fr")))]
mod imp {
    use super::super::data::FontData;

    #[inline(always)]
    pub fn on() -> bool {
        false
    }
    #[inline(always)]
    pub fn set(_on: bool) {}
    #[inline(always)]
    pub fn tr(_line_h: impl FnOnce() -> u16, text: &str) -> &str {
        text
    }
    #[inline(always)]
    pub fn bits(_code: u32) -> Option<(&'static [u8], u8)> {
        None
    }
    #[inline(always)]
    pub fn extra(_addr: u32) -> Option<&'static FontData> {
        None
    }
}

pub use imp::*;
