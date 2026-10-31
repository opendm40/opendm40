//! Host test of src/pool.rs: every pool's table (specs inside the pool, one
//! spec per tenant, 8-byte offsets; no two tenants the code lets be live
//! together (`*_COEXIST`) share a byte; no `Never` tenant overlaps
//! another), then the lease rules on a test pool (take / held / eviction /
//! generation counters / a `Never` holder refusing, the debug text).
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
fn table(name: &str, bytes: usize, specs: &[Spec], coexist: &[(Tenant, Tenant)]) {
    for (i, s) in specs.iter().enumerate() {
        check(name, s.off + s.len <= bytes, format!("{} ends at {} > {} bytes", s.name, s.off + s.len, bytes));
        check(name, s.off % 8 == 0, format!("{} offset {} not 8-byte aligned", s.name, s.off));
        check(name, s.len > 0, format!("{} is empty", s.name));
        for o in &specs[i + 1..] {
            check(name, o.t != s.t, format!("{} listed twice", s.name));
            if pool::overlap(s, o) {
                check(name, s.evict != Evict::Never && o.evict != Evict::Never, format!("{} and {} overlap and one is Never", s.name, o.name));
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

fn main() {
    table("PAGE", pool::PAGE_BYTES, &pool::PAGE_SPECS, pool::PAGE_COEXIST);
    // The table check itself catches a bad table.
    let bad = [
        Spec { t: Tenant::ScopeGraph, name: "X", off: 0, len: 16, evict: Evict::Never },
        Spec { t: Tenant::Gallery, name: "Y", off: 8, len: 16, evict: Evict::Drop },
    ];
    let before = unsafe { FAILS };
    unsafe { QUIET = true };
    table("bad", 16, &bad, &[(Tenant::ScopeGraph, Tenant::Gallery)]);
    let caught = unsafe { FAILS } - before;
    unsafe { (FAILS, QUIET) = (before, false) };
    check("table", caught == 3, format!("a bad table: {caught} of 3 errors found (end past the pool, Never overlap, COEXIST overlap)"));
    leases();
    let (f, c) = unsafe { (FAILS, CASES) };
    println!("pool-test: {} cases, {} failed", c, f);
    if f > 0 {
        std::process::exit(1);
    }
}
