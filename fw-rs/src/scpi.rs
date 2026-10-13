//! Minimal SCPI command layer for the USBTMC interface (DMM subset).
//!
//! Commands are case-insensitive, long or short form (MEASure -> MEAS), several
//! per message separated by ';'. Supported:
//!   *IDN?  *RST  *CLS  *OPC?  *ESR?  *STB?  SYSTem:ERRor?
//!   CONFigure:<f>  MEASure:<f>?  READ?  FETCh?  [SENSe:]FUNCtion?
//!   [SENSe:]FUNCtion "<f>"
//! where <f> = VOLTage[:DC] | VOLTage:AC | VOLTage:ACDC | CURRent[:DC] |
//! CURRent:AC | CURRent:ACDC | RESistance | CONTinuity | DIODe | CAPacitance |
//! FREQuency | TEMPerature. Functions are stock DMM mode numbers (see dmm.rs),
//! 16 / 17 = V / A AC+DC (true RMS of AC+DC = sqrt(AC^2 + DC^2), the stock
//! DM40B/C sub-mode; FUNCtion? answers "VOLT:ACDC" / "CURR:ACDC").
//!
//! Reply timing of READ? / MEASure:<f>? / FETCh? (main.rs, scpi_wait):
//!   - a reading of the current function already flowing (READ?, MEAS:<f>?
//!     naming the current function): the next fresh reading, or the latest
//!     one if none comes within SCPI_QUICK_MS (0.85 s). libsigrok's USBTMC
//!     transport has a hard 1000 ms Bulk-IN timeout, so this must hold also
//!     in a slow capacitance / temperature range or during a range hunt.
//!   - just switched (MEAS:<f>? changing the function, CONF:<f> then READ?,
//!     FETCh? before any reading): the first reading after the converter's
//!     settle deadline (Dmm::deadline: 2.5 s for resistance, then the
//!     autorange, about 3 s in all), MEAS:<f>? preferring a full-resolution
//!     average for at most SCPI_SETTLE_MS after the command. Meant for hosts
//!     with a timeout of 5 s or more (pyvisa, a human); the 2 s PyVISA
//!     default is too short for resistance, and libsigrok waits after a
//!     function change itself.
//!   - +9.9E37 and "Data stale" in SYST:ERR? only if the current function
//!     has no reading at all SCPI_WAIT_MS (1.9 s) after that deadline.
//!   - no late reply to a query the host gave up on: USBTMC
//!     INITIATE_ABORT_BULK_IN / INITIATE_CLEAR, or a new command ("Query
//!     INTERRUPTED").
//!
//! Temperature / system (fw-rs extensions):
//!   CONFigure:TEMPerature [<sensor>]   MEASure:TEMPerature? [<sensor>]
//!     <sensor> = TCK | PT100 | PT1000 | NTC10K3950 | NTC10K3435 | NTC100K3950
//!     | NTCUSER (dmm.rs T_NAMES; also TC / K for TCK); without it the current
//!     sensor is kept. Reading in C (OL = +9.9E37).
//!   [SENSe:]TEMPerature:TRANsducer <sensor> / ?   select / query the sensor
//!   [SENSe:]TEMPerature:OFFSet <C> / ?   offset of the CURRENT sensor, -20..20
//!     in 0.1 C steps (thermocouple: applied to the cold junction)
//!   [SENSe:]TEMPerature:NTC <R25_ohm>,<Beta> / ?   the NTCUSER parameters
//!   [SENSe:]TEMPerature:EXCitation?   range,R_ohm,I_A,P_W of a resistive sensor
//!     (I, P estimated with the inferred 0.85 V ohm source; TC: error)
//!   MEASure:TEMPerature:CJ?   cold junction (HY3131 internal sensor, C, incl.
//!     the TC offset),age_s. Measured only in thermocouple mode; outside it the
//!     last value with its age (and "Data stale" in SYST:ERR?); never measured:
//!     +9.9E37 and an error.
//!   SYSTem:TEMPerature?   MCU (GD32) internal sensor, C (typical constants)
//!   SYSTem:RAIL AUTO|BOTH / ?   analog rail policy (default AUTO, persisted)
//!   SYSTem:RAIL:STATe?   dmm_rail,scope_rail (1/0)
//!
//! Statistics / limits of the CURRENT DMM function (FR-01 / 02 / 06; values
//! in SI units of the function, as READ?):
//!   CALCulate:AHOLd ON|OFF / ?            AutoHold (as RUN held 1 s)
//!   CALCulate:AVERage[:AVERage]?  CALCulate:AVERage:MINimum? / MAXimum?
//!     no sample yet: +9.9E37 and "Data stale" in SYST:ERR?
//!   CALCulate:AVERage:COUNt?              samples in the statistics
//!   CALCulate:AVERage:CLEar               restart the statistics
//!   CALCulate:AVERage:STATe ON|OFF / ?    recording session (no idle
//!     power-off; ON restarts the statistics)
//!   CALCulate:LIMit:LOWer|UPPer <v> / ?   set a limit (turns the check on);
//!     query: +9.9E37 when not set
//!   CALCulate:LIMit:STATe ON|OFF / ?      limit check on / off
//!   CALCulate:LIMit:CLEar                 clear both limits (fw-rs; not the
//!     Keysight meaning "clear the fail indicator")
//!   CALCulate:LIMit:FAIL?                 1 = the value is outside the limits
//!
//! Conversion rate / display smoothing (FR-05; persisted):
//!   [SENSe:]RATE SLOW|MEDium|FAST / ?     AD1 conversion rate of DC V / mV /
//!     A and ohm (dmm.rs FR-05 table; other functions keep the stock timing);
//!     query: SLOW / MED / FAST
//!   [SENSe:]RATE:PERiod?                  measured interval between readings,
//!     s (0 until two readings were seen)
//!   CALCulate:SMOothing[:STATe] ON|OFF / ?   display smoothing on (LOW) / off
//!   CALCulate:SMOothing:RESPonse SLOW|MEDium|FAST / ?   SLOW = HIGH (long
//!     window), MEDium / FAST = LOW; query SLOW / FAST. The smoothing feeds
//!     the readout, READ? / MEAS? / FETCh?; MIN/MAX/AVG and the CSV log stay
//!     unsmoothed (meter.rs header)

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
    /// Scope / generator command; `None` argument = query.
    Instr(Cmd),
    /// Debug command (DBG:...), handled by the main loop.
    Dbg(Dbg),
    /// Data-logger command (LOG:...), handled by the main loop with the logger.
    Log(Log),
    /// USB-disk file command (MMEMory:...), handled by the main loop (csvlog.rs).
    Mmem(Mmem),
}

