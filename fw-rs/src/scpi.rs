//! Minimal SCPI command layer for the USBTMC interface (DMM subset).
//!
//! Commands are case-insensitive, long or short form (MEASure -> MEAS), several
//! per message separated by ';'. Supported:
//!   *IDN?  *RST  *CLS  *OPC?  *ESR?  *STB?  SYSTem:ERRor?
//!   CONFigure:<f>  MEASure:<f>?  READ?  FETCh?  [SENSe:]FUNCtion?
//!   [SENSe:]FUNCtion "<f>"
//! where <f> = VOLTage[:DC] | VOLTage:AC | CURRent[:DC] | CURRent:AC |
//! RESistance | CONTinuity | DIODe | CAPacitance | FREQuency | TEMPerature.
//! Functions are stock DMM mode numbers (see dmm.rs).

use crate::dmm::Reading;

pub const IDN: &[u8] = b"Alientek,DM40A,0,dm40-fw 0.1";

pub enum Out {
    /// Nothing to send (command without a query).
    None,
    /// Reply text is in the buffer.
    Text,
    /// Switch function (CONFigure / FUNCtion set), no reply.
    Configure(u8),
    /// Switch function (if Some) and reply with the next fresh reading.
    Measure(Option<u8>),
    /// Reply with the last reading.
    Fetch,
}

pub struct Scpi {
    pub buf: [u8; 128],
    pub len: usize,
    err: Option<&'static str>,
}

/// Match `tok` (upper case) against a mnemonic written as e.g. "MEASure":
/// accepted are the short form (upper-case letters) and the full word.
fn kw(tok: &[u8], mnem: &str) -> bool {
    let short: usize = mnem.bytes().take_while(|b| b.is_ascii_uppercase() || *b == b'*').count();
    let full = mnem.len();
    let m = mnem.as_bytes();
    let eq = |n: usize| tok.len() == n && tok.iter().zip(m).all(|(a, b)| *a == b.to_ascii_uppercase());
    eq(short) || eq(full)
}

fn parse_func(parts: &[&[u8]]) -> Option<u8> {
    let p0 = *parts.first()?;
    let ac = parts.get(1).map(|p| kw(p, "AC")).unwrap_or(false);
    let dc_or_none = parts.get(1).map(|p| kw(p, "DC")).unwrap_or(true);
    if kw(p0, "VOLTage") {
        if ac { Some(0) } else if dc_or_none { Some(1) } else { None }
    } else if kw(p0, "CURRent") {
        if ac { Some(5) } else if dc_or_none { Some(8) } else { None }
    } else if kw(p0, "RESistance") || kw(p0, "FRESistance") {
        Some(10)
    } else if kw(p0, "CONTinuity") {
        Some(11)
    } else if kw(p0, "DIODe") {
        Some(12)
    } else if kw(p0, "CAPacitance") {
        Some(13)
    } else if kw(p0, "FREQuency") {
        Some(14)
    } else if kw(p0, "TEMPerature") {
        Some(15)
    } else {
        None
    }
}

fn func_name(mode: u8) -> &'static str {
    match mode {
        0 | 2 => "\"VOLT:AC\"",
        1 | 3 => "\"VOLT\"",
        4..=6 => "\"CURR:AC\"",
        7..=9 => "\"CURR\"",
        10 => "\"RES\"",
        11 => "\"CONT\"",
        12 => "\"DIOD\"",
        13 => "\"CAP\"",
        14 => "\"FREQ\"",
        _ => "\"TEMP\"",
    }
}

impl Scpi {
    pub const fn new() -> Self {
        Scpi { buf: [0; 128], len: 0, err: None }
    }

