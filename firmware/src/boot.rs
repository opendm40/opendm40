//! Native boot decision (the "shim"), called by the native reset path
//! (src/shim.rs).
//!
//! Runs before .data/.bss exist: no statics are touched until `native::start`
//! has initialised RAM (only `set_info`/`info` below use one, afterwards).
//! Every wait is bounded and feeds the FWDGT.
//!
//! Recovery zeroes the loader marker (src/recovery.rs): a one-way door into
//! the vendor updater. The shim takes it only on the backup-SRAM request
//! word, which nothing sets now: no key hold and no hang count lead there
//! (owner 2026-10-29). The ways into the loader are DBG:UPDATE / DBG:RECOVER,
//! the on-meter update and the TX-GND short (docs/native-boot.md).
//!
//! Decision, in this order (docs/native-boot.md "Shim (reset path)"):
//!   backup-SRAM request word == REQ_MAGIC     -> recovery (Requested; consumed)
//!   clock init failed (HXTAL and IRC16M), or a GENUINE watchdog reset after
//!     our own normal boot: crash counter += 1 (shown by DBG:BOOT?);
//!       clock failed, counter < CLOCK_RETRIES -> SYSRESETREQ, retry (ClockFail)
//!       clock failed                          -> normal   (ClockFail)
//!       else                                  -> normal   (WdtRetry)
//!   FWDGTRSTF, previous boot was recovery     -> normal   (WdtAfterRecovery)
//!   FWDGTRSTF otherwise                       -> normal   (WdtStale)
//!   otherwise                                 -> normal
//! "Genuine" = FWDGTRSTF set and neither SWRSTF nor PORRSTF: the shim clears
//! the flags on every native boot, so a hang during a native session shows
//! FWDGT alone, while a loader install adds SW and a power-on adds POR (a
//! stale FWDGT left over from a stock session then does not count). The crash
//! counter is cleared by `mark_healthy` after 60 s of main loop and on entry
//! to recovery.
//!
//! The backlight comes on at once: feedback for the power-on press.

use crate::clock::{self, Src};
use crate::mmio::{modify32, r32, w32};

pub const RCU_RSTSCK: u32 = 0x4002_3874;
/// RCU_RSTSCK (GD32F4xx RCU layout, = STM32 RCC_CSR): write 1 to clear flags.
pub const RSTFC: u32 = 1 << 24;
#[allow(dead_code)]
pub const BORRSTF: u32 = 1 << 25;
#[allow(dead_code)]
pub const EPRSTF: u32 = 1 << 26;
pub const PORRSTF: u32 = 1 << 27;
#[allow(dead_code)]
pub const SWRSTF: u32 = 1 << 28;
pub const FWDGTRSTF: u32 = 1 << 29;

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const PMU_CTL: u32 = 0x4000_7000;
const PMU_CS: u32 = 0x4000_7004;
#[cfg_attr(not(any(feature = "recovery-file", feature = "recovery-usb")), allow(dead_code))]
const GPIOC: u32 = 0x4002_0800;
const GPIOA: u32 = 0x4002_0000;
const GPIOD: u32 = 0x4002_0C00;
const GPIOE: u32 = 0x4002_1000;
const SCB_AIRCR: u32 = 0xE000_ED0C;

/// PD6 (power key) level when pressed: 1. Confirmed in the stock V2.6 code:
/// FUN_0800b7e0 sets PD6 input with pull-down (gpio_mode_set(GPIOD 0x40020C00,
/// 0, PUPD 2, 0x40)); the TIM7 1 kHz ISR FUN_0800b87c reads IDR & 0x40
/// (FUN_08006000) and raises the key-down event (state 0 -> 1 -> 2, flag bit 0)
/// only while the bit reads 1 (app_decompile.c:6524-6556). Idle measured 0.
pub const PD6_PRESSED_HIGH: bool = true;
/// A clock failure retries the boot (SYSRESETREQ) while the crash counter is
/// below this; then opendm40 runs on the clock it has (never recovery).
pub const CLOCK_RETRIES: u32 = 2;
/// Main-loop time after which `mark_healthy` clears the crash counter.
// (60 s of RUNNING main loop since this boot, not 60 s between two resets)
pub const HEALTHY_MS: u64 = 60_000;