/// File commands on the USB disk's root (csvlog.rs gallery backend):
///   MMEMory:CATalog?        <used>,<free>{,"<name>,<type>,<size>"} (bytes; type
///                           BIN = S*.BMP, ASC = LOG*.CSV; csvlog::catalog_scpi_on)
///   MMEMory:DELete "<name>" delete one S*.BMP / LOG*.CSV (USB only: refused
///                           over Bluetooth; refused for the file being recorded)
#[derive(Clone, Copy)]
pub enum Mmem {
    Cat,
    Del([u8; 11]),
}

/// Data-logger commands (FR-38), executed by the main loop against `Logger`:
///   LOG:STATe ON|OFF        start / stop the fast stream
///   LOG:STATe?              1 / 0
///   LOG:OSR <0..7>          AD1OSR code (OSR = 256 << n); takes effect at START
///   LOG:OSR?                current code
///   LOG:RATE?               measured mean sample rate, Hz (0 until 2 samples)
///   LOG:COUNt?              buffered,captured,overwritten,missed
///   LOG:PREamble?           mode,range,osr,nominal_us,measured_hz (scaling: raw
///                           AD1 counts, 1.389 V / 8388607 cts * range decade;
///                           calibration gain/offset NOT applied)
///   LOG:DATA?               definite-length #4nnnn block of 12-byte LE records
///                           {u32 seq, u32 ts_us, i32 raw_ad1}, oldest first
///   LOG:CSV ON|OFF          CSV recording of the DMM readings to the USB disk
///                           (csvlog.rs; as the DMM page swipe down). ON fails
///                           ("Settings conflict") without a reading or while a
///                           PC has the disk mounted
///   LOG:CSV?                state,file,rows,bytes,committed,dropped (state 0 idle,
///                           1 recording, 2 stopping, 3 done, 4 full, 5 no disk,
///                           6 error; file -1 until the first commit)
#[derive(Clone, Copy)]
pub enum Log {
    State(Option<bool>),
    Osr(Option<u8>),
    Rate,
    Count,
    Preamble,
    Data,
    Csv(Option<bool>),
}

