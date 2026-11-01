//! Read-only data that only region C code reads, placed in region C
//! (`.rodata.cold`, link-native.x): an anonymous constant (a string
//! literal, a struct constant copied at run time) lands in region B even
//! when its only readers are in region C (docs/native-boot.md "Region B
//! audit #10"). Host builds: the same macros without the section.

/// A string literal in region C: a named static in `.rodata.cold`
/// (link-native.x), not an anonymous constant (those go to region B, even
/// when only region C code reads them). Only for text that region C code
/// alone reads: never on a path that runs while an AD2 stream does.
#[macro_export]
macro_rules! cold_str {
    ($s:literal) => {{
        #[cfg_attr(target_os = "none", link_section = ".rodata.cold")]
        static S: [u8; $s.len()] = {
            let b = $s.as_bytes();
            let mut a = [0u8; $s.len()];
            let mut i = 0;
            while i < b.len() {
                a[i] = b[i];
                i += 1;
            }
            a
        };
        // A literal's bytes: UTF-8.
        unsafe { core::str::from_utf8_unchecked(&S) }
    }};
}

/// `*dst = VALUE` (a `const` of type `T`) with the value's bytes in region
/// C (`.rodata.cold`, see `cold_str!`): a constant struct assigned at run
/// time is copied from an anonymous constant in region B otherwise. For
/// region C code only; `T` has no destructor (the old value is overwritten).
#[macro_export]
macro_rules! cold_set {
    ($dst:expr, $t:ty, $v:expr) => {{
        const _: () = assert!(!core::mem::needs_drop::<$t>());
        #[cfg_attr(target_os = "none", link_section = ".rodata.cold")]
        static V: $crate::cold::Cold<$t> = $crate::cold::Cold($v);
        let d: *mut $t = $dst;
        unsafe { core::ptr::copy_nonoverlapping(&V.0, d, 1) }
    }};
}

/// A value read only (`cold_set!`'s static: plain data, never written).
pub struct Cold<T>(pub T);
// SAFETY: never written (a `static` of plain data), only copied out.
unsafe impl<T> Sync for Cold<T> {}
