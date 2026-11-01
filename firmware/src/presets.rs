//! Presets: named copies of the instrument state (`*SAV` / `*RCL`, the main
//! menu's Presets page; app_presets.rs). `_PRESETS.BIN` in the root of the
//! FAT16 volume, next to `_SETTING.BIN` (setfile.rs) and as hidden from the
//! USB host: the generated volume (vdisk.rs) does not list it.
//!
//! File: `N` slots of one 4 KiB block each (one erase sector, spiflash.rs),
//! made once at boot (all 0xFF) before USB exists, never resized or moved:
//! a save, a rename or a clear rewrites one slot's block in place, no FAT or
//! directory block. Slot layout (little-endian words):
//!   0 magic "DPRE"   1 slot format (FORMAT)   2 persist version
//!   3 payload words n (= persist::words_of(version))   4 info (page, flags)
//!   5..9 the name (NAME_LEN bytes, 0-padded)   9.. the n payload words
//!   then CRC-32 over all of it; the rest of the block 0xFF.
//! The payload is the settings record (persist.rs, app.rs persist_words) at
//! the time of the save: an older record loads zero-extended through
//! `persist::words_of`, as the settings file's, so a preset saved by an
//! older build recalls on a newer one (its newer fields at their defaults).
//! A slot of an unknown format or persist version (a newer build's) or with
//! a bad CRC (a torn write) reads as `Slot::Bad`: not recalled, kept until
//! it is saved over or cleared.
//!
//! The slot headers (state, name, page) are read at boot (`boot`, native.rs
//! pre_main) into a small cache in SRAM; the payload is read on a recall.
//! A torn save loses that one slot (the user saves again); the others are
//! separate blocks.
//!
//! Region C (`.cold`, link-native.x): runs on a tap / a SCPI command only,
//! never while the AD2 stream runs (app_presets.rs refuses then).
//!
//! Host test: tools/fatren-test/run.sh ("presets").

#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use crate::calbackup::{chain_of, crc32, write_file, Source};
use crate::diskbuf::blocks;
use crate::msc::BlockDevice;
use crate::persist::{self, Record, WORDS};

pub const NAME: [u8; 11] = *b"_PRESETSBIN";
/// Preset slots (1..=N in the UI and SCPI).
pub const N: usize = 4;
const BLK: usize = 4096;
pub const SIZE: u32 = (N * BLK) as u32;
const MAGIC: u32 = 0x4552_5044; // "DPRE"
/// Slot format. A later format keeps words 0..4 and adds after the name.
pub const FORMAT: u32 = 1;
pub const NAME_LEN: usize = 16;
/// Header words before the payload.
const HDR: usize = 5 + NAME_LEN / 4;
const _: () = assert!(4 * (HDR + WORDS + 1) <= BLK);

/// Info word: bits 0..7 the page (`PAGES` index), bit 8 the generator
/// output was on at the save (it is recalled OFF), bit 9 the name was given
/// by the user (else made from the state: a save over it makes a new one).
pub const INFO_GEN_ON: u32 = 1 << 8;
pub const INFO_NAMED: u32 = 1 << 9;

// --- The slot format (pure; host-tested) ---------------------------------------

