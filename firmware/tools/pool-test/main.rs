//! Host test of src/pool.rs: every pool's table (specs inside the pool, one
//! spec per tenant, 8-byte offsets; no two tenants the code lets be live
//! together (`*_COEXIST`) share a byte; an overlap in which a take can be
//! refused (a `Never` / `Yield` side) only where the code hands the bytes
//! over (`*_HANDOFF`), never two `Never`s), then the
//! lease rules on a test pool (take / held / eviction / generation counters
//! / a `Never` holder refusing, the debug text) and the record tail TAIL:
//! its layout after the scope ring, the record length per state and the
//! conflict matrix (every lender against the scope's tail, the hand-over
//! order, the MMEM:CAT? cache dropped).
//!   tools/pool-test/run.sh
#[allow(dead_code)]
#[path = "../../src/pool.rs"]
mod pool;

use pool::{Evict, Mem, Pool, Spec, Tenancy, Tenant};

static mut FAILS: u32 = 0;
static mut CASES: u32 = 0;
/// The bad-table self-check: count, do not print.
static mut QUIET: bool = false;

fn check(name: &str, ok: bool, msg: String) {
    unsafe {
        CASES += 1;
        if !ok {
            FAILS += 1;
            if !QUIET {
                println!("FAIL {name}: {msg}");
            }
        }
    }
}

/// The table rules for one pool.
fn table(name: &str, bytes: usize, specs: &[Spec], coexist: &[(Tenant, Tenant)], handoff: &[(Tenant, Tenant)]) {
    for (i, s) in specs.iter().enumerate() {
        check(name, s.off + s.len <= bytes, format!("{} ends at {} > {} bytes", s.name, s.off + s.len, bytes));
        check(name, s.off % 8 == 0, format!("{} offset {} not 8-byte aligned", s.name, s.off));
        check(name, s.len > 0, format!("{} is empty", s.name));
        for o in &specs[i + 1..] {
            check(name, o.t != s.t, format!("{} listed twice", s.name));
            if pool::overlap(s, o) {
                // An overlap in which a take can be refused (a Never / Yield
                // side): only a listed hand-over (the code handles the
                // refusal), never two Nevers.
                let hard = |e: Evict| matches!(e, Evict::Never | Evict::Yield);
                if hard(s.evict) || hard(o.evict) {
                    let listed = handoff.iter().any(|&(a, b)| (a, b) == (s.t, o.t) || (a, b) == (o.t, s.t));
                    check(name, listed && !(s.evict == Evict::Never && o.evict == Evict::Never), format!("{} and {} overlap, a take can be refused and no hand-over is listed", s.name, o.name));
                }
            }
        }
    }
    for &(a, b) in coexist {
        let sa = specs.iter().find(|s| s.t == a);
        let sb = specs.iter().find(|s| s.t == b);
        check(name, sa.is_some() && sb.is_some(), format!("{a:?} / {b:?} in COEXIST but not in the pool"));
        if let (Some(sa), Some(sb)) = (sa, sb) {
            check(name, !pool::overlap(sa, sb), format!("{} and {} can be live together but share bytes", sa.name, sb.name));
        }
    }
}

// --- lease rules on a test pool: A (Drop) and B (Drop) overlap, C (Never)
// overlaps B only.
struct A([u32; 4]);
struct B([u8; 24]);
#[allow(dead_code)]
struct C(u64);

const T_SPECS: [Spec; 3] = [
    Spec { t: Tenant::ScopeGraph, name: "A", off: 0, len: 16, evict: Evict::Drop },
    Spec { t: Tenant::Gallery, name: "B", off: 8, len: 24, evict: Evict::Drop },
    Spec { t: Tenant::GenPreview, name: "C", off: 24, len: 8, evict: Evict::Never },
];
static T_MEM: Mem<4> = Mem::new();
static T: Pool = Pool::new("T", &T_SPECS, 32, T_MEM.0.get() as *mut u8);

static mut INITS: [u32; 3] = [0; 3];

unsafe impl Tenancy for A {
    const T: Tenant = Tenant::ScopeGraph;
    const OFF: usize = 0;
    fn init(p: *mut Self) {
        unsafe {
            p.write(A([0xAAAA_AAAA; 4]));
            INITS[0] += 1;
        }
    }
}
unsafe impl Tenancy for B {
    const T: Tenant = Tenant::Gallery;
    const OFF: usize = 8;
    fn init(p: *mut Self) {
        unsafe {
            p.write(B([0xBB; 24]));
            INITS[1] += 1;
        }
    }
}
unsafe impl Tenancy for C {
    const T: Tenant = Tenant::GenPreview;
    const OFF: usize = 24;
    fn init(p: *mut Self) {
        unsafe {
            p.write(C(0xCC));
            INITS[2] += 1;
        }
    }
}

