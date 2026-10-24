//! Host test of src/scpi_msg.rs: compound SCPI messages run command by
//! command with one joined response (IEEE 488.2: replies in order, ';'
//! between them, one terminator; a message with a query always gets a
//! response). A small fake instrument stands in for main.rs's dispatch:
//! text queries, deferred commands that set an error, a reading wait that
//! pauses the message, block replies sent from the transport's buffer.
//! No hardware. run.sh builds and runs it.

#[path = "../../src/scpi_msg.rs"]
#[allow(dead_code)]
mod scpi_msg;

use scpi_msg::*;

#[derive(Default)]
struct Dev {
    err: Option<&'static str>,
    outp: bool,
    /// Responses sent (one per message at most).
    sent: Vec<Vec<u8>>,
    /// A READ? waiting for its reading.
    waiting: bool,
}

enum Act {
    None,
    Text(Vec<u8>),
    Wait,
    Raw(Vec<u8>),
}

impl Dev {
    /// One command, as Scpi::handle + main.rs's dispatch would run it.
    fn exec(&mut self, part: &[u8], multi: bool) -> Act {
        let s = String::from_utf8_lossy(part).trim().to_ascii_uppercase();
        let s = s.trim_start_matches(':').to_string();
        let (head, arg) = match s.find(' ') {
            Some(i) => (s[..i].to_string(), s[i + 1..].trim().to_string()),
            None => (s.clone(), String::new()),
        };
        match head.as_str() {
            "*IDN?" => Act::Text(b"Alientek,DM40A,0,opendm40 0.1".to_vec()),
            "*OPC?" => Act::Text(b"1".to_vec()),
            "*CLS" => {
                self.err = None;
                Act::None
            }
            "SYST:ERR?" => Act::Text(match self.err.take() {
                Some(e) => format!("-100,\"{}\"", e).into_bytes(),
                None => b"0,\"No error\"".to_vec(),
            }),
            // Deferred to the app (Out::Instr): refused with a reason.
            "POW:PHAS:CAL" => {
                if arg != "RES" {
                    self.err = Some("Illegal parameter value");
                } else {
                    self.err = Some("FLAT");
                }
                Act::None
            }
            "POW:PHAS:CAL?" => Act::Text(b"FLAT".to_vec()),
            "OUTP" => {
                self.outp = arg == "ON" || arg == "1";
                Act::None
            }
            "OUTP?" => Act::Text(if self.outp { b"1".to_vec() } else { b"0".to_vec() }),
            "READ?" => {
                self.waiting = true;
                Act::Wait
            }
            "DBG:OK" => Act::Text(b"OK".to_vec()),
            "WAV:DATA?" => {
                let mut v = b"#3400".to_vec();
                v.extend(std::iter::repeat(b'x').take(400));
                Act::Text(v)
            }
            "MMEM:DATA?" => {
                if multi {
                    self.err = Some("Query error");
                    return Act::Text(vec![]);
                }
                if arg.contains("NOPE") {
                    self.err = Some("File name not found");
                    Act::None
                } else {
                    Act::Raw(b"#40003abc".to_vec())
                }
            }
            _ => {
                self.err = Some("Undefined header");
                Act::None
            }
        }
    }

    /// The main loop's passes until the message has its response: the
    /// reading arrives `delay` passes after READ? waits.
    fn run(&mut self, m: &mut Msg, msg: &[u8]) {
        m.start(msg);
        let mut part = [0u8; 256];
        for _pass in 0..100 {
            while let Some(n) = m.next(&mut part) {
                let multi = m.multi();
                match self.exec(&part[..n], multi) {
                    Act::None => {}
                    Act::Text(t) => assert!(m.reply(&t), "reply room"),
                    Act::Raw(t) => {
                        self.sent.push(t);
                        m.raw_sent();
                    }
                    Act::Wait => {
                        m.pause();
                        break;
                    }
                }
            }
            if self.waiting {
                // A later pass: the reading comes.
                self.waiting = false;
                m.reply(b"+1.234567E+00");
                continue;
            }
            if let Some(r) = m.finish() {
                self.sent.push(r.to_vec());
            }
            if !m.active() {
                return;
            }
        }
        panic!("message never finished");
    }
}

fn case(msg: &str, want: Option<&str>) -> Dev {
    let mut d = Dev::default();
    let mut m = Msg::new();
    d.run(&mut m, msg.as_bytes());
    let got: Vec<String> = d.sent.iter().map(|v| String::from_utf8_lossy(v).into_owned()).collect();
    match want {
        Some(w) => assert_eq!(got, vec![w.to_string()], "message {:?}", msg),
        None => assert!(got.is_empty(), "message {:?}: unexpected response {:?}", msg, got),
    }
    assert!(!m.active(), "{:?} still active", msg);
    println!("ok  {:<55} -> {:?}", format!("{:?}", msg), want.unwrap_or("(no response)"));
    d
}

