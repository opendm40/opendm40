//! Static RAM pools with explicit tenants (docs/ram-pools-audit.md,
//! "Design"): no heap; a pool is one static byte area, each tenant has a
//! fixed range in it (`Spec`), and a tenant reaches its bytes only through
//! `Pool::take`, typed by its `Tenancy` impl.
//!
//! - Holders: a bit per tenant (`owner`). `take` by a tenant that does not
//!   hold its bytes evicts every holder whose range overlaps, when that
//!   holder's rule allows it (`Evict::Drop` / `Restart`), bumps the holder's
//!   generation (`gen`) and sets the new tenant's bytes up (`Tenancy::init`).
//!   An overlapping `Evict::Never` holder makes `take` return None.
//! - An evicted tenant notices on its next `take` (its bytes are set up
//!   again) or by comparing `gen` with the value it kept (a job that
//!   spans main-loop passes).
//! - Main loop only, as every tenant: plain atomic loads / stores (no
//!   read-modify-write), so a debug query can read them. No reference is
//!   kept across a call that may `take` for another tenant of the same pool.
//! - The compatibility table: per pool, the tenant pairs the code lets be
//!   live at the same time (`*_COEXIST`). tools/pool-test fails when two of
//!   them share a byte, a spec leaves its pool, or an overlap involves a
//!   `Never` tenant.
//!
//! Pools:
//! - `PAGE` (SRAM): page state that only one view needs at a time and that
//!   is rebuilt on its next use (all `Drop`): the scope graph (scope_ui.rs
//!   `GraphSet`: GRAPH, SHOWN, ROWS), the gallery (gallery.rs), the
//!   generator preview's trace rows (ui.rs `GenSp`).
//! - `TAIL` (SRAM, right after the scope record ring `REC.buf`): the
//!   buffers that lend their bytes to the scope record while idle (the CH2
//!   store, the logger ring's slots, the CSV row FIFO and its MMEM:CAT?
//!   cache); the scope takes the free prefix as the record's tail
//!   (`ScopeT1..3`, `Evict::Yield`: `Scope::yield_tail`).

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Every tenant of every pool (the bit / index in `Pool::owner` / `gen`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Tenant {
    ScopeGraph = 0,
    Gallery = 1,
    GenPreview = 2,
    /// TAIL: the stored CH2 samples (scope_ch2.rs `Data`).
    Ch2Store = 3,
    /// TAIL: the logger ring's slots (logger.rs `Slots`).
    RingSlots = 4,
    /// TAIL: the CSV recording's / a fast-log session's row FIFO (csvlog.rs).
    RowFifo = 5,
    /// TAIL: the MMEM:CAT? reply cache in the FIFO's head (csvlog.rs `Cat`).
    CatCache = 6,
    /// TAIL: the scope record's tail over the CH2 store / the ring's slots
    /// / the FIFO (scope.rs `Seg`).
    ScopeT1 = 7,
    ScopeT2 = 8,
    ScopeT3 = 9,
}

pub const TENANTS: usize = 10;

/// What happens to a holder when an overlapping tenant takes the bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evict {
    /// Never evicted (DMA / an ISR / unsaved data): the other `take` fails.
    #[allow(dead_code)]
    Never,
    /// Dropped: set up again (`Tenancy::init`) on its next `take`.
    Drop,
    /// A job that starts over when its `gen` moved.
    #[allow(dead_code)]
    Restart,
    /// Restartable, but it gives its bytes up itself (`give`) before an
    /// overlapping tenant takes them: it has DMA to stop or data to move
    /// first (the scope record's tail, `Scope::yield_tail`). The pool
    /// refuses an overlapping `take` while it holds them, as for `Never`.
    #[allow(dead_code)]
    Yield,
}

/// A tenant's range in its pool.
#[derive(Clone, Copy)]
pub struct Spec {
    pub t: Tenant,
    pub name: &'static str,
    pub off: usize,
    pub len: usize,
    pub evict: Evict,
}

/// A type that lives in a pool as tenant `T`.
///
/// # Safety
/// `init` must leave a valid `Self` in `*p` from arbitrary bytes, and the
/// impl must check `size_of::<Self>() <= spec.len` and the alignment
/// (`pool::fits`, in a `const` assert next to the impl).
pub unsafe trait Tenancy: Sized + 'static {
    const T: Tenant;
    /// Its offset in its pool (`spec_of(..).off`).
    const OFF: usize;
    /// Set fresh bytes up (the previous tenant's are garbage).
    fn init(p: *mut Self);
}