/// Backup SRAM (0x40024000, 4 KiB). persist.rs uses the start: record v4 =
/// magic, version, 40 words, CRC = 43 words, 0x40024000..0x400240AB; these
/// boot words are the last four of the 4 KiB, 0x40024FF0..0x40024FFF (no
/// overlap; checked at compile time in persist.rs).
/// Crash counter (`CRASH_TAG | n`).
pub const BKP_CRASH: u32 = 0x4002_4FF0;
const CRASH_TAG: u32 = 0x4352_0000; // "CR"
/// Last recovery's result (0x54520010 marker zeroed, 0x54520011 failed;
/// recovery-file: `0x5452_0000 | fatren::Trigger`).
pub const BKP_TRIG: u32 = 0x4002_4FF4;
pub const BKP_REQ: u32 = 0x4002_4FF8;
pub const BKP_LAST: u32 = 0x4002_4FFC;
pub const REQ_MAGIC: u32 = 0x5245_4356; // "VCER" LE: recovery requested
const LAST_NORMAL: u32 = 0x4C4D_524E; // "NRML"
const LAST_RECOVERY: u32 = 0x5256_4352; // "RCVR"

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Recovery,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    None,
    WdtAfterRecovery,
    WdtStale,
    WdtRetry,
    Requested,
    ClockFail,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClockState {
    Hxtal,
    Irc16m,
    Failed,
}

#[derive(Clone, Copy)]
pub struct BootInfo {
    /// RCU_RSTSCK as found (before RSTFC), and as read back after clearing.
    pub rstsck: u32,
    pub rstsck_after: u32,
    pub mode: Mode,
    pub reason: Reason,
    pub clock: ClockState,
    /// Previous boot's mode word (BKP_LAST) as found.
    pub last: u32,
}

#[inline(always)]
pub fn feed() {
    crate::wdt::kick();
}

/// Power key pressed (PD6, GPIOD clock + input mode set by `pd6_input`).
#[inline(always)]
pub fn pd6_pressed() -> bool {
    (unsafe { r32(GPIOD + 0x10) } & (1 << 6) != 0) == PD6_PRESSED_HIGH
}

/// PD6 input WITH pull-down, as the stock key init does
/// (app_decompile.c:6482, gpio_mode_set(GPIOD, input, PUPD 2, 0x40)). After a
/// reset the ROM/loader leave PD6 floating (PUPD 00); without the pull a
/// floating 1 would read as a held key (recovery-usb reads the key without
/// board::init). Then let the pull settle.
fn pd6_input() {
    unsafe {
        modify32(RCU_AHB1EN, 0, 1 << 3); // GPIOD
        modify32(GPIOD + 0x0C, 3 << 12, 2 << 12); // PUPD: pull-down
        modify32(GPIOD, 3 << 12, 0); // input
    }
    // >= 10 us at any clock up to 200 MHz (internal pull ~40 kOhm, pin few pF).
    crate::mmio::spin(4000);
}

/// Backlight full on as plain GPIO (PA6 high): instant feedback that the unit
/// is on, long before the LCD is initialised (board.rs later hands PA6 to
/// TIM13 with the duty at 0 until App::new sets the level).
fn backlight(on: bool) {
    unsafe {
        modify32(RCU_AHB1EN, 0, 1 << 0); // GPIOA
        w32(GPIOA + 0x18, if on { 1 << 6 } else { 1 << (6 + 16) });
        modify32(GPIOA + 0x04, 1 << 6, 0);
        modify32(GPIOA, 3 << 12, 1 << 12);
    }
}

/// Power hold: PE2 = 1. The ROM already raises PE2 and PC4 first thing on
/// every reset (0x08000574), the stock app keeps PE2 (FUN_0800b410); this
/// re-asserts it. PE2 is the DMM analog supply and, with PC4 (scope supply),
/// what keeps the unit on: the stock power-off drops both. Output level is set
/// before the mode so the pin never drives low.
pub fn latch_on() {
    unsafe {
        modify32(RCU_AHB1EN, 0, 1 << 4); // GPIOE
        w32(GPIOE + 0x18, 1 << 2);
        modify32(GPIOE + 0x04, 1 << 2, 0);
        modify32(GPIOE + 0x08, 0, 3 << 4);
        modify32(GPIOE + 0x0C, 3 << 4, 0);
        modify32(GPIOE, 3 << 4, 1 << 4);
    }
}