fn get(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

fn put(b: &mut [u8], i: usize, v: u32) {
    b[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
}

/// A name as stored: printable ASCII only (anything else is '?'), at most
/// NAME_LEN bytes, 0-padded; leading / trailing spaces dropped.
pub fn clean_name(s: &[u8]) -> [u8; NAME_LEN] {
    let mut out = [0u8; NAME_LEN];
    let s = trim(s);
    for (d, &c) in out.iter_mut().zip(s.iter()) {
        *d = if (0x20..0x7F).contains(&c) { c } else { b'?' };
    }
    out
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [b' ', r @ ..] = s {
        s = r;
    }
    while let [r @ .., b' '] = s {
        s = r;
    }
    s
}

/// The name's text (up to the first 0).
pub fn name_str(n: &[u8; NAME_LEN]) -> &str {
    let k = n.iter().position(|&c| c == 0).unwrap_or(NAME_LEN);
    core::str::from_utf8(&n[..k]).unwrap_or("?")
}

/// Fill a 4 KiB slot block: header, `n` payload words `word(0..n)`, CRC,
/// 0xFF.
pub fn encode(b: &mut [u8], version: u32, n: usize, info: u32, name: &[u8; NAME_LEN], word: impl Fn(usize) -> u32) {
    b[..BLK].fill(0xFF);
    put(b, 0, MAGIC);
    put(b, 1, FORMAT);
    put(b, 2, version);
    put(b, 3, n as u32);
    put(b, 4, info);
    for i in 0..NAME_LEN / 4 {
        put(b, 5 + i, u32::from_le_bytes([name[4 * i], name[4 * i + 1], name[4 * i + 2], name[4 * i + 3]]));
    }
    for i in 0..n {
        put(b, HDR + i, word(i));
    }
    let c = crc32(&b[..4 * (HDR + n)]);
    put(b, HDR + n, c);
}

/// A slot block's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Slot {
    /// Never written or cleared (no magic).
    Empty = 0,
    Used,
    /// The magic, but a CRC failure (a torn write) or a format / persist
    /// version this build does not know.
    Bad,
}

/// (version, payload words, info, name) of a valid slot block, or its state.
pub fn decode(b: &[u8]) -> Result<(u32, usize, u32, [u8; NAME_LEN]), Slot> {
    if get(b, 0) != MAGIC {
        return Err(Slot::Empty);
    }
    if get(b, 1) != FORMAT {
        return Err(Slot::Bad);
    }
    let version = get(b, 2);
    let n = persist::words_of(version).ok_or(Slot::Bad)?;
    if get(b, 3) as usize != n || crc32(&b[..4 * (HDR + n)]) != get(b, HDR + n) {
        return Err(Slot::Bad);
    }
    let mut name = [0u8; NAME_LEN];
    name.copy_from_slice(&b[20..20 + NAME_LEN]);
    Ok((version, n, get(b, 4), name))
}

/// The preset in a valid slot block: the record zero-extended to `WORDS`.
pub struct Preset {
    pub rec: Record,
    pub info: u32,
    /// (The page shows the cached name; the host test checks this one.)
    #[cfg_attr(target_os = "none", allow(dead_code))]
    pub name: [u8; NAME_LEN],
}

pub fn preset(b: &[u8]) -> Option<Preset> {
    let (version, n, info, name) = decode(b).ok()?;
    let mut words = [0u32; WORDS];
    for (i, w) in words[..n].iter_mut().enumerate() {
        *w = get(b, HDR + i);
    }
    Some(Preset { rec: Record { version, words }, info, name })
}

/// The same block with another name (and the INFO_NAMED bit), CRC redone.
/// False: not a valid slot.
pub fn rename_block(b: &mut [u8], name: &[u8; NAME_LEN]) -> bool {
    let Ok((_, n, info, _)) = decode(b) else { return false };
    put(b, 4, info | INFO_NAMED);
    b[20..20 + NAME_LEN].copy_from_slice(name);
    let c = crc32(&b[..4 * (HDR + n)]);
    put(b, HDR + n, c);
    true
}

// --- State ---------------------------------------------------------------------

/// The file at boot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FileState {
    NotRun = 0,
    Present,
    Created,
    /// No medium / volume, no room, a write failed, or its blocks are not
    /// `N` single-block clusters: no presets.
    NoFile,
}

/// Why a preset operation did nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PErr {
    /// No file (FileState NoFile), or the slot number is out of range.
    NoFile,
    /// The slot holds no valid preset.
    Empty,
    /// The write or its read back failed (the slot may be torn now).
    Write,
}

struct St {
    file: u8,
    lba: [u32; N],
    slot: [u8; N],
    info: [u32; N],
    name: [[u8; NAME_LEN]; N],
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut ST: St = St { file: 0, lba: [0; N], slot: [0; N], info: [0; N], name: [[0; NAME_LEN]; N] };

#[inline(always)]
fn st() -> &'static mut St {
    unsafe { &mut *core::ptr::addr_of_mut!(ST) }
}

fn file_ok() -> bool {
    matches!(st().file, x if x == FileState::Present as u8 || x == FileState::Created as u8)
}

pub fn file_state() -> FileState {
    [FileState::NotRun, FileState::Present, FileState::Created, FileState::NoFile][(st().file as usize).min(3)]
}

/// Slot `k` (0-based) from the cache: state, info word, name.
pub fn slot(k: usize) -> (Slot, u32, &'static str) {
    let s = st();
    if k >= N || !file_ok() {
        return (Slot::Empty, 0, "");
    }
    let state = [Slot::Empty, Slot::Used, Slot::Bad][(s.slot[k] as usize).min(2)];
    (state, s.info[k], if state == Slot::Used { name_str(&s.name[k]) } else { "" })
}

/// The name of slot `k` as stored (Used slots; else all 0).
pub fn name_of(k: usize) -> [u8; NAME_LEN] {
    if slot(k).0 == Slot::Used { st().name[k] } else { [0; NAME_LEN] }
}

fn cache(k: usize, b: &[u8]) {
    let s = st();
    match decode(b) {
        Ok((_, _, info, name)) => {
            s.slot[k] = Slot::Used as u8;
            s.info[k] = info;
            s.name[k] = name;
        }
        Err(e) => {
            s.slot[k] = e as u8;
            s.info[k] = 0;
            s.name[k] = [0; NAME_LEN];
        }
    }
}

// --- Boot ------------------------------------------------------------------------

struct Blank;
impl Source for Blank {
    fn size(&self) -> u32 {
        SIZE
    }
    fn fill(&self, _: u32, buf: &mut [u8]) {
        buf.fill(0xFF);
    }
}

