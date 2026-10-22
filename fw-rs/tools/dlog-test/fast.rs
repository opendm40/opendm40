// Kind 3 (the fast logger's stream: src/fastenc.rs coder + pager, the
// src/dlog.rs generator):
//   1. round trip: synthetic logger records (AD2 at 38400 / 9600 / 4800 S/s,
//      AD1 at 640 / 40 S/s, stamp jitter, ring-overflow holes, a stamp wrap
//      past 2^32 us, full-scale noise) coded through FastEnc into a NOR model
//      (pages programmed only by `pump`, a busy chip between calls, two
//      extents); the CSV generated from the file: every row the fixed width,
//      t_s / raw / lost_before equal to the reference (from the records),
//      value = a + b * raw;
//   2. random access (Gen::read at random offsets) == the sequential CSV;
//   3. truncation at every byte (stepped) and at every chunk end: exactly the
//      whole chunks decode;
//   4. power cut after a chip operation (every one of the first 300, then
//      ~250 spread over the rest): the image holds the programs
//      so far; the file (the whole allocation, as the directory entry says
//      during a run) decodes to a prefix of the rows, never a wrong row;
//   5. bytes per sample, pages / partial pages / header pieces programmed.
// Writes f<i>.dlg + f<i>.csv for tools/dlog.py.

use crate::dlog::*;
use crate::fastenc::{FastEnc, Nor, Pump};

struct R(u64);
impl R {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let (a, b) = (self.unit().max(1e-12), self.unit());
        (-2.0 * a.ln()).sqrt() * (2.0 * std::f64::consts::PI * b).cos()
    }
}

/// A NOR chip: programs AND into the image; busy for a few polls after
/// each operation; every operation recorded (power-cut replay).
struct Chip {
    mem: Vec<u8>,
    ops: Vec<(u32, Vec<u8>)>,
    busy: u32,
    r: R,
}

impl Nor for Chip {
    fn busy(&mut self) -> bool {
        if self.busy > 0 {
            self.busy -= 1;
            return true;
        }
        false
    }
    fn program(&mut self, addr: u32, d: &[u8]) {
        assert!(self.busy == 0, "program while busy");
        assert_eq!(addr / 256, (addr + d.len() as u32 - 1) / 256, "program crosses a page");
        for (i, &b) in d.iter().enumerate() {
            self.mem[addr as usize + i] &= b;
        }
        self.ops.push((addr, d.to_vec()));
        self.busy = (self.r.next() % 4) as u32;
    }
    fn erase(&mut self, _addr: u32, _big: bool) {
        panic!("no erase during a run");
    }
    fn read(&mut self, addr: u32, b: &mut [u8]) -> bool {
        b.copy_from_slice(&self.mem[addr as usize..addr as usize + b.len()]);
        true
    }
}

struct Set {
    name: &'static str,
    par: FPar,
    unit: &'static [u8],
    recs: Vec<(u32, i32)>,
}

