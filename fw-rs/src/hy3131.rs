//! HY3131 DMM front end on PC10 / PC11 / PC12 + CS PD2
//! (firmware/re/analysis/hy3131_v2_6.h).
//!
//! SCK = PC10, MOSI = PC12, MISO = PC11, CS = PD2 (active low). MSB first, SCK
//! idle low, CS low for the whole transfer. Command byte = (reg << 1) | rd;
//! registers auto-increment. A read has one extra SCK pulse after the command
//! byte (the datasheet's read period), then MISO is sampled with SCK low before
//! each clock pulse.
//!
//! Reads go over the GD32's hardware SPI2, writes stay bit-banged
//! (docs/dmm-input-scope-feasibility.md 2.2 and 8):
//!
//! SPI2 (reads): 0x4000_3C00 on APB1 (50 MHz), PC10 / PC11 / PC12 on AF6
//! (GD32F450xx datasheet Rev1.1 table 5; SPI2EN = RCU_APB1EN bit 15, SPI_CTL0
//! / SPI_STAT per GD32F4xx UM V2.2 21.11), master, mode 0 (HY3131 DS 7.1 /
//! 7.2: CK idle low, DO changes after the falling edge, sampled on the
//! rising one), software NSS, 8-bit frames, PSC /16 = 3.125 MHz. CS stays a
//! GPIO. Budget at 3.125 MHz (160 ns half period): DO path = isolator out
//! (CA-IS3741, ~10-15 ns, INFERRED) + TDODL <= 60 ns + back = ~80-90 ns:
//! ~70 ns margin. Device 2026-10-22 (fca31fd DIAG image): 2000 x {1, 6, 23
//! byte} reads and 256 values read back clean at /8, /16 and /32.
//! The read period (the 9th clock) cannot come out of a byte-wide SPI: the
//! command byte goes out on SPI2, PC10 is switched to GPIO output for one
//! hand-made SCK pulse (ODR = 0 = the SPI idle level: no edge on either
//! switch) and back to AF, then the data bytes: the exact datasheet frame.
//!
//! Writes: bit-banged, the pins switched to GPIO for the frame (SCK low on
//! both sides of the switch, CS high). On the device every SPI2-written data
//! byte with bit 7 = 1 and bit 0 = 0 arrived with bit 0 = 1 (all 64 such
//! values of 256, every byte of a multi-byte write, at /16 and /32, CS
//! setup / hold / idle 0.2 .. 5 us, pin speed 2 .. 200 MHz, both read
//! framings alike; CPHA = 1 wrote nothing): the chip takes the LSB late, after
//! the end of the byte, when the GD32 no longer holds it on MOSI. Commands
//! (bit 7 = 0) and reads are unaffected. Writes are rare (mode changes,
//! counter restarts, the power meter's R34 phases): ~25 us per byte.
//!
//! The all-bit-banged bus (the `hyspi-bitbang` fallback build) is gone since
//! the read-SPI2 / write-bit-bang bus passed on the device (7889e6c).

use crate::mmio::{modify32, r32, spin, w16, w32};

const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const MODER: u32 = 0x00;
const OTYPER: u32 = 0x04;
const PUPD: u32 = 0x0C;
const BOP: u32 = 0x18;

const SCK: u32 = 10; // PC10
const MISO: u32 = 11; // PC11
const MOSI: u32 = 12; // PC12
const CS: u32 = 2; // PD2

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

fn out_pin(port: u32, pin: u32) {
    unsafe {
        modify32(port + MODER, 3 << (2 * pin), 1 << (2 * pin));
        modify32(port + OTYPER, 1 << pin, 0);
        modify32(port + PUPD, 3 << (2 * pin), 0);
    }
}