/// Pool byte area: 8-byte aligned words.
#[repr(C, align(8))]
pub struct Mem<const W: usize>(pub core::cell::UnsafeCell<[u64; W]>);

// SAFETY: main loop only (module doc).
unsafe impl<const W: usize> Sync for Mem<W> {}

impl<const W: usize> Mem<W> {
    pub const fn new() -> Self {
        Mem(core::cell::UnsafeCell::new([0; W]))
    }
}

/// A pool's bookkeeping (the bytes themselves are a `Mem` static that may
/// live in a NOLOAD section).
pub struct Pool {
    pub name: &'static str,
    pub specs: &'static [Spec],
    pub bytes: usize,
    base: *mut u8,
    owner: AtomicU32,
    gen: [AtomicU32; TENANTS],
}

/// `size_of::<T>() <= len` and 8-byte alignment or less (`Tenancy` impls).
pub const fn fits<T>(len: usize) -> bool {
    core::mem::size_of::<T>() <= len && core::mem::align_of::<T>() <= 8
}

/// Byte ranges `a` and `b` share a byte.
pub const fn overlap(a: &Spec, b: &Spec) -> bool {
    a.off < b.off + b.len && b.off < a.off + a.len
}

/// The spec of `t` in `specs` (const: the `Tenancy` impls' asserts).
pub const fn spec_of(specs: &[Spec], t: Tenant) -> Spec {
    let mut i = 0;
    while i < specs.len() {
        if specs[i].t as u8 == t as u8 {
            return specs[i];
        }
        i += 1;
    }
    panic!("tenant not in this pool");
}

// SAFETY: main loop only (module doc); `base` is a static's address.
unsafe impl Sync for Pool {}

impl Pool {
    pub const fn new(name: &'static str, specs: &'static [Spec], bytes: usize, base: *mut u8) -> Pool {
        Pool { name, specs, bytes, base, owner: AtomicU32::new(0), gen: [const { AtomicU32::new(0) }; TENANTS] }
    }

