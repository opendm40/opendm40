//! The protocol decoder view (src/ui_decode.rs, View::Decode): frames from
//! the real decoder (src/decode.rs fed with synthetic UART / Modbus sample
//! streams), drawn by the firmware's drawing code. The view is opendm40's
//! own: the full layout rules in the list, the top bar, the counter row and
//! the setup popup's page; the panel buttons keep the scope page's stock
//! geometry (text collisions checked). Worst cases: the widest rows (8 x
//! 0xFF, a long Modbus field line, a cut line), the widest top-bar and
//! counter texts, every setup state; the MF Settings tab with its Decode
//! button.
use super::*;
use decode as dc;
use scope_ui::dec as ud;

/// Codes of `bytes` (UART, `fmt` 0 8N1 / 1 8E1 / 3 7E1) at `baud`, sampled
/// at `fs`, with `idle_bits` of idle before and `gap_bits` after each frame
/// given as a list of frames; 3.3 V logic at the decoder's 2 V/div
/// (codes ~512 .. ~690).
fn stream(frames: &[&[u8]], baud: f64, fs: f64, fmt: u8, gap_bits: f64) -> Vec<u16> {
    let (nb, par) = [(8, 0), (8, 1), (8, 2), (7, 1)][fmt as usize];
    let mut bits: Vec<(f64, bool)> = vec![(20.0, true)];
    for f in frames {
        for &b in f.iter() {
            bits.push((1.0, false));
            let mut ones = 0;
            for i in 0..nb {
                let bit = b >> i & 1 == 1;
                ones += bit as u32;
                bits.push((1.0, bit));
            }
            match par {
                1 => bits.push((1.0, ones % 2 == 1)),
                2 => bits.push((1.0, ones % 2 == 0)),
                _ => {}
            }
            bits.push((1.0, true));
        }
        bits.push((gap_bits, true));
    }
    let total: f64 = bits.iter().map(|b| b.0).sum::<f64>() / baud;
    let n = (total * fs) as usize;
    let mut out = Vec::with_capacity(n);
    let (mut k, mut end) = (0usize, bits[0].0 / baud);
    for i in 0..n {
        let t = i as f64 / fs;
        while t >= end && k + 1 < bits.len() {
            k += 1;
            end += bits[k].0 / baud;
        }
        out.push(if bits[k].1 { 690 } else { 512 } + (i % 3) as u16);
    }
    out
}

/// A decoder fed with `codes` (as the firmware: the ring, absolute counts).
fn decoded(cfg: dc::Cfg, codes: &[u16], t0_us: u64) -> Box<dc::Dec> {
    let mut d: Box<dc::Dec> = Box::new(dc::Dec::new());
    let fs = dc::fs_for(cfg.baud);
    d.restart(cfg, fs, t0_us, None);
    let cap = scope::CAP;
    let mut ring = vec![0u16; cap];
    let mut wr = 0u64;
    for ch in codes.chunks(4000) {
        for &c in ch {
            ring[(wr % cap as u64) as usize] = c;
            wr += 1;
        }
        d.poll(&ring, wr);
    }
    // The idle tail ends the last frame.
    for _ in 0..(fs as usize / 20) {
        ring[(wr % cap as u64) as usize] = 690;
        wr += 1;
        if wr % 4000 == 0 {
            d.poll(&ring, wr);
        }
    }
    d.poll(&ring, wr);
    d
}

fn crc(d: &[u8]) -> Vec<u8> {
    let mut v = d.to_vec();
    let c = dc::crc16(d);
    v.push(c as u8);
    v.push((c >> 8) as u8);
    v
}