/// Power off exactly as app.rs power_off(): PE2 = 0, PC4 = 0. On battery the
/// unit switches off once the power key is released; on USB power it may stay
/// up (then nothing else happens; the caller keeps running).
#[cfg_attr(not(any(feature = "recovery-file", feature = "recovery-usb")), allow(dead_code))]
pub fn latch_off() {
    unsafe {
        modify32(RCU_AHB1EN, 0, (1 << 2) | (1 << 4));
        w32(GPIOE + 0x18, 1 << (2 + 16));
        w32(GPIOC + 0x18, 1 << (4 + 16));
    }
}

/// Backup SRAM access (as persist::init): PMU clock, BKPWEN, BKPSRAM clock,
/// backup regulator on.
/// Region B (not the boot module's .cold): `mark_healthy` calls it once,
/// 60 s after the start, maybe while a stream runs.
#[cfg_attr(target_os = "none", link_section = ".text.bkp_enable")]
pub fn bkp_enable() {
    unsafe {
        modify32(RCU_APB1EN, 0, 1 << 28);
        modify32(PMU_CTL, 0, 1 << 8);
        modify32(RCU_AHB1EN, 0, 1 << 18);
        modify32(PMU_CS, 0, 1 << 9);
    }
}

// --- The 12-byte loader marker at 0x08008000 ------------------------------
//
// The ROM (0x08000980) and the FS-loader (0x080a3c88) launch the app only if
// this record is valid: word2 type 0x0001 (bytes 10/11 = 01 00), word0 low
// half == ~high half == CRC16 of word1 || word2, and the body CRC matches. The
// loader writes it last after a good install. Zeroing it (what the loader
// itself does first when it finds any dm40*.bin, 0x080a1815) makes both
// reject the app on ANY reset, cold or warm, independent of PA2 (the debug
// header's TX pad, the hardware boot strap: hw/README.md), so the loader
// stays in its update screen with its own working USB disk until a valid
// dm40*.bin is installed (stock/re/docs/fsloader-install-trigger.md, "Entry without a file: zeroing the marker").

pub const MARKER: u32 = 0x0800_8000;
const FMC_KEY: u32 = 0x4002_3C04;
const FMC_STAT: u32 = 0x4002_3C0C;
const FMC_CTL: u32 = 0x4002_3C10;
const FMC_UNLOCK1: u32 = 0x4567_0123;
const FMC_UNLOCK2: u32 = 0xCDEF_89AB;
/// FMC_STAT: BUSY bit 16; error bits as the loader tests them (OPERR 1,
/// WPERR 4, PGMERR 5, PGAERR 6, PGSERR 7, RDDERR 8). The loader clears 0xD3.
const STAT_BUSY: u32 = 1 << 16;
const STAT_ERR: u32 = 0x1F2;
/// Clear every error bit we test (the loader writes 0xD3, which leaves
/// PGMERR bit 5 set if it was).
const STAT_CLEAR: u32 = 0x1F3;
/// FMC_CTL: PG bit 0, PSZ bits 9:8 (0b10 = 32-bit), LK bit 31.
const CTL_PG: u32 = 1;
const CTL_PSZ_MASK: u32 = 0x300;
const CTL_PSZ_32: u32 = 0x200;
const CTL_LK: u32 = 1 << 31;

/// The three marker words.
pub fn marker() -> [u32; 3] {
    unsafe { [r32(MARKER), r32(MARKER + 4), r32(MARKER + 8)] }
}

/// The marker passes the ROM/loader header test (type 1, complement, CRC;
/// fatren::header_valid). The body CRC (also checked by them) is not
/// recomputed here.
pub fn marker_valid() -> bool {
    let mut h = [0u8; 12];
    for (d, w) in h.chunks_mut(4).zip(marker()) {
        d.copy_from_slice(&w.to_le_bytes());
    }
    crate::fatren::header_valid(&h)
}

fn fmc_wait() -> bool {
    let mut n = 0u32;
    while unsafe { r32(FMC_STAT) } & STAT_BUSY != 0 {
        n += 1;
        if n & 0xFF == 0 {
            feed();
        }
        if n >= 0x10_0000 {
            return false; // a word program takes ~16 us; this is > 100 ms
        }
    }
    true
}