fn sets() -> Vec<Set> {
    let mut v = Vec::new();
    let ad2 = |rate: u32| FPar { a: 0.00012, b: 4.76837e-6, rate_mhz: rate * 1000, flags: FP_SCALED, src: 2, mode: 0, range: 0 };
    let ad1 = |rate_mhz: u32| FPar { a: 0.0, b: 0.0, rate_mhz, flags: 0, src: 1, mode: 1, range: 1 };
    // grid: n samples at `rate` mHz, stamp jitter +-j us, start `t0`, keep(i)
    let grid = |n: usize, rate_mhz: u32, jit: f64, t0: u64, seed: u64, keep: &dyn Fn(usize, &mut R) -> bool, val: &dyn Fn(f64, &mut R) -> i32| {
        let mut r = R(seed);
        let p = 1e9 / rate_mhz as f64;
        let mut out = Vec::new();
        for i in 0..n {
            let t = t0 as f64 + (i + 1) as f64 * p + (r.unit() - 0.5) * 2.0 * jit;
            if !keep(i, &mut r) {
                continue;
            }
            out.push(((t as u64) as u32, val(t / 1e6, &mut r)));
        }
        out
    };
    let all = |_: usize, _: &mut R| true;
    v.push(Set {
        name: "AD2 38400 50 Hz sine + noise",
        par: ad2(38400),
        unit: b"V",
        recs: grid(200_000, 38_400_000, 1.0, 0, 1, &all, &|t, r| (150_000.0 * (2.0 * 3.14159265 * 50.0 * t).sin() + 6.0 * r.gauss()) as i32),
    });
    v.push(Set {
        name: "AD2 9600 DC + noise, holes",
        par: ad2(9600),
        unit: b"A",
        // ring-overflow holes: bursts of 1..300 missing samples
        recs: grid(100_000, 9_600_000, 0.5, 0, 2, &|i, r| !(i % 3000 > 2700 && r.next() % 2 == 0 || (i / 7000) % 5 == 4 && i % 7000 < 300), &|_, r| (12_000.0 + 4.0 * r.gauss()) as i32),
    });
    v.push(Set {
        name: "AD2 4800 full-scale noise",
        par: ad2(4800),
        unit: b"V",
        recs: grid(40_000, 4_800_000, 1.0, 0, 3, &all, &|_, r| (r.next() % 524_287) as i32 - 262_143),
    });
    v.push(Set {
        name: "AD1 640 ramp + noise",
        par: ad1(640_000),
        unit: b"cts",
        recs: grid(60_000, 640_000, 40.0, 0, 4, &all, &|t, r| (1_234_567.0 + 3000.0 * t + 25.0 * r.gauss()) as i32),
    });
    v.push(Set {
        name: "AD1 40 steady",
        par: ad1(40_000),
        unit: b"cts",
        recs: grid(6_000, 40_000, 300.0, 0, 5, &all, &|_, r| (-765_432.0 + 3.0 * r.gauss()) as i32),
    });
    v.push(Set {
        name: "AD2 1200 stamp wrap 2^32 us",
        par: ad2(1200),
        unit: b"V",
        recs: grid(30_000, 1_200_000, 1.0, (1u64 << 32) - 9_000_000, 6, &all, &|t, r| (80_000.0 * (2.0 * 3.14159265 * 3.0 * t).sin() + 5.0 * r.gauss()) as i32),
    });
    v
}

const HDR_TXT: &[u8] = b"# Alientek DM40A, fw-rs test, fast log\r\nt_s,raw,value,lost_before\r\n";

/// Code `s` through FastEnc + the chip. (file bytes, chip, enc).
fn run(s: &Set, seed: u64) -> (Vec<u8>, Chip, FastEnc, Vec<u8>) {
    let cb = 4096u32;
    // two extents, cluster 2 at byte 0 of the chip
    let ext = [(10u32, 60u32), (200u32, 700u32)];
    let mem_len = (2 + 200 + 700) as usize * cb as usize;
    let mut chip = Chip { mem: vec![0xFF; mem_len], ops: Vec::new(), busy: 0, r: R(seed) };
    let mut fifo = vec![0u8; 8192];
    let mut e = FastEnc::new(&ext, 0, cb, fifo.len(), s.par.p_us());
    let mut h = vec![0u8; FHDR + HDR_TXT.len()];
    let n = file_header(KIND_FAST, s.unit, HDR_TXT, &mut h);
    let mut head = h[..n].to_vec();
    head.extend_from_slice(&s.par.bytes());
    e.put_raw(&mut fifo, &head);
    let mut r = R(seed ^ 0x55);
    for &(ts, raw) in &s.recs {
        while !e.room(1) {
            e.pump(&fifo, &mut chip);
        }
        e.add(&mut fifo, ts, raw);
        // 0..2 passes per record
        for _ in 0..(r.next() % 3) {
            e.pump(&fifo, &mut chip);
        }
    }
    e.finish(&mut fifo);
    let mut guard = 0;
    while !(e.flushed() && chip.busy == 0 && !e.is_open()) {
        if e.is_open() {
            e.finish(&mut fifo);
        }
        let _ = e.pump(&fifo, &mut chip) == Pump::Idle;
        guard += 1;
        assert!(guard < 10_000_000);
    }
    let file: Vec<u8> = (0..e.wr).map(|o| chip.mem[e.addr(o) as usize]).collect();
    (file, chip, e, head)
}

