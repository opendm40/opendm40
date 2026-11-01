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

/// The shutdown is due: not held (App::off_held, which covers Never) and
/// idle longer than the shutdown time.
pub fn off_due(idle: u64, off_ms: u64, held: bool) -> bool {
    !held && off_ms != 0 && idle > off_ms
}
