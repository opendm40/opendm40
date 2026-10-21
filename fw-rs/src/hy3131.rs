//! HY3131 DMM front end over bit-banged SPI (firmware/re/analysis/hy3131_v2_6.h).
//!
//! SCK = PC10, MOSI = PC12, MISO = PC11, CS = PD2 (active low). MSB first, SCK
//! idle low, CS low for the whole transfer. Command byte = (reg << 1) | rd;
//! registers auto-increment. A read has one extra SCK pulse after the command
//! byte (the datasheet's read period), then MISO is sampled with SCK low before
//! each clock pulse.

use crate::mmio::{modify32, r32, spin, w32};

const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const MODER: u32 = 0x00;
const OTYPER: u32 = 0x04;
const PUPD: u32 = 0x0C;
const IDR: u32 = 0x10;
const BOP: u32 = 0x18;

const SCK: u32 = 10; // PC10
const MISO: u32 = 11; // PC11
const MOSI: u32 = 12; // PC12
const CS: u32 = 2; // PD2

/// Half SCK period in spin iterations (~1 us at 200 MHz, well inside the
/// HY3131 SPI timing).
const HALF: u32 = 40;

pub const R_AD1: u8 = 0x00;
pub const R_INTF: u8 = 0x1E;
pub const R_INTE: u8 = 0x1F;
pub const R_CFG: u8 = 0x20; // R20..R35: 22 bytes of mode configuration
pub const CFG_LEN: usize = 22;

pub const INTF_AD1: u8 = 1 << 2;
pub const INTF_AD2: u8 = 1 << 1;
pub const INTF_LPF: u8 = 1 << 3;
pub const INTF_RMS: u8 = 1 << 4;
pub const INTF_CT: u8 = 1 << 0;

/// R26 bit 7 / R34 bit 7: enable AD2 / AD3.
const ENAD2: u8 = 0x80;
const ENAD3: u8 = 0x80;

#[inline(always)]
fn set(port: u32, pin: u32, high: bool) {
    let bit = if high { 1 << pin } else { 1 << (pin + 16) };
    unsafe { w32(port + BOP, bit) };
}

#[inline(always)]
fn miso() -> bool {
    (unsafe { r32(GPIOC + IDR) } & (1 << MISO)) != 0
}

fn out_pin(port: u32, pin: u32) {
    unsafe {
        modify32(port + MODER, 3 << (2 * pin), 1 << (2 * pin));
        modify32(port + OTYPER, 1 << pin, 0);
        modify32(port + PUPD, 3 << (2 * pin), 0);
    }
}

/// Configure the four bus pins (other pins on these ports are left alone).
pub fn init_pins() {
    set(GPIOD, CS, true);
    set(GPIOC, SCK, false);
    out_pin(GPIOC, SCK);
    out_pin(GPIOC, MOSI);
    out_pin(GPIOD, CS);
    unsafe { modify32(GPIOC + MODER, 3 << (2 * MISO), 0) }; // input
}

fn clock() {
    set(GPIOC, SCK, true);
    spin(HALF);
    set(GPIOC, SCK, false);
    spin(HALF);
}

fn tx(b: u8) {
    for i in (0..8).rev() {
        set(GPIOC, MOSI, b & (1 << i) != 0);
        spin(HALF);
        clock();
    }
}

fn rx() -> u8 {
    let mut b = 0u8;
    for _ in 0..8 {
        b = (b << 1) | miso() as u8;
        clock();
    }
    b
}

fn begin() {
    set(GPIOC, SCK, false);
    set(GPIOD, CS, false);
    spin(HALF);
}

fn end() {
    spin(HALF);
    set(GPIOD, CS, true);
    set(GPIOC, SCK, false);
    spin(HALF);
}

pub fn write(reg: u8, data: &[u8]) {
    begin();
    tx(reg << 1);
    for &b in data {
        tx(b);
    }
    end();
}

