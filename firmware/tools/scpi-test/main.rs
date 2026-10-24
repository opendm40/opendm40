//! Host test of src/scpi.rs (Scpi::handle and parse_num_u). The crate
//! modules scpi.rs refers to are stubs here; the vdisk DBG block is left out
//! (no `vdisk` feature). run.sh builds and runs it.

#[path = "../../src/ident.rs"]
mod ident;
#[path = "../../src/scpi.rs"]
mod scpi;

mod boot {
    pub fn put_info(_s: &mut crate::scpi::Scpi) {}
    pub fn enter_loader_update() -> bool {
        false
    }
}
mod clock {
    pub const HCLK_HZ: u32 = 200_000_000;
}
mod csvlog {
    pub const INTERVALS_S: [u32; 4] = [0, 1, 10, 60];
    pub const DURATIONS_S: [u32; 5] = [0, 600, 3600, 8 * 3600, 24 * 3600];
    pub fn parse_name(s: &[u8]) -> Option<[u8; 11]> {
        if s.is_empty() { None } else { Some([b' '; 11]) }
    }
}
mod danger {
    pub const LOUD: u8 = 0;
    pub const VOLUME: u8 = 1;
    pub const OFF: u8 = 2;
}
mod dmm {
    pub const FUNC_V_AUTO: u8 = 19;
    pub const FUNC_A_AUTO: u8 = 20;
    pub const T_NAMES: [&str; 7] = ["TCK", "PT100", "PT1000", "NTC10K3950", "NTC10K3435", "NTC100K3950", "NTCUSER"];
    pub struct Val {
        pub dec: u8,
        pub unit: &'static str,
        pub ol: bool,
        pub ext: bool,
    }
    pub struct Reading {
        pub main: Val,
        pub si: f32,
    }
    pub mod pw {
        pub const FUNC: u8 = 18;
        pub const V_LIM: [f32; 4] = [6.0, 60.0, 600.0, 1000.0];
        pub const I_LIM: [f32; 6] = [600e-6, 6e-3, 0.06, 0.6, 6.0, 10.0];
        pub const R34_XY: u8 = 0x88;
        pub const R34_XX: u8 = 0x80;
        pub const R34_YY: u8 = 0x8C;
        pub const T_LIM_DEFAULT: u16 = 30;
        pub const COUP_ACDC: u8 = 0;
        pub const COUP_DC: u8 = 1;
        pub const COUP_AC: u8 = 3;
    }
}
mod genuine {
    #[derive(PartialEq)]
    pub enum GenuineResult {
        Genuine,
    }
    pub struct Inspection {
        pub uid: [u8; 12],
        pub stored: [u8; 12],
        pub expected: [u8; 12],
        pub result: GenuineResult,
    }
    pub fn inspect() -> Inspection {
        Inspection { uid: [0; 12], stored: [0; 12], expected: [0; 12], result: GenuineResult::Genuine }
    }
}
mod logger {
    pub const FMT_ROWS: u8 = 0;
    pub const FMT_PACKED: u8 = 1;
    pub const FMT_DLG: u8 = 2;
}
mod hyspi {
    pub const N_MAX: u32 = 2000;
}
mod sentinel {
    pub const M_ALL: u8 = 15;
    pub static QUES_EVENT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);
}
mod uid {
    pub fn bytes() -> &'static [u8; 24] {
        b"000102030405060708090A0B"
    }
}

use scpi::{parse_num, parse_num_u, Cmd, Log, Out, Rng, Scpi};

static mut FAILS: u32 = 0;

fn check(ok: bool, what: &str) {
    if ok {
        println!("ok   {what}");
    } else {
        println!("FAIL {what}");
        unsafe { FAILS += 1 };
    }
}

fn close(a: Option<f32>, b: f32) -> bool {
    a.is_some_and(|a| (a - b).abs() <= b.abs() * 1e-6)
}

/// The reply text of `msg` ("" when none).
fn reply(s: &mut Scpi, msg: &str) -> String {
    let _ = s.handle(msg.as_bytes(), 1);
    String::from_utf8_lossy(&s.buf[..s.len]).into_owned()
}

fn err(s: &mut Scpi) -> String {
    reply(s, "SYST:ERR?")
}