pub struct View {
    pub top: (u8, &'static str, String, String, bool),
    pub d: Box<dc::Dec>,
    /// The first row's sequence number; None: the newest rows (LIVE).
    pub first: Option<u32>,
    pub note: Option<&'static str>,
    pub paused: bool,
    pub cells: [u32; 6],
    pub setup: Option<(bool, u8, bool, String, bool, String)>,
}

/// The whole view as the app draws it (dec_page + a full dec_tick).
pub fn draw(v: &View) {
    host::reset(0);
    ud::chrome(&mut || {});
    let live = v.first.is_none();
    ud::panel(live, v.paused, &mut || {});
    let (state, proto, rate, thr, inv) = &v.top;
    ud::top(&ud::Top { state: *state, proto, rate, thr, inv: *inv, battery: 4, charging: false });
    let st = &v.d.st;
    // The setup popup open: the app draws no rows under it.
    if v.setup.is_some() {
    } else if let Some(n) = v.note {
        ud::row(0, Some(&ud::Row { time: "", text: n, ascii: "", status: "", fg: 0 }));
    } else {
        let top = v.first.unwrap_or(st.next.saturating_sub(ud::ROWS as u32).max(st.first));
        let base = st.get(st.first).map_or(0, |h| h.t_us);
        for i in 0..ud::ROWS {
            let s = top + i as u32;
            if s < st.next {
                ud::frame_row(i, st, s, base);
            }
        }
    }
    ud::cells(&v.cells);
    if let Some((mb, fmt, inv, baud, auto, thr)) = &v.setup {
        ud::setup(&ud::SetupVals { modbus: *mb, fmt: *fmt, inv: *inv, baud, thr_auto: *auto, thr }, &mut || {});
    }
}

fn cnt(d: &dc::Dec, mb: bool) -> [u32; 6] {
    let c = d.st.cnt;
    [c.frames, c.fe, c.pe, c.crc, c.ovr, mb as u32]
}

/// The views of the manual / the contact sheet.
pub fn uart_view() -> View {
    let cfg = dc::Cfg { modbus: false, baud: 0, fmt: 0, inverted: false };
    let lines: [&[u8]; 4] = [b"Hello, opendm40!\r\n", b"T=23.5C H=41%\r\n", b"\x02\x10\xff\x00ACK\x03", b"OK\r\n"];
    let mut fr: Vec<&[u8]> = Vec::new();
    for _ in 0..3 {
        fr.extend_from_slice(&lines);
    }
    let d = decoded(cfg, &stream(&fr, 115_200.0, 1.25e6, 0, 300.0), 0);
    let c = cnt(&d, false);
    View { top: (1, "UART", "Auto 115200 8N1".into(), "A 1.42V".into(), false), d, first: None, note: None, paused: false, cells: c, setup: None }
}

pub fn modbus_view() -> View {
    let cfg = dc::Cfg { modbus: true, baud: 19200, fmt: 1, inverted: false };
    let req = crc(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x02]);
    let rsp = crc(&[0x01, 0x03, 0x04, 0x00, 0xEB, 0x01, 0x9A]);
    let w6 = crc(&[0x11, 0x06, 0x00, 0x05, 0x04, 0xD2]);
    let w16 = crc(&[0x11, 0x10, 0x00, 0x10, 0x00, 0x02, 0x04, 0x00, 0x0A, 0x01, 0x02]);
    let w16r = crc(&[0x11, 0x10, 0x00, 0x10, 0x00, 0x02]);
    let exr = crc(&[0x01, 0x04, 0x00, 0x10, 0x00, 0x01]);
    let exc = crc(&[0x01, 0x84, 0x02]);
    let mut bad = req.clone();
    bad[6] ^= 0x5A;
    let fr: Vec<&[u8]> = vec![&req, &rsp, &w6, &w6, &w16, &w16r, &exr, &exc, &bad, &req, &rsp, &req, &rsp, &w6, &w6, &req];
    let d = decoded(cfg, &stream(&fr, 19200.0, dc::fs_for(19200) as f64, 1, 120.0), 0);
    let c = cnt(&d, true);
    View { top: (1, "MODBUS", "19200 8E1".into(), "A 1.42V".into(), false), d, first: None, note: None, paused: false, cells: c, setup: None }
}

fn empty_view(note: &'static str, state: u8) -> View {
    let d = Box::new(dc::Dec::new());
    View { top: (state, "UART", "Auto 8N1".into(), "A --".into(), false), d, first: None, note: Some(note), paused: state == 2, cells: [0; 6], setup: None }
}

