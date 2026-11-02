//! Hand-rolled DWC2 device: EP0 + bulk EP1 IN/OUT in polled slave mode, BOT + SCSI.
//! Transfers block in place while `pump` serves EP0 and bus resets.
use crate::hw::*;
use crate::spi;

const B: u32 = 0x5000_0000;
const GUSBCFG: u32 = B + 0x0C;
const GRSTCTL: u32 = B + 0x10;
const GINTSTS: u32 = B + 0x14;
const GRXSTSP: u32 = B + 0x20;
const GRXFSIZ: u32 = B + 0x24;
const DIEPTXF0: u32 = B + 0x28;
const GCCFG: u32 = B + 0x38;
const DIEPTXF1: u32 = B + 0x104;
const DCFG: u32 = B + 0x800;
const DCTL: u32 = B + 0x804;
const PCGCCTL: u32 = B + 0xE00;
const IN0: u32 = B + 0x900;
const OUT0: u32 = B + 0xB00;
const IN1: u32 = IN0 + 0x20;
const OUT1: u32 = OUT0 + 0x20;
const FIFO0: u32 = B + 0x1000;
const CTL_INT: u32 = 8; // DxEPINT
const CTL_SIZ: u32 = 0x10; // DxEPTSIZ
const EPENA: u32 = 1 << 31;
const EPDIS: u32 = 1 << 30;
const SD0PID: u32 = 1 << 28;
const SNAK: u32 = 1 << 27;
const CNAK: u32 = 1 << 26;
const STALL: u32 = 1 << 21;
const USBAEP: u32 = 1 << 15;
const BULK64: u32 = USBAEP | 2 << 18 | 64;
const PKT1: u32 = 1 << 19;
const USBRST: u32 = 1 << 12;
const RXFLVL: u32 = 1 << 4;
const STUP: u32 = 1 << 3;

static DEVICE: [u8; 18] = [18, 1, 0, 2, 0, 0, 0, 64, 0xE9, 0x28, 0x8F, 0x02, 0, 1, 0, 0, 0, 1];
static CONFIG: [u8; 32] = [
    9, 2, 32, 0, 1, 1, 0, 0x80, 50, //
    9, 4, 0, 0, 2, 8, 6, 0x50, 0, //
    7, 5, 0x81, 2, 64, 0, 0, //
    7, 5, 0x01, 2, 64, 0, 0,
];
static INQUIRY: [u8; 36] = *b"\x00\x80\x02\x02\x1F\x00\x00\x00ATK     DM40 LOADER     1.0 ";

fn rearm(out: u32) {
    w(out + CTL_SIZ, 3 << 29 | PKT1 | 64);
    set(out, EPENA | CNAK);
}

/// One packet at most: TX FIFO of `ep` (0 or 1) gets it, the core sends it on the IN token.
#[inline(never)]
fn tx(ep: u32, d: &[u8]) {
    let i = IN0 + 0x20 * ep;
    w(i + CTL_SIZ, PKT1 | d.len() as u32);
    set(i, EPENA | CNAK);
    for c in d.chunks(4) {
        let mut v = [0u8; 4];
        v[..c.len()].copy_from_slice(c);
        w(FIFO0 + 0x1000 * ep, u32::from_le_bytes(v));
    }
}

fn stall0() {
    set(IN0, STALL);
    set(OUT0, STALL);
}

pub struct Usb {
    setup: [u8; 8],
    abort: bool,
    sense: u8,
    /// Written to since the last rescan; t_io = last READ(10) / WRITE(10).
    wrote: bool,
    t_io: u32,
}