fn suffixes() {
    let cases: &[(&str, &[u8], Option<f32>)] = &[
        ("1", b"", Some(1.0)),
        ("1m", b"", Some(1e-3)),
        ("1M", b"", Some(1e-3)),
        ("1MA", b"", Some(1e6)),
        ("1ma", b"", Some(1e6)),
        ("2.5k", b"", Some(2.5e3)),
        ("2.5K", b"", Some(2.5e3)),
        ("3u", b"", Some(3e-6)),
        ("3U", b"", Some(3e-6)),
        ("4n", b"", Some(4e-9)),
        ("1G", b"", Some(1e9)),
        ("7p", b"", Some(7e-12)),
        ("-1.5k", b"", Some(-1.5e3)),
        ("2e-3", b"", Some(2e-3)),
        ("2E+3", b"", Some(2e3)),
        ("1 k", b"", Some(1e3)),
        ("1x", b"", None),
        ("1V", b"", None),
        ("1E", b"", None),
        ("1E99999", b"", None),
        ("", b"", None),
        ("k", b"", None),
        ("1MHZ", b"HZ", Some(1e6)),
        ("1mhz", b"HZ", Some(1e6)),
        ("1MAHZ", b"HZ", Some(1e6)),
        ("1KHZ", b"HZ", Some(1e3)),
        ("1khz", b"HZ", Some(1e3)),
        ("50HZ", b"HZ", Some(50.0)),
        ("1M", b"HZ", Some(1e-3)),
        ("1MA", b"HZ", Some(1e6)),
        ("2.5 MHz", b"HZ", Some(2.5e6)),
        ("1V", b"HZ", None),
        ("1MOHM", b"OHM", Some(1e6)),
        ("1mohm", b"OHM", Some(1e6)),
        ("10KOHM", b"OHM", Some(1e4)),
        ("60OHM", b"OHM", Some(60.0)),
        ("5MA", b"A", Some(5e-3)),
        ("5ma", b"A", Some(5e-3)),
        ("5MAA", b"A", Some(5e6)),
        ("600UA", b"A", Some(600e-6)),
        ("5A", b"A", Some(5.0)),
        ("100 mV", b"V", Some(0.1)),
        ("100MV", b"V", Some(0.1)),
        ("6V", b"V", Some(6.0)),
        ("100US", b"S", Some(1e-4)),
        ("2MS", b"S", Some(2e-3)),
        ("50PCT", b"PCT", Some(50.0)),
    ];
    for &(t, u, want) in cases {
        let got = parse_num_u(t.as_bytes(), u);
        let ok = match want {
            Some(w) => close(got, w),
            None => got.is_none(),
        };
        check(ok, &format!("parse_num_u({t:?}, {:?}) = {got:?}, want {want:?}", String::from_utf8_lossy(u)));
    }
    check(close(parse_num(b"1m"), 1e-3), "parse_num(\"1m\") is milli (IEEE 488.2), not mega");
}