/// Reference rows: (t us, raw, lost).
fn reference(s: &Set) -> Vec<(u64, i32, u32)> {
    let mut out = Vec::new();
    let mut t = 0u64;
    let mut last = 0u32;
    for (i, &(ts, raw)) in s.recs.iter().enumerate() {
        let dt = if i == 0 { 0 } else { ts.wrapping_sub(last) };
        t = if i == 0 { ts as u64 } else { t + dt as u64 };
        last = ts;
        let lost = if i == 0 { 0 } else { (((dt as f64) * s.par.rate_mhz as f64 / 1e9).round() as i64 - 1).max(0) as u32 };
        out.push((t, raw, lost));
    }
    out
}

fn gen_csv(f: &[u8]) -> Option<(Info, Vec<u8>)> {
    let mut g = Gen::open(f, f.len() as u32)?;
    let mut out = vec![0u8; g.info.csv as usize];
    let mut o = 0;
    while o < out.len() {
        let n = (out.len() - o).min(512);
        g.read(o as u32, &mut out[o..o + n]);
        o += n;
    }
    Some((g.info, out))
}

/// A prefix / damaged image decodes to exactly `n` rows of `csv` (the
/// header and the last rows compared: rows are fixed width and every row
/// was checked in the full decode).
fn prefix_ok(f: &[u8], csv: &[u8], hl: usize, w: usize) -> Option<usize> {
    let mut g = Gen::open(f, f.len() as u32)?;
    let n = g.info.rows as usize;
    if g.info.csv as usize != hl + n * w {
        return None;
    }
    let lo = if n > 3 { hl + (n - 3) * w } else { 0 };
    let end = hl + n * w;
    let mut b = vec![0u8; end - lo];
    g.read(lo as u32, &mut b);
    (b[..] == csv[lo..end]).then_some(n)
}

/// Check the CSV's rows against the reference; Some(error).
fn check_rows(csv: &[u8], hl: usize, w: usize, s: &Set, refr: &[(u64, i32, u32)]) -> Option<String> {
    let body = &csv[hl..];
    if body.len() % w != 0 {
        return Some(format!("body {} not a multiple of {w}", body.len()));
    }
    for (k, row) in body.chunks(w).enumerate() {
        let t = std::str::from_utf8(row).ok()?;
        if !t.ends_with("\r\n") {
            return Some(format!("row {k}: no CRLF"));
        }
        let f: Vec<&str> = t.trim_end().split(',').collect();
        let (tu, raw, lost) = refr[k];
        let ts = f[0].trim();
        let (a, b) = ts.split_once('.')?;
        let us = a.parse::<u64>().ok()? * 1_000_000 + b.parse::<u64>().ok()?;
        let fr = f[1].trim().parse::<i32>().ok()?;
        let fl = f[f.len() - 1].trim().parse::<u32>().ok()?;
        if us != tu || fr != raw || fl != lost {
            return Some(format!("row {k}: {t:?} vs t {tu} raw {raw} lost {lost}"));
        }
        if s.par.flags & FP_SCALED != 0 {
            let v: f64 = f[2].trim().parse().ok()?;
            let want = s.par.a as f64 + s.par.b as f64 * raw as f64;
            if (v - want).abs() > 1e-5 * want.abs() + 1e-9 {
                return Some(format!("row {k}: value {v} vs {want}"));
            }
        }
    }
    None
}