/// Debug commands over USBTMC (development aids, not SCPI standard):
///   DBG:PING? [ms]          uptime_ms,flags,keepalive_left_ms (extends keepalive)
///   DBG:TAP x,y  DBG:SWIPE L|R  inject a touch tap / view swipe
///   DBG:KEY n[,ms]          side key n (0 MENU, 1 RUN, 2 AUTO, 3 MF): a full
///                           click (key-down, key-up); with ms > 0 the key-up
///                           follows ms later (long press: RUN >= 1000 ms in a
///                           DMM view = AutoHold on/off); max 60000
///   DBG:SWIPE D,n           synthetic n-finger swipe down (3 = screenshot, 2 = power off)
///   DBG:GRAB <ms> | ?       scope drag hold-to-grab time (0 = direct drag, default)
///   DBG:GEST?               last scope drags, oldest first: n then per drag
///                           ";target,axis,delta_px,ms" (target 1 level, 2 trigger
///                           position / pan, 3 offset, 4/5 X/Y cursor a, 14/15 b;
///                           axis H|V)
///   DBG:DRAG x0,y0,x1,y1,ms[,hold]
///                           synthetic one-finger drag through the gesture
///                           tracker (axis lock, capture): down at x0,y0, held
///                           still hold ms, moved to x1,y1 in ms ms (0 = 300),
///                           released; runs synchronously
///   DBG:HWTRIG?             comparator trigger (hwtrig.rs): TIM2 SR,CNT,CCR1,
///                           PA15<<16|DOR2,enabled,available,map rising lo,hi,
///                           map falling lo,hi,post_ticks
///   DBG:HWTRIG:ENABLE 0|1   use the comparator trigger (default off,
///                           hwtrig::DEFAULT_ENABLED; 1 resets the miss count)
///   DBG:HWTRIG:CAL?         wire-free calibration (scope rail on, MCX open):
///                           flip00,flip01,flip10,flip11,map0lo,map0hi,map1lo,
///                           map1hi,hyst,slope,code0,code1 or the error name
///   DBG:HWTRIG:STAT?        hits,misses,delta_last,delta_min,delta_max,mean
///                           (samples, refined crossing - hardware estimate)
///   DBG:HWTRIG:STAT:RESET   clear the statistics
///   DBG:GENUINE?            device check (src/genuine.rs, docs/genuine-check.md):
///                           uid_hex,stored_hex,expected_hex,GENUINE|MISMATCH (12 bytes
///                           each, memory order); read-only
///   DBG:LCD? x,y,w,h,n      n raw GRAM words after RAMRD (#4 block)
///   DBG:DIAG?               diagnostic block (#3)
///   DBG:FLASH?              SPI disk: jedec_id_hex,blocks,failed
///   DBG:RELEASE             stop feeding the watchdog. RAM: back to stock in ~20 s.
///                           Native: the WDT reset counts as a crash (1st: fw-rs
///                           reboots; 2nd without 60 s healthy: recovery = marker
///                           zeroed, DESTRUCTIVE)
///   DBG:RESET               RAM: reset to stock now. Native: reboot fw-rs (SW reset)
///   DBG:RECOVER, DBG:UPDATE RAM: RECOVER = plain reset (Stage 0). Native, DESTRUCTIVE:
///                           clear the previous-mode word, zero the 12-byte loader
///                           marker, reset -> vendor update screen + its USB disk until
///                           a dm40*.bin is installed. "Flash error" = marker did not
///                           read back as zero (no reset). Sync + eject a copied file first.
///   DBG:BOOT?               native boot info (src/boot.rs put_info)
#[derive(Clone, Copy)]
pub enum Dbg {
    Ping(u32),
    Input(u8, u16, u16),
    /// x0, y0, x1, y1, ms, hold ms.
    Drag([u16; 6]),
    Lcd(u16, u16, u16, u16, u32),
    Diag,
    Flash,
    Release,
    Reset,
}

fn nums<const N: usize>(a: &[u8]) -> [u32; N] {
    let mut out = [0u32; N];
    for (i, part) in a.split(|&b| b == b',').take(N).enumerate() {
        out[i] = parse_num(part).unwrap_or(0.0) as u32;
    }
    out
}

