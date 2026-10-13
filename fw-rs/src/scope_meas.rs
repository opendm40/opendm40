//! Scope measurements (scope_gen_ui_v2_6.h section 4): computed on the
//! samples of the visible window, refreshed at most every 300 ms.
//! Stock item IDs: 0 Freq 1 +Duty 2 -Duty 3 +Wid 4 -Wid 5 +Pul 6 -Pul
//! 9 Cycl 10 Vpp 11 Vrms 12 Vmax 13 Vmin 14 Vtop 15 Vbas 16 Vamp 18 Vcrm 19 Vavg
//! (7/8/17 are the cursor readouts). Invalid values: NaN.

use crate::scope::Scope;

pub const ITEMS: [(u8, &str, Unit); 20] = [
    (0, "Freq", Unit::Hz),
    (9, "Cycl", Unit::S),
    (10, "Vpp", Unit::V),
    (11, "Vrms", Unit::V),
    (12, "Vmax", Unit::V),
    (13, "Vmin", Unit::V),
    (14, "Vtop", Unit::V),
    (15, "Vbas", Unit::V),
    (16, "Vamp", Unit::V),
    (19, "Vavg", Unit::V),
    (18, "Vcrm", Unit::V),
    (1, "+Duty", Unit::Pct),
    (2, "-Duty", Unit::Pct),
    (3, "+Wid", Unit::S),
    (4, "-Wid", Unit::S),
    (5, "+Pul", Unit::N),
    (6, "-Pul", Unit::N),
    (7, "X2-1", Unit::S),
    (8, "Xf", Unit::Hz),
    (17, "Y2-1", Unit::V),
];

/// Default selection (stock 0x0804c6ac): Freq, Vpp, Cycl, Vrms.
pub const DEFAULT_SEL: [u8; 4] = [0, 10, 9, 11];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    V,
    Hz,
    S,
    Pct,
    N,
}

/// Results indexed by stock item ID (0..20).
#[derive(Clone, Copy)]
pub struct Results {
    pub v: [f32; 20],
}

fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..3 {
        y = 0.5 * (y + x / y);
    }
    y
}

pub fn compute(d: &[u16], fs: f32, sc: &Scope) -> Results {
    let mut r = Results { v: [f32::NAN; 20] };
    let n = d.len();
    if n < 4 {
        return r;
    }
    // Volts are linear in the code: precompute instead of calling the
    // calibrated map per sample (that made a 50k-sample frame take ~100 ms).
    let v_0 = sc.volts(0.0);
    let v_k = sc.volts(1.0) - v_0;
    let volts = |c: f32| v_0 + v_k * c;
    let mut hist = [0u16; 1024];
    let (mut lo, mut hi) = (1023u16, 0u16);
    let (mut sum, mut sq) = (0f32, 0f32);
    for &c in d {
        let c = c.min(1023);
        hist[c as usize] = hist[c as usize].saturating_add(1);
        lo = lo.min(c);
        hi = hi.max(c);
        let v = volts(c as f32);
        sum += v;
        sq += v * v;
    }
    let (vmax, vmin) = (volts(hi as f32), volts(lo as f32));
    r.v[12] = vmax;
    r.v[13] = vmin;
    r.v[19] = sum / n as f32;
    r.v[11] = sqrt(sq / n as f32);
    let clipped = lo == 0 || hi >= 1023;
    r.v[10] = if clipped { f32::NAN } else { vmax - vmin };
    // Top / base: most populated bin in the upper / lower 30 % band.
    let pp_c = (hi - lo) as f32;
    let top_from = (hi as f32 - 0.3 * pp_c) as usize;
    let base_to = (lo as f32 + 0.3 * pp_c) as usize;
    let mode_in = |a: usize, b: usize| -> usize {
        let mut best = a;
        for i in a..=b.min(1023) {
            if hist[i] > hist[best] {
                best = i;
            }
        }
        best
    };
    let tb = mode_in(top_from, hi as usize);
    let bb = mode_in(lo as usize, base_to);
    let (vtop, vbas) = (volts(tb as f32), volts(bb as f32));
    r.v[14] = vtop;
    r.v[15] = vbas;
    r.v[16] = if bb == 0 || tb == 1023 { f32::NAN } else { vtop - vbas };
    // Edge analysis on codes: 10 / 50 / 90 % of base..top.
    if tb > bb + 4 {
        let span = (tb - bb) as f32;
        let l10 = bb as f32 + 0.1 * span;
        let l50 = bb as f32 + 0.5 * span;
        let l90 = bb as f32 + 0.9 * span;
        // State: true = high (passed 90 %), false = low (passed 10 %).
        let mut state: Option<bool> = None;
        let mut rises = [0usize; 2]; // first, last rising 50 % crossing
        let mut nrise = 0u32;
        let (mut high_n, mut low_n) = (0u32, 0u32);
        let (mut hp, mut lp) = (0u32, 0u32);
        let mut prev_v = d[0] as f32;
        let mut cross50: Option<usize> = None;
        for (i, &c) in d.iter().enumerate() {
            let v = c as f32;
            if prev_v < l50 && v >= l50 {
                cross50 = Some(i);
            }
            if prev_v >= l50 && v < l50 {
                cross50 = Some(i);
            }
            match state {
                None => {
                    if v >= l90 {
                        state = Some(true);
                    } else if v <= l10 {
                        state = Some(false);
                    }
                }
                Some(false) if v >= l90 => {
                    state = Some(true);
                    let at = cross50.unwrap_or(i);
                    if nrise == 0 {
                        rises[0] = at;
                    }
                    rises[1] = at;
                    nrise += 1;
                    lp += 1;
                }
                Some(true) if v <= l10 => {
                    state = Some(false);
                    hp += 1;
                }
                _ => {}
            }
            if nrise >= 1 {
                if v >= l50 {
                    high_n += 1;
                } else {
                    low_n += 1;
                }
            }
            prev_v = v;
        }
        if nrise >= 2 && rises[1] > rises[0] {
            let cycles = (nrise - 1) as f32;
            let samples = (rises[1] - rises[0]) as f32;
            let f = fs * cycles / samples;
            r.v[0] = f;
            r.v[9] = 1.0 / f;
            // Duty / widths over whole cycles.
            let (mut hn, mut ln) = (0u32, 0u32);
            let (mut s2, mut cnt) = (0f32, 0u32);
            for &c in &d[rises[0]..rises[1]] {
                if c as f32 >= l50 {
                    hn += 1;
                } else {
                    ln += 1;
                }
                let v = volts(c as f32);
                s2 += v * v;
                cnt += 1;
            }
            let tot = (hn + ln).max(1) as f32;
            r.v[1] = hn as f32 * 100.0 / tot;
            r.v[2] = 100.0 - r.v[1];
            r.v[3] = hn as f32 / cycles / fs;
            r.v[4] = ln as f32 / cycles / fs;
            r.v[18] = sqrt(s2 / cnt.max(1) as f32);
        }
        let _ = (high_n, low_n);
        r.v[5] = hp as f32;
        r.v[6] = lp as f32;
    }
    r
}