/// Configure the bus (other pins on these ports are left alone): CS high
/// first and for good, SCK low as a GPIO, then SPI2 configured and enabled
/// (it drives SCK low, its idle level) before the pins are handed to it, so
/// SCK never moves and CS never glitches (the HY3131 ignores CK while CS is
/// high and needs CK low when CS falls, DS 7.2 (6)).
pub fn init_pins() {
    set(GPIOD, CS, true);
    set(GPIOC, SCK, false);
    out_pin(GPIOC, SCK);
    out_pin(GPIOC, MOSI);
    out_pin(GPIOD, CS);
    unsafe { modify32(GPIOC + MODER, 3 << (2 * MISO), 0) }; // input
    spi::init();
}

/// The SPI2 prescaler of the read bus (8 / 16 / 32 = 6.25 / 3.125 / 1.5625
/// MHz).
pub const DIV: u8 = 16;

/// Read divider for the next frames (DBG:HYSPI:SELF? only; the caller
/// restores `DIV`).
pub fn set_div(div: u8) {
    spi::set_div(div);
}

/// Write `data` to `reg`, `reg + 1`, .. (bit-banged, see the module notes).
pub fn write(reg: u8, data: &[u8]) {
    spi::to_gpio();
    bb::write(reg, data);
    spi::to_af();
}

/// Read `buf.len()` registers from `reg`.
pub fn read(reg: u8, buf: &mut [u8]) {
    spi::read(reg, buf);
}

mod spi {
    use super::*;

    const RCU_APB1EN: u32 = 0x4002_3840;
    const RCU_SPI2EN: u32 = 1 << 15;
    const SPI2: u32 = 0x4000_3C00;
    const SPI_CTL0: u32 = SPI2 + 0x00;
    const SPI_STAT: u32 = SPI2 + 0x08;
    const SPI_DATA: u32 = SPI2 + 0x0C;
    const SPI_I2SCTL: u32 = SPI2 + 0x1C;
    const STAT_RBNE: u32 = 1 << 0;
    const STAT_TBE: u32 = 1 << 1;
    const STAT_TRANS: u32 = 1 << 7;
    /// Master (MSTMOD), software NSS high (SWNSSEN | SWNSS), MSB first, 8 bit,
    /// CKPL = CKPH = 0 (mode 0); PSC goes into bits 5:3.
    const CTL0_BASE: u32 = (1 << 9) | (1 << 8) | (1 << 2);
    const CTL0_SPIEN: u32 = 1 << 6;
    const OSPEED: u32 = 0x08;
    const AFRH: u32 = 0x24;
    const PINS2: u32 = (3 << (2 * SCK)) | (3 << (2 * MISO)) | (3 << (2 * MOSI));
    const AF_MASK: u32 = (0xF << (4 * (SCK - 8))) | (0xF << (4 * (MISO - 8))) | (0xF << (4 * (MOSI - 8)));
    const AF6: u32 = (6 << (4 * (SCK - 8))) | (6 << (4 * (MISO - 8))) | (6 << (4 * (MOSI - 8)));
    const MODER_AF: u32 = (2 << (2 * SCK)) | (2 << (2 * MISO)) | (2 << (2 * MOSI));
    const MODER_GPIO: u32 = (1 << (2 * SCK)) | (1 << (2 * MOSI));
    /// OSPEED 01 = 25 MHz for SCK / MOSI (2 .. 200 MHz made no difference on
    /// the device).
    const OSPEED_PINS: u32 = (3 << (2 * SCK)) | (3 << (2 * MOSI));
    const OSPEED_SPI: u32 = (1 << (2 * SCK)) | (1 << (2 * MOSI));
    /// CS low -> first SCK edge, last SCK edge -> CS high, CS high time
    /// (spin iterations, 5 cycles = 25 ns each at 200 MHz, measured):
    /// ~200 / ~100 / ~500 ns (0.2 .. 5 us made no difference on the device).
    const CS_SETUP: u32 = 8;
    const CS_HOLD: u32 = 4;
    const CS_IDLE: u32 = 20;
    /// The hand-made read-period pulse: ~100 ns high (TCKHI >= 20 ns), then
    /// ~100 ns low before the SPI's first sampling edge (another half period
    /// later) for DO (TDODL 60 ns + the isolator both ways).
    const PULSE_HI: u32 = 4;
    const PULSE_LO: u32 = 4;

