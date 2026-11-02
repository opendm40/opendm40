//! FAT16 root directory and cluster chain, directly on the SPI flash (no sector buffer).
use crate::spi;

#[derive(Clone, Copy, Default)]
pub struct Vol {
    pub fat: u32,
    pub root: u32,
    pub entries: u32,
    pub data: u32,
    pub cl_size: u32,
}

impl Vol {
    /// BPB at offset 0, no partition table. None without a plausible FAT.
    #[inline(never)]
    pub fn mount() -> Option<Vol> {
        let mut b = [0u8; 24];
        spi::read(0, &mut b);
        let g = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as u32;
        let bps = g(11);
        let bad = bps == 0 || bps & (bps - 1) != 0 || b[13] == 0 || b[16] == 0 || g(22) == 0 || g(17) == 0;
        if bad || spi::read_u16(0x1FE) != 0xAA55 {
            return None;
        }
        let fat = g(14) * bps;
        let root = fat + b[16] as u32 * g(22) * bps;
        let entries = g(17);
        let data = root + ((entries * 32 + bps - 1) & !(bps - 1));
        Some(Vol { fat, root, entries, data, cl_size: b[13] as u32 * bps })
    }

    pub fn cl_addr(&self, c: u16) -> u32 {
        self.data + (c as u32).wrapping_sub(2).wrapping_mul(self.cl_size)
    }

    pub fn next(&self, c: u16) -> u16 {
        spi::read_u16(self.fat + c as u32 * 2)
    }
}

/// The first SPI sector holds nothing but 0xFF.
pub fn blank() -> bool {
    let mut a = 0;
    while a < 4096 {
        let mut b = [0u8; 64];
        spi::read(a, &mut b);
        if b.iter().any(|&x| x != 0xFF) {
            return false;
        }
        a += 64;
    }
    true
}

/// Geometry of the stock volume: 4096-byte sectors, 1 per cluster, 1 reserved,
/// 2 FATs of 2 sectors, 512 root entries, 4096 sectors. Data sectors stay as they are.
static BOOT: [u8; 62] = *b"\xEB\x3C\x90MSDOS5.0\x00\x10\x01\x01\x00\x02\x00\x02\x00\x10\xF8\x02\x00\x01\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x80\x00\x29\xF0\xB1\xCF\xD4ATK-DM40   FAT16   ";

/// Erases sectors 0..8 (boot, FATs, root), writes an empty FAT16.
pub fn format() {
    const FAT1: u32 = 0x1000;
    const FAT2: u32 = 0x3000;
    const ROOT: u32 = 0x5000;
    let mut a = 0;
    while a < 0x9000 {
        spi::erase(a);
        a += 0x1000;
    }
    spi::program(0, &BOOT);
    spi::program(0x1FE, &[0x55, 0xAA]);
    a = FAT1;
    while a < 0x9000 {
        let mut p = [0u8; 256];
        if a == FAT1 || a == FAT2 {
            p[..4].copy_from_slice(&[0xF8, 0xFF, 0xFF, 0xFF]);
        }
        if a == ROOT {
            p[..11].copy_from_slice(b"ATK-DM40   ");
            p[11] = 8;
        }
        spi::program(a, &p);
        a += 256;
    }
}

/// DM40*.BIN, a plain file (no LFN slot, volume label or directory).
pub fn matches(e: &[u8; 32]) -> bool {
    e[0] == b'D' && e[1] == b'M' && e[2] == b'4' && e[3] == b'0' && e[8] == b'B' && e[9] == b'I' && e[10] == b'N' && e[11] & 0x18 == 0
}

/// Read-only long names: the 0x0F entries before a short entry, in the order
/// the root holds them (last part first).
#[cfg(feature = "lfn")]
pub struct Lfn {
    /// Part number expected next; 0 = complete.
    next: u8,
    sum: u8,
    ok: bool,
    /// Cut to the screen width; blanks where nothing was stored.
    pub name: [u8; NAME],
}

/// 32 text columns right of the key labels, less the installed mark.
#[cfg(feature = "lfn")]
pub const NAME: usize = 30;

#[cfg(feature = "lfn")]
impl Lfn {
    pub fn new() -> Lfn {
        Lfn { next: 0, sum: 0, ok: false, name: [b' '; NAME] }
    }

    pub fn reset(&mut self) {
        self.ok = false;
    }

    /// One 0x0F entry. Part N|0x40 starts a name; anything out of sequence
    /// or with another checksum drops it.
    pub fn part(&mut self, e: &[u8; 32]) {
        let seq = e[0] & 0x1F;
        if e[0] & 0x40 != 0 {
            self.ok = true;
            self.next = seq;
            self.sum = e[13];
            self.name = [b' '; NAME];
        }
        if !self.ok || seq == 0 || seq != self.next || e[13] != self.sum {
            self.ok = false;
            return;
        }
        const AT: [u8; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        let mut k = 0;
        while k < 13 {
            let o = AT[k] as usize & 31;
            let u = u16::from_le_bytes([e[o], e[(o + 1) & 31]]);
            // 0x0000 ends the name, 0xFFFF pads the rest of the part.
            if u != 0 && u != 0xFFFF {
                if let Some(c) = self.name.get_mut((seq as usize - 1) * 13 + k) {
                    *c = if crate::lcd::in_font(u) { u as u8 } else { b'?' };
                }
            }
            k += 1;
        }
        self.next = seq - 1;
    }

    /// The collected name belongs to this short entry.
    pub fn valid(&self, e: &[u8; 32]) -> bool {
        let mut s = 0u8;
        let mut i = 0;
        while i < 11 {
            s = s.rotate_right(1).wrapping_add(e[i]);
            i += 1;
        }
        self.ok && self.next == 0 && s == self.sum
    }
}