/// FMC status of the last `zero_marker` (error bits, 0x8000_0000 = timeout).
static mut FMC_LAST: u32 = 0;

/// Program the 12 bytes at 0x08008000..0x0800800B to 0x00 (no erase: only
/// 1 -> 0 bit changes, as the loader does), relock. Touches no other flash
/// address. The read-back is the result: true iff all three words read 0
/// (FMC error flags are only recorded, see `fmc_last`). Runs after RAM init.
pub fn zero_marker() -> bool {
    feed();
    let mut st = 0u32;
    unsafe {
        if r32(FMC_CTL) & CTL_LK != 0 {
            w32(FMC_KEY, FMC_UNLOCK1);
            w32(FMC_KEY, FMC_UNLOCK2);
        }
        if !fmc_wait() {
            st |= 0x8000_0000;
        } else {
            w32(FMC_STAT, STAT_CLEAR);
            modify32(FMC_CTL, CTL_PSZ_MASK, CTL_PSZ_32);
            modify32(FMC_CTL, 0, CTL_PG);
            for i in 0..3u32 {
                let a = MARKER + 4 * i;
                if r32(a) != 0 {
                    w32(a, 0);
                    core::arch::asm!("dsb");
                    if !fmc_wait() {
                        st |= 0x8000_0000;
                        break;
                    }
                    st |= r32(FMC_STAT) & STAT_ERR;
                }
                feed();
            }
            modify32(FMC_CTL, CTL_PG, 0);
        }
        modify32(FMC_CTL, 0, CTL_LK);
        FMC_LAST = st;
    }
    marker() == [0, 0, 0]
}

/// Error bits / timeout of the last `zero_marker` (0 = clean).
pub fn fmc_last() -> u32 {
    unsafe { FMC_LAST }
}

/// DBG:UPDATE / DBG:RECOVER (native): forget the previous-mode word (so a
/// later stale watchdog flag cannot count as our crash, review H3) and reset
/// the crash counter (so a count left from this session cannot survive the
/// stock interlude and turn the first hang after the reinstall into recovery,
/// review N2), then zero the marker. The caller resets (deferred SYSRESETREQ)
/// into the loader's update screen. Returns the read-back result.
pub fn enter_loader_update() -> bool {
    bkp_enable();
    unsafe {
        w32(BKP_LAST, 0);
        w32(BKP_CRASH, CRASH_TAG);
    }
    zero_marker()
}

/// Recovery: remember what fatren::make_trigger did (shown by DBG:BOOT?).
pub fn record_trigger(code: u32) {
    bkp_enable();
    unsafe { w32(BKP_TRIG, 0x5452_0000 | code) };
}

// (BKP_REQ = REQ_MAGIC before a reset asks the shim for recovery. No caller
// sets it now: DBG:RECOVER zeroes the loader marker instead; kept as the
// in-app recovery request for later.)

/// Clear the latched reset flags and reset by SYSRESETREQ (SW reset: the shim
/// then boots normally unless a request word is set).
pub fn sysreset() -> ! {
    unsafe {
        modify32(RCU_RSTSCK, 0, RSTFC);
        core::arch::asm!("dsb");
        w32(SCB_AIRCR, 0x05FA_0004);
    }
    loop {
        feed();
    }
}