    const fn psc_of(div: u8) -> u32 {
        match div {
            8 => 2,
            32 => 4,
            _ => 3,
        }
    }

    fn config(div: u8) {
        let cfg = CTL0_BASE | (psc_of(div) << 3);
        unsafe {
            w32(SPI_CTL0, CTL0_BASE);
            modify32(SPI_I2SCTL, 1 << 11, 0); // SPI, not I2S
            w32(SPI_CTL0, cfg);
            w32(SPI_CTL0, cfg | CTL0_SPIEN);
            let _ = r32(SPI_DATA); // drop a stale byte
        }
    }

    pub(super) fn init() {
        unsafe {
            modify32(RCU_APB1EN, 0, RCU_SPI2EN);
            let _ = r32(RCU_APB1EN); // clock on before the first register access
            modify32(GPIOC + OSPEED, OSPEED_PINS, OSPEED_SPI);
            modify32(GPIOC + AFRH, AF_MASK, AF6);
        }
        config(DIV);
        to_af();
    }

    /// Between frames only (SPI2 idle, SCK low, CS high).
    pub(super) fn set_div(div: u8) {
        idle();
        config(div);
    }

    /// Pins to GPIO for a bit-banged frame: SCK output low (= the SPI idle
    /// level), MOSI output, MISO input. SPI2 stays enabled and idle.
    pub(super) fn to_gpio() {
        idle();
        set(GPIOC, SCK, false);
        unsafe { modify32(GPIOC + MODER, PINS2, MODER_GPIO) };
    }

    /// Pins back to SPI2 (SCK low on both sides of the switch).
    pub(super) fn to_af() {
        set(GPIOC, SCK, false);
        unsafe { modify32(GPIOC + MODER, PINS2, MODER_AF) };
    }

    fn idle() {
        unsafe { while r32(SPI_STAT) & STAT_TRANS != 0 {} }
    }

    #[inline(always)]
    fn xfer(b: u8) -> u8 {
        unsafe {
            while r32(SPI_STAT) & STAT_TBE == 0 {}
            w16(SPI_DATA, b as u16);
            while r32(SPI_STAT) & STAT_RBNE == 0 {}
            r32(SPI_DATA) as u8
        }
    }

    /// Read `buf.len()` registers from `reg`. The command byte has bit 7 = 0,
    /// so the late-LSB effect of SPI2 writes (module notes) cannot touch it.
    pub(super) fn read(reg: u8, buf: &mut [u8]) {
        set(GPIOD, CS, false);
        spin(CS_SETUP);
        xfer((reg << 1) | 1);
        // Read period: SPI2 idle (SCK low after the 8th clock), PC10 to GPIO
        // output (ODR 0: stays low), one pulse, back to AF.
        idle();
        unsafe {
            modify32(GPIOC + MODER, 3 << (2 * SCK), 1 << (2 * SCK));
            set(GPIOC, SCK, true);
            spin(PULSE_HI);
            set(GPIOC, SCK, false);
            spin(PULSE_LO);
            modify32(GPIOC + MODER, 3 << (2 * SCK), 2 << (2 * SCK));
        }
        // MOSI 1 during the data phase, as on the bit-banged bus (the R bit).
        for b in buf.iter_mut() {
            *b = xfer(0xFF);
        }
        idle();
        spin(CS_HOLD);
        set(GPIOD, CS, true);
        spin(CS_IDLE);
    }
}

/// The bit-banged bus (~1 us half period): every write.
mod bb {
    use super::*;