impl Usb {
    #[inline(never)]
    pub fn new() -> Usb {
        set(RCU_AHB2EN, 1 << 7);
        wait(GRSTCTL, 1 << 31, 1 << 31);
        set(GRSTCTL, 1);
        wait(GRSTCTL, 1, 0);
        w(GCCFG, 1 << 16 | 1 << 21); // PWRON, VBUSIG
        w(GUSBCFG, 1 << 30 | 6 << 10 | 1 << 6); // device, TRDT for AHB >= 32 MHz
        ms(25);
        w(PCGCCTL, 0);
        w(DCFG, 3);
        w(GRXFSIZ, 128);
        w(DIEPTXF0, 32 << 16 | 128);
        w(DIEPTXF1, 64 << 16 | 160);
        rearm(OUT0);
        w(GINTSTS, !0);
        w(DCTL, 2); // soft-disconnected until `connect`
        Usb { setup: [0; 8], abort: false, sense: 0, wrote: false, t_io: 0 }
    }

    /// Once, ~3 s after the last disk transfer of a session with writes. TEST
    /// UNIT READY polling does not count: hosts keep doing it on an idle disk.
    pub fn idle_rescan(&mut self) -> bool {
        if self.wrote && now().wrapping_sub(self.t_io) > 3000 * CYC_MS {
            self.wrote = false;
            return true;
        }
        false
    }

    pub fn connect(&mut self, on: bool) {
        w(DCTL, if on { 0 } else { 2 });
    }

    fn bus_reset(&mut self) {
        w(GINTSTS, USBRST);
        m(DCFG, 0x7F << 4, 0);
        self.abort_in();
        w(IN1, SNAK); // not active until SET_CONFIGURATION
        w(OUT1, SNAK);
        w(GRSTCTL, 1 << 4); // flush RX
        wait(GRSTCTL, 0x10, 0);
        w(OUT0 + CTL_INT, 0xFF);
        rearm(OUT0);
    }

    /// Drops a bulk IN packet in flight and ends the running command.
    fn abort_in(&mut self) {
        if r(IN1) & EPENA != 0 {
            set(IN1, EPDIS | SNAK);
        }
        w(GRSTCTL, 0x10 << 6 | 1 << 5); // flush all TX
        wait(GRSTCTL, 0x20, 0);
        self.abort = true;
    }

    /// One step of the core: bus reset, one RX FIFO entry, SETUP handling.
    /// Returns the bytes of an EP1 OUT packet, stored in `dst`.
    #[inline(never)]
    fn pump(&mut self, dst: &mut [u8]) -> usize {
        let s = r(GINTSTS);
        if s & USBRST != 0 {
            self.bus_reset();
        }
        let mut got = 0;
        if s & RXFLVL != 0 {
            let st = r(GRXSTSP);
            let ep = st & 15;
            let n = (st >> 4 & 0x7FF) as usize;
            match st >> 17 & 15 {
                k @ (2 | 6) => {
                    let t: &mut [u8] = if k == 6 {
                        &mut self.setup
                    } else if ep == 1 {
                        got = n;
                        dst
                    } else {
                        &mut []
                    };
                    let mut i = 0;
                    while i < n {
                        let mut v = r(FIFO0);
                        let mut k = 0;
                        while k < 4 && i + k < n {
                            if let Some(x) = t.get_mut(i + k) {
                                *x = v as u8;
                            }
                            v >>= 8;
                            k += 1;
                        }
                        i += 4;
                    }
                }
                // OUT transfer complete: the core has set NAK, take the next packet.
                3 => rearm(OUT0 + 0x20 * ep),
                _ => {}
            }
        }
        if r(OUT0 + CTL_INT) & STUP != 0 {
            w(OUT0 + CTL_INT, 0xFF);
            self.on_setup();
            rearm(OUT0);
        }
        got
    }

    fn on_setup(&mut self) {
        let [rt, req, _, vh, il, _, ll, lh] = self.setup;
        let want = u16::from_le_bytes([ll, lh]) as usize;
        let d: &[u8] = match (rt, req) {
            (0x80, 6) if vh == 1 => &DEVICE,
            (0x80, 6) if vh == 2 => &CONFIG,
            (0x80..=0x82, 0) => &[0, 0],
            (0x80, 8) | (0x81, 10) => &[1],
            (0xA1, 0xFE) => &[0],
            (0x00, 5) => {
                m(DCFG, 0x7F << 4, (self.setup[2] as u32) << 4);
                &[]
            }
            (0x00, 9) => {
                w(IN1, BULK64 | 1 << 22 | SD0PID | SNAK);
                w(OUT1, BULK64 | SD0PID);
                rearm(OUT1);
                &[]
            }
            (0x02, 1) => {
                set(if il & 0x80 != 0 { IN1 } else { OUT1 }, SD0PID);
                &[]
            }
            (0x21, 0xFF) => {
                self.abort_in();
                &[]
            }
            _ if rt & 0x80 == 0 => &[],
            _ => return stall0(),
        };
        tx(0, &d[..d.len().min(want)]);
    }