/// The whole shim sequence.
#[link_section = ".shim"]
#[inline(never)]
pub fn shim() -> BootInfo {
    // (b) watchdog first: from here on any hang resets into this shim.
    crate::wdt::start::fwdgt();
    // Keep the unit powered before anything slow (the key may be released),
    // and light the backlight at once (feedback for the power-on press).
    latch_on();
    backlight(true);
    // (c) latch and clear the sticky reset flags.
    let rstsck = unsafe { r32(RCU_RSTSCK) };
    unsafe { modify32(RCU_RSTSCK, 0, RSTFC) };
    let rstsck_after = unsafe { r32(RCU_RSTSCK) };
    pd6_input();
    // (d) clock: HXTAL, else the internal RC, else stay on 16 MHz.
    feed();
    let clock = if clock::init_core(Src::Hxtal) {
        ClockState::Hxtal
    } else if clock::init_core(Src::Irc16m) {
        ClockState::Irc16m
    } else {
        ClockState::Failed
    };
    feed();
    // (e) request word (consumed), previous mode, crash counter.
    bkp_enable();
    let req = unsafe { r32(BKP_REQ) } == REQ_MAGIC;
    if req {
        unsafe { w32(BKP_REQ, 0) };
    }
    let last = unsafe { r32(BKP_LAST) };
    let crash = unsafe { r32(BKP_CRASH) };
    let crash = if crash & 0xFFFF_0000 == CRASH_TAG { crash & 0xFF } else { 0 };

    let wdt = rstsck & FWDGTRSTF != 0;
    // Genuine = our own hang: FWDGT alone (no SW reset, no power-on) after a
    // native normal boot. Stale stock flags or an install never count.
    let genuine = wdt && rstsck & (SWRSTF | PORRSTF) == 0 && last == LAST_NORMAL;
    let failed = clock == ClockState::Failed;
    let (mode, reason) = if req {
        (Mode::Recovery, Reason::Requested)
    } else if failed || genuine {
        // Counted for DBG:BOOT? only: a hang reboots opendm40, every time.
        let n = (crash + 1).min(0xFF);
        unsafe { w32(BKP_CRASH, CRASH_TAG | n) };
        if failed && n < CLOCK_RETRIES {
            // Transient clock failure: retry the whole boot (the ROM sets the
            // clock up again) instead of running opendm40 at 16 MHz.
            unsafe { w32(BKP_LAST, LAST_NORMAL) };
            sysreset()
        }
        (Mode::Normal, if failed { Reason::ClockFail } else { Reason::WdtRetry })
    } else if wdt && last == LAST_RECOVERY {
        (Mode::Normal, Reason::WdtAfterRecovery)
    } else if wdt {
        (Mode::Normal, Reason::WdtStale)
    } else {
        (Mode::Normal, Reason::None)
    };
    unsafe {
        if mode == Mode::Recovery {
            w32(BKP_CRASH, CRASH_TAG); // a fresh start after the reinstall
        }
        w32(BKP_LAST, if mode == Mode::Recovery { LAST_RECOVERY } else { LAST_NORMAL });
    }
    feed();
    BootInfo { rstsck, rstsck_after, mode, reason, clock, last }
}

// --- After RAM init: boot info for DBG:BOOT? --------------------------------

static mut INFO: Option<BootInfo> = None;
static mut BOARD: Option<crate::board::Before> = None;

pub fn set_info(i: BootInfo) {
    unsafe { INFO = Some(i) };
}

pub fn set_board(b: crate::board::Before) {
    unsafe { BOARD = Some(b) };
}

#[allow(dead_code)]
pub fn info() -> Option<BootInfo> {
    unsafe { INFO }
}

static mut HEALTHY: bool = false;