fn set_commands() {
    let mut s = Scpi::new();
    // Good values.
    check(matches!(s.handle(b"SOUR:FREQ 1KHZ", 1), Out::Instr(Cmd::GenFreq(Some(v))) if v == 1000.0), "SOUR:FREQ 1KHZ");
    check(matches!(s.handle(b"sour:freq 2.5mhz", 1), Out::Instr(Cmd::GenFreq(Some(v))) if v == 2.5e6), "sour:freq 2.5mhz = 2.5 MHz");
    check(matches!(s.handle(b"SOUR:FREQ 1M", 1), Out::Instr(Cmd::GenFreq(Some(v))) if (v - 1e-3).abs() < 1e-9), "SOUR:FREQ 1M = 1 mHz");
    check(matches!(s.handle(b"SOUR:FREQ 1MA", 1), Out::Instr(Cmd::GenFreq(Some(v))) if v == 1e6), "SOUR:FREQ 1MA = 1 MHz");
    check(matches!(s.handle(b"SOUR:FREQ?", 1), Out::Instr(Cmd::GenFreq(None))), "SOUR:FREQ? is the query");
    check(matches!(s.handle(b"SOUR:VOLT 500MV", 1), Out::Instr(Cmd::GenVolt(Some(v))) if (v - 0.5).abs() < 1e-6), "SOUR:VOLT 500MV");
    check(matches!(s.handle(b"SOUR:VOLT:OFFS AUTO", 1), Out::Instr(Cmd::GenOffs(Some(v))) if v.is_nan()), "SOUR:VOLT:OFFS AUTO");
    check(matches!(s.handle(b"SOUR:FUNC:SQU:DCYC 25", 1), Out::Instr(Cmd::GenDuty(Some(v))) if v == 25.0), "SOUR:FUNC:SQU:DCYC 25");
    check(matches!(s.handle(b"SOUR:FUNC tri", 1), Out::Instr(Cmd::GenFunc(Some(2)))), "SOUR:FUNC tri");
    check(matches!(s.handle(b"OUTP ON", 1), Out::Instr(Cmd::GenOut(Some(true)))), "OUTP ON");
    check(matches!(s.handle(b"TIM:SCAL 100US", 1), Out::Instr(Cmd::TimScale(Some(v))) if (v - 1e-4).abs() < 1e-9), "TIM:SCAL 100US");
    check(matches!(s.handle(b"TIM:MAIN:SCAL 1m", 1), Out::Instr(Cmd::TimScale(Some(v))) if (v - 1e-3).abs() < 1e-9), "TIM:MAIN:SCAL 1m");
    check(matches!(s.handle(b"CHAN1:SCAL 0.5", 1), Out::Instr(Cmd::ChScale(Some(v))) if v == 0.5), "CHAN1:SCAL 0.5");
    check(matches!(s.handle(b"CHAN:COUP AC", 1), Out::Instr(Cmd::ChCoup(Some(true)))), "CHAN:COUP AC");
    check(matches!(s.handle(b"TRIG:SLOP NEG", 1), Out::Instr(Cmd::TrigSlope(Some(false)))), "TRIG:SLOP NEG");
    check(matches!(s.handle(b"TRIG:EDGE:SLOP POS", 1), Out::Instr(Cmd::TrigSlope(Some(true)))), "TRIG:EDGE:SLOP POS");
    check(matches!(s.handle(b"TRIG:EDGE:LEV 1.5", 1), Out::Instr(Cmd::TrigLevel(Some(v))) if v == 1.5), "TRIG:EDGE:LEV 1.5");
    check(matches!(s.handle(b"TRIG:SWE NORM", 1), Out::Instr(Cmd::TrigSweep(Some(1)))), "TRIG:SWE NORM");
    check(matches!(s.handle(b"VOLT:RANG 600MV", 1), Out::Instr(Cmd::Range(_, Rng::Value(v))) if (v - 0.6).abs() < 1e-6), "VOLT:RANG 600MV");
    check(matches!(s.handle(b"CURR:RANG 6MA", 1), Out::Instr(Cmd::Range(_, Rng::Value(v))) if (v - 6e-3).abs() < 1e-9), "CURR:RANG 6MA = 6 mA");
    check(matches!(s.handle(b"RES:RANG 6MOHM", 1), Out::Instr(Cmd::Range(_, Rng::Value(v))) if v == 6e6), "RES:RANG 6MOHM = 6 Mohm");
    check(matches!(s.handle(b"CONF:VOLT:DC 60mV", 1), Out::Instr(Cmd::Conf(_, Rng::Value(v), None)) if (v - 0.06).abs() < 1e-6), "CONF:VOLT:DC 60mV");
    check(err(&mut s) == "0,\"No error\"", "no error after the good values");

    // Bad values: no reply, no Cmd (the setting kept), an error.
    let bad: &[(&str, &str)] = &[
        ("SOUR:FREQ abc", "-224"),
        ("SOUR:FREQ", "-109"),
        ("SOUR:FREQ -5", "-222"),
        ("SOUR:FREQ 0", "-222"),
        ("SOUR:FREQ 1V", "-224"),
        ("SOUR:VOLT x", "-224"),
        ("SOUR:VOLT -1", "-222"),
        ("SOUR:VOLT:OFFS x", "-224"),
        ("SOUR:FUNC:SQU:DCYC 150", "-222"),
        ("SOUR:FUNC NOISE", "-224"),
        ("SOUR:FUNC", "-109"),
        ("OUTP MAYBE", "-224"),
        ("OUTP", "-109"),
        ("TIM:SCAL fast", "-224"),
        ("TIM:SCAL 0", "-222"),
        ("TIM:POS 1", "-113"),
        ("CHAN1:SCAL x", "-224"),
        ("CHAN1:COUP GND", "-224"),
        ("CHAN1:OFFS x", "-224"),
        ("CHAN1:DISP ON", "-113"),
        ("TRIG:SLOP UP", "-224"),
        ("TRIG:SWE NEVER", "-224"),
        ("TRIG:LEV x", "-224"),
        ("TRIG:LEV", "-109"),
        ("LOG:STAT MAYBE", "-224"),
        ("LOG:OSR 9", "-224"),
        ("LOG:CHOP 2", "-224"),
        ("LOG:CSV x", "-224"),
        ("VOLT:RANG x", "-224"),
        ("VOLT:RANG", "-109"),
        ("FOO:BAR 1", "-113"),
        ("*IDN? 5", "-108"),
        ("*CLS 1", "-108"),
        ("READ? 2", "-108"),
        ("SOUR#FREQ 5", "-102"),
        ("SYST:ERR??", "-102"),
    ];
    for &(m, code) in bad {
        let o = s.handle(m.as_bytes(), 1);
        let none = matches!(o, Out::None) && s.len == 0;
        let e = err(&mut s);
        check(none && e.starts_with(&format!("{code},\"")), &format!("{m:?}: no reply / Cmd ({none}), SYST:ERR? {e:?} (want {code})"));
    }
    let long = format!("SOUR:FREQ {}", "1".repeat(70));
    let o = s.handle(long.as_bytes(), 1);
    let e = err(&mut s);
    check(matches!(o, Out::None) && e.starts_with("-102,"), &format!("a command over 64 bytes: refused, {e:?}"));
    // A bad value inside a message: the rest still runs.
    let o = s.handle(b"SOUR:FREQ x;SOUR:FREQ?", 1);
    check(matches!(o, Out::Instr(Cmd::GenFreq(None))) && err(&mut s).starts_with("-224,"), "SOUR:FREQ x;SOUR:FREQ?: the query runs, -224 queued");
    check(matches!(s.handle(b"LOG:STAT ON", 1), Out::Log(Log::State(Some(true)))), "LOG:STAT ON");
}

