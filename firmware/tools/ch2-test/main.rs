//! Host test of src/scope_ch2.rs: a frame's column lookups
//! (Store::span_run, rising times, f32) against Store::span (f64, one
//! anchor search per column) on random streams: rates 1200 .. 38 400 S/s,
//! stores short of one anchor and wrapped many times, late reads (a stamp
//! up to 0.4 periods late), a lag, the DWT wrap; frames before the oldest
//! sample, across the store, after the newest one. A run of columns out of
//! order must give span's results too. Then holes: a stamp 2 periods or
//! more on restarts the store (nothing from before it is found, the
//! samples after it are where their stamps say); one stamp up to 0.9
//! periods late (interrupt latency) does not.
//!   tools/ch2-test/run.sh
#![allow(static_mut_refs)]

mod scope {
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub struct WaveScale {
        pub a: f32,
        pub b: f32,
        pub vdiv: f32,
        pub amps: bool,
        pub ac: bool,
    }
}

mod clock {
    pub const HCLK_HZ: u32 = 200_000_000;
}

#[allow(dead_code)]
#[path = "../../src/scope_ch2.rs"]
mod scope_ch2;

use scope::WaveScale;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

fn main() {
    let mut r = Rng(0x5eed_c42);
    let (mut cols, mut some, mut edge, mut fails) = (0u64, 0u64, 0u64, 0u64);
    scope_ch2::init();
    for case in 0..600 {
        let fs = [1200u32, 2400, 4800, 9600, 38_400][r.below(5) as usize];
        let c0 = if case % 3 == 0 { u32::MAX - r.below(50_000_000) } else { r.next() };
        scope_ch2::set_lag_us(r.below(400) as i32 - 100);
        let st = scope_ch2::store();
        let sc = WaveScale { a: 0.0, b: 1e-6, vdiv: 1.0, amps: false, ac: false };
        st.reset(c0, fs, sc);
        let n = [r.below(127) + 1, r.below(4096), 4096 + r.below(20_000)][r.below(3) as usize];
        let per_us = 1e6 / fs as f64;
        let mut k = 0u64;
        for _ in 0..n {
            // A late read now and then (the stamp, not a lost sample).
            let late = if r.below(500) == 0 { 0.4 * per_us } else { 0.0 };
            let ts = (k as f64 * per_us + late + r.below(3) as f64) as u32;
            st.push(ts, (r.below(1 << 19) as i32) - (1 << 18));
            k += 1;
        }
        let period = clock::HCLK_HZ as f64 / fs as f64;
        let end = c0.wrapping_add((k as f64 * per_us * 200.0) as u32);
        let store_cyc = (n.min(4096) as f64 * period) as u32;
        for frame in 0..8 {
            // Column length: < 1 sample .. ~50 samples.
            let len = (period * [0.3, 1.0, 2.7, 13.0, 50.0][r.below(5) as usize]) as f32;
            let span_cyc = (len as f64 * 400.0) as u32;
            let back = r.below(store_cyc.saturating_add(span_cyc).saturating_add(span_cyc / 2).max(1));
            let start = end.wrapping_sub(back).wrapping_add(r.below(span_cyc / 4 + 1));
            let mut run = st.run();
            let order: Vec<usize> = if frame == 7 { (0..400).map(|i| (i * 37) % 400).collect() } else { (0..400).collect() };
            for i in order {
                let t0 = start.wrapping_add((i as f64 * len as f64) as u32);
                let a = st.range(t0, len);
                let b = st.range_run(&mut run, t0, len);
                cols += 1;
                let ok = match (a, b) {
                    (None, None) => true,
                    (Some(x), Some(y)) => {
                        some += 1;
                        let d = (x.0 as i64 - y.0 as i64).abs().max((x.1 as i64 - y.1 as i64).abs());
                        if d > 0 {
                            edge += 1;
                        }
                        d <= 1
                    }
                    // A rounding edge at the store's ends.
                    (Some(x), None) | (None, Some(x)) => {
                        edge += 1;
                        x.1 - x.0 == 1
                    }
                };
                if !ok {
                    fails += 1;
                    if fails <= 10 {
                        println!("FAIL case {case} frame {frame} col {i}: fs {fs} n {n} len {len} span {a:?} run {b:?}");
                    }
                }
            }
        }
    }
    fails += holes(&mut r);
    let rate = edge as f64 / some.max(1) as f64;
    println!("ch2-test: {cols} columns, {some} with samples, {edge} at a rounding edge ({:.4} %), {fails} failed", rate * 100.0);
    if fails > 0 || rate > 0.01 || some < cols / 4 {
        println!("FAIL");
        std::process::exit(1);
    }
    println!("PASS");
}