/// Call from the main loop every iteration with the uptime in ms: after
/// HEALTHY_MS of running it clears the crash counter once, so the counter
/// shows the hangs in a row (DBG:BOOT?).
// Call site: main.rs main loop (`now` = ms since main started, after pre_main).
pub fn mark_healthy(now_ms: u64) {
    if now_ms >= HEALTHY_MS && !unsafe { HEALTHY } {
        unsafe { HEALTHY = true };
        bkp_enable();
        unsafe { w32(BKP_CRASH, CRASH_TAG) };
    }
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

fn dec(s: &mut crate::scpi::Scpi, mut v: u32) {
    let mut t = [0u8; 10];
    let mut i = t.len();
    loop {
        i -= 1;
        t[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    s.put_bytes(&t[i..]);
}

/// DBG:BOOT? reply (reason: NONE WDT_AFTER_RECOVERY WDT_STALE WDT_RETRY
/// REQUESTED CLOCK_FAIL; clock: HXTAL IRC16M FAILED):
/// `rstsck,rstsck_after,mode,reason,clock,last,bcr1,btr1,bwtr1,lcd_id,
/// fatren_status,found,renamed_8.3_name(11 chars),old_backup,old_deleted,
/// freed_clusters,others,rcvr_deleted,last_trigger` (others > 0: more
/// dm40*.bin left untouched = warning; rcvr_deleted: a leftover DM40RCVR.BIN
/// was removed; last_trigger: see below), marker_w0,w1,w2,VALID|INVALID,
/// crash_word (0x435200nn), HEALTHY|-, fmc_last (0 = clean),
/// PD6,moder,pupd,idr (power key pin read live: MODER[13:12] 0 = input,
/// PUPD[13:12] 2 = pull-down, IDR bit 6 1 = pressed), then
/// native::put_extra: STAGED,n,CALBK,status,written,conflict,differs,STACK,used,size,CALMS,ms,SPICLK,...
#[inline(never)] // .cold (link-native.x): not inlined into the hot caller
pub fn put_info(s: &mut crate::scpi::Scpi) {
    s.len = 0;
    let Some(i) = (unsafe { INFO }) else {
        s.put_bytes(b"NONE");
        return;
    };
    hex(s, i.rstsck);
    s.put_bytes(b",");
    hex(s, i.rstsck_after);
    s.put_bytes(if i.mode == Mode::Normal { b",NORMAL," } else { b",RECOVERY," });
    s.put_bytes(match i.reason {
        Reason::None => b"NONE" as &[u8],
        Reason::WdtAfterRecovery => b"WDT_AFTER_RECOVERY",
        Reason::WdtStale => b"WDT_STALE",
        Reason::WdtRetry => b"WDT_RETRY",
        Reason::Requested => b"REQUESTED",
        Reason::ClockFail => b"CLOCK_FAIL",
    });
    s.put_bytes(match i.clock {
        ClockState::Hxtal => b",HXTAL," as &[u8],
        ClockState::Irc16m => b",IRC16M,",
        ClockState::Failed => b",FAILED,",
    });
    hex(s, i.last);
    let b = unsafe { BOARD }.unwrap_or_default();
    for v in [b.bcr1, b.btr1, b.bwtr1, b.lcd_id] {
        s.put_bytes(b",");
        hex(s, v);
    }
    let r = crate::fatren::result();
    s.put_bytes(match r.status {
        crate::fatren::Status::NotRun => b",NOTRUN," as &[u8],
        crate::fatren::Status::NoMedium => b",NOMEDIUM,",
        crate::fatren::Status::BadBpb => b",BADBPB,",
        crate::fatren::Status::Clean => b",CLEAN,",
        crate::fatren::Status::DryRun => b",DRYRUN,",
        crate::fatren::Status::Renamed => b",RENAMED,",
        crate::fatren::Status::Error => b",ERROR,",
    });
    dec(s, r.found as u32);
    s.put_bytes(b",");
    s.put_bytes(&r.renamed);
    s.put_bytes(if r.old_backup { b",1," } else { b",0," });
    s.put_bytes(if r.old_deleted { b"1," } else { b"0," });
    dec(s, r.freed_clusters);
    s.put_bytes(b",");
    dec(s, r.others as u32);
    s.put_bytes(if r.fake_deleted { b",1," } else { b",0," });
    // Last recovery: 0x54520010 marker zeroed, 0x54520011 zeroing failed;
    // recovery-file: 0x5452000n, n = 0 Created, 1 AlreadyThere, 2 NoMedium,
    // 3 BadBpb, 4 NoSpace, 5 Error (0 / other = none recorded).
    hex(s, unsafe { r32(BKP_TRIG) });
    for w in marker() {
        s.put_bytes(b",");
        hex(s, w);
    }
    s.put_bytes(if marker_valid() { b",VALID," } else { b",INVALID," });
    // Crash counter word, healthy flag, last zero_marker FMC status.
    hex(s, unsafe { r32(BKP_CRASH) });
    s.put_bytes(if unsafe { HEALTHY } { b",HEALTHY," } else { b",-," });
    hex(s, fmc_last());
    // PD6 (power key) pin config read live: MODER[13:12] (0 = input),
    // PUPD[13:12] (2 = pull-down, review B2), IDR bit 6 (1 = pressed).
    let (moder, pupd, idr) = unsafe { (r32(GPIOD), r32(GPIOD + 0x0C), r32(GPIOD + 0x10)) };
    s.put_bytes(b",PD6,");
    dec(s, moder >> 12 & 3);
    s.put_bytes(b",");
    dec(s, pupd >> 12 & 3);
    s.put_bytes(b",");
    dec(s, idr >> 6 & 1);
    crate::native::put_extra(s);
}