    fn spec(&self, t: Tenant) -> Option<&'static Spec> {
        self.specs.iter().find(|s| s.t == t)
    }

    /// Tenant `T`'s bytes, taken (and set up) when it does not hold them.
    #[allow(dead_code)] // the firmware's tenants use `take_ptr` (tools/pool-test this)
    #[inline(always)]
    pub fn take<T: Tenancy>(&self) -> Option<&'static mut T> {
        // SAFETY: set up and held (`take_raw`).
        self.take_raw::<T>().map(|p| unsafe { &mut *p })
    }

    /// `take` for a pool whose tenants overlapping `T` are all `Drop` /
    /// `Restart` (tools/pool-test checks PAGE; not TAIL), where it cannot fail; a raw
    /// pointer (no reference to the whole tenant is made), so that the
    /// callers project disjoint fields (`addr_of_mut!((*p).f)`) as they did
    /// from separate statics.
    #[inline(always)]
    pub fn take_ptr<T: Tenancy>(&self) -> *mut T {
        match self.take_raw::<T>() {
            Some(p) => p,
            // SAFETY: no `Never` tenant overlaps and the size is asserted
            // at compile time (`fits`), so `acquire_raw` returns non-null.
            None => unsafe { core::hint::unreachable_unchecked() },
        }
    }

    /// Tenant `T`'s bytes only while it holds them (no eviction).
    #[allow(dead_code)]
    #[inline(always)]
    pub fn held<T: Tenancy>(&self) -> Option<&'static mut T> {
        // SAFETY: held, so set up.
        (self.owner.load(Relaxed) & (1 << T::T as u32) != 0).then(|| unsafe { &mut *(self.base.add(T::OFF) as *mut T) })
    }

    #[inline(always)]
    fn take_raw<T: Tenancy>(&self) -> Option<*mut T> {
        if self.owner.load(Relaxed) & (1 << T::T as u32) != 0 {
            // SAFETY: held; inside the pool (pool-test).
            return Some(unsafe { self.base.add(T::OFF) } as *mut T);
        }
        fn init<T: Tenancy>(p: *mut u8) {
            T::init(p as *mut T);
        }
        let p = self.acquire_raw(T::T, core::mem::size_of::<T>(), T::OFF, init::<T>) as *mut T;
        (!p.is_null()).then_some(p)
    }

    /// `take_raw`'s slow path, once for every tenant type: evict the
    /// overlapping holders, take, set up; null when it cannot.
    #[inline(never)]
    fn acquire_raw(&self, t: Tenant, size: usize, off: usize, init: fn(*mut u8)) -> *mut u8 {
        let Some(s) = self.spec(t) else { return core::ptr::null_mut() };
        if size > s.len || off != s.off {
            return core::ptr::null_mut();
        }
        let own = self.owner.load(Relaxed);
        let mut lose = 0u32;
        for o in self.specs {
            if own & (1 << o.t as u32) != 0 && o.t != s.t && overlap(o, s) {
                if matches!(o.evict, Evict::Never | Evict::Yield) {
                    return core::ptr::null_mut();
                }
                lose |= 1 << o.t as u32;
            }
        }
        for o in self.specs {
            if lose & (1 << o.t as u32) != 0 {
                let g = &self.gen[o.t as usize];
                g.store(g.load(Relaxed).wrapping_add(1), Relaxed);
            }
        }
        self.owner.store((own & !lose) | 1 << s.t as u32, Relaxed);
        // SAFETY: inside the pool (pool-test: every spec ends <= bytes).
        let p = unsafe { self.base.add(s.off) };
        init(p);
        p
    }

    /// `t` holds its bytes.
    pub fn holds(&self, t: Tenant) -> bool {
        self.owner.load(Relaxed) & (1 << t as u32) != 0
    }

    /// Times `t` lost its bytes.
    #[allow(dead_code)]
    pub fn gen(&self, t: Tenant) -> u32 {
        self.gen[t as usize].load(Relaxed)
    }

    /// Give `t`'s bytes up (counts as a loss for `gen`).
    #[allow(dead_code)]
    pub fn give(&self, t: Tenant) {
        let own = self.owner.load(Relaxed);
        if own & (1 << t as u32) != 0 {
            self.owner.store(own & !(1 << t as u32), Relaxed);
            let g = &self.gen[t as usize];
            g.store(g.load(Relaxed).wrapping_add(1), Relaxed);
        }
    }

    /// DBG:POOL? text: `NAME,bytes,holders;tenant:off+len,rule,held,gen;...`
    #[cfg_attr(not(any(feature = "dbg-pool", test)), allow(dead_code))]
    pub fn text(&self, out: &mut [u8]) -> usize {
        let mut n = 0;
        let mut put = |b: &[u8]| {
            for &c in b {
                if n < out.len() {
                    out[n] = c;
                    n += 1;
                }
            }
        };
        let mut t = [0u8; 10];
        put(self.name.as_bytes());
        put(b",");
        put(dec(&mut t, self.bytes as u32));
        for s in self.specs {
            put(b";");
            put(s.name.as_bytes());
            put(b":");
            put(dec(&mut t, s.off as u32));
            put(b"+");
            put(dec(&mut t, s.len as u32));
            put(match s.evict {
                Evict::Never => b",never",
                Evict::Drop => b",drop",
                Evict::Restart => b",restart",
                Evict::Yield => b",yield",
            });
            put(if self.holds(s.t) { b",held," } else { b",free," });
            put(dec(&mut t, self.gen(s.t)));
        }
        n
    }
}

#[cfg_attr(not(any(feature = "dbg-pool", test)), allow(dead_code))]
fn dec(t: &mut [u8; 10], mut v: u32) -> &[u8] {
    let mut k = 10;
    loop {
        k -= 1;
        t[k] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            return &t[k..];
        }
    }
}

// ------------------------------------------------------------------ PAGE

/// Bytes of the page pool: the largest tenant, the scope graph (GRAPH
/// 16 380 + SHOWN 344 + ROWS 320 on the device; each tenant's impl asserts
/// it fits, so this fails the build when one grows).
#[cfg(target_os = "none")]
pub const PAGE_BYTES: usize = 17_048;
/// Host builds (tools/ui-render, touch-test, gallery-test): 64-bit layouts.
#[cfg(not(target_os = "none"))]
pub const PAGE_BYTES: usize = 32 * 1024;

