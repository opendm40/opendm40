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

## bt_scpi (SCPI over Bluetooth)
Apply `tools/bt-scpi-test/main-hooks.patch` (base d882651; re-apply by hand if main.rs moved):
two Scpi sessions `scpis[USB|BT]`, `scpi_wait[2]`, `bt.pump()` at the loop top, BT lines taken
when no BT query is pending, `Out::Dbg(_) if !port.trusted()` refused, replies via
`&mut dyn bt::ScpiPort`. Keep every debug/destructive SCPI command under the `DBG:` root (or add
the root to `bt_scpi::UNTRUSTED_DENY`). Optional: report `bt.tx_dma_ok`/`tx_dropped` in DBG:DIAG?.
Device check first: a long reply's CONTENT over BT (TX DMA0 ch4 request 4 is inferred from the
STM32F4 map) and the phone app (cmd 09) still working.
Memory: .dmabuf 184.8/196.6 KB after this — watch new buffers.

## FR-05 rate/smoothing + UF-21 (dmm.rs/meter.rs/persist.rs committed)
app.rs:
```rust
use crate::meter::{Meter, Smooth};
fn restore_func(version: u32, code: u8) -> u8 {
    let max = if version >= 3 { 17 } else { 15 };
    match code { 2 | 3 => code - 2, c if c <= max => c, _ => 1 }
}   // doc: "The mV modes (2, 3) restart as AC / DC V: AUTO steps down to mV again by itself, AUTO+ stays on V."
// persist_words(), before `p`:
p[crate::persist::W_DMM_EXT] = crate::meter::ext_word(self.dmm.rate(), self.meter.smoothing());
// restore(): replace the set_auto_flags line and the `let mode` line with:
let ext = crate::meter::ext_settings(p[crate::persist::W_DMM_EXT]);
self.dmm.set_rate(ext.rate);
self.meter.set_smoothing(ext.smooth);
let v_auto = if ext.v_auto_valid { (p[2] >> 8) as u8 } else { 1 };
self.dmm.set_auto_flags(v_auto, (p[2] >> 16) as u8, (p[2] >> 24) as u8);
let mode = if same_session { restore_func(rec.version, p[2] as u8) } else { 1 };
// App::reading HV warning: let l = &u.raw;   (was &u.live)
// App::scpi arms:
C::SensRate(Some(i)) => { if let Some(r) = dmm::Rate::from_u8(i) { self.dmm.set_rate(r); } self.redraw |= self.view == View::Settings; false }
C::SensRate(None) => { out.put_bytes(dmm::RATE_NAMES[self.dmm.rate() as usize].as_bytes()); true }
C::SensRatePer => { out.put_f32(self.dmm.reading_period_ms() as f32 / 1000.0); true }
C::CalcSmoState(Some(on)) => { if on != (self.meter.smoothing() != Smooth::Off) { self.meter.set_smoothing(if on { Smooth::Low } else { Smooth::Off }); } self.redraw |= self.view == View::Settings; false }
C::CalcSmoState(None) => { out.put_bytes(if self.meter.smoothing() != Smooth::Off { b"1" } else { b"0" }); true }
C::CalcSmoResp(Some(l)) => { self.meter.set_smoothing(Smooth::from_u8(l).unwrap_or(Smooth::Low)); self.redraw |= self.view == View::Settings; false }
C::CalcSmoResp(None) => { out.put_bytes(if self.meter.smoothing() == Smooth::High { b"SLOW" } else { b"FAST" }); true }
```
scpi.rs:
```rust
// Cmd enum, after CalcLimFail:
SensRate(Option<u8>), SensRatePer, CalcSmoState(Option<bool>), CalcSmoResp(Option<u8>),
// SENSe arm guard: add `|| kw(p0, "RATE")`; before the FUNCtion check:
if rest.first().map(|p| kw(p, "RATE")).unwrap_or(false) {
    let a = trim(arg);
    let c = match rest.get(1) {
        Some(l) if kw(l, "PERiod") && query => Some(Cmd::SensRatePer),
        None if query => Some(Cmd::SensRate(None)),
        None if kw(a, "SLOW") => Some(Cmd::SensRate(Some(0))),
        None if kw(a, "MEDium") => Some(Cmd::SensRate(Some(1))),
        None if kw(a, "FAST") => Some(Cmd::SensRate(Some(2))),
        _ => None,
    };
    match c { Some(c) => out = Out::Instr(c), None => self.err = Some("Illegal parameter value") }
    continue;
}
// CALCulate branch, before `} else if kw(sub, "LIMit") {`:
} else if kw(sub, "SMOothing") {
    match leaf {
        Some(l) if kw(l, "RESPonse") => {
            if query { Some(Cmd::CalcSmoResp(None)) }
            else if kw(a, "SLOW") { Some(Cmd::CalcSmoResp(Some(2))) }
            else if kw(a, "MEDium") || kw(a, "FAST") { Some(Cmd::CalcSmoResp(Some(1))) }
            else { None }
        }
        Some(l) if kw(l, "STATe") => state.map(Cmd::CalcSmoState),
        None => state.map(Cmd::CalcSmoState),
        _ => None,
    }
// + scpi.rs header doc lines for SENS:RATE[:PER?] and CALC:SMO[:STAT|:RESP].
```
UI: Settings rows "Rate SLOW/MED/FAST" (grey when !rate_applies(), show readings/s) and "Smoothing
OFF/LOW/HIGH"; Range page: show dmm::MV_AUTO_WARN when dmm.mv_auto_warn(), mark AUTO+ recommended.
Note: hooks 2+4 are what makes existing units boot in AUTO+.