fn main() {
    // is_query: the header decides ('?' inside an argument does not).
    assert!(is_query(b"*IDN?"));
    assert!(is_query(b"  :SYST:ERR?  "));
    assert!(is_query(b"MMEM:DATA? \"A?.DLG\""));
    assert!(!is_query(b"MMEM:DEL \"A?.DLG\""));
    assert!(!is_query(b"POW:PHAS:CAL RES"));
    assert!(!is_query(b""));

    // The reported bug: a deferred command, then a query in the same message.
    case("POW:PHAS:CAL RES;:SYST:ERR?", Some("-100,\"FLAT\""));
    case("POW:PHAS:CAL RES;SYST:ERR?;SYST:ERR?", Some("-100,\"FLAT\";0,\"No error\""));
    case("POW:PHAS:CAL RES;:POW:PHAS:CAL?;:SYST:ERR?", Some("FLAT;-100,\"FLAT\""));
    // Replies in order, one response.
    case("*IDN?;*OPC?", Some("Alientek,DM40A,0,opendm40 0.1;1"));
    case("OUTP ON;:OUTP?;*OPC?;:SYST:ERR?", Some("1;1;0,\"No error\""));
    case("*CLS;*CLS;*CLS;OUTP 0;OUTP?", Some("0"));
    // Separators: ';' ';:' '\n' '\r\n', empty commands.
    case("*OPC?;;*OPC?\r\n", Some("1;1"));
    case(";*OPC?;", Some("1"));
    case("*OPC?\n*IDN?", Some("1;Alientek,DM40A,0,opendm40 0.1"));
    // No query: no response (also for an error).
    case("OUTP ON", None);
    case("BOGUS", None);
    case("", None);
    case(" ; ;\r\n", None);
    // A query without a reply: no unit, but the message still gets a
    // response (an empty one if nothing else answered).
    case("BOGUS?", Some(""));
    case("BOGUS?;:SYST:ERR?", Some("-100,\"Undefined header\""));
    case("*OPC?;BOGUS?;*OPC?", Some("1;1"));
    // A query waiting for a reading pauses the message; the rest runs after.
    case("READ?", Some("+1.234567E+00"));
    case("*IDN?;READ?;:SYST:ERR?", Some("Alientek,DM40A,0,opendm40 0.1;+1.234567E+00;0,\"No error\""));
    case("READ?;READ?", Some("+1.234567E+00;+1.234567E+00"));
    case("POW:PHAS:CAL RES;READ?;SYST:ERR?", Some("+1.234567E+00;-100,\"FLAT\""));
    // Block replies: alone (with commands around) as before; refused next
    // to other queries.
    case("MMEM:DATA? \"LOG001.DLG\"", Some("#40003abc"));
    case("*CLS;MMEM:DATA? \"LOG001.DLG\";OUTP ON", Some("#40003abc"));
    case("MMEM:DATA? \"NOPE.DLG\"", Some(""));
    case("MMEM:DATA? \"NOPE.DLG\";:SYST:ERR?", Some("-100,\"Query error\""));
    case("*IDN?;MMEM:DATA? \"LOG001.DLG\";SYST:ERR?", Some("Alientek,DM40A,0,opendm40 0.1;-100,\"Query error\""));
    // A non-query's text (DBG "OK") is a unit too.
    case("DBG:OK;*OPC?", Some("OK;1"));
    // 405-byte WAV:DATA? fits with a short reply beside it; more does not
    // (the unit is dropped, reply() says so: main.rs records "Out of memory").
    case("*OPC?;WAV:DATA?", Some(&format!("1;#3400{}", "x".repeat(400))));
    {
        let mut m = Msg::new();
        let mut part = [0u8; 256];
        m.start(b"WAV:DATA?;WAV:DATA?");
        m.next(&mut part);
        assert!(m.reply(&[b'y'; 405]));
        m.next(&mut part);
        assert!(!m.reply(&[b'y'; 405]));
        assert!(m.next(&mut part).is_none());
        assert_eq!(m.finish().map(|r| r.len()), Some(405));
        println!("ok  overflow: second 405-byte unit refused");
    }
    // Device clear while waiting: no response, nothing left.
    {
        let mut m = Msg::new();
        let mut part = [0u8; 256];
        m.start(b"*IDN?;READ?;*OPC?");
        assert!(m.next(&mut part).is_some());
        assert!(m.reply(b"IDN"));
        assert!(m.next(&mut part).is_some());
        m.pause();
        assert!(m.next(&mut part).is_none() && !m.ready() && m.active());
        assert!(m.finish().is_none(), "no response while waiting");
        m.reset();
        assert!(!m.active() && m.finish().is_none());
        println!("ok  device clear while waiting: no response");
    }
    // A new message replaces an unfinished one (query interrupted).
    {
        let mut m = Msg::new();
        let mut part = [0u8; 256];
        m.start(b"READ?;*OPC?");
        m.next(&mut part);
        m.pause();
        m.start(b"*OPC?");
        let n = m.next(&mut part).unwrap();
        assert_eq!(&part[..n], b"*OPC?");
        assert!(m.reply(b"1"));
        assert!(m.next(&mut part).is_none());
        assert_eq!(m.finish(), Some(&b"1"[..]));
        println!("ok  a new message replaces a waiting one");
    }
    // A 256-byte message of 63 commands: every one runs.
    {
        let msg = vec!["*OPC?"; 42].join(";");
        assert!(msg.len() <= MSG_MAX);
        let d = case(&msg, Some(&vec!["1"; 42].join(";")));
        assert!(d.err.is_none());
    }
    println!("PASS");
}