pub fn read(reg: u8, buf: &mut [u8]) {
    begin();
    tx((reg << 1) | 1);
    clock(); // read period
    for b in buf.iter_mut() {
        *b = rx();
    }
    end();
}

pub fn read1(reg: u8) -> u8 {
    let mut b = [0u8];
    read(reg, &mut b);
    b[0]
}

/// Signed 24-bit little-endian value at `b[0..3]`.
pub fn s24(b: &[u8]) -> i32 {
    ((b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16) << 8) as i32 >> 8
}

/// Mode change as the stock driver does it: stop AD2/AD3 and R36, then the
/// caller switches relays, then the 22 configuration bytes, then INTE = 0.
pub fn quiesce() {
    write(0x26, &[0]);
    write(0x34, &[0]);
    write(0x36, &[0]);
}

pub fn configure(cfg: &[u8; CFG_LEN]) {
    let (r26, r34) = (cfg[6], cfg[20]);
    if r26 & ENAD2 != 0 && r34 & ENAD3 != 0 {
        // Every AC row (f0, f2, f4, f5_6) runs the RMS converter on
        // sum(AD2 * AD3) / N (R34 SVXI). With the row written as is, ENAD2
        // (R26) is set some 14 bytes before ENAD3 (R34), and when AD3 then
        // starts in a bad phase of the shared AD2/AD3 decimation (period
        // OSR2 / FAD2CLK = 13 us) sqrt(RMS) beats against the signal for as
        // long as the row stays: +-11 % on the 40 Hz PWM at AC V r1, both in
        // plain AC and in AC+DC (docs/acdc-stock-vs-fwrs.md, "Read-path
        // investigation"). Enabling AD3 first and AD2 last was clean for
        // every skew tried (0..15 us), so the row goes out with both off
        // and they are switched on in that order.
        let mut c = *cfg;
        c[6] = r26 & !ENAD2;
        c[20] = r34 & !ENAD3;
        write(R_CFG, &c);
        write(R_INTE, &[0]);
        write(0x34, &[r34]);
        write(0x26, &[r26]);
    } else {
        write(R_CFG, cfg);
        write(R_INTE, &[0]);
    }
}

// ------------------------------------------------------- AD1 conversion timing
//
// R22 = AD1OS[7:5] AD1CHOP[4:3] AD1OSR[2:0] (hy3131_v2_6.h; datasheet 13.1).
// The AD1 modulator runs at AD1CLK = SYSCLK / 30 (fixed divider, chapter 11)
// and delivers one result per OSR1 = 256 << AD1OSR clocks; with AD1CHOP = 1x
// the chip alternates the input polarity per conversion to cancel the offset
// and "the ADC output rate will be one time slower", i.e. halved:
//   period = OSR1 * 30 / SYSCLK * (AD1CHOP = 1x ? 2 : 1), SYSCLK 4.9152 MHz
// Every R22 the stock writes is chopped (AD1CHOP = 10):
//   0x13 OSR 2048   25 ms  40/s   (ohm over range; the data logger default)
//   0x14 OSR 4096   50 ms  20/s   (continuity while the reading moves)
//   0x15 OSR 8192  100 ms  10/s   (every DC row; the stock's moving reading)
//   0x16 OSR 16384 200 ms   5/s   (temperature)
//   0x17 OSR 32768 400 ms 2.5/s   (the stock's steady DC / ohm reading)
// Unchopped, the same codes run twice as fast (OSR 256: 1.5625 ms, 640/s;
// the datasheet's "5 Hz" headline is OSR 32768 unchopped). Source:
// emu/hy3131/RESEARCH.md 3; the emulator models this formula (and agrees
// with the data logger's 38 Hz measured on the device at 0x13); a direct
// device measurement of the DMM rates is still pending (TODO.md).
// Before 2026-10-14 fw-rs assumed OSR / SYSCLK here (16x too short at OSR
// 2048: 417 us) and an "SPI-limited" logger; the bit-banged link needs
// ~122 us per sample (read1(INTF) ~45 us + 3-byte AD1 read ~77 us), far
// below every conversion period, so the chip sets the pace at every code.

