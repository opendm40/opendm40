//! The idle shutdown's rules (App::tick), kept free of hardware access for
//! the host test (tools/autooff-test).
//!
//! USB power holds the shutdown off (owner, 2026-10-30): the timer counts
//! afresh from the unplug. "USB power" is PB10 high (the charger's input,
//! power.rs) or a USB host that has the meter configured. A configured host
//! alone covers a PB10 that reads 0 with the cable in (two power-offs on a
//! PC's USB port, 2026-11-01, cause not caught); PB10 alone covers a wall
//! charger or a suspended host (no Configured state then).

/// USB power present: `pins` bit 0 = PB10 (Power::pins), `configured` = a
/// host has the meter configured (main.rs `usb_host`).
pub fn usb_power(pins: u8, configured: bool) -> bool {
    pins & 1 != 0 || configured
}

/// Idle time for the shutdown: since the last touch / key or host activity.
pub fn idle_ms(now: u64, last_input: u64, last_host: u64) -> u64 {
    now.saturating_sub(last_input.max(last_host))
}

/// The Shutdown setting's codes (p[0] bits 4..7). 0..3 are the stock's four,
/// as records from before the longer list hold them (bits 6..7 were 0 there);
/// new values only take new codes.
pub const OFF_MS: [u64; 15] = [
    600_000, 1_800_000, 3_600_000, 0, // 10 min, 30 min, 1 hour, Never
    60_000, 300_000, // 1, 5 min
    2 * H, 3 * H, 4 * H, 5 * H, 6 * H, 7 * H, 8 * H, 9 * H, 10 * H,
];
const H: u64 = 3_600_000;
pub const OFF_TXT: [&str; 15] = [
    crate::tr!("10 min"), crate::tr!("30 min"), crate::tr!("1 hour"), crate::tr!("Never"), crate::tr!("1 min"), crate::tr!("5 min"), crate::tr!("2 hours"), crate::tr!("3 hours"), crate::tr!("4 hours"), crate::tr!("5 hours"), crate::tr!("6 hours"), crate::tr!("7 hours"),
    crate::tr!("8 hours"), crate::tr!("9 hours"), crate::tr!("10 hours"),
];
/// The default code (30 min).
pub const OFF_DEFAULT: usize = 1;
/// The row's order, shortest first: its -/+ keys walk it, no wrap-around.
const OFF_ORDER: [u8; 15] = [4, 5, 0, 1, 2, 6, 7, 8, 9, 10, 11, 12, 13, 14, 3];

/// The code in p[0]; an unknown one (15) reads as the default.
pub fn off_code(p0: u32) -> usize {
    let c = (p0 >> 4 & 0xF) as usize;
    if c < OFF_MS.len() { c } else { OFF_DEFAULT }
}

/// The code `d` (-1, +1) steps from `code`.
pub fn off_step(code: usize, d: i8) -> usize {
    let i = OFF_ORDER.iter().position(|&c| c as usize == code).unwrap_or(0) as i32;
    OFF_ORDER[(i + d as i32).clamp(0, OFF_ORDER.len() as i32 - 1) as usize] as usize
}

/// The warning before the shutdown (App::off_warn), ms.
pub const OFF_WARN_MS: u64 = 30_000;
/// The shortest choice leaves the whole warning a place: it starts at 30 s idle.
const _: () = assert!(60_000 >= 2 * OFF_WARN_MS);

/// The shutdown is due: not held (App::off_held, which covers Never) and
/// idle longer than the shutdown time.
pub fn off_due(idle: u64, off_ms: u64, held: bool) -> bool {
    !held && off_ms != 0 && idle > off_ms
}