pub fn tests(dir: Option<String>) -> u32 {
    let mut fails = 0;
    println!("{:<32} {:>7} {:>8} {:>8} {:>7} {:>7} {:>6} {:>6}", "fast set (kind 3)", "rows", "DLG B/s", "CSV B/r", "pages", "partial", "hdr", "chunks");
    for (si, s) in sets().iter().enumerate() {
        let (file, chip, e, head) = run(s, 7 + si as u64);
        let refr = reference(s);
        let w = s.par.row_len() as usize;
        let hl = HDR_TXT.len();
        // 1. round trip
        let Some((info, csv)) = gen_csv(&file) else {
            println!("FAIL {}: not a DLG", s.name);
            fails += 1;
            continue;
        };
        if info.rows as usize != s.recs.len() || csv.len() != hl + s.recs.len() * w {
            println!("FAIL {}: rows {} of {}", s.name, info.rows, s.recs.len());
            fails += 1;
            continue;
        }
        if let Some(err) = check_rows(&csv, hl, w, s, &refr) {
            println!("FAIL {}: {err}", s.name);
            fails += 1;
            continue;
        }
        // 2. random access
        let mut g = Gen::open(&file[..], file.len() as u32).unwrap();
        let mut rr = R(1234 + si as u64);
        for _ in 0..3000 {
            let off = (rr.next() % (csv.len() as u64 + 10)) as usize;
            let len = 1 + (rr.next() % 700) as usize;
            let mut b = vec![0xAAu8; len];
            g.read(off as u32, &mut b);
            let want: Vec<u8> = (off..off + len).map(|i| csv.get(i).copied().unwrap_or(0)).collect();
            if b != want {
                println!("FAIL {}: random read at {off}+{len}", s.name);
                fails += 1;
                break;
            }
        }
        // chunk ends (rows so far)
        let mut ends = Vec::new();
        let mut w8 = Win::new();
        let mut off = head.len() as u32;
        let mut rows = 0u32;
        let mut src: &[u8] = &file[..];
        while let Some(c) = fchunk_at(&mut src, &mut w8, off, file.len() as u32, true) {
            rows += c.nrows;
            ends.push((c.end() as usize, rows as usize));
            off = c.end();
        }
        assert_eq!(rows as usize, s.recs.len());
        let whole = |l: usize| ends.iter().filter(|c| c.0 <= l).map(|c| c.1).last().unwrap_or(0);
        // 3. truncation
        let step = (file.len() / 1500).max(1);
        let mut bad = 0;
        let mut ls: Vec<usize> = (head.len()..=file.len()).step_by(step).collect();
        for &(c, _) in &ends {
            ls.extend([c - 1, c, c + 1].iter().filter(|&&l| l <= file.len()));
        }
        for l in ls {
            if prefix_ok(&file[..l], &csv, hl, w) != Some(whole(l)) {
                bad += 1;
            }
        }
        if bad > 0 {
            println!("FAIL {}: {bad} truncations decode wrong", s.name);
            fails += 1;
        }
        // 4. power cut after chip operation k (every op up to 300, then ~250 more)
        let cap = e.cap as usize;
        let nops = chip.ops.len();
        let mut img = vec![0xFFu8; chip.mem.len()];
        let mut prev = 0usize;
        let mut bad = 0;
        let mut k = 0;
        let mut checked = 0;
        while k <= nops {
            if k > 0 {
                let (a, d) = &chip.ops[k - 1];
                for (i, &b) in d.iter().enumerate() {
                    img[*a as usize + i] &= b;
                }
            }
            if k < 300 || k % (nops / 250).max(1) == 0 || k == nops {
                let f: Vec<u8> = (0..cap as u32).map(|o| img[e.addr(o) as usize]).collect();
                match prefix_ok(&f, &csv, hl, w) {
                    None if k < 4 => {}
                    Some(n) if n >= prev => prev = n,
                    _ => bad += 1,
                }
                checked += 1;
            }
            k += 1;
        }
        if bad > 0 || prev != s.recs.len() {
            println!("FAIL {}: power cut: {bad} bad images, {prev} rows at the end", s.name);
            fails += 1;
        }
        let secs = (refr.last().unwrap().0 - refr[0].0) as f64 / 1e6;
        println!(
            "{:<32} {:>7} {:>8.0} {:>8} {:>7} {:>7} {:>6} {:>6}  {:.3} B/sample, cut-checked {checked}  ok",
            s.name,
            s.recs.len(),
            (file.len() - head.len()) as f64 / secs,
            w,
            e.programs - e.partials - e.pieces,
            e.partials,
            e.pieces,
            e.chunks,
            (file.len() - head.len()) as f64 / s.recs.len() as f64
        );
        if let Some(d) = &dir {
            std::fs::write(format!("{d}/f{si}.dlg"), &file).unwrap();
            std::fs::write(format!("{d}/f{si}.csv"), &csv).unwrap();
        }
    }
    fails
}