/// HY3131 system clock (crystal), Hz.
pub const SYSCLK_HZ: u32 = 4_915_200;
/// AD1CLK = SYSCLK / AD1CLK_DIV (fixed in the chip).
pub const AD1CLK_DIV: u32 = 30;
/// R22 AD1CHOP = 10: chopped, offset cancelled, half the rate. The stock DC
/// base (AD1OS = 000) is 0x10 | AD1OSR.
pub const R22_CHOP: u8 = 0x10;

/// AD1 conversion period in microseconds for an AD1OSR code (0..7, OSR =
/// 256 << code) with or without the chopper: 1563 us (OSR 256, unchopped)
/// .. 400000 us (OSR 32768, chopped); rounded to the nearest us.
pub const fn ad1_period_us(osr_code: u8, chop: bool) -> u32 {
    let clocks = (256u64 << (osr_code & 7)) * AD1CLK_DIV as u64 * if chop { 2 } else { 1 };
    ((clocks * 1_000_000 + SYSCLK_HZ as u64 / 2) / SYSCLK_HZ as u64) as u32
}

/// `ad1_period_us` of a raw R22 value (AD1OSR = bits 2..0, AD1CHOP bit 4).
pub const fn ad1_period_us_r22(r22: u8) -> u32 {
    ad1_period_us(r22 & 7, r22 & R22_CHOP != 0)
}

/// AD1OSR run-time code of the data logger's default (R22 = 0x13, OSR 2048,
/// chopped: 25 ms, 40/s, the stock's fastest setting).
pub const AD1OSR_STREAM: u8 = 3;

/// Write R22 with AD1OS = 000 (no DC offset) and the given AD1OSR code
/// (clamped to 0..7) and chopper. Lower code = faster, fewer effective bits;
/// `chop` false (AD1CHOP = 00: VX + VOS) doubles the rate but leaves the
/// converter offset in every result (the calibration was taken chopped) and
/// raises the low-frequency noise. A legal setting: Hycon's own ENOB tool
/// runs AD1 unchopped in its AC rows (R22 = 0x04 / 0x07).
pub fn set_ad1_osr(code: u8, chop: bool) {
    let chop = if chop { R22_CHOP } else { 0 };
    write(0x22, &[chop | (code & 7)]);
}

/// True when AD1 has a fresh conversion (INTF AD1F). Reading the AD1 data
/// registers afterwards clears the flag, so this paces the logger at the
/// conversion rate without an external timer.
pub fn ad1_ready() -> bool {
    read1(R_INTF) & INTF_AD1 != 0
}

/// Read the current signed 24-bit AD1 conversion (R00..R02). Clears AD1F.
pub fn read_ad1() -> i32 {
    let mut b = [0u8; 3];
    read(R_AD1, &mut b);
    s24(&b)
}

// ------------------------------------------------------- AD2 streaming (FR-38 B)
//
// R26 = ENAD2 . 0 . ENCHOPAD2 . AD2RG . SAD2CLK . AD2OSR[2:0] (datasheet 13.2,
// emu/hy3131/ref/registers.md). AD2CLK = SYSCLK / 2 (SAD2CLK = 0) or / 4;
// one result per OSR2 = 32 << AD2OSR clocks (codes 5..7 all 1024); ENCHOPAD2
// halves the rate (offset cancelled). Every stock AC row writes R26 = 0x80:
// /2, OSR 32, unchopped = 76.8 kS/s (13 us). The data logger's AD2 source
// keeps the row (mux, gain, op amps) and only rewrites R26 to a slower rung
// (docs/dmm-input-scope-feasibility.md 1.1); results: 19-bit signed in
// R03..R05 (bits 23:19 a copy of the sign), flag INTF.AD2F, set by AD3 too.
// No FIFO: an unread result is overwritten by the next one.

