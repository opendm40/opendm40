//! Presets (View::Presets, SCPI `*SAV` / `*RCL` / `MEMory:STATe:...`): save
//! the instrument state into one of presets::N slots on the SPI disk and
//! recall it (presets.rs holds the file, ui_presets.rs draws the page).
//!
//! A preset is the settings record (`persist_words`) at the save, the page
//! it was on (View::CYCLE: Multimeter, DMM + Scope, Oscilloscope, Signal;
//! from a menu page the one it was opened from) and a name. A recall
//! restores the measurement (`restore` with `prefs` false): the DMM
//! function, coupling, AUTO / fixed range, ohm variant, rate, smoothing,
//! trend timebase, AutoHold and the limits, the temperature sensor, the
//! power meter's settings, the Record page's interval / duration, the
//! scope's timebase, V/div, coupling, probe, trigger, positions, math /
//! FFT, CH2 / GEN traces and measurements, the generator's waveform,
//! frequency, amplitude, offset and duty. The generator output is always
//! OFF after a recall (safety). Not in a preset: the preferences of the
//! Settings pages, the touch calibration, the power meter's DC zeros, REL,
//! HOLD and the statistics (cleared by the recall, as a function change).
//!
//! Refused while the AD2 stream runs (the Wave view, the inrush page, a
//! fast-log session), a logger session or the disk is held (main.rs /
//! `ps_busy`); a recall also during a CSV recording.
//!
//! Region C (`.cold`, link-native.x): a tap or a SCPI command, never per
//! pass.

use super::*;
use crate::presets::{self as pr, PErr, Slot, NAME_LEN};
use crate::ui::presets as up;

/// Page state. In SRAM (`.sram_bss`), not in App (the TCM stack's headroom).
struct Ps {
    sel: u8,
    /// A pending Replace / Clear: the button + 1 (0 none).
    confirm: u8,
    /// The rename keyboard is up.
    kb: bool,
    lower: bool,
    n: u8,
    buf: [u8; NAME_LEN],
    /// The status line's message (0 bytes: the selected slot's summary).
    msg: [u8; 40],
    msg_n: u8,
    warn: bool,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PS: Ps = Ps { sel: 0, confirm: 0, kb: false, lower: false, n: 0, buf: [0; NAME_LEN], msg: [0; 40], msg_n: 0, warn: false };

fn ps() -> &'static mut Ps {
    unsafe { &mut *core::ptr::addr_of_mut!(PS) }
}

/// A small text buffer.
struct Txt<const L: usize> {
    b: [u8; L],
    n: usize,
}