/// One view at a time: every tenant starts at 0 and is rebuilt on its next
/// use.
pub const PAGE_SPECS: [Spec; 3] = [
    Spec { t: Tenant::ScopeGraph, name: "ScopeGraph", off: 0, len: PAGE_BYTES, evict: Evict::Drop },
    Spec { t: Tenant::Gallery, name: "Gallery", off: 0, len: 1024, evict: Evict::Drop },
    Spec { t: Tenant::GenPreview, name: "GenPreview", off: 0, len: 512, evict: Evict::Drop },
];

/// Tenant pairs of PAGE the code lets be live at the same time: none. The
/// scope graph is drawn on the Scope / combined / wave views only (scope_ui
/// page paths; `App::scope_poll` draws only with `sp_on`), the gallery on
/// the Gallery view only (app.rs: `gallery::get` callers), the generator
/// preview's rows in the Gen page's strip passes only (ui.rs
/// `gen_preview`). Off-page writers use `held` (ch2_set, the wave view's
/// clear, sp_leave).
#[cfg_attr(not(test), allow(dead_code))]
pub const PAGE_COEXIST: &[(Tenant, Tenant)] = &[];

#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static PAGE_MEM: Mem<{ PAGE_BYTES.div_ceil(8) }> = Mem::new();

pub static PAGE: Pool = Pool::new("PAGE", &PAGE_SPECS, PAGE_BYTES, PAGE_MEM.0.get() as *mut u8);

// ------------------------------------------------------------------ TAIL

/// The scope record ring's fixed part (samples; `scope::CAP`): 44 K, 1 K
/// less per RAM-hungry debug feature (dbg-touch, acdc-trace).
pub const REC_BASE: usize = (44 - cfg!(feature = "dbg-touch") as usize - cfg!(feature = "acdc-trace") as usize) * 1024;
/// TAIL segments in address order (bytes): the CH2 store (scope_ch2.rs
/// `Data`: 4096 codes + the anchors), the logger ring's slots (logger.rs
/// `Slots`: 2048 x 8 B), the row FIFO (csvlog.rs, 8 KiB). The tenants'
/// impls assert their sizes.
pub const SEG_CH2: usize = 8736;
pub const SEG_RING: usize = 16_384;
pub const SEG_FIFO: usize = 8192;
pub const SEGS: [usize; 3] = [SEG_CH2, SEG_RING, SEG_FIFO];
pub const TAIL_BYTES: usize = SEG_CH2 + SEG_RING + SEG_FIFO;
/// Longest record (samples): DMA1 S4's NDTR is 16 bit.
pub const REC_MAX: usize = REC_BASE + TAIL_BYTES / 2;
const _: () = assert!(REC_MAX <= 65_535 && SEG_CH2 % 8 == 0 && SEG_RING % 8 == 0 && SEG_FIFO % 8 == 0);
/// Scope ring length (samples) with the tail up to, not including,
/// segment `k` (0..=3): CAP, + the CH2 store, + the ring's slots, + the FIFO.
pub const fn tail_keep(k: usize) -> usize {
    let mut n = REC_BASE;
    let mut j = 0;
    while j < k && j < 3 {
        n += SEGS[j] / 2;
        j += 1;
    }
    n
}

/// A stored record of `len` samples cut to `keep` (scope.rs `cut`): the
/// first sample kept, so the window around the screen centre `anchor`
/// stays (a roll record's anchor is its newest sample: its newest `keep`).
pub fn cut_from(len: usize, keep: usize, anchor: usize) -> usize {
    if len <= keep { 0 } else { anchor.saturating_sub(keep / 2).min(len - keep) }
}

/// The MMEM:CAT? cache: the FIFO's head (csvlog.rs `CAT_MAX`).
pub const CAT_BYTES: usize = 6144;

/// The tail's tenants. Each lender holds its segment while it has data
/// there (CH2 on or a stopped record's CH2; a stream or unread host
/// records; a recording or a fast-log session); the scope takes the free
/// prefix as `ScopeT1..3` (`Yield`: it stops its DMA / cuts its stored
/// record before giving them, main.rs `App::tail_lend` before each
/// lender's start). The cache is dropped by the FIFO's other tenants.
pub const TAIL_SPECS: [Spec; 7] = [
    Spec { t: Tenant::Ch2Store, name: "Ch2Store", off: 0, len: SEG_CH2, evict: Evict::Never },
    Spec { t: Tenant::RingSlots, name: "RingSlots", off: SEG_CH2, len: SEG_RING, evict: Evict::Never },
    Spec { t: Tenant::RowFifo, name: "RowFifo", off: SEG_CH2 + SEG_RING, len: SEG_FIFO, evict: Evict::Never },
    Spec { t: Tenant::CatCache, name: "CatCache", off: SEG_CH2 + SEG_RING, len: CAT_BYTES, evict: Evict::Drop },
    Spec { t: Tenant::ScopeT1, name: "ScopeT1", off: 0, len: SEG_CH2, evict: Evict::Yield },
    Spec { t: Tenant::ScopeT2, name: "ScopeT2", off: SEG_CH2, len: SEG_RING, evict: Evict::Yield },
    Spec { t: Tenant::ScopeT3, name: "ScopeT3", off: SEG_CH2 + SEG_RING, len: SEG_FIFO, evict: Evict::Yield },
];

