//! The UI language of a localized image (features `zh`, `ru`, `it`, `fr`, one
//! per image; the default image is English and this module adds nothing).
//!
//! Resolved at build time: a UI string is written `crate::tr!("Settings")` at
//! its call site (or in its const table). In a localized image `tr!` is
//! generated per language (src/tr_<lang>.rs, one macro arm per translated
//! literal), so the image holds only the translation; a literal without a
//! translation stays English. No lookup at run time, no switch on the device.
//! Text that leaves the device (SCPI, DBG:VIEW?, files) keeps its plain
//! English literal. tools/lang-render fails the build when a translation is
//! wider than its box, lacks a glyph of its font or is drawn in a font under
//! the language's minimum line height (stockfont.rs `ui_text`, host only).
//!
//! i18n/<lang>.tsv -> tools/mklang.py -> tr_<lang>.rs + lang_<lang>.rs (the
//! glyphs). Chinese: each hanzi is U+0400 + its glyph index in one 14 px 1 bpp
//! set (`bits`, every font). Other languages: glyphs missing from a stock font
//! come per font (`extra`).

#[cfg(any(all(feature = "zh", any(feature = "ru", feature = "it", feature = "fr")), all(feature = "ru", any(feature = "it", feature = "fr")), all(feature = "it", feature = "fr")))]
compile_error!("one UI language per image");

// `tr!` (#[macro_export]) and its host check data.
#[cfg(feature = "zh")]
#[path = "tr_zh.rs"]
pub mod tr;
#[cfg(feature = "ru")]
#[path = "tr_ru.rs"]
pub mod tr;
#[cfg(feature = "it")]
#[path = "tr_it.rs"]
pub mod tr;
#[cfg(feature = "fr")]
#[path = "tr_fr.rs"]
pub mod tr;

#[cfg(any(feature = "zh", feature = "ru", feature = "it", feature = "fr"))]
mod imp {
    #[allow(unused_imports)]
    use super::super::data::{Entry, FontData, BIT1, E, HUF};

    #[cfg(feature = "zh")]
    include!("lang_zh.rs");
    #[cfg(feature = "ru")]
    include!("lang_ru.rs");
    #[cfg(feature = "it")]
    include!("lang_it.rs");
    #[cfg(feature = "fr")]
    include!("lang_fr.rs");

    const BASE: u32 = 0x400;
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

/// The UI text `$s` (English image: the literal itself).
#[cfg(not(any(feature = "zh", feature = "ru", feature = "it", feature = "fr")))]
#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $s
    };
}

#[cfg(not(any(feature = "zh", feature = "ru", feature = "it", feature = "fr")))]
mod imp {
    use super::super::data::FontData;

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
