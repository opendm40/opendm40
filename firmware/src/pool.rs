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

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Every tenant of every pool (the bit / index in `Pool::owner` / `gen`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Tenant {
    ScopeGraph = 0,
    Gallery = 1,
    GenPreview = 2,
}

pub const TENANTS: usize = 3;

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
    #[inline(always)]
    pub fn take<T: Tenancy>(&self) -> Option<&'static mut T> {
        if self.owner.load(Relaxed) & (1 << T::T as u32) != 0 {
            // SAFETY: held; the range was checked by `acquire`.
            return Some(unsafe { &mut *(self.base.add(T::OFF) as *mut T) });
        }
        self.acquire::<T>()
    }

    /// `take` for a pool whose tenants overlapping `T` are all `Drop` /
    /// `Restart` (tools/pool-test checks PAGE), where it cannot fail; a raw
    /// pointer, so that the callers project disjoint fields
    /// (`addr_of_mut!((*p).f)`) as they did from separate statics.
    #[inline(always)]
    pub fn take_ptr<T: Tenancy>(&self) -> *mut T {
        match self.take::<T>() {
            Some(t) => t as *mut T,
            // SAFETY: no `Never` tenant overlaps and the size is asserted
            // at compile time (`fits`), so `acquire` returns Some.
            None => unsafe { core::hint::unreachable_unchecked() },
        }
    }

    /// Tenant `T`'s bytes only while it holds them (no eviction).
    #[inline(always)]
    pub fn held<T: Tenancy>(&self) -> Option<&'static mut T> {
        // SAFETY: as `take`.
        (self.owner.load(Relaxed) & (1 << T::T as u32) != 0).then(|| unsafe { &mut *(self.base.add(T::OFF) as *mut T) })
    }


    #[inline(always)]
    fn acquire<T: Tenancy>(&self) -> Option<&'static mut T> {
        fn init<T: Tenancy>(p: *mut u8) {
            T::init(p as *mut T);
        }
        let p = self.acquire_raw(T::T, core::mem::size_of::<T>(), T::OFF, init::<T>) as *mut T;
        // SAFETY: set up by `init`; held now.
        (!p.is_null()).then(|| unsafe { &mut *p })
    }

    /// `acquire`'s body, once for every tenant type: evict the overlapping
    /// holders, take, set up; null when it cannot.
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
                if o.evict == Evict::Never {
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
pub const PAGE_SPECS: [Spec; TENANTS] = [
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