/// A hole (a stamp 2..41 periods on: 1..40 samples missed) restarts the store: no column before
/// it finds a sample, columns after it find the post-hole ones at their own
/// time (an anchor segment across the gap would put the pre-hole samples
/// there, or shift the post-hole ones). The failures.
fn holes(r: &mut Rng) -> u64 {
    let mut fails = 0;
    for case in 0..300 {
        let fs = [1200u32, 2400, 4800, 9600, 38_400][r.below(5) as usize];
        let c0 = r.next();
        scope_ch2::set_lag_us(0);
        let st = scope_ch2::store();
        let sc = WaveScale { a: 0.0, b: 1.0, vdiv: 1.0, amps: false, ac: false };
        st.reset(c0, fs, sc);
        let per_us = 1e6 / fs as f64;
        let (n1, n2) = (1 + r.below(3000) as u64, 1 + r.below(600) as u64);
        let skip = 1 + r.below(40) as u64;
        // Codes: pre-hole -1000 (raw), post-hole +1000.
        let raw = |pre: bool| if pre { -1000 * 8 } else { 1000 * 8 };
        for k in 0..n1 {
            st.push((k as f64 * per_us) as u32, raw(true));
        }
        for k in 0..n2 {
            st.push(((n1 + skip + k) as f64 * per_us) as u32, raw(false));
        }
        let cyc = |k: f64| c0.wrapping_add((k * per_us * 200.0) as u32);
        let period = 200.0 * per_us as f32;
        let post = st.span(cyc((n1 + skip) as f64 + 0.1), period * 0.5);
        let pre = st.span(cyc(n1 as f64 - 1.0), period * 0.5);
        let gap = st.span(cyc((n1 + skip / 2) as f64), period * 0.5);
        let ok = pre.is_none() && post.is_some_and(|(lo, _)| lo > 0.0) && (skip < 3 || gap.is_none());
        if !ok {
            fails += 1;
            if fails <= 10 {
                println!("FAIL hole case {case}: fs {fs} n1 {n1} skip {skip} n2 {n2}: pre {pre:?} gap {gap:?} post {post:?}");
            }
        }
    }
    println!("ch2-test: 300 holes, {fails} failed");
    fails + late(r)
}

/// One stamp read late by up to 0.9 periods (the stream ISR held off by
/// another interrupt; the gap before it is up to 1.9 periods, the one
/// after it short): not a hole, the store keeps the samples before it.
/// The failures.
fn late(r: &mut Rng) -> u64 {
    let mut fails = 0;
    for case in 0..300 {
        let fs = [1200u32, 2400, 4800, 9600, 38_400][r.below(5) as usize];
        let c0 = r.next();
        scope_ch2::set_lag_us(0);
        let st = scope_ch2::store();
        let sc = WaveScale { a: 0.0, b: 1.0, vdiv: 1.0, amps: false, ac: false };
        st.reset(c0, fs, sc);
        let per_us = 1e6 / fs as f64;
        let n = 200 + r.below(2000) as u64;
        let k_late = 1 + r.below(n as u32 - 2) as u64;
        let d = per_us * (0.5 + 0.4 * r.below(1001) as f64 / 1000.0);
        for k in 0..n {
            let ts = k as f64 * per_us + if k == k_late { d } else { 0.0 };
            st.push(ts as u32, if k < k_late { -1000 * 8 } else { 1000 * 8 });
        }
        let cyc = |k: f64| c0.wrapping_add((k * per_us * 200.0) as u32);
        let first = st.span(cyc(0.1), 200.0 * per_us as f32 * 0.5);
        // SI of the raw codes: about 496 before the late stamp, 527 after.
        if !first.is_some_and(|(_, hi)| hi < 511.0) {
            fails += 1;
            if fails <= 10 {
                println!("FAIL late case {case}: fs {fs} n {n} late {k_late} by {d:.1} us: first {first:?}");
            }
        }
    }
    println!("ch2-test: 300 late stamps, {fails} failed");
    fails
}
