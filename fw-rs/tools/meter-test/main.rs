// Host test crate for src/meter.rs, src/dmm.rs and src/logger.rs (FR-05 smoothing / rate,
// UF-21): the modules are compiled for the host with their register-level
// dependencies; the tests only call pure logic (no MMIO is touched).
#![allow(dead_code, unused_imports, clippy::all)]
#[path = "../../src/mmio.rs"]
mod mmio;
#[path = "../../src/hy3131.rs"]
mod hy3131;
#[path = "../../src/hy3131_tables.rs"]
mod hy3131_tables;
#[path = "../../src/clock.rs"]
mod clock;
#[path = "../../src/sentinel.rs"]
mod sentinel;
#[path = "../../src/dmm.rs"]
mod dmm;
#[path = "../../src/meter.rs"]
mod meter;
// The data logger's ring / gap accounting and the AD2 rate ladder (hy3131.rs).
#[path = "../../src/logger.rs"]
mod logger;
// The wave view's streaming trigger (logger.rs `wave`).
#[path = "../../src/trigsoft.rs"]
mod trigsoft;

fn main() {}

// DMM frequency mode (mode 14) end to end from synthetic HY3131 counter
// values: counters -> kHz -> auto range -> readout text (main and the AC V/A
// aux strip), plus the SI value SCPI / BT report. Owner report: 700 Hz and
// 999 Hz showed "1 kHz", 1.5 kHz "2 kHz".
#[cfg(test)]
mod freq {
    use crate::dmm::{counter_vals, freq_range_for, freq_val};

    /// 60000 counts: factor 6 * over 1.03.
    const X: f32 = 6.0 * 1.03;

    /// Counter registers for an input of `hz` with 50 % duty: the gate of
    /// 0x40000 time base ticks (4.9152 MHz) ends on the next input edge.
    fn counters(hz: f64) -> (u32, u32, u32) {
        let tb = 4_915_280.0;
        let ctb = (262_144.0 * hz / tb).ceil().max(1.0);
        let n = (ctb * tb / hz).round();
        ((n / 2.0) as u32, ctb as u32, (n - 262_144.0) as u32)
    }

    fn show(hz: f64) -> (String, String, String, String, f32, f32) {
        let (ctc, ctb, cta) = counters(hz);
        let (f, duty) = counter_vals(ctc, ctb, cta);
        let r = freq_range_for(f, X, 0);
        let mut b = [0u8; 16];
        let m = freq_val(f, r, false);
        let a = freq_val(f, r, true);
        let mt = crate::meter::text(&mut b, &m).to_string();
        let at = crate::meter::text(&mut b, &a).to_string();
        (mt, m.unit.to_string(), at, a.unit.to_string(), f * 1e3, duty)
    }

    #[test]
    fn readout_text() {
        let cases: [(f64, &str, &str, &str); 11] = [
            (50.0, "50.000", "Hz", "50.00"),
            (600.0, "600.00", "Hz", "600.0"),
            (700.0, "0.7000", "kHz", "0.700"),
            (999.0, "0.9990", "kHz", "0.999"),
            (1_000.0, "1.0000", "kHz", "1.000"),
            (1_500.0, "1.5000", "kHz", "1.500"),
            (32_768.0, "32.768", "kHz", "32.77"),
            (40_000.0, "40.000", "kHz", "40.00"),
            (100_000.0, "100.00", "kHz", "100.0"),
            (1_000_000.0, "1.0000", "MHz", "1.000"),
            (10_000_000.0, "10.000", "MHz", "10.00"),
        ];
        for (hz, main, unit, aux) in cases {
            let (mt, mu, at, au, si, duty) = show(hz);
            assert_eq!((mt.as_str(), mu.as_str()), (main, unit), "{hz} Hz main");
            assert_eq!((at.as_str(), au.as_str()), (aux, unit), "{hz} Hz aux");
            assert!(((si as f64) - hz).abs() <= hz * 2e-5, "{hz} Hz: SI {si}");
            assert!((duty - 50.0).abs() < 0.01, "{hz} Hz: duty {duty}");
        }
    }

    #[test]
    fn ranges() {
        // Range r spans up to X * 10^(r-2) kHz; above all ranges: unchanged.
        let r = |f: f32| freq_range_for(f, X, 9);
        assert_eq!([r(0.0), r(0.0617), r(0.0619), r(0.617), r(0.619), r(6.17), r(6.19)], [0, 0, 1, 1, 2, 2, 3]);
        assert_eq!([r(61.7), r(61.9), r(617.0), r(619.0), r(6170.0), r(6190.0), r(61_900.0)], [3, 4, 4, 5, 5, 6, 9]);
    }
}