fn inits() -> [u32; 3] {
    unsafe { INITS }
}

fn leases() {
    let n = "lease";
    check(n, T.held::<A>().is_none(), "A held before any take".into());
    let a = T.take::<A>().unwrap();
    check(n, a.0 == [0xAAAA_AAAA; 4] && inits() == [1, 0, 0], format!("A set up once: {:?}", inits()));
    a.0[0] = 7;
    check(n, T.take::<A>().unwrap().0[0] == 7 && inits()[0] == 1, "a second take of a holder keeps its bytes".into());
    check(n, T.held::<A>().is_some(), "held while A holds".into());
    // B overlaps A (bytes 8..16): A loses its bytes, its gen moves.
    let b = T.take::<B>().unwrap();
    check(n, b.0 == [0xBB; 24] && !T.holds(Tenant::ScopeGraph) && T.gen(Tenant::ScopeGraph) == 1, format!("B evicts A: gen {}", T.gen(Tenant::ScopeGraph)));
    check(n, T.held::<A>().is_none(), "A not held after its eviction".into());
    // C (Never) overlaps B (24..32): B is Drop, so C evicts it.
    let _c = T.take::<C>().unwrap();
    check(n, !T.holds(Tenant::Gallery) && T.gen(Tenant::Gallery) == 1, "C evicts B".into());
    // A does not overlap C: both held.
    let a = T.take::<A>().unwrap();
    check(n, a.0[0] == 0xAAAA_AAAA && inits()[0] == 2 && T.holds(Tenant::GenPreview), "A again: set up anew, C kept".into());
    // B overlaps C (Never): refused, nobody loses anything.
    check(n, T.take::<B>().is_none() && T.holds(Tenant::ScopeGraph) && T.gen(Tenant::ScopeGraph) == 1, "B refused while C (Never) holds".into());
    T.give(Tenant::GenPreview);
    check(n, !T.holds(Tenant::GenPreview) && T.gen(Tenant::GenPreview) == 1, "give: C free, its gen moved".into());
    check(n, T.take::<B>().is_some() && !T.holds(Tenant::ScopeGraph) && T.gen(Tenant::ScopeGraph) == 2, "B after C gave up: evicts A".into());
    let mut out = [0u8; 200];
    let k = T.text(&mut out);
    let t = std::str::from_utf8(&out[..k]).unwrap();
    check(n, t == "T,32;A:0+16,drop,free,2;B:8+24,drop,held,1;C:24+8,never,free,1", format!("text {t:?}"));
    let k = T.text(&mut out[..10]);
    check(n, k == 10, format!("text into a short buffer: {k}"));
    check(n, pool::fits::<A>(16) && !pool::fits::<A>(15) && pool::fits::<C>(8), "fits".into());
}

// --- TAIL: the record tail (scope.rs "record tail", app_tail.rs).

macro_rules! tenant {
    ($name:ident, $t:ident, $len:expr) => {
        #[allow(dead_code)]
        struct $name([u8; $len]);
        unsafe impl Tenancy for $name {
            const T: Tenant = Tenant::$t;
            const OFF: usize = pool::spec_of(&pool::TAIL_SPECS, Tenant::$t).off;
            fn init(_: *mut Self) {}
        }
    };
}
tenant!(Ch2, Ch2Store, pool::SEG_CH2);
tenant!(Slots, RingSlots, pool::SEG_RING);
tenant!(Fifo, RowFifo, pool::SEG_FIFO);
tenant!(Cat, CatCache, pool::CAT_BYTES);
tenant!(S1, ScopeT1, pool::SEG_CH2);
tenant!(S2, ScopeT2, pool::SEG_RING);
tenant!(S3, ScopeT3, pool::SEG_FIFO);

const SEG_T: [Tenant; 3] = [Tenant::ScopeT1, Tenant::ScopeT2, Tenant::ScopeT3];

/// scope.rs `grab_tail`: the free prefix in order; the ring length.
fn grab(ch2_on: bool) -> usize {
    let t = &pool::TAIL;
    let k = if ch2_on {
        0
    } else if t.take::<S1>().is_none() {
        0
    } else if t.take::<S2>().is_none() {
        1
    } else if t.take::<S3>().is_none() {
        2
    } else {
        3
    };
    for &s in &SEG_T[k..] {
        t.give(s);
    }
    pool::tail_keep(k)
}