    /// Blocks until the IN packet went out, bus reset or BOT reset.
    fn send(&mut self, d: &[u8]) {
        tx(1, d);
        while r(IN1) & EPENA != 0 && !self.abort {
            self.pump(&mut []);
        }
    }

    fn recv(&mut self, d: &mut [u8]) -> usize {
        loop {
            let n = self.pump(d);
            if n != 0 || self.abort {
                return n;
            }
        }
    }

    pub fn poll(&mut self) {
        let mut cbw = [0u8; 64];
        if self.pump(&mut cbw) == 31 && u32::from_le_bytes([cbw[0], cbw[1], cbw[2], cbw[3]]) == 0x4342_5355 {
            self.abort = false;
            self.command(&cbw);
        }
    }

    #[inline(never)]
    fn command(&mut self, c: &[u8; 64]) {
        let len = u32::from_le_bytes([c[8], c[9], c[10], c[11]]);
        let lba = u32::from_be_bytes([c[17], c[18], c[19], c[20]]);
        let cnt = u32::from_be_bytes([0, 0, c[22], c[23]]);
        let mut ok = true;
        let mut done = 0u32;
        let mut reply: &[u8] = &[];
        let mut sense = [0u8; 18];
        sense[0] = 0x70;
        sense[7] = 10;
        if self.sense != 0 {
            sense[2] = 5;
            sense[12] = 0x20;
        }
        match c[15] {
            0x00 | 0x1B | 0x1E | 0x35 => {}
            0x03 => {
                reply = &sense;
                self.sense = 0;
            }
            0x12 => reply = &INQUIRY,
            0x25 => reply = &[0, 0, 0x0F, 0xFF, 0, 0, 0x10, 0],
            0x1A => reply = &[3, 0, 0, 0],
            0x28 if lba + cnt <= 4096 => {
                let mut a = lba * 4096;
                let mut pkt = [0u8; 64];
                while done < cnt * 4096 && !self.abort {
                    spi::read(a, &mut pkt);
                    self.send(&pkt);
                    a += 64;
                    done += 64;
                }
                self.t_io = now();
            }
            0x2A if lba + cnt <= 4096 => {
                let mut a = lba * 4096;
                let mut page = [0u8; 256];
                while done < cnt * 4096 && !self.abort {
                    if a & 0xFFF == 0 {
                        spi::erase(a);
                    }
                    let mut f = 0;
                    while f < 256 && !self.abort {
                        f += self.recv(&mut page[f..]);
                    }
                    spi::program(a, &page);
                    a += 256;
                    done += 256;
                }
                self.t_io = now();
                self.wrote = cfg!(feature = "idle");
            }
            _ => ok = false,
        }
        if !ok {
            self.sense = 5;
            // No endpoint halt logic: pad the data phase, the CSW carries the failure.
            let mut pad = [0u8; 64];
            while done < len && !self.abort {
                if c[12] & 0x80 != 0 {
                    self.send(&pad);
                    done += 64;
                } else {
                    done += self.recv(&mut pad).max(1) as u32;
                }
            }
            done = 0;
        } else if !reply.is_empty() {
            let n = reply.len().min(len as usize);
            self.send(&reply[..n]);
            done = n as u32;
        }
        if self.abort {
            return;
        }
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(b"USBS");
        csw[4..8].copy_from_slice(&c[4..8]);
        csw[8..12].copy_from_slice(&len.saturating_sub(done).to_le_bytes());
        csw[12] = !ok as u8;
        self.send(&csw);
    }
}
