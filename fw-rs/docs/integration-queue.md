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

## Generator (gen.rs/genplan.rs committed)
NOTE: persist word 38 is taken by W_DMM_EXT (FR-05) — store the generator offset in p[39]
(add `pub const W_GEN_OFFS: usize = 39;` to persist.rs).
scpi.rs: `GenOffs(Option<f32>)` after `GenVolt(Option<f32>),`; doc `SOURce:VOLTage:OFFSet <V>|AUTO`;
before `} else if kw(sub, "VOLTage") {` insert
```rust
} else if kw(sub, "VOLTage") && parts.len() >= 3 && kw(parts[2], "OFFSet") {
    Some(Cmd::GenOffs(if query { None } else if kw(a, "AUTO") { Some(f32::NAN) } else { parse_num(a) }))
```
app.rs SCPI: add `| C::GenOffs(_)` to the scope_cmd matches!; `C::GenFreq(None)` -> `out.put_f32(self.gen.actual_hz()); Some(())`;
`C::GenVolt(Some(v)) => { g = g.with_amp(v); None }`;
`C::GenOffs(Some(o)) => { g = g.with_offset(Some(o)); None }`;
`C::GenOffs(None) => { out.put_f32(g.offset_v.unwrap_or(g.amp_v / 2.0)); Some(()) }`;
optional `C::GenDuty(None) => out.put_f32(self.gen.out.duty_pct())`. VOLT?/OFFS? return the SET values.
app.rs persist: `p[W_GEN_OFFS] = g.offset_v.map_or(0, |o| (-o.abs()).to_bits() | 0x8000_0000);`
restore: `g.offset_v = if p[W_GEN_OFFS] == 0 { None } else { Some(f32::from_bits(p[W_GEN_OFFS] & 0x7FFF_FFFF)) };`
and `a.clamp(0.5, 3.0)` -> `a.clamp(gen::AMP_MIN, gen::AMP_MAX)`.
app.rs keypad: `1 => st = st.with_amp(v),`, `5 => st = st.with_offset(Some(v)),`,
freq `st.freq_hz = gen::clamp_freq((v * m + 0.5).max(1.0) as u32);`, freq digit limit `self.kp_n < 7`;
offset target 5: `self.kp_target >= 3` -> `matches!(self.kp_target, 3 | 4)` everywhere; kp_labels `5 => ("Offset", "V")`;
kp_kind `1 | 2 | 5 => ui::KP_NUM`; accept `1 | 2 | 5 => self.kp_n < 5`.
Gen view: duty gate `if st.wave == Wave::Square { self.kp_open(2, clk); }` (drop out_hz() <= 50_000);
four boxes 66 px at y 40/110/180/250 (Frequency, Vpp, Offset, Duty) -> keypad targets 0,1,5,2.
ui.rs gen_values: show actual_hz() in full digits ("32768.08 Hz", 3 decimals < 1 kHz); while freq_clamped()
show the set value grey + wave.max_note(); Vpp/Offset ("auto" when None), grey + "logic level" when
out.fixed_level(); duty enabled for Square at any f, show out.duty_pct(); drop the out == 0 branch.
gen_state: eng(.., g.actual_hz(), "Hz").
Device: frequency list (1, 7, 1k, 4k [old bug], 32768 sine/square, 44100, 50k, 50000/50001, 75k, 2M, 7M, 10M)
vs SOUR:FREQ?; K>1 table visual check; clamp round trip; ramp rising; offset/DC with the DMM; PWM level;
measure apply() time (expect tens of ms).