/// Scope / generator commands (handled by the main loop with the app):
///   SOURce:FUNCtion SIN|SQU|TRI|RAMP   SOURce:FREQuency <Hz>
///   SOURce:VOLTage <Vpp>   SOURce:VOLTage:OFFSet <V>|AUTO (FR-28: centre level;
///   AUTO = stock 0..Vpp; kept in [Vpp/2, 3 - Vpp/2])
///   SOURce:FUNCtion:SQUare:DCYCle <%>   OUTPut ON|OFF
///   Queries: FREQ? = the actual (clamped + quantised) frequency, DCYC? the
///   effective duty, VOLT? / OFFS? the set values (OFFS? with AUTO: Vpp/2)
///   TIMebase:SCALe <s/div>   CHANnel1:SCALe <V/div>   CHANnel1:COUPling AC|DC
///   TRIGger:LEVel <V>   TRIGger:SLOPe POS|NEG   TRIGger:SWEep AUTO|NORM|SING
///   RUN  STOP  SINGle   SCOPe:MEASure:VPP|VAVG|VRMS|FREQ?
///   WAVeform:DATA? (#3400 + 400 bytes, 0..255)   WAVeform:PREamble?
#[derive(Clone, Copy)]
pub enum Cmd {
    GenFunc(Option<u8>),
    GenFreq(Option<f32>),
    GenVolt(Option<f32>),
    /// SOUR:VOLT:OFFS: Some(NaN) = AUTO.
    GenOffs(Option<f32>),
    GenDuty(Option<f32>),
    GenOut(Option<bool>),
    TimScale(Option<f32>),
    ChScale(Option<f32>),
    ChCoup(Option<bool>),
    TrigLevel(Option<f32>),
    TrigSlope(Option<bool>),
    TrigSweep(Option<u8>),
    Run,
    Stop,
    Single,
    Meas(u8),
    WavData,
    WavPre,
    /// Debug: sample the DAC output pin PA4 with the MCU ADC.
    DiagDac,
    /// Debug: count PB14 (PWM output) rising edges for 2 ms.
    DiagPwm,
    /// DMM / system commands (not scope commands, see App::scpi).
    TempSensor(Option<u8>),
    TempOffs(Option<f32>),
    TempNtc(Option<(f32, f32)>),
    TempExc,
    TempCj,
    SysTemp,
    SysRail(Option<bool>),
    SysRailState,
    /// CALCulate (FR-01 / 02 / 06; App::set_ahold etc.).
    CalcAhold(Option<bool>),
    /// 0 MIN, 1 MAX, 2 AVER, 3 COUNt.
    CalcAver(u8),
    CalcAverClear,
    CalcAverState(Option<bool>),
    /// 0 LOWer, 1 UPPer; None = query.
    CalcLim(u8, Option<f32>),
    CalcLimClear,
    CalcLimState(Option<bool>),
    CalcLimFail,
    /// FR-05: [SENSe:]RATE (0 SLOW, 1 MED, 2 FAST; None = query),
    /// RATE:PERiod?, CALC:SMOothing[:STATe], CALC:SMOothing:RESPonse
    /// (1 LOW = FAST / MED, 2 HIGH = SLOW).
    SensRate(Option<u8>),
    SensRatePer,
    CalcSmoState(Option<bool>),
    CalcSmoResp(Option<u8>),
    /// DBG:GRAB <ms> / ?: hold-to-grab time of scope drags (0 = direct).
    DbgGrab(Option<u16>),
    /// DBG:GEST?: log of the last scope drags.
    DbgGest,
    /// DBG:HWTRIG...: 0 query, 1 ENABLE (bool), 2 CAL?, 3 STAT?, 4 STAT:RESET.
    DbgHwTrig(u8, bool),
}

/// ON | 1 -> true, OFF | 0 -> false.
fn on_off(a: &[u8]) -> Option<bool> {
    match a {
        b"ON" | b"1" => Some(true),
        b"OFF" | b"0" => Some(false),
        _ => None,
    }
}

/// Parse a number like "1.5", "-2e-3", "10k" / "5m" (suffixes k, M, m, u, n).
pub fn parse_num(a: &[u8]) -> Option<f32> {
    let a = trim(a);
    if a.is_empty() {
        return None;
    }
    let mut end = a.len();
    let mut mult = 1.0f32;
    let last = a[end - 1];
    let suffix = match last {
        b'K' | b'k' => Some(1e3),
        b'M' => Some(1e6),
        b'm' => Some(1e-3),
        b'U' | b'u' => Some(1e-6),
        b'N' | b'n' => Some(1e-9),
        _ => None,
    };
    if let Some(m) = suffix {
        mult = m;
        end -= 1;
    }
    let t = &a[..end];
    let mut i = 0;
    let neg = t.first() == Some(&b'-');
    if neg || t.first() == Some(&b'+') {
        i += 1;
    }
    let mut v = 0.0f32;
    let mut any = false;
    while i < t.len() && t[i].is_ascii_digit() {
        v = v * 10.0 + (t[i] - b'0') as f32;
        i += 1;
        any = true;
    }
    if i < t.len() && t[i] == b'.' {
        i += 1;
        let mut k = 0.1f32;
        while i < t.len() && t[i].is_ascii_digit() {
            v += (t[i] - b'0') as f32 * k;
            k *= 0.1;
            i += 1;
            any = true;
        }
    }
    if !any {
        return None;
    }
    if i < t.len() && (t[i] == b'e' || t[i] == b'E') {
        i += 1;
        let eneg = t.get(i) == Some(&b'-');
        if eneg || t.get(i) == Some(&b'+') {
            i += 1;
        }
        let mut e = 0i32;
        while i < t.len() && t[i].is_ascii_digit() {
            e = e * 10 + (t[i] - b'0') as i32;
            i += 1;
        }
        for _ in 0..e {
            v = if eneg { v / 10.0 } else { v * 10.0 };
        }
    }
    if i != t.len() {
        return None;
    }
    Some(if neg { -v } else { v } * mult)
}