    /// Half SCK period in spin iterations (~1 us at 200 MHz, well inside the
    /// HY3131 SPI timing).
    const HALF: u32 = 40;

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

/// R36: SPHACAL (HY3131 only), the AD2 / AD3 phase trim of the power
/// measurement: bit 7 = AD2 leads AD3, bits 6..0 = delay in AD2CLK periods
/// (1 / 2.4576 MHz = 0.407 us, up to 51.7 us).
pub const R_SPHACAL: u8 = 0x36;

/// Power-meter row (dmm::pw): as `configure` (AD3 enabled before AD2, the
/// beat fix) with SPHACAL written before both enables, as the datasheet
/// asks ("Set SPHACAL first, then ENAD2 and ENAD3 = 1, otherwise the data
/// would be stabilized later"). `quiesce` zeroed R36 before.
pub fn configure_ph(cfg: &[u8; CFG_LEN], r36: u8) {
    let mut c = *cfg;
    c[6] &= !ENAD2;
    c[20] &= !ENAD3;
    write(R_CFG, &c);
    write(R_INTE, &[0]);
    write(R_SPHACAL, &[r36]);
    write(0x34, &[cfg[20]]);
    write(0x26, &[cfg[6]]);
}

/// RMS<37:0> (R09..R0D, `b[0..5]`) as a signed 38-bit value: sum(X*Y)/N of
/// the power statistic is negative when the energy flows the other way.
/// (The stock and `poll_ac` read 37 bits unsigned: right for the squares.)
pub fn rms_signed(b: &[u8]) -> i64 {
    let raw = b[0] as u64 | (b[1] as u64) << 8 | (b[2] as u64) << 16 | (b[3] as u64) << 24 | ((b[4] & 0x3F) as u64) << 32;
    ((raw << 26) as i64) >> 26
}

/// LPF<18:0> (R06..R08, `b[0..3]`), 19-bit signed.
pub fn lpf19(b: &[u8]) -> i32 {
    (((b[0] as u32) << 13 | (b[1] as u32) << 21 | (b[2] as u32) << 29) as i32) >> 13
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
// one result per OSR2 = 32 << AD2OSR clocks (codes 5..7 all 1024). ENCHOPAD2
// does NOT halve the output rate on the device, whatever the datasheet says
// (device check of 9dd1222: R26 0x25 and 0x05 both 422 us per flag-synced
// sample, 0x24 and 0x04 both 232 us; the logger at "2400" = 0x24 took a
// result on every P/2 gate). Every stock AC row writes R26 = 0x80:
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

/// AD2 output period in nanoseconds for an R26 value (ENAD2 and ENCHOPAD2
/// ignored): 13021 ns (0x80, the stock AC rows) .. 833_333 ns (/4, OSR 1024).
pub const fn ad2_period_ns(r26: u8) -> u32 {
    let osr = 32u64 << if r26 & 7 > 5 { 5 } else { r26 & 7 };
    let clocks = osr * if r26 & R26_CLK4 != 0 { 4 } else { 2 };
    ((clocks * 1_000_000_000 + SYSCLK_HZ as u64 / 2) / SYSCLK_HZ as u64) as u32
}

/// AD2 output rate in mHz for an R26 value (9600 S/s = 9_600_000).
#[allow(dead_code)] // tests, docs
pub const fn ad2_rate_mhz(r26: u8) -> u32 {
    let osr = 32u64 << if r26 & 7 > 5 { 5 } else { r26 & 7 };
    let clocks = osr * if r26 & R26_CLK4 != 0 { 4 } else { 2 };
    ((SYSCLK_HZ as u64 * 1000 + clocks / 2) / clocks) as u32
}

/// The data logger's AD2 rate ladder: R26 low bits for a requested rate in
/// S/s. Prefers the /2 clock (more modulator samples per result) and the
/// largest OSR that gives the rate: 1200 .. 9600 S/s, the same rungs with
/// `chop` (ENCHOPAD2, offset cancelled, the logger default) and without
/// (ENCHOPAD2 does not slow AD2 down, see above; /4, OSR 1024 = 1200 S/s is
/// the slowest the chip has). A request between two rungs takes the nearest
/// one in log scale (rounding down on a tie); the achieved rung is what
/// LOG:PREamble? reports. Above ~6.5 kS/s the bit-banged bus was the limit
/// (device 9dd1222: 6.2-6.6 kS/s captured at 9600); LOG:COUNt? lost shows
/// what the bus in use misses.
pub const fn ad2_r26_for(req_hz: u32, chop: bool) -> u8 {
    // (rate S/s, R26 low bits without ENCHOPAD2)
    const T: [(u32, u8); 4] = [(1200, R26_CLK4 | 5), (2400, 5), (4800, 4), (9600, 3)];
    let c = if chop { R26_CHOP } else { 0 };
    let t = &T;
    let mut i = 0;
    while i + 1 < t.len() {
        // geometric midpoint of a 2x step: rate * sqrt(2); req^2 vs 2 rate^2
        let r = t[i].0 as u64;
        if (req_hz as u64) * (req_hz as u64) <= 2 * r * r {
            return t[i].1 | c;
        }
        i += 1;
    }
    t[t.len() - 1].1 | c
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
        for hz in [1200u32, 2400, 4800, 9600] {
            let r = ad2_r26_for(hz, true);
            assert_eq!(ad2_rate_mhz(r), hz * 1000, "chop {hz}");
            assert!(r & R26_CHOP != 0);
            let r = ad2_r26_for(hz, false);
            assert_eq!(ad2_rate_mhz(r), hz * 1000, "plain {hz}");
            assert!(r & R26_CHOP == 0);
        }
        // The device (9dd1222 check): ENCHOPAD2 does not halve the rate.
        assert_eq!(ad2_period_ns(0x25), ad2_period_ns(0x05));
        assert_eq!(ad2_period_ns(0x24), 208_333);
        // The 9dd1222 labels: 0x2D was "600" (measured 1125/s captured,
        // 886 us spacing <= the real 833 us + the gate), 0x24 was "2400".
        assert_eq!(ad2_rate_mhz(0x2D), 1_200_000);
        // Out-of-ladder requests: nearest rung, clamped at the ends.
        assert_eq!(ad2_rate_mhz(ad2_r26_for(1, true)), 1_200_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(600, true)), 1_200_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(600, false)), 1_200_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(50_000, true)), 9_600_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(3000, true)), 2_400_000);
        assert_eq!(ad2_rate_mhz(ad2_r26_for(3500, true)), 4_800_000);
        // 9600 S/s: 104.167 us; 1200 S/s: 833.3 us.
        assert_eq!(ad2_period_ns(ad2_r26_for(9600, true)), 104_167);
        assert_eq!(ad2_period_ns(ad2_r26_for(1200, true)), 833_333);
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

    #[test]
    fn rms_and_lpf_sign() {
        assert_eq!(rms_signed(&[0x10, 0, 0, 0, 0]), 16);
        // Bit 37 set: negative; bits 39..38 (sign copies or not) ignored.
        assert_eq!(rms_signed(&[0xFF, 0xFF, 0xFF, 0xFF, 0x3F]), -1);
        assert_eq!(rms_signed(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF]), -1);
        assert_eq!(rms_signed(&[0, 0, 0, 0, 0x20]), -(1i64 << 37));
        assert_eq!(rms_signed(&[0xFF, 0xFF, 0xFF, 0xFF, 0x1F]), (1i64 << 37) - 1);
        assert_eq!(lpf19(&[0xFF, 0xFF, 0x07]), -1);
        assert_eq!(lpf19(&[0x00, 0x00, 0x04]), -262_144);
        assert_eq!(lpf19(&[0xFF, 0xFF, 0x03]), 262_143);
    }
}