/// scope.rs `yield_tail` (the bookkeeping part): segments k.. back.
fn yield_from(k: usize) {
    for &s in &SEG_T[k..] {
        pool::TAIL.give(s);
    }
}

fn idle() {
    for s in &pool::TAIL_SPECS {
        pool::TAIL.give(s.t);
    }
}

fn tail() {
    let n = "tail";
    let t = &pool::TAIL;
    // Layout: the tail right after the ring's CAP samples, segments in
    // order and back to back, the longest record within the 16-bit NDTR.
    let base = pool::rec_base() as usize;
    let tail = pool::REC.tail.0.get() as usize;
    check(n, tail == base + pool::REC_BASE * 2, format!("tail at +{} B, ring is {} B", tail - base, pool::REC_BASE * 2));
    let mut off = 0;
    for (k, &st) in SEG_T.iter().enumerate() {
        let sp = pool::spec_of(&pool::TAIL_SPECS, st);
        check(n, sp.off == off && sp.len == pool::SEGS[k], format!("segment {k} at {} + {}", sp.off, sp.len));
        off += sp.len;
    }
    check(n, off == pool::TAIL_BYTES && pool::REC_MAX <= 65_535, format!("tail {off} B, record max {}", pool::REC_MAX));
    check(n, pool::tail_keep(0) == 43_008 && pool::tail_keep(1) == 47_376 && pool::tail_keep(2) == 55_568 && pool::tail_keep(3) == 59_664 && pool::REC_MAX == 59_664,
        format!("lengths {} {} {} {}", pool::tail_keep(0), pool::tail_keep(1), pool::tail_keep(2), pool::tail_keep(3)));

    // Record length per state (the lenders holding their segments).
    idle();
    check(n, grab(false) == 59_664, "idle: CAP + the whole tail".into());
    check(n, grab(false) == 59_664 && t.holds(Tenant::ScopeT3), "a second grab keeps it".into());
    idle();
    check(n, grab(true) == 43_008 && !t.holds(Tenant::ScopeT1), "CH2 on: none (the store is CH2's)".into());
    let cases: [(&str, &[Tenant], usize); 5] = [
        ("CH2 stream (store + slots)", &[Tenant::Ch2Store, Tenant::RingSlots], 43_008),
        ("a stopped record's CH2 (store)", &[Tenant::Ch2Store], 43_008),
        ("LOG:STATe / wave / inrush (slots)", &[Tenant::RingSlots], 47_376),
        ("CSV recording (FIFO)", &[Tenant::RowFifo], 55_568),
        ("fast log (slots + FIFO)", &[Tenant::RingSlots, Tenant::RowFifo], 47_376),
    ];
    for (what, hold, want) in cases {
        idle();
        for &h in hold {
            let ok = match h {
                Tenant::Ch2Store => t.take::<Ch2>().is_some(),
                Tenant::RingSlots => t.take::<Slots>().is_some(),
                _ => t.take::<Fifo>().is_some(),
            };
            check(n, ok, format!("{what}: {h:?} taken"));
        }
        let got = grab(false);
        check(n, got == want, format!("{what}: record {got}, want {want}"));
        for &h in hold {
            check(n, t.holds(h), format!("{what}: {h:?} kept by the scope's grab"));
        }
    }
    // The MMEM:CAT? cache is dropped by the scope's tail; refused while
    // the scope holds it; the FIFO's lender drops it too.
    idle();
    check(n, t.take::<Cat>().is_some(), "cache taken while idle".into());
    let g = t.gen(Tenant::CatCache);
    check(n, grab(false) == 59_664 && !t.holds(Tenant::CatCache) && t.gen(Tenant::CatCache) == g + 1, "the scope's grab drops the cache".into());
    check(n, t.take::<Cat>().is_none(), "no cache while the scope holds the FIFO".into());
    yield_from(2);
    check(n, t.take::<Cat>().is_some() && t.take::<Fifo>().is_some() && !t.holds(Tenant::CatCache), "a recording drops the cache".into());

    // Conflict matrix: each lender against each scope prefix. A lender's
    // take is refused while the scope holds its segment, granted after
    // `yield_tail` from that segment; the yield leaves the segments
    // before it with the scope.
    let lenders: [(Tenant, usize); 3] = [(Tenant::Ch2Store, 0), (Tenant::RingSlots, 1), (Tenant::RowFifo, 2)];
    for (lt, seg) in lenders {
        for held in 0..=3usize {
            idle();
            for &s in &SEG_T[..held] {
                let ok = match s {
                    Tenant::ScopeT1 => t.take::<S1>().is_some(),
                    Tenant::ScopeT2 => t.take::<S2>().is_some(),
                    _ => t.take::<S3>().is_some(),
                };
                check(n, ok, format!("scope takes {s:?}"));
            }
            let take = || match lt {
                Tenant::Ch2Store => t.take::<Ch2>().is_some(),
                Tenant::RingSlots => t.take::<Slots>().is_some(),
                _ => t.take::<Fifo>().is_some(),
            };
            let blocked = held > seg;
            let first = take();
            check(n, first != blocked, format!("{lt:?} with the scope holding {held} segment(s): take {first}"));
            if !first {
                yield_from(seg);
                check(n, take(), format!("{lt:?} after yield_tail({seg})"));
                for &s in &SEG_T[..seg] {
                    check(n, t.holds(s), format!("{lt:?}: the scope keeps {s:?}"));
                }
            }
            // The lender holds it: the scope's next grab stops before it.
            let got = grab(false);
            check(n, got == pool::tail_keep(seg), format!("{lt:?} held: next record {got}"));
        }
    }
    idle();

    // The cut (scope.rs `cut`): the window around the anchor stays.
    let c = |len, keep, anchor| pool::cut_from(len, keep, anchor);
    check(n, c(43_000, 47_376, 100) == 0, "a record that fits: not moved".into());
    check(n, c(59_664, 43_008, 59_663) == 59_664 - 43_008, "roll (anchor = newest): the newest part".into());
    check(n, c(59_664, 43_008, 10) == 0, "anchor near the start: from 0".into());
    check(n, c(59_664, 47_376, 30_000) == 30_000 - 47_376 / 2, "anchor in the middle: centred".into());
    for (len, keep) in [(59_664, 43_008), (59_664, 47_376), (55_568, 43_008), (47_376, 43_008)] {
        for anchor in [0, 1, 20_000, 30_000, 40_000, len - 1] {
            let f = c(len, keep, anchor);
            check(n, f + keep <= len && anchor >= f && anchor - f < keep, format!("cut {len} -> {keep} at {anchor}: from {f}"));
            // The screen around the anchor (up to 25 000 samples) stays
            // whole unless it reaches past the record's ends.
            let lo = anchor.saturating_sub(12_500);
            let hi = (anchor + 12_500).min(len);
            check(n, lo >= f && hi <= f + keep, format!("cut {len} -> {keep} at {anchor}: screen {lo}..{hi} kept in {f}..{}", f + keep));
        }
    }
}

