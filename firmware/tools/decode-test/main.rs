//! Host test of src/decode.rs (UART / Modbus RTU decoded from the scope's
//! circular stream) on synthetic sample streams: bit timing at sample rates
//! from 10.9 to 20 samples per bit, auto-baud, the formats and parity,
//! polarity, framing errors, noise and glitches, baud mismatch, auto
//! threshold on an idle input, stream laps (overrun), Modbus CRC / gap /
//! request-reply pairing, the DECode:FRAMes? text. Needs no hardware.
//!   tools/decode-test/run.sh
#[allow(dead_code)]
#[path = "../../src/decode.rs"]
mod decode;
use decode::*;

static mut BAD: u32 = 0;
static mut N: u32 = 0;
fn t(name: &str, ok: bool) {
    unsafe {
        N += 1;
        if !ok {
            BAD += 1;
            println!("FAIL {name}");
        }
    }
}

/// The scope ring (scope.rs CAP).
const CAP: usize = 26 * 1024;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    /// -1.0 .. 1.0
    fn f(&mut self) -> f64 {
        (self.next() % 20001) as f64 / 10000.0 - 1.0
    }
}

/// A line signal: segments of (seconds, mark) sampled at fs into codes.
struct Sig {
    seg: Vec<(f64, bool)>,
}

impl Sig {
    fn new() -> Sig {
        Sig { seg: Vec::new() }
    }
    fn idle(&mut self, s: f64) -> &mut Sig {
        self.seg.push((s, true));
        self
    }
    fn space(&mut self, s: f64) -> &mut Sig {
        self.seg.push((s, false));
        self
    }
    /// One character: start, data LSB first, parity (0 none, 1 even, 2
    /// odd; 3 = a wrong even bit), stop (`stop` false: a framing error).
    fn ch(&mut self, baud: f64, v: u16, bits: u8, parity: u8, stop: bool) -> &mut Sig {
        let b = 1.0 / baud;
        self.seg.push((b, false));
        let mut ones = 0;
        for i in 0..bits {
            let bit = v >> i & 1 == 1;
            ones += bit as u32;
            self.seg.push((b, bit));
        }
        match parity {
            1 => self.seg.push((b, ones % 2 == 1)),
            2 => self.seg.push((b, ones % 2 == 0)),
            3 => self.seg.push((b, ones % 2 == 0)),
            _ => {}
        }
        self.seg.push((b, stop));
        self
    }
    fn bytes(&mut self, baud: f64, d: &[u8], fmt: u8) -> &mut Sig {
        let (bits, par) = [(8, 0), (8, 1), (8, 2), (7, 1)][fmt as usize];
        for &v in d {
            self.ch(baud, v as u16, bits, par, true);
        }
        self
    }
    /// Codes at fs: mark = `hi` (normal) / `lo` (inverted), noise +-`noise`
    /// codes, a start phase of `ph` samples, glitches every `glitch` samples
    /// (0: none), 1 sample long.
    fn codes(&self, fs: f64, lo: f64, hi: f64, inv: bool, noise: f64, ph: f64, glitch: usize, rng: &mut Rng) -> Vec<u16> {
        let total: f64 = self.seg.iter().map(|s| s.0).sum();
        let n = ((total * fs) as usize).max(1);
        let mut out = Vec::with_capacity(n);
        let (mut k, mut t_end) = (0usize, self.seg[0].0);
        for i in 0..n {
            let t = (i as f64 + ph) / fs;
            while t >= t_end && k + 1 < self.seg.len() {
                k += 1;
                t_end += self.seg[k].0;
            }
            let mark = self.seg[k].1;
            let mut v = if mark != inv { hi } else { lo };
            v += noise * rng.f();
            if glitch > 0 && i % glitch == glitch / 2 {
                v = if v > (lo + hi) / 2.0 { lo } else { hi };
            }
            out.push(v.round().clamp(0.0, 1023.0) as u16);
        }
        out
    }
}

