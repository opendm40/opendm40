//! Memory probe commands (cargo feature `memprobe`, debug builds only):
//! peek / poke / RAM test / load timing / clock gate, to look for memory the
//! GD32F427 has beyond its datasheet (docs/hidden-memory.md).
//!
//! Every access goes through `probe_rw` (asm below): a bus fault on it does
//! not hang the meter. With this feature the HardFault vector is
//! `probe_fault` (src/shim.rs): while `PROBE_ACTIVE` holds `ARMED` it
//! records CFSR / HFSR / BFAR, clears them, and resumes at `probe_rw_land`;
//! otherwise it is the plain `b .` of `default_handler`. PRIMASK is set
//! outside an AD2 stream, so a BusFault escalates to HardFault: one vector
//! is enough. A store is followed by `dsb`, so an imprecise write fault is
//! taken inside the armed window.
//!
//! SCPI (src/scpi.rs, `DBG:` + these; addresses decimal or 0x hex):
//!   DBG:PEEK? addr[,n]        n (1..32) words from addr (word aligned):
//!                             0xXXXXXXXX each, or F<cfsr>/<bfar> (hex) for
//!                             a word whose read faulted
//!   DBG:POKE[?] addr,value    one word, only inside `WRITABLE`; replies the
//!                             read-back or F<cfsr>/<bfar> (a host reads the
//!                             reply of the query form only)
//!   DBG:MASK? addr            which bits of a register inside `WRITABLE`
//!                             hold data: <original>,<read after writing
//!                             all ones>,<read after writing 0>; the original
//!                             is written back
//!   DBG:MTEST? addr,words[,seed]
//!                             RAM test inside `WRITABLE` (words <= 16384):
//!                             alias check first (one word written, every
//!                             possible mirror in the real SRAM / TCM read
//!                             before and after, restored), then pattern,
//!                             inverse, pattern again (left in place for
//!                             MVER?). Reply: <ok>,<bad>,<first bad addr>,
//!                             <faults>,<alias addr or 0>
//!   DBG:MVER? addr,words[,seed]
//!                             verify only (retention): <ok>,<bad>,<first bad>,
//!                             <faults>
//!   DBG:MTIME? addr,n[,stride]
//!                             n (1..4096) word loads at addr + i*stride
//!                             (default 4), timed with DWT_CYCCNT, interrupts
//!                             masked: <first>,<min>,<max>,<total> cycles per
//!                             load including the counter read (~calibrate
//!                             against TCM). The first and the last address
//!                             are read through probe_rw first: a fault
//!                             refuses the run (F...).
//!   DBG:MCLK[?] name,0|1      clock gate of a block the probes need, by
//!                             name only (IPA / USBHS / ENET: RCU_AHB1EN bit
//!                             23 / 29 / 25, TLI: RCU_APB2EN bit 26, CAN0 /
//!                             CAN1: RCU_APB1EN bit 25 / 26); replies the register
//!                             after the write. Nothing else in the RCU.

use core::arch::global_asm;
use core::ptr::{read_volatile, write_volatile};

// `PROBE_ACTIVE` = 0x50524F31 (armed) while a probe access is in flight,
// 0x50524F32 after the handler caught a fault on it (both literals in the
// asm). Anything else (0 after .bss init, or garbage before it): a fault is
// genuine and hangs as before.

#[no_mangle]
static mut PROBE_ACTIVE: u32 = 0;
#[no_mangle]
static mut PROBE_CFSR: u32 = 0;
#[no_mangle]
static mut PROBE_HFSR: u32 = 0;
#[no_mangle]
static mut PROBE_BFAR: u32 = 0;