impl<const L: usize> Txt<L> {
    fn new() -> Self {
        Txt { b: [0; L], n: 0 }
    }
    fn s(&mut self, t: &str) -> &mut Self {
        for &c in t.as_bytes() {
            if self.n < L {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }
    fn u(&mut self, v: u32) -> &mut Self {
        let mut t = [0u8; 10];
        let mut i = t.len();
        let mut v = v;
        loop {
            i -= 1;
            t[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.s(core::str::from_utf8(&t[i..]).unwrap_or(""))
    }
    fn str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

fn msg(t: &str, warn: bool) {
    let p = ps();
    let n = t.len().min(p.msg.len());
    p.msg[..n].copy_from_slice(&t.as_bytes()[..n]);
    p.msg_n = n as u8;
    p.warn = warn;
}

/// The text of a failed operation.
fn err_text(e: PErr) -> &'static str {
    match e {
        PErr::NoFile => crate::cold_str!("No presets file on the disk"),
        PErr::Empty => crate::cold_str!("That preset is empty"),
        PErr::Write => crate::cold_str!("Disk write failed"),
    }
}

impl App {
    /// The page a preset stores: the instrument view, or the one a menu
    /// page was opened from.
    fn ps_page(&self) -> u32 {
        let v = if View::CYCLE.contains(&self.view) { self.view } else { self.back_view };
        View::CYCLE.iter().position(|&c| c == v).unwrap_or(0) as u32
    }

    /// Why a preset cannot be written / recalled now (a SCPI error text).
    fn ps_busy(&self, recall: bool) -> Option<&'static str> {
        if crate::hy3131::streaming() || self.scope.wave_on() || self.logger_busy || self.view == View::Inrush {
            return Some("Settings conflict");
        }
        if recall && crate::csvlog::active() {
            return Some("Settings conflict");
        }
        if crate::setfile::disk_busy() {
            return Some("Mass storage busy");
        }
        None
    }

    /// A name made from the state (a save into an empty or unnamed slot).
    fn ps_auto_name(&self) -> [u8; NAME_LEN] {
        let mut t = Txt::<NAME_LEN>::new();
        let func = crate::csvlog::func_label(self.dmm.func());
        match View::CYCLE[self.ps_page() as usize % 4] {
            View::Scope => {
                t.s(crate::cold_str!("Scope ")).s(crate::scope_ui::TB_LABELS[self.scope.s.tb.min(29)]);
            }
            View::Gen => {
                let f = self.gen.s.freq_hz;
                t.s(crate::cold_str!("Signal "));
                if f >= 1000 && f % 1000 == 0 {
                    t.u(f / 1000).s(crate::cold_str!(" kHz"));
                } else {
                    t.u(f).s(crate::cold_str!(" Hz"));
                }
            }
            View::DmmScope => {
                t.s(func).s(crate::cold_str!(" + Scope"));
            }
            _ => {
                t.s(func);
            }
        }
        pr::clean_name(&t.b[..t.n])
    }

    /// Save the state into slot `k` (0-based). An unnamed slot (or one
    /// whose name was made from the state) gets a new name from the state.
    #[inline(never)]
    pub(super) fn ps_save(&mut self, k: usize) -> Result<(), &'static str> {
        if let Some(e) = self.ps_busy(false) {
            return Err(e);
        }
        let (state, info, _) = pr::slot(k);
        let named = state == Slot::Used && info & pr::INFO_NAMED != 0;
        let name = if named { pr::name_of(k) } else { self.ps_auto_name() };
        let w = self.persist_words();
        let info = self.ps_page() | if self.gen.s.on { pr::INFO_GEN_ON } else { 0 } | if named { pr::INFO_NAMED } else { 0 };
        let mut dev = crate::setfile::new_dev();
        pr::save(&mut dev, k, &w, info, &name).map_err(|_| "Mass storage error")
    }

    /// Recall slot `k`: the measurement, the generator (output OFF), the
    /// page. True: the generator output was on at the save.
    #[inline(never)]
    pub(super) fn ps_recall(&mut self, clk: &mut Clock, k: usize) -> Result<bool, &'static str> {
        if let Some(e) = self.ps_busy(true) {
            return Err(e);
        }
        let mut dev = crate::setfile::new_dev();
        let p = match pr::load(&mut dev, k) {
            Ok(p) => p,
            Err(PErr::Empty) => return Err("Settings conflict"),
            Err(_) => return Err("Mass storage error"),
        };
        self.restore(&p.rec, true, false, clk);
        // As a function change: REL, HOLD, the statistics and the chart go.
        self.func_changed();
        self.last_raw = None;
        let g = self.gen.s;
        self.gen.apply(gen::Settings { on: false, ..g });
        self.persist_now();
        let v = View::CYCLE[(p.info & 0xFF) as usize % 4];
        self.show(v, clk);
        self.redraw = true;
        Ok(p.info & pr::INFO_GEN_ON != 0)
    }

    /// Main menu > Presets.
    #[inline(never)]
    pub(super) fn ps_open(&mut self, clk: &mut Clock) {
        let p = ps();
        p.confirm = 0;
        p.kb = false;
        p.msg_n = 0;
        self.show(View::Presets, clk);
    }

    /// The selected slot's line: what it holds.
    fn ps_summary(&self, t: &mut Txt<40>) {
        let k = ps().sel as usize;
        match pr::slot(k) {
            _ if pr::file_state() == pr::FileState::NoFile => {
                t.s(crate::cold_str!("No presets file on the disk"));
            }
            (Slot::Used, info, _) => {
                t.s(crate::cold_str!("Opens ")).s(ui::view_name(View::CYCLE[(info & 0xFF) as usize % 4]));
                if info & pr::INFO_GEN_ON != 0 {
                    t.s(crate::cold_str!("; output OFF"));
                }
            }
            (Slot::Bad, ..) => {
                t.s(crate::cold_str!("Unreadable: Save or Clear it"));
            }
            _ => {
                t.s(crate::cold_str!("Empty: Save stores the state now"));
            }
        }
    }

    fn ps_status(&self) {
        let p = ps();
        if p.msg_n > 0 {
            up::status(core::str::from_utf8(&p.msg[..p.msg_n as usize]).unwrap_or(""), p.warn);
        } else {
            let mut t = Txt::<40>::new();
            self.ps_summary(&mut t);
            up::status(t.str(), false);
        }
    }

    fn ps_row(k: usize) -> up::Row<'static> {
        let (s, _, name) = pr::slot(k);
        up::Row { state: s as u8, name: if name.is_empty() { crate::cold_str!("Preset") } else { name } }
    }

    /// Button `i`: label, acts, pending confirm.
    fn ps_button(i: u8) -> (&'static str, bool, bool) {
        let p = ps();
        let s = pr::slot(p.sel as usize).0;
        let ok = pr::file_state() != pr::FileState::NoFile;
        if p.confirm == i + 1 {
            return (if i == up::B_SAVE { crate::cold_str!("Replace?") } else { crate::cold_str!("Clear?") }, true, true);
        }
        let on = ok
            && match i {
                up::B_SAVE => true,
                up::B_CLEAR => s != Slot::Empty,
                _ => s == Slot::Used,
            };
        (up::BUTTONS[i as usize], on, false)
    }