fn error_queue() {
    let mut s = Scpi::new();
    check(err(&mut s) == "0,\"No error\"", "empty queue: 0,\"No error\"");
    check(reply(&mut s, "SYST:ERR:COUN?") == "0", "SYST:ERR:COUN? 0");
    s.set_err("Settings conflict; detail text");
    s.set_err("Data stale");
    s.set_err("Undefined header");
    check(reply(&mut s, "SYST:ERR:COUN?") == "3", "SYST:ERR:COUN? 3");
    check(err(&mut s) == "-221,\"Settings conflict; detail text\"", "FIFO 1: -221 with its detail");
    check(err(&mut s) == "-230,\"Data stale\"", "FIFO 2: -230");
    check(err(&mut s) == "-113,\"Undefined header\"", "FIFO 3: -113");
    check(err(&mut s) == "0,\"No error\"", "then empty");
    for _ in 0..12 {
        s.set_err("Command error");
    }
    check(reply(&mut s, "SYST:ERR:COUN?") == scpi::ERRQ.to_string(), "overflow: the queue holds ERRQ entries");
    for i in 0..scpi::ERRQ - 1 {
        let e = err(&mut s);
        check(e == "-100,\"Command error\"", &format!("overflow: entry {i} kept ({e})"));
    }
    check(err(&mut s) == "-350,\"Queue overflow\"", "overflow: the newest entry is -350");
    check(err(&mut s) == "0,\"No error\"", "overflow: then empty");
    s.set_err("Data out of range");
    s.set_err("Mass storage busy");
    let _ = s.handle(b"*RST", 1);
    check(reply(&mut s, "SYST:ERR:COUN?") == "2", "*RST keeps the queue");
    let _ = s.handle(b"*CLS", 1);
    check(err(&mut s) == "0,\"No error\"", "*CLS clears the queue");
    let codes: &[(&str, i16)] = &[
        ("Command error", -100),
        ("Syntax error", -102),
        ("Parameter not allowed", -108),
        ("Missing parameter", -109),
        ("Undefined header", -113),
        ("Execution error", -200),
        ("Command protected", -203),
        ("Settings conflict", -221),
        ("Data out of range", -222),
        ("Too much data", -223),
        ("Illegal parameter value", -224),
        ("Data stale", -230),
        ("Queue overflow", -350),
        ("Query error", -400),
        ("Query INTERRUPTED", -410),
        ("File name not found", -256),
        ("Upload not started", -200),
    ];
    for &(m, c) in codes {
        check(scpi::err_code(m) == c, &format!("err_code({m:?}) = {} (want {c})", scpi::err_code(m)));
    }
}

fn idn() {
    let mut s = Scpi::new();
    let r = reply(&mut s, "*IDN?");
    check(r == format!("Alientek,DM40A,000102030405060708090A0B,opendm40 {}", ident::VERSION), &format!("*IDN? {r}"));
    let _ = s.handle(b"SYST:IDN:EMUL 34401A", 1);
    let r = reply(&mut s, "*IDN?");
    check(r == format!("HEWLETT-PACKARD,34401A,0,opendm40 {}", ident::VERSION), &format!("*IDN? emulated {r}"));
    let _ = s.handle(b"SYST:IDN:EMUL OFF", 1);
    let want = std::env::var("FW_VERSION").unwrap_or_default();
    check(!want.is_empty() && ident::VERSION == want, &format!("ident::VERSION {:?} = tools/fw-version.sh {want:?}", ident::VERSION));
}

fn main() {
    suffixes();
    set_commands();
    error_queue();
    idn();
    let f = unsafe { FAILS };
    if f > 0 {
        println!("{f} FAILED");
        std::process::exit(1);
    }
    println!("PASS");
}