global_asm!(
    r#"
    .section .text.a2.probe_fault, "ax", %progbits
    .thumb_func
    .global probe_fault
probe_fault:
    ldr   r0, =PROBE_ACTIVE
    ldr   r1, [r0]
    ldr   r2, =0x50524F31
    cmp   r1, r2
    bne   2f
    ldr   r2, =0xE000ED38      /* BFAR before CFSR is cleared (the device */
    ldr   r3, [r2]             /* read 0xE000ED38 when it came after) */
    ldr   r1, =PROBE_BFAR
    str   r3, [r1]
    ldr   r2, =0xE000ED28      /* CFSR: read, write 1s back to clear */
    ldr   r3, [r2]
    str   r3, [r2]
    ldr   r1, =PROBE_CFSR
    str   r3, [r1]
    ldr   r2, =0xE000ED2C      /* HFSR */
    ldr   r3, [r2]
    str   r3, [r2]
    ldr   r1, =PROBE_HFSR
    str   r3, [r1]
    ldr   r1, =0x50524F32
    str   r1, [r0]
    tst   lr, #4
    ite   eq
    mrseq r0, msp
    mrsne r0, psp
    ldr   r1, =probe_rw_land
    bic   r1, r1, #1
    str   r1, [r0, #24]        /* stacked PC (same offset with an FP frame) */
    bx    lr
2:  b     2b
    .ltorg

    .section .text.a2.probe_rw, "ax", %progbits
    .thumb_func
    .global probe_rw
    /* u64 probe_rw(addr r0, value r1, write r2): r0 = word read back,
       r1 = 1 if the access faulted. r4 survives the exception (the
       hardware frame is r0-r3, r12, lr, pc, xpsr; the handler uses r0-r3). */
probe_rw:
    push  {{r4, lr}}
    ldr   r4, =PROBE_ACTIVE
    ldr   r3, =0x50524F31
    str   r3, [r4]
    dsb
    cbz   r2, 1f
    str   r1, [r0]
    dsb
1:  ldr   r0, [r0]
    dsb
    .thumb_func
    .global probe_rw_land
probe_rw_land:
    ldr   r3, [r4]
    ldr   r2, =0x50524F32
    movs  r1, #0
    cmp   r3, r2
    it    eq
    moveq r1, #1
    movs  r3, #0
    str   r3, [r4]
    pop   {{r4, pc}}
    .ltorg
"#
);

extern "C" {
    fn probe_rw(addr: u32, value: u32, write: u32) -> u64;
}

/// Read one word: Ok(value) or Err((cfsr, bfar)).
pub fn peek(addr: u32) -> Result<u32, (u32, u32)> {
    let r = unsafe { probe_rw(addr, 0, 0) };
    if r >> 32 != 0 {
        Err(unsafe { (PROBE_CFSR, PROBE_BFAR) })
    } else {
        Ok(r as u32)
    }
}

/// Write one word and read it back (caller checked `writable`).
fn poke(addr: u32, v: u32) -> Result<u32, (u32, u32)> {
    let r = unsafe { probe_rw(addr, v, 1) };
    if r >> 32 != 0 {
        Err(unsafe { (PROBE_CFSR, PROBE_BFAR) })
    } else {
        Ok(r as u32)
    }
}

/// The only ranges POKE / MTEST may write ([start, end)). Reasoned in
/// docs/hidden-memory.md; none is used by opendm40. Never the backup SRAM
/// (the settings record, `.bkp_bss`, the boot words) or the USBHS FIFO RAM
/// (`.usbhs_bss`): the running firmware keeps live state in both.
const WRITABLE: [(u32, u32); 12] = [
    (0x2003_0000, 0x200B_0000), // ADDSRAM (UM: 512 KB on "some" GD32F4xx)
    (0x4002_B000, 0x4002_B040), // IPA registers (presence check)
    (0x4001_6800, 0x4001_6840), // TLI registers (presence check)
    // ENET (unused, no PHY): data-only registers, never MAC_CFG / DMA_CTL.
    (0x4002_8008, 0x4002_8010), // ENET_MAC_HLH, HLL (hash list)
    (0x4002_801C, 0x4002_8020), // ENET_MAC_VLT
    (0x4002_8040, 0x4002_8060), // ENET_MAC_ADDR0H..ADDR3L
    (0x4002_8710, 0x4002_8724), // ENET_PTP_TSUH, TSUL, TSADDEND, ETH, ETL
    (0x4002_900C, 0x4002_9014), // ENET_DMA_RDTADDR, TDTADDR
    // CAN0 (unused): transmit mailbox data (never CAN_TMIx: its bit 0
    // starts a transmission) and the 28 filter banks (CAN0 only, shared
    // with CAN1; writable while CAN_FCTL.FLD = 1, its reset value).
    (0x4000_6588, 0x4000_6590), // CAN_TMDATA0/1 mailbox 0
    (0x4000_6598, 0x4000_65A0), // mailbox 1
    (0x4000_65A8, 0x4000_65B0), // mailbox 2
    (0x4000_6640, 0x4000_6720), // CAN_F0DATA0..F27DATA1
];

fn writable(a: u32, words: u32) -> bool {
    let end = a as u64 + words as u64 * 4;
    a & 3 == 0 && words > 0 && WRITABLE.iter().any(|&(s, e)| a >= s && end <= e as u64)
}

/// Possible mirrors of `a` in the real SRAM (0x20000000..0x20030000, any
/// power-of-two or 192 KiB wrap) and in the TCM.
fn mirrors(a: u32, out: &mut [u32; 16]) -> usize {
    let mut n = 0;
    if !(0x2003_0000..0x200B_0000).contains(&a) {
        return 0;
    }
    for k in 1..=11u32 {
        let m = a.wrapping_sub(k * 0x1_0000);
        if (0x2000_0000..0x2003_0000).contains(&m) && n < out.len() {
            out[n] = m;
            n += 1;
        }
    }
    for wrap in [0x4_0000u32, 0x8_0000] {
        let m = 0x2000_0000 + (a - 0x2000_0000) % wrap;
        if m < 0x2003_0000 && !out[..n].contains(&m) && n < out.len() {
            out[n] = m;
            n += 1;
        }
    }
    if n < out.len() {
        out[n] = 0x1000_0000 + (a & 0xFFFF);
        n += 1;
    }
    n
}

/// Write `a` with two values; a mirror whose content follows is reported
/// (and restored). Returns the mirror address or 0.
fn alias_check(a: u32) -> u32 {
    let mut m = [0u32; 16];
    let n = mirrors(a, &mut m);
    let mut save = [0u32; 16];
    for i in 0..n {
        save[i] = unsafe { read_volatile(m[i] as *const u32) };
    }
    let orig = peek(a).unwrap_or(0);
    let mut hit = 0;
    for v in [0xA5C3_5A3Cu32 ^ a, !(0xA5C3_5A3Cu32 ^ a)] {
        if poke(a, v).is_err() {
            break;
        }
        for i in 0..n {
            let now = unsafe { read_volatile(m[i] as *const u32) };
            if now == v && save[i] != v {
                hit = m[i];
            }
            if now != save[i] {
                unsafe { write_volatile(m[i] as *mut u32, save[i]) };
            }
        }
        if hit != 0 {
            break;
        }
    }
    let _ = poke(a, orig);
    hit
}

fn pat(a: u32, seed: u32) -> u32 {
    a.wrapping_mul(0x9E37_79B1) ^ seed
}

/// (ok, bad, first bad, faults)
fn fill_check(a: u32, words: u32, seed: u32, inv: bool, write: bool) -> (u32, u32, u32, u32) {
    let (mut ok, mut bad, mut first, mut faults) = (0, 0, 0, 0);
    if write {
        for i in 0..words {
            let x = a + 4 * i;
            let v = if inv { !pat(x, seed) } else { pat(x, seed) };
            if unsafe { probe_rw(x, v, 1) } >> 32 != 0 {
                faults += 1;
            }
        }
    }
    for i in 0..words {
        let x = a + 4 * i;
        let v = if inv { !pat(x, seed) } else { pat(x, seed) };
        match peek(x) {
            Ok(r) if r == v => ok += 1,
            Ok(_) => {
                if bad == 0 {
                    first = x;
                }
                bad += 1;
            }
            Err(_) => faults += 1,
        }
    }
    (ok, bad, first, faults)
}

const DWT_CYCCNT: u32 = 0xE000_1004;

/// (first, min, max, total) cycles per load.
#[inline(never)]
#[link_section = ".text.a2.memprobe_time"]
fn time_loads(a: u32, n: u32, stride: u32) -> (u32, u32, u32, u32) {
    let (mut first, mut lo, mut hi, mut tot) = (0, u32::MAX, 0, 0u32);
    // is_active() = interrupts were enabled (PRIMASK clear): only then
    // re-enable them (opendm40 runs with PRIMASK set outside an AD2 stream).
    let was_enabled = cortex_m::register::primask::read().is_active();
    cortex_m::interrupt::disable();
    for i in 0..n {
        let p = (a + i * stride) as *const u32;
        unsafe {
            let t0 = read_volatile(DWT_CYCCNT as *const u32);
            let _ = read_volatile(p);
            let t1 = read_volatile(DWT_CYCCNT as *const u32);
            let d = t1.wrapping_sub(t0);
            if i == 0 {
                first = d;
            }
            lo = lo.min(d);
            hi = hi.max(d);
            tot = tot.wrapping_add(d);
        }
    }
    if was_enabled {
        unsafe { cortex_m::interrupt::enable() };
    }
    (first, lo, hi, tot)
}

fn num(t: &[u8]) -> Option<u32> {
    let t = trim(t);
    if t.len() > 2 && t[0] == b'0' && (t[1] | 0x20) == b'x' {
        let mut v: u32 = 0;
        for &c in &t[2..] {
            let d = (c as char).to_digit(16)?;
            v = v.checked_mul(16)?.checked_add(d)?;
        }
        Some(v)
    } else {
        core::str::from_utf8(t).ok()?.parse().ok()
    }
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', r @ ..] = s {
        s = r;
    }
    while let [r @ .., b' ' | b'\t'] = s {
        s = r;
    }
    s
}