    #[inline(never)]
    pub(super) fn ps_draw(&self) {
        let p = ps();
        if p.kb {
            up::keyboard(p.sel as usize, core::str::from_utf8(&p.buf[..p.n as usize]).unwrap_or(""), p.lower);
            return;
        }
        let rows = [Self::ps_row(0), Self::ps_row(1), Self::ps_row(2), Self::ps_row(3)];
        let b = [Self::ps_button(0), Self::ps_button(1), Self::ps_button(2), Self::ps_button(3)];
        let mut t = Txt::<40>::new();
        let (st, warn) = if p.msg_n > 0 {
            (core::str::from_utf8(&p.msg[..p.msg_n as usize]).unwrap_or(""), p.warn)
        } else {
            self.ps_summary(&mut t);
            (t.str(), false)
        };
        up::page(crate::cold_str!("Presets"), &rows, p.sel as usize, &b, st, warn);
    }

    /// The buttons and the status line again (after a selection or an
    /// operation on the page).
    fn ps_draw_lower(&self) {
        self.ps_status();
        for i in 0..4 {
            let (l, on, c) = Self::ps_button(i);
            up::button(i as usize, l, on, c, false);
        }
    }

    /// Pressed feedback (app_press.rs): `item` 0..3 the rows, 4..7 the
    /// buttons; on the keyboard the key.
    pub(super) fn ps_press_hit(&self, x: u16, y: u16) -> Option<u8> {
        if ui::title_back_hit(y) {
            return None;
        }
        if ps().kb {
            return up::key_hit(x, y);
        }
        match up::hit(x, y)? {
            up::Hit::Row(i) => Some(i),
            up::Hit::Button(i) if Self::ps_button(i).1 => Some(4 + i),
            up::Hit::Button(_) => None,
        }
    }

    pub(super) fn ps_press_draw(&self, item: u8, on: bool) {
        let p = ps();
        if p.kb {
            up::key(item, p.lower, on);
        } else if item < 4 {
            up::row(item as usize, Self::ps_row(item as usize), item == p.sel, on);
        } else {
            let (l, a, c) = Self::ps_button(item - 4);
            up::button(item as usize - 4, l, a, c, on);
        }
    }

    /// MENU on the page: the keyboard closes (true), else the caller goes
    /// to the main menu.
    pub(super) fn ps_key_back(&mut self) -> bool {
        let p = ps();
        if p.kb {
            p.kb = false;
            self.redraw = true;
            return true;
        }
        false
    }