pub struct Scpi {
    pub buf: [u8; 512],
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
    let acdc = parts.get(1).map(|p| kw(p, "ACDC")).unwrap_or(false);
    let dc_or_none = parts.get(1).map(|p| kw(p, "DC")).unwrap_or(true);
    if kw(p0, "VOLTage") {
        if ac { Some(0) } else if acdc { Some(16) } else if dc_or_none { Some(1) } else { None }
    } else if kw(p0, "CURRent") {
        if ac { Some(5) } else if acdc { Some(17) } else if dc_or_none { Some(8) } else { None }
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

/// Temperature sensor by name (dmm.rs T_NAMES; TC / K = TCK).
fn parse_sensor(a: &[u8]) -> Option<u8> {
    let a = trim(a);
    if a == b"TC" || a == b"K" {
        return Some(0);
    }
    crate::dmm::T_NAMES.iter().position(|n| n.as_bytes() == a).map(|k| k as u8)
}

/// Function with an optional temperature sensor argument: TEMP <sensor> ->
/// 32 + sensor (Dmm::select).
fn parse_func_arg(parts: &[&[u8]], arg: &[u8]) -> Option<u8> {
    let f = parse_func(parts)?;
    if f == 15 && !trim(arg).is_empty() {
        return parse_sensor(arg).map(|k| 32 + k);
    }
    Some(f)
}

fn func_name(mode: u8) -> &'static str {
    match mode {
        0 | 2 => "\"VOLT:AC\"",
        1 | 3 => "\"VOLT\"",
        4..=6 => "\"CURR:AC\"",
        7..=9 => "\"CURR\"",
        16 => "\"VOLT:ACDC\"",
        17 => "\"CURR:ACDC\"",
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
        Scpi { buf: [0; 512], len: 0, err: None }
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
                _ if kw(p0, "SYSTem") && parts.len() > 1 && kw(parts[1], "TEMPerature") && query => {
                    out = Out::Instr(Cmd::SysTemp)
                }
                _ if kw(p0, "SYSTem") && parts.len() > 1 && kw(parts[1], "RAIL") => {
                    let a = trim(arg);
                    out = Out::Instr(if parts.len() > 2 && query {
                        Cmd::SysRailState
                    } else if query {
                        Cmd::SysRail(None)
                    } else if kw(a, "BOTH") {
                        Cmd::SysRail(Some(true))
                    } else if kw(a, "AUTO") {
                        Cmd::SysRail(Some(false))
                    } else {
                        self.err = Some("Illegal parameter value");
                        continue;
                    });
                }
                _ if kw(p0, "MEASure") && query && parts.len() > 2 && kw(parts[1], "TEMPerature") && kw(parts[2], "CJ") => {
                    out = Out::Instr(Cmd::TempCj)
                }
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
                _ if kw(p0, "MEASure") && query => match parse_func_arg(&parts[1..], arg) {
                    Some(f) => out = Out::Measure(Some(f)),
                    None => self.err = Some("Command error"),
                },
                _ if kw(p0, "CONFigure") && !query => match parse_func_arg(&parts[1..], arg) {
                    Some(f) => out = Out::Configure(f),
                    None => self.err = Some("Command error"),
                },
                (b"READ", true) => out = Out::Measure(None),
                _ if kw(p0, "SOURce") || kw(p0, "OUTPut") => {
                    let a = trim(arg);
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let cmd = if kw(p0, "OUTPut") {
                        Some(Cmd::GenOut(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else if kw(sub, "FUNCtion") && parts.len() >= 4 {
                        Some(Cmd::GenDuty(if query { None } else { parse_num(a) }))
                    } else if kw(sub, "FUNCtion") {
                        let w = if kw(a, "SINusoid") {
                            Some(0)
                        } else if kw(a, "SQUare") {
                            Some(1)
                        } else if kw(a, "TRIangle") {
                            Some(2)
                        } else if kw(a, "RAMP") {
                            Some(3)
                        } else {
                            None
                        };
                        Some(Cmd::GenFunc(if query { None } else { w }))
                    } else if kw(sub, "FREQuency") {
                        Some(Cmd::GenFreq(if query { None } else { parse_num(a) }))
                    } else if kw(sub, "VOLTage") && parts.len() >= 3 && kw(parts[2], "OFFSet") {
                        if query {
                            Some(Cmd::GenOffs(None))
                        } else if kw(a, "AUTO") {
                            Some(Cmd::GenOffs(Some(f32::NAN)))
                        } else {
                            parse_num(a).map(|v| Cmd::GenOffs(Some(v)))
                        }
                    } else if kw(sub, "VOLTage") {
                        Some(Cmd::GenVolt(if query { None } else { parse_num(a) }))
                    } else {
                        None
                    };
                    match cmd {
                        Some(c) => out = Out::Instr(c),
                        None => self.err = Some("Undefined header"),
                    }
                }
                _ if kw(p0, "TIMebase") => out = Out::Instr(Cmd::TimScale(if query { None } else { parse_num(arg) })),
                _ if kw(p0.strip_suffix(b"1").unwrap_or(p0), "CHANnel") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    out = Out::Instr(if kw(sub, "COUPling") {
                        Cmd::ChCoup(if query { None } else { Some(a == b"AC") })
                    } else {
                        Cmd::ChScale(if query { None } else { parse_num(a) })
                    });
                }
                _ if kw(p0, "TRIGger") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    out = Out::Instr(if kw(sub, "SLOPe") {
                        Cmd::TrigSlope(if query { None } else { Some(!kw(a, "NEGative")) })
                    } else if kw(sub, "SWEep") {
                        Cmd::TrigSweep(if query {
                            None
                        } else if kw(a, "NORMal") {
                            Some(1)
                        } else if kw(a, "SINGle") {
                            Some(2)
                        } else {
                            Some(0)
                        })
                    } else {
                        Cmd::TrigLevel(if query { None } else { parse_num(a) })
                    });
                }
                (b"RUN", false) => out = Out::Instr(Cmd::Run),
                (b"STOP", false) => out = Out::Instr(Cmd::Stop),
                _ if kw(p0, "SINGle") && !query => out = Out::Instr(Cmd::Single),
                _ if kw(p0, "SCOPe") && query => {
                    let m = parts.last().copied().unwrap_or(&[]);
                    let k = if kw(m, "VPP") {
                        0
                    } else if kw(m, "VAVG") {
                        1
                    } else if kw(m, "VRMS") {
                        2
                    } else {
                        3
                    };
                    out = Out::Instr(Cmd::Meas(k));
                }
                // Scope drag A/B aids (App::scpi): DBG:GRAB <ms> (0 = direct
                // drag) / DBG:GRAB?, DBG:GEST? (last drags).
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"GRAB"[..]) => {
                    out = Out::Instr(Cmd::DbgGrab(if query { None } else { parse_num(arg).map(|v| v.clamp(0.0, 5000.0) as u16) }))
                }
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"GEST"[..]) => out = Out::Instr(Cmd::DbgGest),
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"HWTRIG"[..]) => {
                    let c = match (parts.get(2).copied(), parts.get(3).copied(), query) {
                        (None, _, true) => Some(Cmd::DbgHwTrig(0, false)),
                        (Some(b"ENABLE"), None, false) => on_off(trim(arg)).map(|on| Cmd::DbgHwTrig(1, on)),
                        (Some(b"CAL"), None, true) => Some(Cmd::DbgHwTrig(2, false)),
                        (Some(b"STAT"), None, true) => Some(Cmd::DbgHwTrig(3, false)),
                        (Some(b"STAT"), Some(b"RESET"), false) => Some(Cmd::DbgHwTrig(4, false)),
                        _ => None,
                    };
                    match c {
                        Some(c) => out = Out::Instr(c),
                        None => self.err = Some("Undefined header"),
                    }
                }
                _ if p0 == b"DBG" => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    out = Out::Dbg(match sub {
                        b"PING" => Dbg::Ping(nums::<1>(a)[0]),
                        b"TAP" => {
                            let v = nums::<2>(a);
                            Dbg::Input(0, v[0] as u16, v[1] as u16)
                        }
                        b"SWIPE" => match a.first() {
                            Some(&b'R') => Dbg::Input(2, 0, 0),
                            // D,n: synthetic n-finger swipe down (gesture.rs synth_swipe_down).
                            Some(&b'D') => {
                                let n = a.split(|&b| b == b',').nth(1).and_then(parse_num).unwrap_or(1.0);
                                Dbg::Input(4, (n as u16).clamp(1, 5), 0)
                            }
                            _ => Dbg::Input(1, 0, 0),
                        },
                        b"DRAG" => {
                            let v = nums::<6>(a);
                            let ms = if v[4] == 0 { 300 } else { v[4].min(10_000) };
                            Dbg::Drag([v[0].min(479) as u16, v[1].min(319) as u16, v[2].min(479) as u16, v[3].min(319) as u16, ms as u16, v[5].min(10_000) as u16])
                        }
                        b"KEY" => { let v = nums::<2>(a); Dbg::Input(3, v[0] as u16, v[1].min(60_000) as u16) } // n[,hold_ms]
                        b"LCD" => {
                            let v = nums::<5>(a);
                            Dbg::Lcd(v[0] as u16, v[1] as u16, v[2].max(1) as u16, v[3].max(1) as u16, v[4])
                        }
                        b"DIAG" => Dbg::Diag,
                        b"FLASH" => Dbg::Flash,
                        b"RELEASE" => Dbg::Release,
                        b"RESET" => Dbg::Reset,
                        // DBG:UPDATE = DBG:RECOVER (native): zero the 12-byte loader
                        // marker at 0x08008000, then the deferred SYSRESETREQ ->
                        // vendor update screen + its USB disk (docs/native-boot.md).
                        // RAM build: RECOVER = plain reset. DBG:BOOT? = boot info.
                        // Stock device-authenticity check, read-only (docs/genuine-check.md).
                        b"GENUINE" => {
                            let i = crate::genuine::inspect();
                            self.len = 0;
                            for v in [i.uid, i.stored, i.expected] {
                                for b in v {
                                    let h = b"0123456789ABCDEF";
                                    self.put(&[h[(b >> 4) as usize], h[(b & 15) as usize]]);
                                }
                                self.put(b",");
                            }
                            let ok = i.result == crate::genuine::GenuineResult::Genuine;
                            self.put(if ok { b"GENUINE" } else { b"MISMATCH" });
                            continue;
                        }
                        #[cfg(any(feature = "native", feature = "test-initcore"))]
                        b"BOOT" => {
                            crate::boot::put_info(self);
                            continue;
                        }
                        #[cfg(feature = "native")]
                        b"RECOVER" | b"UPDATE" => { if !crate::boot::enter_loader_update() { self.err = Some("Flash error"); continue; } Dbg::Reset }
                        #[cfg(not(feature = "native"))]
                        b"RECOVER" => Dbg::Reset,
                        // (block kept at its original line count: panic locations)
                        _ => {
                            self.err = Some("Undefined header");
                            continue;
                        }
                    });
                }
                _ if kw(p0, "DIAG") && query => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    out = Out::Instr(if kw(sub, "PWM") { Cmd::DiagPwm } else { Cmd::DiagDac });
                }
                _ if kw(p0, "LOG") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    out = if kw(sub, "STATe") {
                        Out::Log(Log::State(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else if kw(sub, "OSR") {
                        Out::Log(Log::Osr(if query { None } else { Some(parse_num(a).unwrap_or(0.0) as u8) }))
                    } else if kw(sub, "RATE") && query {
                        Out::Log(Log::Rate)
                    } else if kw(sub, "COUNt") && query {
                        Out::Log(Log::Count)
                    } else if kw(sub, "PREamble") && query {
                        Out::Log(Log::Preamble)
                    } else if kw(sub, "DATA") && query {
                        Out::Log(Log::Data)
                    } else if kw(sub, "CSV") {
                        Out::Log(Log::Csv(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else {
                        self.err = Some("Undefined header");
                        continue;
                    };
                }
                _ if kw(p0, "MMEMory") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    out = if kw(sub, "CATalog") && query {
                        Out::Mmem(Mmem::Cat)
                    } else if kw(sub, "DELete") && !query {
                        match crate::csvlog::parse_name(trim(arg)) {
                            Some(n) => Out::Mmem(Mmem::Del(n)),
                            None => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
                    } else {
                        self.err = Some("Undefined header");
                        continue;
                    };
                }
                _ if kw(p0, "WAVeform") && query => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    out = Out::Instr(if kw(sub, "PREamble") { Cmd::WavPre } else { Cmd::WavData });
                }
                _ if kw(p0, "FETCh") && query => out = Out::Fetch,
                _ if kw(p0, "SENSe") || kw(p0, "FUNCtion") || kw(p0, "TEMPerature") || kw(p0, "RATE") => {
                    let rest = if kw(p0, "SENSe") { &parts[1..] } else { parts };
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
                        match c {
                            Some(c) => out = Out::Instr(c),
                            None => self.err = Some("Illegal parameter value"),
                        }
                        continue;
                    }
                    if rest.first().map(|p| kw(p, "TEMPerature")).unwrap_or(false) {
                        let sub = rest.get(1).copied().unwrap_or(&[]);
                        let a = trim(arg);
                        let c = if kw(sub, "TRANsducer") {
                            match (query, parse_sensor(a)) {
                                (true, _) => Some(Cmd::TempSensor(None)),
                                (false, Some(k)) => Some(Cmd::TempSensor(Some(k))),
                                _ => None,
                            }
                        } else if kw(sub, "OFFSet") {
                            if query { Some(Cmd::TempOffs(None)) } else { parse_num(a).map(|v| Cmd::TempOffs(Some(v))) }
                        } else if kw(sub, "NTC") {
                            let mut it = a.split(|&b| b == b',');
                            match (query, it.next().and_then(parse_num), it.next().and_then(parse_num)) {
                                (true, _, _) => Some(Cmd::TempNtc(None)),
                                (false, Some(r), Some(b)) => Some(Cmd::TempNtc(Some((r, b)))),
                                _ => None,
                            }
                        } else if kw(sub, "EXCitation") && query {
                            Some(Cmd::TempExc)
                        } else {
                            None
                        };
                        match c {
                            Some(c) => out = Out::Instr(c),
                            None => self.err = Some("Illegal parameter value"),
                        }
                        continue;
                    }
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
                _ if kw(p0, "CALCulate") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let leaf = parts.get(2).copied();
                    let a = trim(arg);
                    // ON|OFF set or the query: Some(None) = query.
                    let state = if query { Some(None) } else { on_off(a).map(Some) };
                    let c = if kw(sub, "AHOLd") && leaf.is_none() {
                        state.map(Cmd::CalcAhold)
                    } else if kw(sub, "AVERage") {
                        match leaf {
                            None if query => Some(Cmd::CalcAver(2)),
                            Some(l) if query && kw(l, "MINimum") => Some(Cmd::CalcAver(0)),
                            Some(l) if query && kw(l, "MAXimum") => Some(Cmd::CalcAver(1)),
                            Some(l) if query && kw(l, "AVERage") => Some(Cmd::CalcAver(2)),
                            Some(l) if query && kw(l, "COUNt") => Some(Cmd::CalcAver(3)),
                            Some(l) if !query && kw(l, "CLEar") => Some(Cmd::CalcAverClear),
                            Some(l) if kw(l, "STATe") => state.map(Cmd::CalcAverState),
                            _ => None,
                        }
                    } else if kw(sub, "SMOothing") {
                        match leaf {
                            Some(l) if kw(l, "RESPonse") => {
                                if query {
                                    Some(Cmd::CalcSmoResp(None))
                                } else if kw(a, "SLOW") {
                                    Some(Cmd::CalcSmoResp(Some(2)))
                                } else if kw(a, "MEDium") || kw(a, "FAST") {
                                    Some(Cmd::CalcSmoResp(Some(1)))
                                } else {
                                    None
                                }
                            }
                            Some(l) if kw(l, "STATe") => state.map(Cmd::CalcSmoState),
                            None => state.map(Cmd::CalcSmoState),
                            _ => None,
                        }
                    } else if kw(sub, "LIMit") {
                        match leaf {
                            Some(l) if kw(l, "LOWer") || kw(l, "UPPer") => {
                                let k = kw(l, "UPPer") as u8;
                                if query { Some(Cmd::CalcLim(k, None)) } else { parse_num(a).map(|v| Cmd::CalcLim(k, Some(v))) }
                            }
                            Some(l) if kw(l, "STATe") => state.map(Cmd::CalcLimState),
                            Some(l) if !query && kw(l, "CLEar") => Some(Cmd::CalcLimClear),
                            Some(l) if query && kw(l, "FAIL") => Some(Cmd::CalcLimFail),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    match c {
                        Some(c) => out = Out::Instr(c),
                        None if !query && !a.is_empty() => self.err = Some("Illegal parameter value"),
                        None => self.err = Some("Undefined header"),
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

    /// Append raw bytes to the reply.
    pub fn put_bytes(&mut self, b: &[u8]) {
        self.put(b);
    }

    /// Record an error for the next SYSTem:ERRor? query.
    pub fn set_err(&mut self, e: &'static str) {
        self.err = Some(e);
    }

    /// Append a number in NR3.
    pub fn put_f32(&mut self, v: f32) {
        let mut b = [0u8; 20];
        let n = nr3(&mut b, v);
        self.put(&b[..n]);
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