    fn put(&mut self, s: &[u8]) {
        let n = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s[..n]);
        self.len += n;
    }

    /// Process one message. Returns what the caller must do; text replies are
    /// accumulated in `buf` (cleared at the start).
    pub fn handle(&mut self, msg: &[u8], cur: u8) -> Out {
        self.len = 0;
        let mut out = Out::None;
        for cmd in msg.split(|&b| b == b';' || b == b'\n' || b == b'\r') {
            let cmd = trim(cmd);
            if cmd.is_empty() {
                continue;
            }
            let mut up = [0u8; 64];
            let n = cmd.len().min(64);
            for (d, s) in up.iter_mut().zip(&cmd[..n]) {
                *d = s.to_ascii_uppercase();
            }
            let cmd = &up[..n];
            let (head, arg) = match cmd.iter().position(|&b| b == b' ') {
                Some(i) => (&cmd[..i], trim(&cmd[i + 1..])),
                None => (cmd, &cmd[..0]),
            };
            let query = head.last() == Some(&b'?');
            let head = if query { &head[..head.len() - 1] } else { head };
            let mut parts_buf: [&[u8]; 6] = [&[]; 6];
            let mut np = 0;
            for p in head.split(|&b| b == b':').filter(|p| !p.is_empty()) {
                if np < 6 {
                    parts_buf[np] = p;
                    np += 1;
                }
            }
            let parts = &parts_buf[..np];
            let Some(&p0) = parts.first() else { continue };

            if self.len > 0 && query {
                self.put(b";");
            }
            match (p0, query) {
                (b"*IDN", true) => self.put(IDN),
                (b"*RST", false) | (b"*CLS", false) => self.err = None,
                (b"*OPC", true) => self.put(b"1"),
                (b"*ESR", true) | (b"*STB", true) => self.put(b"0"),
                _ if kw(p0, "SYSTem") && parts.len() > 1 && kw(parts[1], "ERRor") && query => {
                    match self.err.take() {
                        Some(e) => {
                            self.put(b"-100,\"");
                            self.put(e.as_bytes());
                            self.put(b"\"");
                        }
                        None => self.put(b"0,\"No error\""),
                    }
                }
                _ if kw(p0, "MEASure") && query => match parse_func(&parts[1..]) {
                    Some(f) => out = Out::Measure(Some(f)),
                    None => self.err = Some("Command error"),
                },
                _ if kw(p0, "CONFigure") && !query => match parse_func(&parts[1..]) {
                    Some(f) => out = Out::Configure(f),
                    None => self.err = Some("Command error"),
                },
                (b"READ", true) => out = Out::Measure(None),
                _ if kw(p0, "FETCh") && query => out = Out::Fetch,
                _ if kw(p0, "SENSe") || kw(p0, "FUNCtion") => {
                    let rest = if kw(p0, "SENSe") { &parts[1..] } else { parts };
                    if rest.first().map(|p| kw(p, "FUNCtion")).unwrap_or(false) {
                        if query {
                            self.put(func_name(cur).as_bytes());
                        } else {
                            let a = trim(arg);
                            let a = a.strip_prefix(b"\"").unwrap_or(a);
                            let a = a.strip_suffix(b"\"").unwrap_or(a);
                            let mut ap: [&[u8]; 3] = [&[]; 3];
                            let mut k = 0;
                            for p in a.split(|&b| b == b':').filter(|p| !p.is_empty()) {
                                if k < 3 {
                                    ap[k] = p;
                                    k += 1;
                                }
                            }
                            match parse_func(&ap[..k]) {
                                Some(f) => out = Out::Configure(f),
                                None => self.err = Some("Illegal parameter value"),
                            }
                        }
                    } else {
                        self.err = Some("Undefined header");
                    }
                }
                _ => self.err = Some("Undefined header"),
            }
        }
        match out {
            Out::None if self.len > 0 => Out::Text,
            o => o,
        }
    }

    /// Format a reading as SCPI NR3 (+1.234567E+00), or the overload value.
    pub fn put_reading(&mut self, r: &Reading) {
        if r.main.ol {
            self.put(b"+9.90000000E+37");
            return;
        }
        let mut b = [0u8; 20];
        let n = nr3(&mut b, r.si);
        self.put(&b[..n]);
    }
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
    let e = s.iter().rposition(|b| !b.is_ascii_whitespace()).map(|i| i + 1).unwrap_or(a);
    &s[a..e.max(a)]
}

/// SCPI NR3 with 7 significant digits: "+1.234567E-03".
fn nr3(b: &mut [u8; 20], v: f32) -> usize {
    let mut i = 0;
    b[i] = if v < 0.0 { b'-' } else { b'+' };
    i += 1;
    let mut x = if v < 0.0 { -v } else { v };
    let mut e: i32 = 0;
    if x != 0.0 {
        while x >= 10.0 {
            x /= 10.0;
            e += 1;
        }
        while x < 1.0 {
            x *= 10.0;
            e -= 1;
        }
    }
    let mut m = (x * 1_000_000.0 + 0.5) as u32;
    if m >= 10_000_000 {
        m /= 10;
        e += 1;
    }
    let digits = [m / 1_000_000, m / 100_000 % 10, m / 10_000 % 10, m / 1000 % 10, m / 100 % 10, m / 10 % 10, m % 10];
    b[i] = b'0' + digits[0] as u8;
    i += 1;
    b[i] = b'.';
    i += 1;
    for d in &digits[1..] {
        b[i] = b'0' + *d as u8;
        i += 1;
    }
    b[i] = b'E';
    i += 1;
    b[i] = if e < 0 { b'-' } else { b'+' };
    i += 1;
    let ea = e.unsigned_abs();
    b[i] = b'0' + (ea / 10 % 10) as u8;
    b[i + 1] = b'0' + (ea % 10) as u8;
    i + 2
}
