// Host test of firmware/src/bode.rs (run.sh builds and runs this): the
// grid, the timebase per point, the V/div retry, the maths without libm, and
// the single-bin DFT (`lockin`) on synthetic CH1 records sampled against
// the generator's real timing (src/genplan.rs: the DAC sine's table loop
// q / k as the scope's GEN channel uses it): amplitude and phase of a sine
// with a known shift, a tail of a part period, noise, a DC offset and
// harmonics; gain / phase against a Thru pass and the -3 dB point of an
// RC low-pass.

#[path = "../../src/bode.rs"]
mod bode;
#[path = "../../src/genplan.rs"]
mod genplan;

use bode::*;

static mut FAILS: u32 = 0;

macro_rules! check {
    ($c:expr, $($a:tt)*) => {
        if !$c {
            unsafe { FAILS += 1 };
            println!("FAIL line {}: {}", line!(), format!($($a)*));
        }
    };
}

/// scope.rs SDIV and the sample rate of each timebase (TB[..].0).
const SDIV: [f32; 30] = [
    10e-9, 20e-9, 50e-9, 100e-9, 200e-9, 500e-9, 1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6, 100e-6, 200e-6, 500e-6, 1e-3,
    2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0,
];
const FS: [u32; 30] = [
    200_000_000, 200_000_000, 200_000_000, 50_000_000, 50_000_000, 50_000_000, 50_000_000, 50_000_000, 50_000_000,
    50_000_000, 50_000_000, 50_000_000, 25_000_000, 12_500_000, 5_000_000, 2_500_000, 1_250_000, 500_000, 250_000,
    125_000, 50_000, 25_000, 12_500, 5_000, 2_500, 1_250, 500, 250, 125, 50,
];
const VDIV: [f32; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];
/// scope.rs CAP.
const CAP: usize = 26 * 1024;

/// A tiny LCG for the noise.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) - 0.5
    }
}

/// The generator's (t0, s, q, k) for a sine at `f` sampled at timebase `tb`
/// with the record starting `t0` ticks into the table loop.
fn sync_for(f: u32, tb: usize, t0: f64) -> (f64, f64, f64, f64, f64) {
    let s = genplan::Settings { on: true, wave: genplan::Wave::Sine, freq_hz: f, amp_v: 2.0, duty: 5000, offset_v: Some(1.5) };
    let out = genplan::compute(&s);
    let (q, k) = genplan::loop_ticks(&out.timing);
    let st = 200e6 / FS[tb] as f64;
    (t0, st, q as f64, k as f64, out.hz())
}

/// A CH1 record: mean + b sin(2 pi periods + theta) + h2 (2nd harmonic)
/// + noise (codes rms), quantised and clipped to 10 bits.
#[allow(clippy::too_many_arguments)]
fn record(n: usize, sy: (f64, f64, f64, f64, f64), mean: f64, b: f64, theta_deg: f64, h2: f64, noise: f64, seed: u64) -> Vec<u16> {
    let (t0, s, q, k, _) = sy;
    let th = theta_deg.to_radians();
    let mut r = Rng(seed);
    (0..n)
        .map(|i| {
            let p = (t0 + i as f64 * s) * k / q;
            let w = std::f64::consts::TAU * p;
            let v = mean + b * (w + th).sin() + h2 * (2.0 * w + 0.3).sin() + noise * r.next() * 3.46;
            v.round().clamp(0.0, 1023.0) as u16
        })
        .collect()
}

fn dph(a: f32, b: f32) -> f32 {
    wrap_deg(a - b).abs()
}