/// The slots' blocks from the file's directory entry and chain: Some(true)
/// found as expected, Some(false) found with another geometry, None absent.
fn locate(dev: &mut dyn BlockDevice) -> Option<bool> {
    let mut cl = [0u16; N];
    let (data, spc, size, n) = chain_of(dev, &NAME, &mut cl)?;
    if size != SIZE {
        return None;
    }
    if spc != 1 || n != N {
        return Some(false);
    }
    for (l, &c) in st().lba.iter_mut().zip(cl.iter()) {
        *l = data + c as u32 - 2;
    }
    Some(true)
}

/// Boot (native.rs pre_main, after setfile; before USB): find the file or
/// make it, read the slot headers.
#[inline(never)]
pub fn boot(dev: &mut dyn BlockDevice) {
    let s = st();
    s.slot = [0; N];
    s.file = match locate(dev) {
        Some(true) => FileState::Present,
        Some(false) => FileState::NoFile,
        None => {
            let r = write_file(dev, NAME, &Blank);
            match (r.status, locate(dev)) {
                (crate::calbackup::Status::Written, Some(true)) => FileState::Created,
                _ => FileState::NoFile,
            }
        }
    } as u8;
    if !file_ok() {
        return;
    }
    let [b, _, _] = blocks();
    for k in 0..N {
        if dev.read_block(s.lba[k], b) {
            cache(k, &b[..]);
        } else {
            s.slot[k] = Slot::Bad as u8;
        }
    }
}

// --- Operations (a tap / SCPI) -------------------------------------------------

/// Write `b` into slot `k`, read back into `rb`, update the cache.
fn write(dev: &mut dyn BlockDevice, k: usize, b: &[u8], rb: &mut [u8]) -> Result<(), PErr> {
    let s = st();
    let ok = dev.write_block(s.lba[k], b) && dev.read_block(s.lba[k], rb) && rb[..] == b[..BLK];
    if ok {
        cache(k, b);
        Ok(())
    } else {
        s.slot[k] = Slot::Bad as u8;
        Err(PErr::Write)
    }
}

/// Save the record `p` (persist::VERSION) into slot `k` with `info` and
/// `name`.
#[inline(never)]
pub fn save(dev: &mut dyn BlockDevice, k: usize, p: &[u32; WORDS], info: u32, name: &[u8; NAME_LEN]) -> Result<(), PErr> {
    if k >= N || !file_ok() {
        return Err(PErr::NoFile);
    }
    let [b, rb, _] = blocks();
    encode(&mut b[..], persist::VERSION, WORDS, info, name, |i| p[i]);
    write(dev, k, &b[..], &mut rb[..])
}

/// The preset in slot `k`.
#[inline(never)]
pub fn load(dev: &mut dyn BlockDevice, k: usize) -> Result<Preset, PErr> {
    if k >= N || !file_ok() {
        return Err(PErr::NoFile);
    }
    let [b, _, _] = blocks();
    if !dev.read_block(st().lba[k], b) {
        return Err(PErr::Write);
    }
    cache(k, &b[..]);
    preset(&b[..]).ok_or(PErr::Empty)
}

/// Give slot `k` another name (the rest of the slot unchanged).
#[inline(never)]
pub fn rename(dev: &mut dyn BlockDevice, k: usize, name: &[u8; NAME_LEN]) -> Result<(), PErr> {
    if k >= N || !file_ok() {
        return Err(PErr::NoFile);
    }
    let [b, rb, _] = blocks();
    if !dev.read_block(st().lba[k], b) {
        return Err(PErr::Write);
    }
    if !rename_block(&mut b[..], name) {
        return Err(PErr::Empty);
    }
    write(dev, k, &b[..], &mut rb[..])
}

/// Clear slot `k` (the block erased: Empty).
#[inline(never)]
pub fn clear(dev: &mut dyn BlockDevice, k: usize) -> Result<(), PErr> {
    if k >= N || !file_ok() {
        return Err(PErr::NoFile);
    }
    let [b, rb, _] = blocks();
    b.fill(0xFF);
    write(dev, k, &b[..], &mut rb[..])
}

// --- SCPI MEMory:STATe:NAME's name (scpi.rs parses, app_presets.rs uses) -----

/// The name of the last MEMory:STATe:NAME set (as sent, before
/// `clean_name`); one more byte than a name holds: a longer one is refused
/// by the parser.
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut SCPI_NAME: ([u8; NAME_LEN], u8) = ([0; NAME_LEN], 0);

/// The parser's copy (at most NAME_LEN bytes). Region B (inlined into the
/// parser): a byte copy.
#[inline(always)]
pub fn set_scpi_name(s: &[u8]) {
    let n = s.len().min(NAME_LEN);
    unsafe {
        let p = &mut *core::ptr::addr_of_mut!(SCPI_NAME);
        p.0[..n].copy_from_slice(&s[..n]);
        p.1 = n as u8;
    }
}

pub fn scpi_name() -> &'static [u8] {
    unsafe {
        let p = &*core::ptr::addr_of!(SCPI_NAME);
        &p.0[..p.1 as usize]
    }
}