fn main() {
    table("PAGE", pool::PAGE_BYTES, &pool::PAGE_SPECS, pool::PAGE_COEXIST, &[]);
    table("TAIL", pool::TAIL_BYTES, &pool::TAIL_SPECS, pool::TAIL_COEXIST, pool::TAIL_HANDOFF);
    // The table check itself catches a bad table.
    let bad = [
        Spec { t: Tenant::ScopeGraph, name: "X", off: 0, len: 16, evict: Evict::Never },
        Spec { t: Tenant::Gallery, name: "Y", off: 8, len: 16, evict: Evict::Drop },
    ];
    let before = unsafe { FAILS };
    unsafe { QUIET = true };
    table("bad", 16, &bad, &[(Tenant::ScopeGraph, Tenant::Gallery)], &[]);
    // Two Never tenants overlapping: caught even when listed as a hand-over.
    let bad2 = [
        Spec { t: Tenant::Ch2Store, name: "P", off: 0, len: 8, evict: Evict::Never },
        Spec { t: Tenant::RingSlots, name: "Q", off: 0, len: 8, evict: Evict::Never },
    ];
    table("bad2", 8, &bad2, &[], &[(Tenant::Ch2Store, Tenant::RingSlots)]);
    let caught = unsafe { FAILS } - before;
    unsafe { (FAILS, QUIET) = (before, false) };
    check("table", caught == 4, format!("bad tables: {caught} of 4 errors found (end past the pool, Never overlap, COEXIST overlap, Never / Never hand-over)"));
    leases();
    tail();
    let (f, c) = unsafe { (FAILS, CASES) };
    println!("pool-test: {} cases, {} failed", c, f);
    if f > 0 {
        std::process::exit(1);
    }
}