/// Tenant pairs of TAIL the code lets be live at the same time: the
/// lenders with each other (CH2 needs the store and the ring; a fast-log
/// session the ring and the FIFO; the cache is warmed before a stream
/// starts), and each scope segment with the lenders of the other
/// segments.
#[cfg_attr(not(test), allow(dead_code))]
pub const TAIL_COEXIST: &[(Tenant, Tenant)] = &[
    (Tenant::Ch2Store, Tenant::RingSlots),
    (Tenant::Ch2Store, Tenant::RowFifo),
    (Tenant::Ch2Store, Tenant::CatCache),
    (Tenant::RingSlots, Tenant::RowFifo),
    (Tenant::RingSlots, Tenant::CatCache),
    (Tenant::ScopeT1, Tenant::ScopeT2),
    (Tenant::ScopeT1, Tenant::ScopeT3),
    (Tenant::ScopeT2, Tenant::ScopeT3),
    (Tenant::ScopeT1, Tenant::RingSlots),
    (Tenant::ScopeT1, Tenant::RowFifo),
    (Tenant::ScopeT1, Tenant::CatCache),
    (Tenant::ScopeT2, Tenant::Ch2Store),
    (Tenant::ScopeT2, Tenant::RowFifo),
    (Tenant::ScopeT2, Tenant::CatCache),
    (Tenant::ScopeT3, Tenant::Ch2Store),
];

/// Overlaps of TAIL in which a `take` can be refused (a `Never` / `Yield`
/// side), each with the code that hands the bytes over: a lender's start
/// calls `App::tail_lend` (the scope's `yield_tail`) first; the scope
/// takes a segment only while its lender is idle (`App::tail_reclaim`
/// gives an idle lender's segment back; `Scope::grab_tail` stops at the
/// first refused one); the cache (`Drop`) is built only when the pool
/// gives its bytes (csvlog.rs `cat_buf`: None = no cache).
#[cfg_attr(not(test), allow(dead_code))]
pub const TAIL_HANDOFF: &[(Tenant, Tenant)] = &[
    (Tenant::Ch2Store, Tenant::ScopeT1),
    (Tenant::RingSlots, Tenant::ScopeT2),
    (Tenant::RowFifo, Tenant::ScopeT3),
    (Tenant::RowFifo, Tenant::CatCache),
    (Tenant::ScopeT3, Tenant::CatCache),
];

/// The scope record ring (`buf`, the DMA's) and right after it the tail's
/// bytes: one `repr(C)` static, so the ring may run on into the tail.
#[repr(C, align(8))]
pub struct RecMem {
    pub buf: core::cell::UnsafeCell<[u16; REC_BASE]>,
    pub tail: Mem<{ TAIL_BYTES / 8 }>,
}

// SAFETY: main loop only (module doc); the DMA writes `buf` and the tail
// segments the scope holds, the stream ISR the ring's slots while the
// logger holds them.
unsafe impl Sync for RecMem {}

const _: () = assert!(core::mem::size_of::<RecMem>() == REC_MAX * 2 && REC_BASE * 2 % 8 == 0);

/// SRAM .dmabuf (NOLOAD): every tenant sets its bytes up (`Tenancy::init`).
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
pub static REC: RecMem = RecMem { buf: core::cell::UnsafeCell::new([0; REC_BASE]), tail: Mem::new() };

/// The record ring's first sample (`REC_MAX` samples run on from it).
#[inline(always)]
pub fn rec_base() -> *mut u16 {
    REC.buf.get() as *mut u16
}

pub static TAIL: Pool = Pool::new("TAIL", &TAIL_SPECS, TAIL_BYTES, REC.tail.0.get() as *mut u8);