/// First AD2 data register (R03..R05, LSB first).
pub const R_AD2: u8 = 0x03;
/// R26 bits.
pub const R26_CHOP: u8 = 0x20;
pub const R26_CLK4: u8 = 0x08;

/// AD2 output period in nanoseconds for an R26 value (ENAD2 ignored): 13021
/// ns (0x80, the stock AC rows) .. 1_666_667 ns (/4, OSR 1024, chopped).
pub const fn ad2_period_ns(r26: u8) -> u32 {
    let osr = 32u64 << if r26 & 7 > 5 { 5 } else { r26 & 7 };
    let clocks = osr * if r26 & R26_CLK4 != 0 { 4 } else { 2 } * if r26 & R26_CHOP != 0 { 2 } else { 1 };
    ((clocks * 1_000_000_000 + SYSCLK_HZ as u64 / 2) / SYSCLK_HZ as u64) as u32
}

/// AD2 output rate in mHz for an R26 value (9600 S/s = 9_600_000).
#[allow(dead_code)] // tests, docs
pub const fn ad2_rate_mhz(r26: u8) -> u32 {
    let osr = 32u64 << if r26 & 7 > 5 { 5 } else { r26 & 7 };
    let clocks = osr * if r26 & R26_CLK4 != 0 { 4 } else { 2 } * if r26 & R26_CHOP != 0 { 2 } else { 1 };
    ((SYSCLK_HZ as u64 * 1000 + clocks / 2) / clocks) as u32
}

/// The data logger's AD2 rate ladder: R26 low bits for a requested rate in
/// S/s. Prefers the /2 clock (more modulator samples per result) and the
/// largest OSR that gives the rate; with `chop` (offset cancelled, the
/// logger default) 600 .. 9600 S/s, without 1200 .. 9600 (600 S/s needs the
/// chopper: /4, OSR 1024 unchopped is already 1200). A request between two
/// rungs takes the nearest one in log scale (rounding down on a tie); the
/// achieved rung is what LOG:PREamble? reports.
pub const fn ad2_r26_for(req_hz: u32, chop: bool) -> u8 {
    // (rate S/s, R26 low bits)
    const CHOP: [(u32, u8); 5] =
        [(600, R26_CHOP | R26_CLK4 | 5), (1200, R26_CHOP | 5), (2400, R26_CHOP | 4), (4800, R26_CHOP | 3), (9600, R26_CHOP | 2)];
    const PLAIN: [(u32, u8); 4] = [(1200, R26_CLK4 | 5), (2400, 5), (4800, 4), (9600, 3)];
    let t: &[(u32, u8)] = if chop { &CHOP } else { &PLAIN };
    let mut i = 0;
    while i + 1 < t.len() {
        // geometric midpoint of a 2x step: rate * sqrt(2); req^2 vs 2 rate^2
        let r = t[i].0 as u64;
        if (req_hz as u64) * (req_hz as u64) <= 2 * r * r {
            return t[i].1;
        }
        i += 1;
    }
    t[t.len() - 1].1
}

/// Logger AD2 stream: AD3 off (the row starts it before AD2, so its grid is
/// skewed and AD2F would fire twice per period), RMS / LPF off (R29 = 0),
/// then R26 = ENAD2 | `r26`. ENAD2 is never cleared (that would zero
/// R03..R05). The stock AC rows leave AD1 off; the misconnect sentinel
/// (sentinel.rs) turns it on in AC V / mV for its peak hold, so AD1 is
/// switched off FIRST: datasheet 13.1 note 7 (no AD2/AD3 switching while AD1
/// converts) then does not apply, and AD2 runs alone. The DMM restores the
/// row, sentinel included, afterwards (dmm.start).
pub fn set_ad2_stream(r26: u8) {
    let r23 = read1(0x23);
    if r23 & 0x80 != 0 {
        write(0x23, &[r23 & 0x7F]);
    }
    write(0x34, &[0]);
    write(0x29, &[0]);
    write(0x26, &[0x80 | (r26 & 0x3F)]);
    write(R_INTF, &[0]); // drop the flags of the old setting
}