/// Feed `codes` into the ring in chunks (sizes from the rng) and poll.
struct Feed {
    ring: Vec<u16>,
    wr: u64,
}

impl Feed {
    fn new() -> Feed {
        Feed { ring: vec![512; CAP], wr: 0 }
    }
    fn feed(&mut self, d: &mut Dec, codes: &[u16], rng: &mut Rng, max_chunk: usize) {
        let mut i = 0;
        while i < codes.len() {
            let n = (100 + rng.next() as usize % max_chunk).min(codes.len() - i);
            for &c in &codes[i..i + n] {
                self.ring[(self.wr % CAP as u64) as usize] = c | 0xFC00; // upper IDR bits set: masked
                self.wr += 1;
            }
            i += n;
            d.poll(&self.ring, self.wr);
        }
    }
}

fn new_dec(cfg: Cfg, fs: u32) -> Box<Dec> {
    let mut d = Box::new(Dec::new());
    d.restart(cfg, fs, 0, None);
    d
}

/// All frames' bytes concatenated, and the frames.
fn frames(d: &Dec) -> (Vec<u8>, Vec<(Hdr, Vec<u8>)>) {
    let mut all = Vec::new();
    let mut fr = Vec::new();
    for s in d.st.first..d.st.next {
        let h = *d.st.get(s).unwrap();
        let mut b = vec![0u8; h.len as usize];
        d.st.copy(&h, &mut b);
        all.extend_from_slice(&b);
        fr.push((h, b));
    }
    (all, fr)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn mb(d: &[u8]) -> Vec<u8> {
    let mut v = d.to_vec();
    let c = crc16(d);
    v.push(c as u8);
    v.push((c >> 8) as u8);
    v
}

fn main() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let text = b"Hello, opendm40! 0123456789 The quick brown fox.\r\n";

    // ---- manual baud, 8N1, every rate at its stream rate, two phases ----
    for &baud in &[1200u32, 9600, 19200, 38400, 57600, 115_200] {
        for ph in [0.0, 0.5] {
            let fs = fs_for(baud);
            let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
            let mut s = Sig::new();
            // The idle tail ends the last frame (2 characters).
            s.idle(0.01).bytes(baud as f64, text, 0).idle(30.0 / baud as f64 + 0.01);
            let c = s.codes(fs as f64, 300.0, 700.0, false, 4.0, ph, 0, &mut rng);
            let mut f = Feed::new();
            f.feed(&mut d, &c, &mut rng, 4000);
            let (all, _) = frames(&d);
            t(&format!("8N1 {baud} ph {ph}: text decoded ({} B, fe {}, pe {})", all.len(), d.st.cnt.fe, d.st.cnt.pe), all == text && d.st.cnt.fe == 0 && d.st.cnt.pe == 0);
        }
    }
    t("fs_for 115200 = 1.25 MS/s", fs_for(115_200) == 1_250_000);
    t("fs_for 9600 = 125 kS/s", fs_for(9600) == 125_000);
    t("fs_for auto = 1.25 MS/s", fs_for(0) == 1_250_000);
    for &b in &STD_BAUD {
        let k = fs_for(b) as f64 / b as f64;
        t(&format!("fs_for {b}: >= 10.8 samples per bit"), k >= 10.8);
    }

    // ---- formats and parity ----
    for fmt in 0..4u8 {
        let baud = 19200;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt, inverted: false }, fs);
        let mut s = Sig::new();
        let data: Vec<u8> = if fmt == 3 { text.iter().map(|b| b & 0x7F).collect() } else { (0..=255u8).step_by(7).collect() };
        s.idle(0.005).bytes(baud as f64, &data, fmt).idle(0.01);
        let c = s.codes(fs as f64, 200.0, 800.0, false, 3.0, 0.3, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("{}: data, no parity / framing error", FMT_NAMES[fmt as usize]), all == data && d.st.cnt.pe == 0 && d.st.cnt.fe == 0);
    }
    {
        // A wrong parity bit on every 3rd character (8E1).
        let baud = 9600;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 1, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.005);
        for i in 0..30u16 {
            s.ch(baud as f64, 0x40 + i, 8, if i % 3 == 0 { 3 } else { 1 }, true);
        }
        s.idle(0.01);
        let c = s.codes(fs as f64, 200.0, 800.0, false, 3.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, fr) = frames(&d);
        t(&format!("8E1 wrong parity: 10 PE counted (got {}), data kept", d.st.cnt.pe), d.st.cnt.pe == 10 && all.len() == 30 && d.st.cnt.fe == 0);
        t("8E1 wrong parity: frame status PE", fr.iter().any(|(h, _)| status(h) == "PE"));
    }

    // ---- framing error ----
    {
        let baud = 38400;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002);
        for i in 0..20u16 {
            s.ch(baud as f64, 0x30 + i, 8, 0, i != 5);
            if i == 5 {
                s.idle(3.0 / baud as f64);
            }
        }
        s.idle(0.01);
        let c = s.codes(fs as f64, 100.0, 900.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, fr) = frames(&d);
        t(&format!("stop bit low: 1 FE (got {}), 20 bytes (got {})", d.st.cnt.fe, all.len()), d.st.cnt.fe == 1 && all.len() == 20);
        t("FE frame status", fr.iter().any(|(h, _)| status(h) == "FE"));
        // A break (line held low 10 ms): one FE character, then decoding goes on.
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002).bytes(baud as f64, b"AB", 0).space(0.01).idle(0.002).bytes(baud as f64, b"CD", 0).idle(0.01);
        let c = s.codes(fs as f64, 100.0, 900.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("break: one FE character, then CD ({:02X?})", all), d.st.cnt.fe == 1 && all.len() == 5 && all.ends_with(b"CD") && all.starts_with(b"AB"));
    }

    // ---- inverted (RS-232) on a +-10 V like swing, clipped ----
    {
        let baud = 9600;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: true }, fs);
        let mut s = Sig::new();
        s.idle(0.01).bytes(baud as f64, text, 0).idle(0.02);
        let c = s.codes(fs as f64, 0.0, 1023.0, true, 5.0, 0.7, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t("inverted 9600: text", all == text && d.st.cnt.fe == 0);
        // The same signal decoded as normal polarity: errors, not the text.
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("inverted signal as normal: no text (fe {})", d.st.cnt.fe), !contains(&all, b"Hello") && d.st.cnt.fe > 0);
    }

    // ---- noise and glitches ----
    {
        let baud = 115_200;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.003);
        for _ in 0..20 {
            s.bytes(baud as f64, text, 0);
        }
        s.idle(0.003);
        // Noise +-25 % of the swing; a 1-sample glitch every 997 samples.
        let c = s.codes(fs as f64, 300.0, 700.0, false, 100.0, 0.2, 997, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 5000);
        let (all, _) = frames(&d);
        let good = all.windows(text.len()).filter(|w| *w == text).count();
        t(&format!("115200, noise +-25 % + glitches: >= 18 of 20 lines intact (got {good}, fe {})", d.st.cnt.fe), good >= 18);
        // Glitches on an idle line: no frames.
        let mut d = new_dec(Cfg { modbus: false, baud: 9600, fmt: 0, inverted: false }, fs_for(9600));
        let mut s = Sig::new();
        s.idle(0.001).bytes(9600.0, b"U", 0).idle(0.3);
        let c = s.codes(fs_for(9600) as f64, 300.0, 700.0, false, 10.0, 0.0, 1501, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("1-sample glitches on idle: only the real byte ({:02X?})", all), all == b"U");
    }

    // ---- auto-baud ----
    for &baud in &[2400u32, 9600, 19200, 38400, 57600, 115_200] {
        let fs = fs_for(0);
        let mut d = new_dec(Cfg { modbus: false, baud: 0, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002).bytes(baud as f64, text, 0).idle(0.002).bytes(baud as f64, text, 0).idle(0.01);
        let c = s.codes(fs as f64, 250.0, 750.0, false, 6.0, 0.4, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        let want: Vec<u8> = [&text[..], &text[..]].concat();
        // At low rates the ring (21 ms at 1.25 MS/s) holds less than the
        // first line: the bytes before the lock that left the ring are lost.
        let full = (text.len() * 10) as f64 / baud as f64 + 0.002 < CAP as f64 / fs as f64;
        // Then decoding starts mid-character: up to 2 framing errors there.
        let ok = if full { all == want && d.st.cnt.fe == 0 } else { all.ends_with(text) && d.st.cnt.fe <= 2 };
        t(&format!("auto-baud {baud}: locked at {} (std), {} B decoded, fe {}", d.baud(), all.len(), d.st.cnt.fe), d.baud() == baud && ok);
    }
    {
        // A non-standard rate (100000): measured, not snapped.
        let fs = fs_for(0);
        let mut d = new_dec(Cfg::DEFAULT, fs);
        let mut s = Sig::new();
        s.idle(0.002).bytes(100_000.0, text, 0).idle(0.01);
        let c = s.codes(fs as f64, 250.0, 750.0, false, 3.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("auto-baud 100000: measured {} (within 3 %), text", d.baud()), d.baud().abs_diff(100_000) < 3000 && all == text);
    }
    {
        // The rate changes 9600 -> 38400 in auto: framing errors, a new lock, decoded again.
        let fs = fs_for(0);
        let mut d = new_dec(Cfg::DEFAULT, fs);
        let mut s = Sig::new();
        s.idle(0.002).bytes(9600.0, b"slow line\r\n", 0).idle(0.005);
        for _ in 0..6 {
            s.bytes(38400.0, text, 0).idle(0.001);
        }
        let c = s.codes(fs as f64, 250.0, 750.0, false, 3.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        let good = all.windows(text.len()).filter(|w| *w == text).count();
        t(&format!("auto 9600 -> 38400: relocked at {}, >= 4 of 6 lines (got {good})", d.baud()), d.baud() == 38400 && good >= 4);
    }

    // ---- baud mismatch (manual) ----
    for (sig, set) in [(9600u32, 19200u32), (19200, 9600), (9600, 10400)] {
        let fs = fs_for(set);
        let mut d = new_dec(Cfg { modbus: false, baud: set, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002);
        for _ in 0..4 {
            s.bytes(sig as f64, text, 0).idle(0.002);
        }
        let c = s.codes(fs as f64, 250.0, 750.0, false, 3.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t(&format!("signal {sig}, set {set}: framing errors ({}), text not recovered", d.st.cnt.fe), d.st.cnt.fe > 10 && !contains(&all, b"Hello"));
    }

    // ---- auto threshold ----
    {
        // Open input: noise of +-10 codes, no swing: nothing decoded.
        let fs = fs_for(0);
        let mut d = new_dec(Cfg::DEFAULT, fs);
        let c: Vec<u16> = (0..200_000).map(|_| (512.0 + 10.0 * rng.f()).round() as u16).collect();
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        t("open input (+-10 codes): no threshold, no frames", d.threshold().is_none() && d.st.next == 0 && d.st.cnt.fe == 0);
        // A 1.8 V logic swing at 2 V/div (~97 codes) on an offset: decoded.
        let mut d = new_dec(Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002).bytes(115_200.0, text, 0).idle(0.002);
        let c = s.codes(fs as f64, 512.0, 609.0, false, 4.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        let th = d.threshold().map(|t| t.0).unwrap_or(0);
        t(&format!("97-code swing: threshold {th} between the levels, text"), (540..580).contains(&th) && all == text);
        // Manual threshold: a level outside the swing gives nothing; inside, the text.
        let mut d = new_dec(Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: false }, fs);
        d.set_threshold(Some(700));
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        t("manual threshold above the signal: nothing", d.st.next == 0);
        let mut d = new_dec(Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: false }, fs);
        d.set_threshold(Some(560));
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (all, _) = frames(&d);
        t("manual threshold 560: text", all == text);
    }

    // ---- UART frames: grouping on gaps, at most UART_MAX bytes ----
    {
        let baud = 9600;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let cht = 10.0 / baud as f64;
        let mut s = Sig::new();
        s.idle(0.002).bytes(baud as f64, b"abc", 0).idle(cht * 0.5).bytes(baud as f64, b"def", 0).idle(cht * 3.0);
        s.bytes(baud as f64, b"ghi", 0).idle(cht * 3.0).bytes(baud as f64, &[b'x'; 40], 0).idle(0.01);
        let c = s.codes(fs as f64, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (_, fr) = frames(&d);
        let lens: Vec<usize> = fr.iter().map(|f| f.1.len()).collect();
        t(&format!("UART grouping: abcdef | ghi | 5 x 8 x ({lens:?})"), lens == vec![6, 3, 8, 8, 8, 8, 8] && fr[0].1 == b"abcdef");
        // Times: frame 1 starts 2 ms in, frame 2 9.5 chars later.
        let t1 = fr[0].0.t_us as f64 * 1e-6;
        let t2 = fr[1].0.t_us as f64 * 1e-6;
        t(&format!("frame times: {t1:.6} s, {t2:.6} s"), (t1 - 0.002).abs() < 20e-6 && (t2 - t1 - 9.5 * cht).abs() < 30e-6);
    }

    // ---- stream lap (the main loop came too late) ----
    {
        let baud = 115_200;
        let fs = fs_for(baud);
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.002);
        for _ in 0..30 {
            s.bytes(baud as f64, text, 0).idle(0.0005);
        }
        let c = s.codes(fs as f64, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        // Feed the first third normally, then one jump of 40 000 samples
        // (more than the ring) without a poll, then the rest.
        let a = c.len() / 3;
        f.feed(&mut d, &c[..a], &mut rng, 3000);
        for &x in &c[a..a + 40_000] {
            f.ring[(f.wr % CAP as u64) as usize] = x;
            f.wr += 1;
        }
        f.feed(&mut d, &c[a + 40_000..], &mut rng, 3000);
        let (all, fr) = frames(&d);
        let good = all.windows(text.len()).filter(|w| *w == text).count();
        t(&format!("lap: 1 overrun counted (got {}), an OVR frame, decoding goes on ({good} lines)", d.st.cnt.ovr), d.st.cnt.ovr == 1 && fr.iter().any(|(h, _)| h.flags & F_OVR != 0) && good >= 15);
    }

    // ---- Modbus RTU ----
    let req = mb(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]);
    let mut rsp_d = vec![0x01, 0x03, 0x14];
    for i in 0..10u16 {
        rsp_d.push((i >> 8) as u8);
        rsp_d.push(i as u8 + 0x30);
    }
    let rsp = mb(&rsp_d);
    let w6 = mb(&[0x11, 0x06, 0x00, 0x05, 0x12, 0x34]);
    let w16 = mb(&[0x11, 0x10, 0x00, 0x10, 0x00, 0x02, 0x04, 0x00, 0x0A, 0x01, 0x02]);
    let w16r = mb(&[0x11, 0x10, 0x00, 0x10, 0x00, 0x02]);
    let exr = mb(&[0x01, 0x04, 0x00, 0x00, 0x00, 0x01]);
    let exc = mb(&[0x01, 0x84, 0x02]);
    let mut bad = req.clone();
    bad[7] ^= 0x55;
    for &(baud, fmt) in &[(9600u32, 1u8), (19200, 0), (115_200, 1)] {
        let cfg = Cfg { modbus: true, baud: 0, fmt, inverted: false };
        let fs = fs_for(0);
        let mut d = new_dec(cfg, fs);
        let cht = (if fmt == 0 { 10.0 } else { 11.0 }) / baud as f64;
        let t35 = if baud > 19200 { 1.75e-3 } else { 3.5 * cht };
        let mut s = Sig::new();
        // Auto-baud needs traffic first: the sequence twice.
        s.idle(0.003);
        for _ in 0..2 {
            for (i, fr) in [&req, &rsp, &w6, &w6, &w16, &w16r, &exr, &exc, &bad].iter().enumerate() {
                s.bytes(baud as f64, fr, fmt);
                // Gaps: just above t3.5 after a request, longer otherwise.
                s.idle(if i % 2 == 0 { t35 * 1.3 } else { t35 * 2.5 });
            }
        }
        s.idle(0.02);
        // Inter-character gaps of 1 char inside a frame must not split it.
        let c = s.codes(fs as f64, 250.0, 750.0, false, 5.0, 0.1, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (_, fr) = frames(&d);
        // The second pass is decoded in full (the first may lose its start to the lock).
        let tail: Vec<&(Hdr, Vec<u8>)> = fr.iter().rev().take(9).rev().collect();
        let kinds: Vec<u8> = tail.iter().map(|f| f.0.kind).collect();
        let want = vec![K_REQ, K_RSP, K_REQ, K_RSP, K_REQ, K_RSP, K_REQ, K_EXC, K_MB];
        t(&format!("Modbus {baud} {}: kinds {kinds:?}", FMT_NAMES[fmt as usize]), kinds == want);
        t("Modbus: frames split on 3.5 chars, bytes intact", tail[0].1 == req && tail[1].1 == rsp && tail[4].1 == w16 && tail[7].1 == exc);
        t("Modbus: CRC bad on the last frame only", tail.iter().enumerate().all(|(i, f)| (f.0.flags & F_CRC != 0) == (i == 8)));
        let rseq = d.st.next - 9;
        t("Modbus: reply paired with its request", tail[1].0.req == rseq + 1 && tail[3].0.req == rseq + 2 + 1 && tail[7].0.req == rseq + 6 + 1);
        let lat = tail[1].0.lat_us as f64 * 1e-6;
        t(&format!("Modbus: reply time {:.0} us ~ the gap {:.0} us", lat * 1e6, t35 * 1.3e6), (lat - t35 * 1.3).abs() < 2.0 * cht + 50e-6);
        t(&format!("Modbus {baud}: counters crc {} fe {} pe {}", d.st.cnt.crc, d.st.cnt.fe, d.st.cnt.pe), d.st.cnt.crc >= 1 && d.st.cnt.fe == 0 && d.st.cnt.pe == 0);
        // Text of the request, reply, exception, write.
        let mut b = [0u8; 600];
        let mut w = W { b: &mut b, n: 0 };
        for s in d.st.next - 9..d.st.next {
            scpi_line(&mut w, &d.st, s);
        }
        let txt = w.as_str().to_string();
        let lines: Vec<&str> = txt.lines().collect();
        if baud == 9600 {
            for l in &lines {
                println!("  {l}");
            }
        }
        t("FRAMes? line: request", lines[0].ends_with(",MB,OK,01030000000AC5CD,REQ 01 03 @0000 n=10"));
        t("FRAMes? line: reply 20 B, paired", lines[1].contains(",MB,OK,010314") && lines[1].contains(",RSP 01 03 20B req="));
        t("FRAMes? line: write 06", lines[2].ends_with("REQ 11 06 @0005 =1234") && lines[3].contains("RSP 11 06 @0005 =1234 req="));
        t("FRAMes? line: exception", lines[7].contains("EXC 01 84 E02 req="));
        t("FRAMes? line: CRC", lines[8].contains(",MB,CRC,") && lines[8].ends_with(",?"));
    }
    {
        // A 1-character gap inside a frame does not split it; 4 chars do.
        let baud = 9600;
        let cfg = Cfg { modbus: true, baud, fmt: 0, inverted: false };
        let fs = fs_for(baud);
        let mut d = new_dec(cfg, fs);
        let cht = 10.0 / baud as f64;
        let mut s = Sig::new();
        s.idle(0.003).bytes(baud as f64, &req[..4], 0).idle(cht).bytes(baud as f64, &req[4..], 0).idle(cht * 4.0).bytes(baud as f64, &rsp, 0).idle(0.02);
        let c = s.codes(fs as f64, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let (_, fr) = frames(&d);
        t(&format!("Modbus: 1-char gap kept in, 4-char gap splits ({} frames)", fr.len()), fr.len() == 2 && fr[0].1 == req && fr[1].1 == rsp && fr[1].0.req == 1);
        // UART text of a frame in the FRAMes? format.
        let mut d = new_dec(Cfg { modbus: false, baud, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.003).bytes(baud as f64, b"Hi\r\n", 0).idle(0.02);
        let c = s.codes(fs as f64, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        let mut b = [0u8; 100];
        let mut w = W { b: &mut b, n: 0 };
        scpi_line(&mut w, &d.st, 0);
        let ts: f64 = w.as_str().split(',').nth(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
        t(&format!("FRAMes? line UART: {:?}", w.as_str()), (ts - 0.003).abs() < 12e-6 && w.as_str().starts_with("0,") && w.as_str().ends_with(",UART,OK,48690D0A,\n"));
    }

    // ---- the store: wrap, host cursor ----
    {
        let mut st = Box::new(Store::new());
        let mut d = Box::new(Dec::new());
        d.restart(Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: false }, 1_250_000, 5_000_000, None);
        let mut s = Sig::new();
        s.idle(0.001);
        for i in 0..400u32 {
            s.bytes(115_200.0, format!("{i:05}").as_bytes(), 0).idle(0.0002);
        }
        let c = s.codes(1.25e6, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        f.feed(&mut d, &c, &mut rng, 3000);
        t(&format!("store: {} frames kept of {} (NF {NF})", d.st.len(), d.st.next), d.st.len() == NF as u32 && d.st.next == 400);
        let h = *d.st.get(d.st.next - 1).unwrap();
        let mut b = [0u8; 8];
        let n = d.st.copy(&h, &mut b);
        t("store: the newest frame's bytes", &b[..n] == b"00399");
        t("store: host cursor follows the oldest kept", d.st.host == d.st.first);
        t("store: times since boot (t0 5 s)", h.t_us > 5_000_000);
        d.clear();
        t("clear: empty, counters 0", d.st.len() == 0 && d.st.cnt.frames == 0 && d.st.cnt.bytes == 0);
        let _ = &mut st;
    }

    // ---- CRC ----
    t("crc16 of 01 03 00 00 00 0A = 0xCDC5", crc16(&[1, 3, 0, 0, 0, 0x0A]) == 0xCDC5);

    // ---- speed (host, for orders of magnitude only) ----
    {
        let fs = 1_250_000;
        let mut d = new_dec(Cfg { modbus: false, baud: 115_200, fmt: 0, inverted: false }, fs);
        let mut s = Sig::new();
        s.idle(0.001);
        for _ in 0..200 {
            s.bytes(115_200.0, text, 0);
        }
        let c = s.codes(fs as f64, 250.0, 750.0, false, 2.0, 0.0, 0, &mut rng);
        let mut f = Feed::new();
        let t0 = std::time::Instant::now();
        f.feed(&mut d, &c, &mut rng, 3000);
        let dt = t0.elapsed().as_secs_f64();
        println!("  speed: {:.1} Msamples/s on the host ({} samples, {} bytes)", c.len() as f64 / dt / 1e6, c.len(), d.st.cnt.bytes);
    }

    let (n, bad) = unsafe { (N, BAD) };
    println!("decode-test: {n} checks, {bad} failed");
    std::process::exit(if bad == 0 { 0 } else { 1 });
}