fn main() {
    // --- maths without libm
    for i in -40..=40 {
        let x = i as f32 * 0.37;
        check!((sin_f(x) - x.sin()).abs() < 2e-6, "sin {x}");
        check!((cos_f(x) - x.cos()).abs() < 2e-6, "cos {x}");
    }
    for &(y, x) in &[(1.0f32, 1.0f32), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (0.3, 5.0), (5.0, 0.3), (-0.01, -4.0), (0.0, -1.0), (2.0, 0.0), (-2.0, 0.0)] {
        check!((atan2_f(y, x) - y.atan2(x)).abs() < 2e-6, "atan2 {y} {x}: {} vs {}", atan2_f(y, x), y.atan2(x));
    }
    for &x in &[1e-6f32, 0.001, 0.5, 0.7071, 1.0, 1.4142, 2.0, 10.0, 1234.5, 5e4, 1e9] {
        check!((ln_f(x) - x.ln()).abs() < 2e-6 * x.ln().abs().max(1.0), "ln {x}: {} vs {}", ln_f(x), x.ln());
        check!((log10_f(x) - x.log10()).abs() < 2e-6 * x.log10().abs().max(1.0), "log10 {x}");
    }
    for &x in &[-20.0f32, -3.0, -0.5, 0.0, 0.1, 1.0, 5.5, 11.0] {
        check!((exp_f(x) - x.exp()).abs() <= 3e-6 * x.exp(), "exp {x}: {} vs {}", exp_f(x), x.exp());
    }
    for &x in &[0.0f32, 1e-6, 0.25, 2.0, 1e4, 3.3e7] {
        check!((sqrt_f(x) - x.sqrt()).abs() <= 1e-6 * x.sqrt().max(1e-3), "sqrt {x}");
    }
    check!(wrap_deg(190.0) == -170.0 && wrap_deg(-180.0) == 180.0 && wrap_deg(540.0) == 180.0, "wrap_deg");

    // --- grid
    for (ppd, n) in [(5u8, 20usize), (10, 38), (20, 75)] {
        check!(grid_n(ppd) == n && n <= MAX_PTS, "grid_n({ppd}) = {} (want {n})", grid_n(ppd));
        check!(grid_f(ppd, 0) == F_START && grid_f(ppd, n - 1) == F_STOP, "grid ends {ppd}");
        for i in 1..n {
            let r = grid_f(ppd, i) / grid_f(ppd, i - 1);
            check!(r > 1.0 && r <= 10f64.powf(1.0 / ppd as f64) * 1.000001, "grid step {ppd} {i}: {r}");
        }
        check!((grid_f(ppd, ppd as usize) - 100.0).abs() < 1e-9, "a decade at {ppd}: {}", grid_f(ppd, ppd as usize));
    }

    // --- timebase per point: >= 4 periods on the screen, at most 10
    for &f in &[10.0f64, 13.0, 50.0, 100.0, 1e3, 1.59e3, 1e4, 5e4] {
        let tb = tb_for(f, &SDIV, 3, 20);
        let per = 10.0 * SDIV[tb] as f64 * f;
        check!((4.0 * 0.999..=10.0 * 1.001).contains(&per), "tb_for {f}: tb {tb}, {per} periods");
    }
    check!(tb_for(1.0, &SDIV, 3, 20) == 20, "tb_for clamps at the top");

    // --- V/div retry
    check!(range_retry(0, 600, 5, &VDIV) == Some(6), "clip low -> up");
    check!(range_retry(300, 1023, 5, &VDIV) == Some(6), "clip high -> up");
    check!(range_retry(0, 1023, 9, &VDIV).is_none(), "clip at the top range: keep");
    check!(range_retry(200, 800, 5, &VDIV).is_none(), "in range: keep");
    check!(range_retry(500, 520, 5, &VDIV) == Some(1), "20 codes at 0.5 V/div -> 0.02 V/div: {:?}", range_retry(500, 520, 5, &VDIV));
    check!(range_retry(500, 520, 0, &VDIV).is_none(), "the lowest range: keep");
    if let Some(r) = range_retry(450, 560, 6, &VDIV) {
        let pp = 110.0 * VDIV[6] / VDIV[r];
        check!(pp < 0.8 * 1023.0 && pp * VDIV[r] / VDIV[r.saturating_sub(1)] >= 0.8 * 1023.0 || r == 0, "retry lands at 40..80 %: {r} {pp}");
    }

    // --- lockin: generator timing of real sine settings, every timebase
    let mut worst = (0.0f32, 0.0f32);
    for (j, &f) in [10u32, 16, 100, 333, 1000, 1585, 7943, 20_000, 39_811, 50_000].iter().enumerate() {
        let tb = tb_for(f as f64, &SDIV, 3, 20);
        for (m, &theta) in [0.0f64, 37.0, -45.0, 90.0, 179.0, -135.0].iter().enumerate() {
            let t0 = 12345.678 * (m as f64 + 1.0) * (j as f64 + 1.0);
            let sy = sync_for(f, tb, t0);
            let n = CAP.min((10.0 * SDIV[tb] as f64 * FS[tb] as f64) as usize + 777);
            let d = record(n, sy, 530.0, 250.0, theta, 12.0, 2.0, (j * 10 + m) as u64 + 1);
            let l = lockin(&d, sy.0, sy.1, sy.2, sy.3).expect("lockin");
            let ea = (l.amp - 250.0).abs() / 250.0;
            let ep = dph(l.ph, theta as f32);
            worst = (worst.0.max(ea), worst.1.max(ep));
            check!(ea < 2e-3, "amp f {f} theta {theta}: {} (periods {})", l.amp, l.periods);
            check!(ep < 0.15, "phase f {f} theta {theta}: {}", l.ph);
            check!(l.periods >= 4 && (l.mean - 530.0).abs() < 1.0, "periods / mean f {f}: {} {}", l.periods, l.mean);
            check!(l.lo < 300 && l.hi > 760, "span f {f}: {} {}", l.lo, l.hi);
        }
    }
    println!("lockin worst: amplitude {:.2e}, phase {:.3} deg", worst.0, worst.1);
    // Small signal: 3 codes peak in 2 codes rms of noise, many periods.
    {
        let sy = sync_for(1000, tb_for(1000.0, &SDIV, 3, 20), 777.0);
        let d = record(CAP, sy, 512.0, 3.0, 60.0, 0.0, 2.0, 99);
        let l = lockin(&d, sy.0, sy.1, sy.2, sy.3).unwrap();
        check!((l.amp - 3.0).abs() < 0.15 && dph(l.ph, 60.0) < 3.0, "small signal: {} {}", l.amp, l.ph);
    }
    // Under one period: None.
    {
        let sy = sync_for(10, 15, 0.0);
        let d = record(1000, sy, 512.0, 100.0, 0.0, 0.0, 0.0, 1);
        check!(lockin(&d, sy.0, sy.1, sy.2, sy.3).is_none(), "under a period");
        check!(lockin(&[], 0.0, 1.0, 100.0, 1.0).is_none(), "empty");
    }
    // The sign convention: CH1 delayed by dt lags (negative phase).
    {
        let f = 1000u32;
        let tb = tb_for(f as f64, &SDIV, 3, 20);
        let sy = sync_for(f, tb, 5000.0);
        let lag_ticks = 200e6 / sy.4 / 8.0; // an eighth of a period later
        let d = record(CAP, (sy.0 - lag_ticks, sy.1, sy.2, sy.3, sy.4), 512.0, 300.0, 0.0, 0.0, 0.0, 3);
        let l = lockin(&d, sy.0, sy.1, sy.2, sy.3).unwrap();
        check!(dph(l.ph, -45.0) < 0.2, "a delay lags: {}", l.ph);
    }

    // --- an RC low-pass (1 kohm, 100 nF: fc 1.59 kHz) against a Thru pass:
    // the path's own gain / phase (here 0.97 and a 2 us lag) cancels.
    {
        let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 100e-9);
        let (mut fs, mut gs, mut ps) = (vec![], vec![], vec![]);
        for i in 0..grid_n(10) {
            let f = grid_f(10, i).round() as u32;
            let tb = tb_for(f as f64, &SDIV, 3, 20);
            let sy = sync_for(f, tb, 1000.0 + i as f64 * 77.0);
            let fa = sy.4;
            let path_deg = -360.0 * fa * 2e-6;
            let n = CAP.min((10.0 * SDIV[tb] as f64 * FS[tb] as f64) as usize);
            let thru = record(n, sy, 512.0, 0.97 * 400.0, path_deg, 0.0, 1.0, i as u64);
            let x = fa / fc;
            let (g, ph) = (1.0 / (1.0 + x * x).sqrt(), -x.atan().to_degrees());
            let dut = record(n, sy, 512.0, 0.97 * 400.0 * g, path_deg + ph, 0.0, 1.0, 1000 + i as u64);
            let lt = lockin(&thru, sy.0, sy.1, sy.2, sy.3).unwrap();
            let ld = lockin(&dut, sy.0, sy.1, sy.2, sy.3).unwrap();
            let (gd, pd) = relative(&Pt { f: fa as f32, amp: ld.amp, ph: ld.ph }, lt.amp, lt.ph);
            let want_g = 20.0 * g.log10();
            check!((gd as f64 - want_g).abs() < 0.05 + 0.02 * want_g.abs(), "RC gain {fa:.1}: {gd} vs {want_g:.3}");
            check!((pd as f64 - ph).abs() < 0.3, "RC phase {fa:.1}: {pd} vs {ph:.3}");
            fs.push(fa as f32);
            gs.push(gd);
            ps.push(pd);
        }
        let m3 = minus3db(&fs, &gs).expect("-3 dB");
        check!((m3 as f64 - fc).abs() / fc < 0.03, "-3 dB at {m3} (fc {fc:.1})");
        println!("RC 1k / 100n: -3 dB at {m3:.1} Hz (fc {fc:.1})");
    }
    check!(minus3db(&[10.0, 20.0], &[0.0, -1.0]).is_none(), "no -3 dB");

    let fails = unsafe { FAILS };
    if fails > 0 {
        println!("bode-test: {fails} FAILED");
        std::process::exit(1);
    }
    println!("bode-test: ok");
}