/// "label value unit" in the stock style (<= 15 chars).
pub fn format<'a>(buf: &'a mut [u8; 24], id: u8, v: f32) -> &'a str {
    let (name, unit) = ITEMS.iter().find(|i| i.0 == id).map(|i| (i.1, i.2)).unwrap_or(("?", Unit::V));
    let mut n = 0;
    let mut put = |b: &mut [u8; 24], s: &[u8]| {
        let k = s.len().min(24 - n);
        b[n..n + k].copy_from_slice(&s[..k]);
        n += k;
    };
    put(buf, name.as_bytes());
    put(buf, b":");
    if !v.is_finite() || (v < 0.0 && unit != Unit::V) {
        put(buf, b" --");
        return core::str::from_utf8(&buf[..n]).unwrap_or("");
    }
    // Scale on a value nudged up by 1e-6 so 0.999999e-3 s shows as 1.00 ms.
    let v = v * 1.000_001;
    let (scaled, pre): (f32, &str) = match unit {
        Unit::V => {
            let a = v.abs();
            if a >= 1e3 {
                (v / 1e3, "kV")
            } else if a >= 1.0 || a == 0.0 {
                (v, "V")
            } else if a >= 1e-3 {
                (v * 1e3, "mV")
            } else {
                (v * 1e6, "uV")
            }
        }
        Unit::Hz => {
            if v >= 1e6 {
                (v / 1e6, "MHz")
            } else if v >= 1e3 {
                (v / 1e3, "kHz")
            } else if v >= 1.0 {
                (v, "Hz")
            } else {
                (v * 1e3, "mHz")
            }
        }
        Unit::S => {
            if v >= 1.0 {
                (v, "s")
            } else if v >= 1e-3 {
                (v * 1e3, "ms")
            } else if v >= 1e-6 {
                (v * 1e6, "us")
            } else {
                (v * 1e9, "ns")
            }
        }
        Unit::Pct => (v, "%"),
        Unit::N => (v, ""),
    };
    let a = scaled.abs();
    let dec = if unit == Unit::N {
        0
    } else if a < 9.995 {
        2
    } else if a < 99.95 {
        1
    } else {
        0
    };
    let mut t = [0u8; 16];
    let s = crate::dmm::format(&mut t, scaled, dec);
    put(buf, s.as_bytes());
    put(buf, pre.as_bytes());
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}