/// True when AD2 has a result not seen yet (INTF AD2F). Reading INTF clears
/// ALL flags (AD1F, RMSF, LPFF too): only while the logger owns the chip.
pub fn ad2_ready() -> bool {
    read1(R_INTF) & INTF_AD2 != 0
}

/// The AD2 result (R03..R05) as signed 19 bit, plus whether bits 23:19 are a
/// clean copy of the sign (bit 18): a torn 3-byte read can break that.
pub fn read_ad2() -> (i32, bool) {
    let mut b = [0u8; 3];
    read(R_AD2, &mut b);
    let v = s24(&b);
    (v, ad2_sign_ok(v))
}

/// Bits 23:19 of a sign-extended 24-bit value equal bit 18.
pub const fn ad2_sign_ok(v: i32) -> bool {
    v >= -0x40000 && v <= 0x3FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ad2_ladder_matches_the_datasheet() {
        // The stock AC rows: /2, OSR 32, unchopped: 76.8 kS/s.
        assert_eq!(ad2_rate_mhz(0x80), 76_800_000);
        assert_eq!(ad2_period_ns(0x80), 13_021);
        for hz in [600u32, 1200, 2400, 4800, 9600] {
            let r = ad2_r26_for(hz, true);
            assert_eq!(ad2_rate_mhz(r), hz * 1000, "chop {hz}");
            assert!(r & R26_CHOP != 0);
            if hz >= 1200 {
                let r = ad2_r26_for(hz, false);
                assert_eq!(ad2_rate_mhz(r), hz * 1000, "plain {hz}");
                assert!(r & R26_CHOP == 0);
            }
        }
        // Out-of-ladder requests: nearest rung, clamped at the ends.
        assert_eq!(ad2_rate_mhz(ad2_r26_for(1, true)), 600_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(600, false)), 1_200_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(50_000, true)), 9_600_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(3000, true)), 2_400_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(3500, true)), 4_800_000);
        // 9600 S/s: 104.167 us; 600 S/s: 1.6667 ms.
        assert_eq!(ad2_period_ns(ad2_r26_for(9600, true)), 104_167);
        assert_eq!(ad2_period_ns(ad2_r26_for(600, true)), 1_666_667);
        // AD2OSR codes 5..7 are all 1024.
        assert_eq!(ad2_period_ns(7), ad2_period_ns(5));
        assert!(ad2_sign_ok(0x3FFFF) && ad2_sign_ok(-0x40000) && !ad2_sign_ok(0x40000) && !ad2_sign_ok(0x7F0000));
    }

    #[test]
    fn ad1_periods_match_the_datasheet() {
        // Chopped, as every stock R22 (emu/hy3131/RESEARCH.md 3).
        assert_eq!(ad1_period_us_r22(0x13), 25_000);
        assert_eq!(ad1_period_us_r22(0x14), 50_000);
        assert_eq!(ad1_period_us_r22(0x15), 100_000);
        assert_eq!(ad1_period_us_r22(0x16), 200_000);
        assert_eq!(ad1_period_us_r22(0x17), 400_000);
        // Unchopped: half; the fastest setting, OSR 256: 1.5625 ms = 640/s.
        assert_eq!(ad1_period_us(0, false), 1_563);
        assert_eq!(ad1_period_us(0, true), 3_125);
        assert_eq!(ad1_period_us(3, false), 12_500);
        assert_eq!(ad1_period_us(7, false), 200_000);
        assert_eq!(ad1_period_us_r22(0x07), 200_000);
        assert_eq!(ad1_period_us_r22(0x18 | 3), 25_000); // AD1CHOP = 11 chops too
        assert_eq!(ad1_period_us(9, true), ad1_period_us(1, true)); // code masked to 0..7
    }
}