/// Worst cases: the widest row contents and texts.
fn worst_views() -> Vec<(String, View)> {
    let mut out = Vec::new();
    // UART: 8 x 0xFF / 8 x 'W' (the widest ASCII), times > 27 h (6 digits).
    let cfg = dc::Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: true };
    let fr: Vec<&[u8]> = vec![&[0xFF; 8], b"WWWWWWWW", b"MMMMMMMM", &[0x00; 8], b"@@@@@@@@", &[0xFF; 8], b"WWWWWWWW", b"MMMMMMMM"];
    let mut codes = stream(&fr, 115_200.0, 1.25e6, 0, 30.0);
    for c in codes.iter_mut() {
        *c = 1202 - *c; // inverted (RS-232 polarity)
    }
    for (k, t0) in [(0, 0u64), (1, 99_000_000), (2, 9_999_000_000), (3, 999_999_000_000)] {
        let d = decoded(cfg, &codes, t0);
        let c = [4_294_967_295, 99_999, 4_000_000, 123_456_789, 99_999, 0];
        let thr = ["A -99.99V", "-99.99V", "A --", "12.34V"][k];
        out.push((format!("uart/{k}"), View { top: ([1, 0, 2, 1][k], "UART", "Auto 115200 7E1".into(), thr.into(), true), d, first: Some(0), note: None, paused: k == 2, cells: c, setup: None }));
    }
    // Modbus: the longest field lines, a cut, a 256-byte frame, an
    // exception, a slow reply.
    let cfg = dc::Cfg { modbus: true, baud: 9600, fmt: 0, inverted: false };
    let r1 = crc(&[0xFF, 0x10, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0x00, 0x00]);
    let r1r = crc(&[0xFF, 0x10, 0xFF, 0xFF, 0xFF, 0xFF]);
    let r3 = crc(&[0xFF, 0x03, 0xFF, 0xFF, 0x00, 0x7D]);
    let mut big = vec![0xFF, 0x03, 0xFA];
    big.extend(std::iter::repeat(0x88).take(250));
    let big = crc(&big);
    let ex = crc(&[0xFF, 0x83, 0xFF]);
    let r6 = crc(&[0xFF, 0x06, 0xFF, 0xFF, 0xFF, 0xFF]);
    let noise: Vec<u8> = (0..255u8).collect();
    let fr: Vec<&[u8]> = vec![&r1, &r1r, &r3, &big, &r3, &ex, &r6, &r6, &noise, &r1, &r1r];
    let d = decoded(cfg, &stream(&fr, 9600.0, dc::fs_for(9600) as f64, 0, 9000.0), 9_000_000_000);
    let c = [99_999, 99_999, 99_999, 99_999, 99_999, 1];
    out.push(("modbus/0".into(), View { top: (1, "MODBUS", "115200 8O1".into(), "A -99.99V".into(), true), d, first: Some(0), note: None, paused: false, cells: c, setup: None }));
    // Notes.
    for (k, (n, s)) in [("Waiting for a signal (no swing)", 0u8), ("Measuring the bit rate", 0), ("No frames yet", 1), ("Stopped", 2)].iter().enumerate() {
        out.push((format!("note/{k}"), empty_view(n, *s)));
    }
    // Setup popup, every state.
    for (k, s) in [
        (false, 0u8, false, "Auto", true, "1.42V"),
        (true, 1, true, "115200", false, "-99.99V"),
        (false, 2, false, "300", false, "-99.99V"),
        (true, 3, true, "76800", true, "--"),
    ]
    .iter()
    .enumerate()
    {
        let mut v = uart_view();
        v.setup = Some((s.0, s.1, s.2, s.3.into(), s.4, s.5.into()));
        out.push((format!("setup/{k}"), v));
    }
    out
}

/// The checks of one drawn view.
fn check_view(ck: &mut Checker, name: &str, popup: bool) {
    let screen = Bx::new(0, 0, 480, 320);
    ck.check_replica(name);
    let list = Bx::new(14, 21, 399, 279);
    if popup {
        // The option lists are the MF popup's (stock) widgets: their items
        // touch by design; the labels and the steppers get the full rules
        // against everything on the page.
        let page = Bx::new(14, 61, 399, 140);
        let mut add: Vec<Bx> = ud::SU_LABELS.iter().map(|r| rb(*r)).collect();
        for r in [ud::SU_BMINUS, ud::SU_BVAL, ud::SU_BPLUS, ud::SU_TMINUS, ud::SU_TVAL, ud::SU_TPLUS] {
            add.push(rb(r));
        }
        ck.check_add(&format!("{name}/setup"), &[("page", page)], &add, Bx::new(13, 20, 401, 181));
    } else {
        ck.check_add(&format!("{name}/list"), &[("list", list)], &[list], list);
    }
    // The top bar and the counter cells use the scope page's (stock)
    // badges and cells: the replica rules above (texts do not collide or
    // spill out of their box).
    let _ = screen;
}

fn rb(r: stockfont::Rect) -> Bx {
    Bx::new(r.x as i32, r.y as i32, r.w as i32, r.h as i32)
}

pub fn run(ck: &mut Checker, out: &str) {
    for (name, v) in [("uart", uart_view()), ("modbus", modbus_view())] {
        draw(&v);
        check_view(ck, &format!("decode/{name}"), false);
        save(out, &format!("d01-decode-{name}"));
    }
    for (name, v) in worst_views() {
        draw(&v);
        check_view(ck, &format!("decode/{name}"), v.setup.is_some());
        if name == "setup/1" {
            save(out, "d02-decode-setup");
        }
        if name == "modbus/0" {
            save(out, "d03-decode-worst-modbus");
        }
        if name == "uart/3" {
            save(out, "d03-decode-worst-uart");
        }
    }
    // Panel: pressed states, RUN when paused (the stock geometry).
    for k in 0..7 {
        let mut v = uart_view();
        v.paused = k == 5;
        draw(&v);
        ud::panel_button(k, k != 1, v.paused, true);
        ck.check_replica(&format!("decode/pressed/{k}"));
    }
    // The MF Settings tab with the Decode button (MF_DECODE).
    let base = &stock::scenes()[0];
    for combined in [false, true] {
        let mut m = base.m;
        m.combined = combined;
        let sc = stock::Scene { name: "mf-settings", m, popup: scope_ui::Popup::Mf(3), trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
        stock::draw(&sc);
        let r = scope_ui::MF_DECODE;
        let page = Bx::new(14, 61, 399, 54);
        ck.check_add(&format!("decode/mf-settings/{}", combined as u8), &[("page", page)], &[Bx::new(r.x as i32, r.y as i32, r.w as i32, r.h as i32)], Bx::new(13, 20, 401, 96));
        if !combined {
            save(out, "d04-mf-settings-decode");
        }
    }
}
