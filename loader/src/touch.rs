//! CST at 0x5A on I2C2 (0x40005C00, SCL PA8, SDA PC9), INT PE5, RST PE6.
//! Side keys only. Hardware I2C at 100 kHz with bounded waits.
use crate::hw::*;

const CTL0: u32 = 0x4000_5C00;
const CTL1: u32 = 0x4000_5C04;
const DATA: u32 = 0x4000_5C10;
const STAT0: u32 = 0x4000_5C14;
const STAT1: u32 = 0x4000_5C18;
const CKCFG: u32 = 0x4000_5C1C;
const RT: u32 = 0x4000_5C20;
const START: u32 = 1 << 8;
const STOP: u32 = 1 << 9;
const ACK: u32 = 1 << 10;
const SBSEND: u32 = 1;
const ADDSEND: u32 = 2;
const BTC: u32 = 4;
const RBNE: u32 = 0x40;
const TBE: u32 = 0x80;
const EXTI_PD: u32 = 0x4001_3C14;
const ADDR: u32 = 0x5A << 1;

fn st(mask: u32) -> bool {
    wait(STAT0, mask, mask)
}

fn start(a: u32) -> bool {
    wait(STAT1, 2, 0) && {
        set(CTL0, START);
        st(SBSEND)
    } && {
        w(DATA, a);
        st(ADDSEND)
    }
}

fn stop() {
    set(CTL0, STOP);
    wait(CTL0, STOP, 0);
}

fn write(b: &[u8]) -> bool {
    let mut ok = start(ADDR);
    let _ = (r(STAT0), r(STAT1));
    for &v in b {
        ok = ok && st(TBE);
        w(DATA, v as u32);
    }
    ok = ok && st(BTC);
    stop();
    ok
}

/// Register read, n >= 3 (the only sequence kept of touch.rs hw_read).
fn read(hi: u8, lo: u8, b: &mut [u8]) -> bool {
    if !write(&[hi, lo]) {
        return false;
    }
    set(CTL0, ACK);
    if !start(ADDR | 1) {
        stop();
        return false;
    }
    let _ = (r(STAT0), r(STAT1));
    let n = b.len();
    let mut i = 0;
    let mut ok = true;
    while i < n {
        let left = n - i;
        if left == 3 {
            ok &= st(BTC);
            m(CTL0, ACK, 0);
        } else if left == 2 {
            ok &= st(BTC);
            set(CTL0, STOP);
        } else {
            ok &= st(RBNE);
        }
        b[i] = r(DATA) as u8;
        i += 1;
    }
    wait(CTL0, STOP, 0);
    ok
}

#[inline(never)]
pub fn init() {
    set(RCU_APB1EN, 1 << 23);
    w(CTL1, 24);
    w(CKCFG, 120);
    w(RT, 25);
    w(CTL0, 1 | ACK);
    ms(20);
    w(GPIOE + BOP, 1 << 6);
    ms(200);
    let mut chip = [0u8; 4];
    write(&[0xD1, 0x01]);
    read(0xD2, 0x04, &mut chip);
    write(&[0xD1, 0x09]);
    // EXTI line 5 <- PE5, falling edge, pending bit polled (NVIC line off).
    set(RCU_APB2EN, 1 << 14);
    m(0x4001_380C, 0xF << 4, 4 << 4);
    set(0x4001_3C0C, 1 << 5);
    set(0x4001_3C00, 1 << 5);
    w(EXTI_PD, 1 << 5);
}

/// Press edge state. A frame lost on the way (a blocking transfer) must not
/// leave it stuck: no frame for 200 ms counts as a release.
pub struct Touch {
    pub held: bool,
    pub last: u32,
}

impl Touch {
    /// A key or a screen contact that began since the last call. Coordinates as
    /// firmware/src/touch.rs, without its per-unit calibration.
    #[inline(never)]
    pub fn poll(&mut self) -> Option<(u16, u16)> {
        if r(EXTI_PD) & (1 << 5) == 0 {
            if self.held && now().wrapping_sub(self.last) > 200 * CYC_MS {
                self.held = false;
            }
            return None;
        }
        w(EXTI_PD, 1 << 5);
        let mut b = [0u8; 7];
        if !read(0xD0, 0x00, &mut b) || b[6] != 0xAB || b[0] == 0xAB {
            return None;
        }
        write(&[0xD0, 0x00, 0xAB]);
        self.last = now();
        // Release: low nibble 0 (0x80 on the device, 0x00 from the emulator's model).
        if b[0] & 0x0F == 0 {
            self.held = false;
            return None;
        }
        if self.held {
            return None;
        }
        let x = (b[1] as u16) << 4 | (b[3] >> 4) as u16;
        let y = (b[2] as u16) << 4 | (b[3] & 15) as u16;
        self.held = true;
        // A side key becomes a tap on its label in the UI (zones 85 px apart);
        // 372 / 628 / 884 / 1140 as measured, halfway limits.
        Some(if b[0] & 0x80 != 0 { (0, x.saturating_sub(250) / 250 * 85) } else { (x, y) })
    }
}