    #[inline(never)]
    pub(super) fn ps_tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let p = ps();
        if p.kb {
            self.ps_kb_tap(clk, x, y);
            return;
        }
        if ui::title_back_hit(y) {
            self.show(View::Menu, clk);
            return;
        }
        let Some(h) = up::hit(x, y) else { return };
        let now = clk.now_ms();
        match h {
            up::Hit::Row(i) => {
                let old = p.sel;
                p.sel = i;
                p.confirm = 0;
                p.msg_n = 0;
                up::row(old as usize, Self::ps_row(old as usize), false, false);
                up::row(i as usize, Self::ps_row(i as usize), true, false);
                self.ps_draw_lower();
            }
            up::Hit::Button(b) => {
                if !Self::ps_button(b).1 {
                    return;
                }
                let k = p.sel as usize;
                let used = pr::slot(k).0 == Slot::Used;
                let mut t = Txt::<40>::new();
                match b {
                    up::B_SAVE | up::B_CLEAR if p.confirm != b + 1 && (used || b == up::B_CLEAR) => {
                        p.confirm = b + 1;
                        t.s(if b == up::B_SAVE { crate::cold_str!("Tap Replace? to save over ") } else { crate::cold_str!("Tap Clear? to clear preset ") }).u(k as u32 + 1);
                        msg(t.str(), true);
                    }
                    up::B_SAVE => {
                        p.confirm = 0;
                        match self.ps_save(k) {
                            Ok(()) => {
                                t.s(crate::cold_str!("Saved to ")).u(k as u32 + 1);
                                msg(t.str(), false);
                                self.beep(now, 60);
                            }
                            Err(e) => msg(if e == "Settings conflict" { crate::cold_str!("Not now: a stream or log runs") } else { e }, true),
                        }
                        up::row(k, Self::ps_row(k), true, false);
                    }
                    up::B_CLEAR => {
                        p.confirm = 0;
                        let mut dev = crate::setfile::new_dev();
                        match self.ps_busy(false).map_or_else(|| pr::clear(&mut dev, k).map_err(err_text), Err) {
                            Ok(()) => {
                                t.s(crate::cold_str!("Preset ")).u(k as u32 + 1).s(crate::cold_str!(" cleared"));
                                msg(t.str(), false);
                            }
                            Err(e) => msg(e, true),
                        }
                        up::row(k, Self::ps_row(k), true, false);
                    }
                    up::B_RECALL => {
                        p.confirm = 0;
                        match self.ps_recall(clk, k) {
                            Ok(gen_on) => {
                                t.s(crate::cold_str!("Preset ")).u(k as u32 + 1).s(if gen_on { crate::cold_str!(" recalled, output OFF") } else { crate::cold_str!(" recalled") });
                                self.toast(now, t.str(), true);
                            }
                            Err(e) => {
                                msg(if e == "Settings conflict" { crate::cold_str!("Not now: a stream, log or recording runs") } else { e }, true);
                                self.ps_draw_lower();
                            }
                        }
                        return;
                    }
                    up::B_RENAME => {
                        // Rename: the keyboard, with the name as it is.
                        p.confirm = 0;
                        p.kb = true;
                        p.lower = false;
                        p.buf = pr::name_of(k);
                        p.n = p.buf.iter().position(|&c| c == 0).unwrap_or(NAME_LEN) as u8;
                        self.redraw = true;
                        return;
                    }
                    _ => return,
                }
                self.ps_draw_lower();
            }
        }
    }

    fn ps_kb_tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let p = ps();
        if ui::title_back_hit(y) {
            p.kb = false;
            self.redraw = true;
            return;
        }
        let Some(k) = up::key_hit(x, y) else { return };
        match k {
            up::K_CASE => {
                p.lower = !p.lower;
                for c in 0..41 {
                    up::key(c, p.lower, false);
                }
                return;
            }
            up::K_OK => {
                let name = pr::clean_name(&p.buf[..p.n as usize]);
                if name[0] == 0 {
                    return;
                }
                let k = p.sel as usize;
                let mut dev = crate::setfile::new_dev();
                let r = self.ps_busy(false).map_or_else(|| pr::rename(&mut dev, k, &name).map_err(err_text), Err);
                let mut t = Txt::<40>::new();
                match r {
                    Ok(()) => msg(t.s(crate::cold_str!("Preset ")).u(k as u32 + 1).s(crate::cold_str!(" renamed")).str(), false),
                    Err(e) => msg(e, true),
                }
                p.kb = false;
                self.redraw = true;
                let _ = clk;
                return;
            }
            up::K_DEL => p.n = p.n.saturating_sub(1),
            up::K_SPACE if p.n > 0 && (p.n as usize) < NAME_LEN => {
                p.buf[p.n as usize] = b' ';
                p.n += 1;
            }
            c if c < 40 && (p.n as usize) < NAME_LEN => {
                p.buf[p.n as usize] = up::key_char(c, p.lower);
                p.n += 1;
            }
            _ => return,
        }
        up::field(core::str::from_utf8(&p.buf[..p.n as usize]).unwrap_or(""));
    }

    /// SCPI (scpi.rs Cmd::Preset): `op` scpi::PS_*, `k` the slot (0-based).
    /// True: a reply.
    #[inline(never)]
    pub(super) fn ps_scpi(&mut self, clk: &mut Clock, op: u8, k: u8, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi as s;
        let k = k as usize;
        let r: Result<(), &'static str> = match op {
            s::PS_SAV => self.ps_save(k),
            s::PS_RCL => self.ps_recall(clk, k).map(|_| ()),
            s::PS_NAME => {
                let name = pr::clean_name(pr::scpi_name());
                if name[0] == 0 {
                    Err("Illegal parameter value")
                } else if pr::slot(k).0 != Slot::Used {
                    Err("Settings conflict")
                } else {
                    let mut dev = crate::setfile::new_dev();
                    self.ps_busy(false).map_or_else(|| pr::rename(&mut dev, k, &name).map_err(|_| "Mass storage error"), Err)
                }
            }
            s::PS_DEL => {
                let mut dev = crate::setfile::new_dev();
                self.ps_busy(false).map_or_else(|| pr::clear(&mut dev, k).map_err(|_| "Mass storage error"), Err)
            }
            s::PS_NAME_Q => {
                out.put_bytes(b"\"");
                out.put_bytes(pr::slot(k).2.as_bytes());
                out.put_bytes(b"\"");
                return true;
            }
            _ => {
                out.put_bytes(if pr::slot(k).0 == Slot::Used { b"1" } else { b"0" });
                return true;
            }
        };
        if let Err(e) = r {
            out.set_err(e);
        }
        if self.view == View::Presets {
            self.redraw = true;
        }
        false
    }
}
