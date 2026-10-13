# Integration queue (hooks into shared files)

Modules written while other agents owned the shared files (main.rs, app.rs,
ui.rs, scpi.rs). Each entry is the exact code to add; integrate in one pass,
then delete the entry.

## touch.rs (I2C2 + EXTI, commit after 258c833)
main.rs main loop, replacing `if now >= next_touch && !logging {`:
```rust
        // EXTI-latched INT edge: read the fresh report now, not on the tick.
        let touch_irq = tp.as_ref().map_or(false, |t| t.irq_pending());
        if (now >= next_touch || touch_irq) && !logging {
            next_touch = now + 10;
```
main.rs `touch_diag()`, after `d[12..19].copy_from_slice(&t.raw);`:
```rust
        d[20..60].copy_from_slice(&t.diag()); // touch bus / IRQ counters
```
Then remove the two `#[allow(dead_code)]` above `irq_pending` / `diag` in touch.rs.
DIAG decode: `mode, ctrl, rise_scl, rise_sda, last_us, max_us, last_n, probe_mode, edge, level, timer, ok, bad, naks, faults = struct.unpack_from('<BBHHHHBB7I', d, 20)`.
Docs: parity.md §6 I2C row -> "hardware I2C2 (400/100 kHz by rise time, bit-bang fallback) + EXTI5 pending-bit gating; no DMA"; TODO.md:35 drop the bit-bang clause.