fn args<const N: usize>(a: &[u8]) -> [Option<u32>; N] {
    let mut v = [None; N];
    for (d, t) in v.iter_mut().zip(a.split(|&b| b == b',')) {
        *d = num(t);
    }
    v
}

fn hex(s: &mut crate::scpi::Scpi, v: u32) {
    let mut t = [0u8; 10];
    t[0] = b'0';
    t[1] = b'x';
    for i in 0..8 {
        t[2 + i] = b"0123456789ABCDEF"[(v >> (28 - 4 * i) & 0xF) as usize];
    }
    s.put_bytes(&t);
}

fn put_fault(s: &mut crate::scpi::Scpi, (cfsr, bfar): (u32, u32)) {
    s.put_bytes(b"F");
    hex(s, cfsr);
    s.put_bytes(b"/");
    hex(s, bfar);
}

fn put_list(s: &mut crate::scpi::Scpi, v: &[u32]) {
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.put_bytes(b",");
        }
        s.put_u32(*x);
    }
}

/// The `DBG:<sub>` commands of this module; false = not one of them.
#[inline(never)]
pub fn scpi(s: &mut crate::scpi::Scpi, sub: &[u8], query: bool, arg: &[u8]) -> bool {
    match (sub, query) {
        (b"PEEK", true) => {
            let [a, n] = args::<2>(arg);
            let (Some(a), n) = (a, n.unwrap_or(1).clamp(1, 32)) else { s.put_bytes(b"ERR"); return true };
            if a & 3 != 0 {
                s.put_bytes(b"ERR align");
                return true;
            }
            for i in 0..n {
                if i > 0 {
                    s.put_bytes(b",");
                }
                match peek(a + 4 * i) {
                    Ok(v) => hex(s, v),
                    Err(f) => put_fault(s, f),
                }
            }
        }
        (b"MASK", true) => {
            let [a] = args::<1>(arg);
            match a {
                Some(a) if writable(a, 1) => {
                    let r = (peek(a), poke(a, u32::MAX), poke(a, 0));
                    match r {
                        (Ok(o), Ok(ones), Ok(zeros)) => {
                            let _ = poke(a, o);
                            for (i, v) in [o, ones, zeros].into_iter().enumerate() {
                                if i > 0 {
                                    s.put_bytes(b",");
                                }
                                hex(s, v);
                            }
                        }
                        (Err(f), ..) | (_, Err(f), _) | (.., Err(f)) => put_fault(s, f),
                    }
                }
                _ => s.put_bytes(b"ERR range"),
            }
        }
        (b"POKE", _) => {
            let [a, v] = args::<2>(arg);
            match (a, v) {
                (Some(a), Some(v)) if writable(a, 1) => match poke(a, v) {
                    Ok(r) => hex(s, r),
                    Err(f) => put_fault(s, f),
                },
                _ => s.put_bytes(b"ERR range"),
            }
        }
        (b"MTEST", true) | (b"MVER", true) => {
            let [a, w, seed] = args::<3>(arg);
            let (Some(a), Some(w)) = (a, w) else { s.put_bytes(b"ERR"); return true };
            let seed = seed.unwrap_or(0x5EED_0001);
            if w > 16384 || !writable(a, w) {
                s.put_bytes(b"ERR range");
                return true;
            }
            if sub == b"MVER" {
                let (ok, bad, first, f) = fill_check(a, w, seed, false, false);
                put_list(s, &[ok, bad]);
                s.put_bytes(b",");
                hex(s, first);
                s.put_bytes(b",");
                s.put_u32(f);
                return true;
            }
            let alias = alias_check(a).max(alias_check(a + 4 * (w - 1)));
            let (mut ok, mut bad, mut first, mut f) = (0, 0, 0, 0);
            if alias == 0 {
                for inv in [false, true, false] {
                    let r = fill_check(a, w, seed, inv, true);
                    ok += r.0;
                    if bad == 0 && r.1 != 0 {
                        first = r.2;
                    }
                    bad += r.1;
                    f += r.3;
                }
            }
            put_list(s, &[ok, bad]);
            s.put_bytes(b",");
            hex(s, first);
            s.put_bytes(b",");
            s.put_u32(f);
            s.put_bytes(b",");
            hex(s, alias);
        }
        (b"MTIME", true) => {
            let [a, n, st] = args::<3>(arg);
            let (Some(a), n, st) = (a, n.unwrap_or(256).clamp(1, 4096), st.unwrap_or(4)) else {
                s.put_bytes(b"ERR");
                return true;
            };
            if a & 3 != 0 || st & 3 != 0 {
                s.put_bytes(b"ERR align");
                return true;
            }
            for x in [a, a + (n - 1) * st] {
                if let Err(f) = peek(x) {
                    put_fault(s, f);
                    return true;
                }
            }
            let (first, lo, hi, tot) = time_loads(a, n, st);
            put_list(s, &[first, lo, hi, tot]);
        }
        (b"MCLK", _) => {
            let mut it = arg.split(|&b| b == b',');
            let name = trim(it.next().unwrap_or(b""));
            let on = it.next().and_then(num).map(|v| v != 0);
            const AHB1EN: u32 = 0x4002_3830;
            const APB2EN: u32 = 0x4002_3844;
            const APB1EN: u32 = 0x4002_3840;
            let (reg, bit) = match name {
                b"IPA" => (AHB1EN, 23),
                b"USBHS" => (AHB1EN, 29),
                b"TLI" => (APB2EN, 26),
                b"ENET" => (AHB1EN, 25),
                b"CAN0" => (APB1EN, 25),
                b"CAN1" => (APB1EN, 26),
                _ => {
                    s.put_bytes(b"ERR name");
                    return true;
                }
            };
            let Some(on) = on else { s.put_bytes(b"ERR"); return true };
            unsafe {
                if on {
                    crate::mmio::modify32(reg, 0, 1 << bit);
                } else {
                    crate::mmio::modify32(reg, 1 << bit, 0);
                }
                hex(s, crate::mmio::r32(reg));
            }
        }
        _ => return false,
    }
    true
}
