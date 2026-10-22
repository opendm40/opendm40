//! Minimal SCPI command layer for the USBTMC interface (DMM subset).
//!
//! Commands are case-insensitive, long or short form (MEASure -> MEAS), several
//! per message separated by ';' (main.rs runs them one at a time through
//! scpi_msg.rs: replies in order, ';'-joined, one response). Supported:
//!   *IDN?  *RST  *CLS  *OPC?  *ESR?  *STB?  SYSTem:ERRor?
//!   CONFigure:<f>  MEASure:<f>?  READ?  FETCh?  [SENSe:]FUNCtion?
//!   [SENSe:]FUNCtion "<f>"   CONFigure?   INITiate  ABORt  SYSTem:LOCal|REMote
//!   (no-ops)   SYSTem:IDN:EMULation OFF|34401A / ?   SYSTem:MODel?
//! (*IDN? and SYSTem:MODel? name the model of the calibration record,
//! docs/model-variants.md; fw-rs uses the DM40C count limits on all models.)
//! where <f> = VOLTage[:DC] | VOLTage:AC | VOLTage:ACDC | CURRent[:DC] |
//! CURRent:AC | CURRent:ACDC | RESistance | CONTinuity | DIODe | CAPacitance |
//! FREQuency | TEMPerature. Functions are stock DMM mode numbers (see dmm.rs),
//! 16 / 17 = V / A AC+DC (true RMS of AC+DC = sqrt(AC^2 + DC^2), the stock
//! DM40B/C sub-mode; FUNCtion? answers "VOLT:ACDC" / "CURR:ACDC"),
//! 19 / 20 = V / A with the AUTO coupling (fw-rs: VOLTage:AUTO /
//! CURRent:AUTO; the reading is the DC or the AC part, whichever AUTO picks,
//! dmm.rs AUTO_*; FUNCtion? answers "VOLT:AUTO" / "CURR:AUTO").
//!
//! Coupling of V / A (fw-rs; the same switch as the V / A button):
//!   [SENSe:]VOLTage:COUPling DC|AC|ACDC|AUTO   select V with that coupling
//!   [SENSe:]CURRent:COUPling DC|AC|ACDC|AUTO   (= CONF:VOLT[:AC|:ACDC|:AUTO])
//!   [SENSe:]VOLTage:COUPling? / CURRent:COUPling?   DC / AC / ACDC, or
//!     AUTO,DC / AUTO,AC (the part AUTO shows now); DC while the function
//!     is not the current one (V / A start in DC, as the stock)
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
//!     average for at most SCPI_SETTLE_MS (1.5 s) after the command (a full
//!     window takes ~3.2 s at OSR 32768, so that first answer is usually a
//!     partial average; a later READ? has the +1 digit). Meant for hosts
//!     with a timeout of 5 s or more (pyvisa, a human); the 2 s PyVISA
//!     default is too short for resistance, and libsigrok waits after a
//!     function change itself.
//!   - +9.9E37 and "Data stale" in SYST:ERR? only if the current function
//!     has no reading at all SCPI_WAIT_MS (1.9 s) after that deadline.
//!   - the settle wait is only for a change this host asked for (CONF,
//!     MEAS:<f>?, FUNC, TEMP:TRAN on the same transport, or READ? powering
//!     the DMM up). After a change made on the meter (touch, keys, the phone
//!     app, DBG:TAP / KEY, the other transport) READ? and MEAS:<f>? naming
//!     the current function answer within SCPI_QUICK_MS: the latest reading
//!     of the current function, else +9.9E37 and "Data stale" (libsigrok
//!     would otherwise time out after 1 s and end the acquisition). Such a
//!     change also cuts a READ? already waiting down to that budget.
//!   - no late reply to a query the host gave up on: USBTMC
//!     INITIATE_ABORT_BULK_IN / INITIATE_CLEAR, or a new command ("Query
//!     INTERRUPTED").
//!
//! CONFigure? answers as the Agilent / HP meters: "<func> <range>,<resolution>"
//! with <func> as FUNCtion? ("VOLT", "VOLT:AC", "CURR:ACDC", "RES", "CAP",
//! "FREQ", "TEMP", ...; "CONT" and "DIOD" bare), the resolution that of the
//! readout (its decimals, one more with the +1 digit), the range 60000 counts
//! of it (DM40 ranges are 6 / 60 / 600..., not the 34401A's 1 / 10 / 100...).
//! No reading of the current function yet: +6.000000E+00,+1.000000E-04.
//!
//! *IDN? compatibility (SYSTem:IDN:EMULation, persisted, default OFF): with
//! 34401A, *IDN? answers "HEWLETT-PACKARD,34401A,0,dm40-fw 0.1" so that hosts
//! with a 34401A profile only (libsigrok >= 0.5.2 / SmuView without the DM40
//! profile) take the meter; READ? then never waits longer than the quick
//! budget (+9.9E37 until the first reading after a function change).
//! Caveats: that profile knows DC / AC V and A, RES (and FRES = RES here),
//! CONT, DIOD, FREQ (and PER, which the DM40 lacks): in CAP, TEMP and AC+DC
//! the host stops with "unknown measurement quantity"; no ranges or
//! resolution settings; the digits come from CONFigure?. PyVISA scripts
//! written for a real 34401A get its command subset only.
//!
//! Temperature / system (fw-rs extensions):
//!   CONFigure:TEMPerature [<sensor>]   MEASure:TEMPerature? [<sensor>]
//!     <sensor> = TCK | PT100 | PT1000 | NTC10K3950 | NTC10K3435 | NTC100K3950
//!     | NTCUSER (dmm.rs T_NAMES; also TC / K for TCK); without it the current
//!     sensor is kept. Reading in the UNIT:TEMP unit (OL = +9.9E37).
//!   UNIT:TEMPerature C | F / ?   global temperature unit (also Settings 2/2):
//!     readings, MEAS:TEMP:CJ? and SYST:TEMP? follow it; offsets stay in C
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
//!   SYSTem:RAIL:STATe?   dmm_rail,scope_rail (1/0),wakes,lost_row,silent (DMM
//!                        rail wakes and stall-watchdog restarts, dmm.rs)
//!   SYSTem:ALERt?   wrong-connection alert in force (sentinel.rs, docs/
//!     power-meter-and-misconnect.md 4): NONE | OVERLOAD (the uA / mA shunt
//!     over range: moved to 10 A, latched) | AJACK (current in the A jack
//!     outside the current functions) | VJACK (voltage on V/ohm, no current,
//!     in DC / AC / AC+DC A) | LIVE (a source on the circuit in ohm /
//!     continuity / diode) | DC (hint: DC on V/ohm in AC V) | AC (hint: AC in
//!     DC V 6 V) | FUSE (hint: an A function at zero for 60 s)
//!   SYSTem:ALERt:ENABle <mask> | ON | OFF / ?   alerts enabled, bit 0 AJACK,
//!     1 VJACK and the DC / AC / FUSE hints, 2 LIVE (Settings 3/3 "Jack
//!     alerts"), 3 the "Leads in A?" reminder (a toast + beep on a change from
//!     a current function to a non-current one, doc 4.9; Settings 3/3 "A-lead
//!     reminder"); default 15 = ON, persisted. A host script that switches
//!     between A and V functions with leads in both jacks clears bit 3
//!     OVERLOAD (the uA / mA shunt over range: the meter moved to the 10 A
//!     shunt, latched; doc 4.7) is not maskable
//!   SYSTem:FUSE:TEST   start the fuse check (doc 4.3: one lead from V/ohm to
//!     A, nothing else): ohm 600 ohm for 3 readings, then the function before
//!     (the power meter restarts its energy); non-blocking
//!   SYSTem:FUSE?   IDLE | BUSY | OK,<ohm> (~48 ohm) | OPEN (OL: fuse or lead)
//!     | FAIL,<ohm> (another resistance) | FAIL,NONE (no reading)
//!   FETCh:BURDen?   burden voltage (V) of the latest reading of a current
//!     function: I x R_path of the shunt in use (POW:BURDen; default the bare
//!     shunt 10 mohm / 0.5 ohm / 47.5 ohm), DC signed, AC and AC+DC the RMS,
//!     REL ignored; outside the A functions / no reading: +9.9E37 and "Data
//!     stale"; over range: +9.9E37 (doc 3.15). Works with the readout off.
//!   DISPlay:BURDen ON|OFF / ?   the burden voltage readout on the DMM page
//!     (DC A aux1, AC A aux2 instead of the duty cycle; not in AC+DC);
//!     Settings 3/3 "Burden voltage", persisted, default OFF
//!   POWer:BURDen <ohm>|DEFault / ?   (power meter section, 3.9) the R_path
//!     used by both: in a DMM A function that function's shunt, else the
//!     power meter's current range's
//!   STATus:QUEStionable:CONDition?   bit 9 while a red alert (AJACK,
//!     OVERLOAD) is on, bit 1 while OVERLOAD is (SCPI CURRent)
//!   STATus:QUEStionable[:EVENt]?   the same bits latched since the last
//!     read / *CLS (cleared by the read); no enable / transition registers,
//!     *STB? stays 0
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
//!     A and ohm (dmm.rs FR-05 table; other functions keep the stock timing):
//!     FAST 10/s, MED and SLOW 2.5/s steady (10/s while moving; SLOW
//!     averages 16 conversions, 32 on ohm r0..r3);
//!     query: SLOW / MED / FAST
//!   [SENSe:]RATE:PERiod?                  measured interval between readings,
//!     s (0 until two readings were seen)
//!   CALCulate:SMOothing[:STATe] ON|OFF / ?   display smoothing on (LOW) / off
//!   CALCulate:SMOothing:RESPonse SLOW|MEDium|FAST / ?   SLOW = HIGH (long
//!     window), MEDium / FAST = LOW; query SLOW / FAST. The smoothing feeds
//!     the readout, READ? / MEAS? / FETCh?; MIN/MAX/AVG and the CSV log stay
//!     unsmoothed (meter.rs header)

use crate::dmm::Reading;

/// *IDN? (model letter at IDN_MODEL_AT = the calibration model byte, `model`).
pub const IDN: &[u8] = b"Alientek,DM40A,0,dm40-fw 0.1";
const IDN_MODEL_AT: usize = 13;

/// Model index (0 DM40A, 1 DM40B, 2 DM40C) and the raw calibration byte
/// (record 0x080C0000 + 0x168, docs/model-variants.md), set once by App::new
/// from `Dmm::model()` / `Dmm::model_raw()`. *IDN? and SYSTem:MODel? report it.
static MODEL: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

pub fn set_model(index: u8, raw: u8) {
    MODEL.store(index.min(2) as u16 | (raw as u16) << 8, core::sync::atomic::Ordering::Relaxed);
}

fn model() -> (u8, u8) {
    let v = MODEL.load(core::sync::atomic::Ordering::Relaxed);
    (v as u8, (v >> 8) as u8)
}
/// *IDN? while SYST:IDN:EMUL 34401A is on: the HP 34401A identity (vendor
/// as the real meter's, libsigrok maps it to "HP"), our firmware in the
/// version field.
pub const IDN_34401A: &[u8] = b"HEWLETT-PACKARD,34401A,0,dm40-fw 0.1";

/// *IDN? compatibility mode (SYSTem:IDN:EMULation OFF|34401A; persisted, app.rs
/// persist_words p[0] bit 14): 0 = off (default), 1 = HP 34401A. For hosts
/// that pick a driver profile from *IDN? and know no DM40, e.g. libsigrok /
/// SmuView builds without the Alientek DM40 scpi-dmm profile: their "HP
/// 34401A" profile (since libsigrok 0.5.2) sends CONF:<f>, CONF?, INIT,
/// READ?, *OPC?, ABORT. With it on, READ? / MEAS:<f>? never wait for a
/// first reading longer than the quick budget (main.rs: +9.9E37 for the
/// first ~1..3 s after a function change), since such hosts do not wait
/// after CONF and time out after 1 s (USBTMC) / 1.5 s.
static IDN_EMUL: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn idn_emul() -> u8 {
    IDN_EMUL.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn set_idn_emul(v: u8) {
    IDN_EMUL.store(if v == 1 { 1 } else { 0 }, core::sync::atomic::Ordering::Relaxed);
}

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
    /// Inrush current (INRush:...), handled by the main loop with the app.
    Inr(Inr),
}

/// Inrush current (src/inrush.rs, src/app_inrush.rs; docs/inrush-current.md):
///   INRush[:STATe] ON|OFF / ?    open the Inrush Current page (the A
///                                function, AC row, 10 A range) and arm /
///                                close it (the A function's settings back)
///   INRush:ARM                   (re)arm: a new capture (opens the page)
///   INRush:THReshold <A> / ?     the trigger step, 0.1 .. 50 A (default 1)
///   INRush:CONDition?            OFF | IDLE | ARMING | ARMED | TRIGGERED |
///                                SAVING | DONE
///   INRush:FETCh?                Irms(100 ms),Ipeak,Iidle,lost: A (peak
///                                signed, +9.9E37 = OL), lost samples;
///                                all +9.9E37 + "Data stale" without a capture
///   INRush:FILE?                 "LOGS/FASTnnn.DLG" of the capture, or ""
///   INRush:PREamble?             rate_hz,samples,trigger_index,a,b (A = a + b
///                                * code; samples 0 without a capture)
///   INRush:DATA? [<start>[,<count>]]  #4nnnn block of the trace's codes,
///                                little-endian i16, count <= 3000 (default)
#[derive(Clone, Copy, PartialEq)]
pub enum Inr {
    State(Option<bool>),
    Arm,
    Thr(Option<f32>),
    Cond,
    Fetch,
    File,
    Pre,
    Data(u32, u32),
}

/// File commands on the USB disk's root (csvlog.rs gallery backend):
///   MMEMory:CATalog?        <used>,<free>{,"<name>,<type>,<size>"} (bytes; type
///                           always BIN: S*.BMP, LOG*.DLG, SCOP*.DLG; csvlog::catalog_scpi_on)
///   MMEMory:DELete "<name>" delete one S*.BMP / LOG*.DLG / SCOP*.DLG (USB only:
///                           refused over Bluetooth; refused for the file being recorded)
///   MMEMory:DATA? "<name>"[,<offset>[,<length>]]
///                           definite-length block #4nnnn of file bytes
///                           offset.. (length default and max 6000; short or
///                           empty at the end). "LOGnnn.CSV" / "SCOPnnn.CSV"
///                           = the CSV generated from LOGnnn.DLG / SCOPnnn.DLG
///                           (dlog.rs, the same bytes as LUN 1's virtual
///                           file); any other root file its raw bytes
#[derive(Clone, Copy)]
pub enum Mmem {
    Cat,
    Del([u8; 11]),
    Data([u8; 11], u32, u32),
}

/// MMEM:DATA? block payload at most (fits the 6144-byte USBTMC response).
pub const MMEM_DATA_MAX: u32 = 6000;

/// Data-logger commands (FR-38), executed by the main loop against `Logger`:
///   LOG:STATe ON|OFF        start / stop the fast stream
///   LOG:STATe?              1 / 0
///   LOG:OSR <0..7>          AD1OSR code (OSR = 256 << n, default 3); takes
///                           effect at START. Period OSR * 30 / 4.9152 MHz, x2
///                           chopped: 3 = 25 ms (40/s) .. 0 = 3.125 ms (320/s)
///   LOG:OSR?                current code
///   LOG:CHOP ON|OFF         AD1 chopper (default ON, as every stock row); OFF
///                           doubles the rate (OSR 256: 640/s, the chip's max)
///                           but leaves the converter offset in the samples
///                           and adds low-frequency noise; takes effect at START
///   LOG:CHOP?               1 / 0
///   LOG:SOURce AD1|AD2      converter (default AD1; takes effect at START).
///                           AD1: DC V / mV / A modes, LOG:OSR + LOG:CHOP.
///                           AD2 (FR-38 B): the stock AC rows, AC mV and AC A /
///                           mA / uA DC-coupled, AC V AC-coupled (~0.5 Hz);
///                           rate LOG:SRATe, LOG:CHOP = ENCHOPAD2 (logger.rs)
///                           ("Settings conflict" while a session runs)
///   LOG:SOURce?             AD1 / AD2
///   LOG:SRATe <S/s>         requested AD2 rate; the nearest rung of 1200,
///                           2400, 4800, 9600, 19200, 38400, 76800 is used (chop on or off: the
///                           chopper does not slow AD2) (default 2400, 4800
///                           with the ad2-isr stream ISR); 76800 needs the DMA
///                           frames (the default; DBG:AD2:DMA 0 or a forced /16
///                           / /32 divider cap it at 38400 / 19200, the rung
///                           used is in LOG:PREamble?); takes effect at START
///   LOG:SRATe?              the requested rate
///   LOG:RATE?               measured mean sample rate, Hz (0 until 2 samples)
///   LOG:COUNt?              buffered,captured,overwritten,missed,lost (lost =
///                           conversions not captured, from the span and the
///                           nominal period; missed = the per-gap estimate;
///                           overwritten = captured but dropped, ring full:
///                           the newest are dropped, seqs stay consecutive)
///   LOG:GAPs?               bad,max_gap_us,g2,g3_4,g5_16,g17_128,g129 (bad =
///                           AD2 reads dropped as torn; gN = gaps of N periods)
///   LOG:PREamble?           mode,range,osr,nominal_us,measured_hz,chop,source,
///                           req_hz,nominal_mhz,r26,coupling (source 0 AD1 / 1
///                           AD2; osr = AD1OSR or AD2OSR code; r26 = AD2 R26 low
///                           bits; coupling 0 DC / 1 AC). Scaling: AD1 raw
///                           counts, 1.389 V / 8388607 cts * range decade; AD2
///                           +-262143 codes = +-1.25 V at the AD2 input (IG
///                           1.0), times the stock range factor (mV: /x10 or
///                           x1 op gain; A: / (shunt * op gain); AC V: the
///                           divider); calibration gain/offset NOT applied
///   LOG:DATA?               definite-length #4nnnn block of 12-byte LE records
///                           {u32 seq, u32 ts_us, i32 raw}, oldest first
///   LOG:CSV ON|OFF          CSV recording of the DMM readings to the USB disk
///                           (csvlog.rs; as the DMM page swipe down). ON is
///                           USB only: refused over Bluetooth ("Command
///                           protected", main.rs; OFF and LOG:CSV? work). ON fails:
///                           "Mass storage busy" while a PC has the disk mounted,
///                           else "Settings conflict" (no reading yet, already on)
///   LOG:CSV?                state,file,rows,bytes,committed,dropped (state 0 idle,
///                           1 recording, 2 stopping, 3 done, 4 full, 5 no disk,
///                           6 error; file -1 until the first commit)
///   LOG:CSV:INTerval <s> / ?  row interval of the next recording (Record page,
///                           decision 2026-10-22 #1): 0 = every reading
///                           (default), 1, 10, 60 = one row per interval with
///                           the mean of its readings; other values: "Illegal
///                           parameter value"; while recording: "Settings
///                           conflict" (the running one keeps its settings)
///   LOG:CSV:DURation <s> / ?  length of the next recording: 0 = until stopped
///                           (default), 600, 3600, 28800, 86400; as INTerval
///   LOG:CSV:FREE?           free bytes on the USB disk ("Mass storage busy"
///                           while a commit / screenshot / upload holds it)
///   LOG:DEST DISK|HOST      where LOG:STATe ON sends the samples (default
///                           HOST = LOG:DATA?). DISK: the fast logger
///                           (fastlog.rs) writes LOGS/FASTnnn.DLG (served as
///                           FASTnnn.CSV): LOG:STATe ON pre-allocates and
///                           pre-erases LOG:DURation's worth (bounded by the
///                           free space; LOG:DISK? shows the progress), then
///                           starts the session; it ends at LOG:STATe OFF, at
///                           the duration or when the file is full. LOG:DATA?
///                           answers an empty block meanwhile. USB only (as
///                           LOG:CSV ON: "Command protected" over Bluetooth)
///   LOG:DEST?               DISK / HOST
///   LOG:DURation <s> / ?    length of a DISK session in device time, 0 =
///                           until stopped / the disk is full (default 60)
///   LOG:DISK?               state,file,alloc,bytes,rows,prep_pct,fifo_hw,programs,
///                           tpp_us,tpp_max_us (state 0 idle, 1 preparing, 2
///                           ready, 3 running, 4 stopping, 5 done, 6 full, 7 no
///                           disk, 8 error; file -1 until the entry exists;
///                           bytes = file bytes written; fifo_hw = the most
///                           bytes coded but not yet programmed, of 8192: the
///                           flash's margin; programs = page / header / partial
///                           programs started; tpp = the page-program probe of
///                           a session above 38400 S/s, mean / max us, 0 = none:
///                           over 800 / 1500 us the session runs at 38400)
/// Bluetooth (an untrusted ~11.5 kB/s link): LOG:STATe ON is refused above
/// BT_MAX_HZ ("Rate too high for Bluetooth", every AD2 rate; AD1 up to
/// 640 S/s works) and with LOG:DEST DISK ("Command protected"); LOG:DATA?
/// from Bluetooth of a faster session (started over USB) is refused too.
#[derive(Clone, Copy)]
pub enum Log {
    State(Option<bool>),
    /// LOG:SOURce AD1 (false) | AD2 (true).
    Source(Option<bool>),
    /// LOG:SRATe <S/s> (AD2).
    SRate(Option<u32>),
    Gaps,
    Osr(Option<u8>),
    Chop(Option<bool>),
    Rate,
    Count,
    Preamble,
    Data,
    Csv(Option<bool>),
    /// LOG:CSV:INTerval / DURation: Some(index into csvlog::INTERVALS_S /
    /// DURATIONS_S) to set, None = query.
    CsvInt(Option<u8>),
    CsvDur(Option<u8>),
    CsvFree,
    /// LOG:DEST DISK (true) | HOST (false) | ? (None).
    Dest(Option<bool>),
    /// LOG:DURation <s> | ?.
    Dur(Option<u32>),
    /// LOG:DISK?
    Disk,
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
///   DBG:SENTinel?           misconnect sentinel (sentinel.rs): kind,alert,mask,
///                           ctsta,pkhmin,pkhmax,streak,hit_windows,samples,
///                           probe_mA,aprot_ms (A-jack protection, doc 4.8)
///   DBG:LEADs?              "Leads in A?" reminders since power-on: n,from,to
///                           (Dmm::func codes of the last change that fired;
///                           255 = none yet; doc 4.9)
///   DBG:SENTinel:VARiant <0..31> / ?
///                           device-check row variants, RAM only (sentinel.rs
///                           VAR_*): +1 AC V keeps OP1's chopper (9dd1222 row),
///                           +2 no AC V sentinel, +4 comparator on DC V 60 V+,
///                           +8 none of the later sentinels (AC+DC, AC A,
///                           continuity, diode, cap r0..4, frequency), +16 no
///                           DC? / AC? input alternation
///   DBG:ACDC?               AC+DC phase diagnostic (Dmm::acdc_diag_text): phases
///                           started, then per recent AC phase (oldest first)
///                           ";late_ms:sq/lpf/dcp,..."; read-only
///   DBG:DRAG x0,y0,x1,y1,ms[,hold]
///                           synthetic one-finger drag through the gesture
///                           tracker (axis lock, capture): down at x0,y0, held
///                           still hold ms, moved to x1,y1 in ms ms (0 = 300),
///                           released; runs synchronously
///   DBG:HWTRIG?             comparator trigger (hwtrig.rs): TIM2 SR,CNT,CCR1,
///                           PA15<<16|DOR2,enabled,available,map rising lo,hi,
///                           map falling lo,hi,post_ticks (available = enabled,
///                           not suspended by 3 misses, map present; a suspension
///                           ends at the next range / timebase / coupling / edge
///                           / mode change)
///   DBG:HWTRIG:ENABLE 0|1   use the comparator trigger (scope default on,
///                           scope.rs Scope::new; 1 resets the miss count and
///                           a suspension)
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
///   DBG:TOUCH? | DBG:TOUCH 0  touch log (frames, events, gestures; touch.rs log_text) / clear
///   DBG:PROF?, DBG:PROF:RESET main-loop profile table (src/prof.rs) / clear it
///   DBG:HYSPI:SELF? n[,div[,cpha]]  HY3131 bus self-test (src/hyspi.rs; cpha 1 =
///                           the data phase sampled with CKPH = 1)
///   DBG:HYSPI:WR? mode[,div] SPI2 write experiment (src/hyspi.rs write_test):
///                           mode 0 hy3131::write, 1 SPI2 one byte per frame + a cut
///                           dummy, 2 SPI2 continuous + a cut dummy, 3 SPI2 as
///                           8e5efa6 (byte by byte, no dummy)
///   DBG:ISR?                AD2 stream ISR (logger.rs `isr`): feature,active,ticks,
///                           captured,late,probes,dropped,bad,timeouts,overruns,
///                           max_isr_ns,max_lat_ns,rate_mhz,forced,locked,ppm_x10,
///                           slips,max_run,edges_after_b,div,cpu_permille,lat_pc,dma
///                           (feature 0: no ISR in the build; timeouts = all
///                           HY3131 frames since boot; rate_mhz = the locked
///                           conversion rate in MCU time, ppm_x10 = its period vs
///                           the nominal rung, signed; lat_pc = the PC (0x hex)
///                           the worst-latency entry interrupted; AD1 sessions
///                           unchopped at 160 .. 640 S/s run on it too; in a main-loop
///                           AD1 session `late` = samples read blind after an
///                           AD1F lost to the INTF read-clear race)
///   DBG:FLASH?              SPI disk: jedec_id_hex,blocks,failed
///   DBG:RELEASE             stop feeding the watchdog: the WDT reset (~20 s) counts
///                           as a crash (1st: fw-rs reboots; 2nd without 60 s
///                           healthy: recovery = marker zeroed, DESTRUCTIVE)
///   DBG:RESET               reboot fw-rs (SW reset)
///   DBG:RECOVER, DBG:UPDATE DESTRUCTIVE: clear the previous-mode word, zero the 12-byte loader
///                           marker, reset -> vendor update screen + its USB disk until
///                           a dm40*.bin is installed. "Flash error" = marker did not
///                           read back as zero (no reset). Sync + eject a copied file first.
///   DBG:BOOT?               boot info (src/boot.rs put_info)
#[derive(Clone, Copy)]
pub enum Dbg {
    Ping(u32),
    Input(u8, u16, u16),
    /// x0, y0, x1, y1, ms, hold ms.
    Drag([u16; 6]),
    Lcd(u16, u16, u16, u16, u32),
    Diag,
    /// DBG:TOUCH? (false) / DBG:TOUCH 0 (true = clear): touch log.
    Touch(bool),
    Flash,
    Release,
    Reset,
    /// DBG:PROF? (false) / DBG:PROF:RESET (true).
    Prof(bool),
    /// DBG:AD2:TORN? r26,n[,SYNC] / NOISE? r26,n[,op1chop] / DRAW 0|1|2 /
    /// DIV 0|8|16|32[,cpha] (the stream ISR's read divider, 0 = automatic:
    /// /8 + CKPH = 1 data phase for CPU frames, /16 for DMA frames) / DMA 0|1
    /// (stream frames by the CPU / by DMA, the default: decision 2026-10-22 #7;
    /// DMA? = Ad2(4, 255): the setting, 0|1)
    /// (src/ad2exp.rs): 0 torn (a = n, b = sync), 1 noise (b = op1chop, 255
    /// = the row's), 2 draw (r26 = level), 5 DCV? range,r26,n[,flags] (the
    /// DC-coupled V row, device test 6.5: n | range << 8, flags).
    Ad2(u8, u8, u16, u8),
    /// DBG:HYSPI:SELF? n[,div[,cpha]] (src/hyspi.rs): (n, div | cpha << 7;
    /// div 0 = hy3131::DIV).
    HySpi(u16, u8),
    /// DBG:HYSPI:WR? mode[,div] (src/hyspi.rs `write_test`).
    HySpiWr(u8, u8),
    /// DBG:ISR? (logger.rs `isr`).
    Isr,
    /// Firmware update file upload (upload.rs): 0 BEGin size,crc32 | 1
    /// END? | 2 ABORt | 3 DBG:UPLoad? (DATA bypasses the parser, main.rs).
    Upload(u8, u32, u32),
}

/// DBG:AD2:DCV? range,r26,n[,flags] -> Dbg::Ad2(5, r26, n | range << 8,
/// flags) (main.rs `dbg_dcv`).
#[inline(never)] // .cold (link-native.x)
fn dcv_arg(a: &[u8]) -> Dbg {
    let w = nums::<4>(a);
    Dbg::Ad2(5, (w[1] & 0x3F) as u8, (w[2].clamp(1, 16) | w[0].min(3) << 8) as u16, (w[3] & 0x0F) as u8)
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
///   Volts (CHAN1:SCAL, TRIG:LEV, measurements, WAV:PRE) are at the probe
///   tip: the probe factor of the scope settings window (x0.1..x1000) is
///   applied; there is no SCPI probe command.
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
    /// UNIT:TEMPerature C|F (true = F) / ?
    TempUnit(Option<bool>),
    SysTemp,
    /// CONFigure?: "<func> <range>,<resolution>" as the Agilent / HP meters
    /// (App::scpi_dmm; "CONT" / "DIOD" bare).
    ConfQuery,
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
    /// [SENSe:]VOLTage:COUPling? (false) / CURRent:COUPling? (true).
    Coup(bool),
    CalcSmoState(Option<bool>),
    CalcSmoResp(Option<u8>),
    /// DBG:GRAB <ms> / ?: hold-to-grab time of scope drags (0 = direct).
    DbgGrab(Option<u16>),
    /// DBG:GEST?: log of the last scope drags.
    DbgGest,
    /// DBG:ACDC?: AC+DC phase diagnostic (Dmm::acdc_diag_text).
    DbgAcdc,
    /// SYSTem:ALERt?: the wrong-connection alert in force (sentinel.rs).
    SysAlert,
    /// STATus:QUEStionable:CONDition? (sentinel::ques_bits of the alert).
    StatQuesCond,
    /// SYSTem:FUSE:TEST (true) / SYSTem:FUSE? (false).
    SysFuse(bool),
    /// SYSTem:ALERt:ENABle <mask> | ON | OFF / ?: alerts enabled (bit 0
    /// A-JACK, 1 V-JACK?, 2 LIVE?, 3 the "Leads in A?" reminder).
    SysAlertEnable(Option<u8>),
    /// DBG:SENTinel?: sentinel raw state (Sentinel::diag_text).
    DbgSent,
    /// DBG:LEADs?: the "Leads in A?" reminder count and last change.
    DbgLeads,
    /// DBG:SENTinel:VARiant <0..31> / ?: device-check row variants
    /// (sentinel::VAR_*, RAM only).
    DbgSentVar(Option<u8>),
    /// DBG:HWTRIG...: 0 query, 1 ENABLE (bool), 2 CAL?, 3 STAT?, 4 STAT:RESET.
    DbgHwTrig(u8, bool),
    /// Power meter (dmm::pw): CONF:POW, FETC:POW:ALL?, [SENS:]POW:..., DBG:POW:...
    Pow(Pow),
    /// FETCh:BURDen?: burden voltage of the latest A-function reading (doc 3.15).
    FetchBurden,
    /// DISPlay:BURDen ON|OFF / ?: the burden voltage readout (Settings 3/3).
    DispBurden(Option<bool>),
}

/// ON | 1 -> true, OFF | 0 -> false.
/// Power-meter commands (`parse_pow`). Ranges: None = query, Some(None) =
/// AUTO, Some(Some(r)) = range index (V 0..3, I 0..5).
#[derive(Clone, Copy)]
pub enum Pow {
    /// CONFigure:POWer [<V>|AUTO[,<A>|AUTO]].
    Conf(Option<u8>, Option<u8>),
    /// FETCh:POWer:ALL?
    All,
    /// MEASure:POWer:APParent? (0) / :PFACtor? (1) / :ENERgy? (2) /
    /// :REACtive? (3) / :VOLTage? (4) / :CURRent? (5): the latest value.
    Get(u8),
    VRange(Option<Option<u8>>),
    IRange(Option<Option<u8>>),
    EnergyReset,
    EnergyState(Option<bool>),
    /// :BURDen <ohm>|DEFault (NaN) / ?, :BURDen:STATe ON|OFF / ?
    Burden(Option<f32>),
    BurdenState(Option<bool>),
    Phase(Option<i8>),
    /// Some(true) REACtive, Some(false) RESistive (refused), None = the query.
    PhaseCal(Option<bool>),
    Cycle,
    Method(Option<bool>),
    /// :LIMit <A>,<s> / ? (continuous current, countdown; s 0 = none).
    Limit(Option<(f32, u16)>),
    /// :COUPling DC|AC|ACDC / ? (pw::COUP_*; AUTO = ACDC, deprecated).
    Coupling(Option<u8>),
    /// :ZERO (0: start the self-zero, leads open), :ZERO CLEar (1), :ZERO? (2).
    Zero(u8),
    /// :VOLTage:CALibrate (0: start the V self-cal on the current V range),
    /// :VOLTage:CALibrate DEFault (1: the factory-derived gain again),
    /// :VOLTage:CALibrate? (2). :VOLTage:GAIN? (3): the 4 gains in force.
    VCal(u8),
    DbgRow,
    DbgPoke(u8, u8),
    DbgStat(u8),
    DbgRaw,
    DbgDisc(Option<u8>),
}

/// The message is a power-meter command (`parse_pow`).
fn is_pow(parts: &[&[u8]], query: bool) -> bool {
    let p0 = parts[0];
    let p1 = parts.get(1).is_some_and(|p| kw(p, "POWer"));
    kw(p0, "POWer")
        || (p1 && (kw(p0, "SENSe") || p0 == b"DBG"))
        || (p1 && kw(p0, "CONFigure") && !query)
        || (p1 && kw(p0, "FETCh") && query)
        || (p1 && kw(p0, "MEASure") && query && parts.len() > 2)
}

/// V range index for a value in V (the smallest that holds it).
fn pow_vrange(a: &[u8]) -> Option<Option<u8>> {
    if kw(a, "AUTO") {
        return Some(None);
    }
    let v = parse_num(a)?;
    crate::dmm::pw::V_LIM.iter().position(|&l| v <= l * 1.0001).map(|k| Some(k as u8))
}

/// I range index for a value in A.
fn pow_irange(a: &[u8]) -> Option<Option<u8>> {
    if kw(a, "AUTO") {
        return Some(None);
    }
    let v = parse_num(a)?;
    crate::dmm::pw::I_LIM.iter().position(|&l| v <= l * 1.0001).map(|k| Some(k as u8))
}

/// Parse a power-meter command (docs/power-meter-and-misconnect.md 3.9).
fn parse_pow(parts: &[&[u8]], arg: &[u8], query: bool) -> Result<Pow, &'static str> {
    const BAD: &str = "Illegal parameter value";
    const HDR: &str = "Undefined header";
    let a = trim(arg);
    let p0 = parts[0];
    let mut args = a.split(|&b| b == b',').map(trim);
    if kw(p0, "CONFigure") {
        let v = match args.next() {
            None | Some(b"") => None,
            Some(x) => Some(pow_vrange(x).ok_or(BAD)?.unwrap_or(255)),
        };
        let i = match args.next() {
            None => None,
            Some(x) => Some(pow_irange(x).ok_or(BAD)?.unwrap_or(255)),
        };
        return Ok(Pow::Conf(v, i));
    }
    if kw(p0, "FETCh") {
        return if parts.get(2).is_some_and(|p| kw(p, "ALL")) { Ok(Pow::All) } else { Err(HDR) };
    }
    if kw(p0, "MEASure") {
        let l = parts[2];
        let k = if kw(l, "APParent") {
            0
        } else if kw(l, "PFACtor") {
            1
        } else if kw(l, "ENERgy") {
            2
        } else if kw(l, "REACtive") {
            3
        } else if kw(l, "VOLTage") {
            4
        } else if kw(l, "CURRent") {
            5
        } else {
            return Err(HDR);
        };
        return Ok(Pow::Get(k));
    }
    // [SENSe:]POWer:..., DBG:POWer:...
    let dbg = p0 == b"DBG";
    let rest = if kw(p0, "POWer") { &parts[1..] } else { &parts[2..] };
    let l0 = rest.first().copied().unwrap_or(b"");
    let l1 = rest.get(1).copied();
    let state = |q: bool| if q { Ok(None) } else { on_off(a).map(Some).ok_or(BAD) };
    if dbg {
        return match l0 {
            b"ROW" if query => Ok(Pow::DbgRow),
            b"RAW" if query => Ok(Pow::DbgRaw),
            b"POKE" if !query => {
                if kw(a, "CLEar") {
                    return Ok(Pow::DbgPoke(255, 0));
                }
                let v = nums::<2>(a);
                if v[0] < 22 && v[1] < 256 { Ok(Pow::DbgPoke(v[0] as u8, v[1] as u8)) } else { Err(BAD) }
            }
            b"STAT" if !query => match a {
                b"XY" => Ok(Pow::DbgStat(crate::dmm::pw::R34_XY)),
                b"XX" => Ok(Pow::DbgStat(crate::dmm::pw::R34_XX)),
                b"YY" => Ok(Pow::DbgStat(crate::dmm::pw::R34_YY)),
                b"AUTO" => Ok(Pow::DbgStat(0)),
                _ => Err(BAD),
            },
            b"DISC" if query => Ok(Pow::DbgDisc(None)),
            b"DISC" => match parse_num(a) {
                Some(v) if (0.0..=8.0).contains(&v) => Ok(Pow::DbgDisc(Some(v as u8))),
                _ => Err(BAD),
            },
            _ => Err(HDR),
        };
    }
    if kw(l0, "VOLTage") && l1.is_some_and(|p| kw(p, "CALibrate")) {
        return if query {
            Ok(Pow::VCal(2))
        } else if a.is_empty() {
            Ok(Pow::VCal(0))
        } else if kw(a, "DEFault") {
            Ok(Pow::VCal(1))
        } else {
            Err(BAD)
        };
    }
    if kw(l0, "VOLTage") && l1.is_some_and(|p| kw(p, "GAIN")) && query {
        return Ok(Pow::VCal(3));
    }
    if kw(l0, "COUPling") && l1.is_none() {
        return if query {
            Ok(Pow::Coupling(None))
        } else if a == b"ACDC" || kw(a, "AUTO") {
            // AUTO (deprecated, doc 3.14): the former auto choice is AC+DC.
            Ok(Pow::Coupling(Some(crate::dmm::pw::COUP_ACDC)))
        } else if a == b"AC" {
            Ok(Pow::Coupling(Some(crate::dmm::pw::COUP_AC)))
        } else if a == b"DC" {
            Ok(Pow::Coupling(Some(crate::dmm::pw::COUP_DC)))
        } else {
            Err(BAD)
        };
    }
    if kw(l0, "ZERO") && l1.is_none() {
        return if query {
            Ok(Pow::Zero(2))
        } else if a.is_empty() {
            Ok(Pow::Zero(0))
        } else if kw(a, "CLEar") {
            Ok(Pow::Zero(1))
        } else {
            Err(BAD)
        };
    }
    if kw(l0, "VOLTage") && l1.is_some_and(|p| kw(p, "RANGe")) {
        return if query { Ok(Pow::VRange(None)) } else { pow_vrange(a).map(|r| Pow::VRange(Some(r))).ok_or(BAD) };
    }
    if kw(l0, "CURRent") && l1.is_some_and(|p| kw(p, "RANGe")) {
        return if query { Ok(Pow::IRange(None)) } else { pow_irange(a).map(|r| Pow::IRange(Some(r))).ok_or(BAD) };
    }
    if kw(l0, "ENERgy") {
        return match l1 {
            Some(p) if kw(p, "RESet") && !query => Ok(Pow::EnergyReset),
            Some(p) if kw(p, "STATe") => state(query).map(Pow::EnergyState),
            _ => Err(HDR),
        };
    }
    if kw(l0, "BURDen") {
        return match l1 {
            Some(p) if kw(p, "STATe") => state(query).map(Pow::BurdenState),
            None if query => Ok(Pow::Burden(None)),
            None if kw(a, "DEFault") => Ok(Pow::Burden(Some(f32::NAN))),
            None => match parse_num(a) {
                Some(v) if (0.0..=1000.0).contains(&v) => Ok(Pow::Burden(Some(v))),
                _ => Err(BAD),
            },
            _ => Err(HDR),
        };
    }
    if kw(l0, "PHASe") {
        return match l1 {
            Some(p) if kw(p, "CALibrate") && query => Ok(Pow::PhaseCal(None)),
            Some(p) if kw(p, "CALibrate") => {
                if a.is_empty() || kw(a, "REACtive") {
                    Ok(Pow::PhaseCal(Some(true)))
                } else if kw(a, "RESistive") {
                    Ok(Pow::PhaseCal(Some(false)))
                } else {
                    Err(BAD)
                }
            }
            None if query => Ok(Pow::Phase(None)),
            None => match parse_num(a) {
                Some(v) if (-127.0..=127.0).contains(&v) => Ok(Pow::Phase(Some(v as i8))),
                _ => Err(BAD),
            },
            _ => Err(HDR),
        };
    }
    if kw(l0, "CYCle") && query {
        return Ok(Pow::Cycle);
    }
    if kw(l0, "METHod") {
        return if query {
            Ok(Pow::Method(None))
        } else if kw(a, "SIMultaneous") || a == b"P" {
            Ok(Pow::Method(Some(false)))
        } else if kw(a, "ALTernate") || a == b"VA" {
            Ok(Pow::Method(Some(true)))
        } else {
            Err(BAD)
        };
    }
    if kw(l0, "LIMit") {
        if query {
            return Ok(Pow::Limit(None));
        }
        let i = args.next().and_then(parse_num).ok_or(BAD)?;
        let t = args.next().map_or(Some(crate::dmm::pw::T_LIM_DEFAULT as f32), parse_num).ok_or(BAD)?;
        return if (0.0..=10.0).contains(&i) && (0.0..=3600.0).contains(&t) { Ok(Pow::Limit(Some((i, t as u16)))) } else { Err(BAD) };
    }
    Err(HDR)
}

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

/// [SENSe:]VOLTage:COUPling (Some(false)) / [SENSe:]CURRent:COUPling
/// (Some(true)); None for any other header.
fn coup_cmd(parts: &[&[u8]]) -> Option<bool> {
    let r = if parts.first().is_some_and(|p| kw(p, "SENSe")) { &parts[1..] } else { parts };
    if r.len() != 2 || !kw(r[1], "COUPling") {
        return None;
    }
    if kw(r[0], "VOLTage") {
        Some(false)
    } else if kw(r[0], "CURRent") {
        Some(true)
    } else {
        None
    }
}

fn parse_func(parts: &[&[u8]]) -> Option<u8> {
    let p0 = *parts.first()?;
    let ac = parts.get(1).map(|p| kw(p, "AC")).unwrap_or(false);
    let acdc = parts.get(1).map(|p| kw(p, "ACDC")).unwrap_or(false);
    let auto = parts.get(1).map(|p| kw(p, "AUTO")).unwrap_or(false);
    let dc_or_none = parts.get(1).map(|p| kw(p, "DC")).unwrap_or(true);
    if kw(p0, "VOLTage") {
        if ac { Some(0) } else if acdc { Some(16) } else if auto { Some(crate::dmm::FUNC_V_AUTO) } else if dc_or_none { Some(1) } else { None }
    } else if kw(p0, "CURRent") {
        if ac { Some(5) } else if acdc { Some(17) } else if auto { Some(crate::dmm::FUNC_A_AUTO) } else if dc_or_none { Some(8) } else { None }
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
    } else if kw(p0, "POWer") && parts.len() == 1 {
        Some(crate::dmm::pw::FUNC)
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
        crate::dmm::FUNC_V_AUTO => "\"VOLT:AUTO\"",
        crate::dmm::FUNC_A_AUTO => "\"CURR:AUTO\"",
        crate::dmm::pw::FUNC => "\"POW\"",
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

    /// Decimal `v`.
    pub fn put_u32(&mut self, mut v: u32) {
        let mut t = [0u8; 10];
        let mut n = t.len();
        loop {
            n -= 1;
            t[n] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.put(&t[n..]);
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
                (b"*IDN", true) if idn_emul() == 1 => self.put(IDN_34401A),
                (b"*IDN", true) => {
                    let mut t = [0u8; IDN.len()];
                    t.copy_from_slice(IDN);
                    t[IDN_MODEL_AT] = b'A' + model().0;
                    self.put(&t);
                }
                // SYSTem:MODel?: "DM40A|B|C,<raw byte>,<counts>": the model the
                // calibration record names (stock rule: record +0x168, a record
                // without version 1 counts as DM40A) and the count limits
                // fw-rs uses on every model (60000, dmm.rs COUNTS_MODEL).
                _ if kw(p0, "SYSTem") && parts.len() == 2 && kw(parts[1], "MODel") && query => {
                    let (i, raw) = model();
                    self.put(b"DM40");
                    self.put(&[b'A' + i, b',']);
                    let mut nb = [0u8; 3];
                    let mut n = 0;
                    let mut v = raw;
                    loop {
                        nb[2 - n] = b'0' + v % 10;
                        n += 1;
                        v /= 10;
                        if v == 0 {
                            break;
                        }
                    }
                    self.put(&nb[3 - n..]);
                    self.put(b",60000");
                }
                _ if kw(p0, "SYSTem") && parts.len() > 2 && kw(parts[1], "IDN") && kw(parts[2], "EMULation") => {
                    if query {
                        self.put(if idn_emul() == 1 { b"34401A" } else { b"OFF" });
                    } else {
                        match trim(arg) {
                            b"OFF" | b"0" => set_idn_emul(0),
                            b"34401A" | b"HP34401A" => set_idn_emul(1),
                            _ => self.err = Some("Illegal parameter value"),
                        }
                    }
                }
                // Accepted for Agilent / HP-style hosts (libsigrok's 34401A
                // profile sends INIT at the start, ABORT at the end); the
                // meter measures continuously, READ? takes the next reading.
                _ if (kw(p0, "INITiate") || kw(p0, "ABORt")) && !query => {}
                _ if kw(p0, "SYSTem") && parts.len() == 2 && (kw(parts[1], "LOCal") || kw(parts[1], "REMote")) && !query => {}
                (b"*RST", false) | (b"*CLS", false) => {
                    self.err = None;
                    crate::sentinel::QUES_EVENT.store(0, core::sync::atomic::Ordering::Relaxed);
                }
                _ if kw(p0, "STATus") && parts.len() > 1 && kw(parts[1], "QUEStionable") && query => {
                    if parts.len() == 3 && kw(parts[2], "CONDition") {
                        out = Out::Instr(Cmd::StatQuesCond);
                    } else if parts.len() == 2 || (parts.len() == 3 && kw(parts[2], "EVENt")) {
                        let v = crate::sentinel::QUES_EVENT.swap(0, core::sync::atomic::Ordering::Relaxed);
                        self.put_u32(v as u32);
                    } else {
                        self.err = Some("Undefined header");
                    }
                }
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
                _ if kw(p0, "SYSTem") && parts.len() > 1 && kw(parts[1], "FUSE") => {
                    out = Out::Instr(match (parts.len(), query) {
                        (2, true) => Cmd::SysFuse(false),
                        (3, false) if kw(parts[2], "TEST") => Cmd::SysFuse(true),
                        _ => {
                            self.err = Some("Undefined header");
                            continue;
                        }
                    });
                }
                _ if kw(p0, "SYSTem") && parts.len() > 1 && kw(parts[1], "ALERt") => {
                    let a = trim(arg);
                    out = Out::Instr(if parts.len() == 2 && query {
                        Cmd::SysAlert
                    } else if parts.len() == 3 && kw(parts[2], "ENABle") && query {
                        Cmd::SysAlertEnable(None)
                    } else if parts.len() == 3 && kw(parts[2], "ENABle") {
                        match on_off(a) {
                            Some(on) => Cmd::SysAlertEnable(Some(if on { crate::sentinel::M_ALL } else { 0 })),
                            None => match parse_num(a) {
                                Some(v) if (0.0..=15.0).contains(&v) && v == (v as u8) as f32 => Cmd::SysAlertEnable(Some(v as u8)),
                                _ => {
                                    self.err = Some("Illegal parameter value");
                                    continue;
                                }
                            },
                        }
                    } else {
                        self.err = Some("Undefined header");
                        continue;
                    });
                }
                _ if kw(p0, "UNIT") && parts.len() > 1 && kw(parts[1], "TEMPerature") => {
                    let a = trim(arg);
                    out = Out::Instr(if query {
                        Cmd::TempUnit(None)
                    } else if kw(a, "C") || kw(a, "CEL") {
                        Cmd::TempUnit(Some(false))
                    } else if kw(a, "F") || kw(a, "FAR") {
                        Cmd::TempUnit(Some(true))
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
                _ if is_pow(parts, query) => match parse_pow(parts, arg, query) {
                    Ok(c) => out = Out::Instr(Cmd::Pow(c)),
                    Err(e) => self.err = Some(e),
                },
                _ if kw(p0, "MEASure") && query => match parse_func_arg(&parts[1..], arg) {
                    Some(f) => out = Out::Measure(Some(f)),
                    None => self.err = Some("Command error"),
                },
                _ if kw(p0, "CONFigure") && query && parts.len() == 1 => out = Out::Instr(Cmd::ConfQuery),
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
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"ACDC"[..]) => out = Out::Instr(Cmd::DbgAcdc),
                _ if p0 == b"DBG" && parts.get(1).is_some_and(|p| kw(p, "SENTinel")) && parts.get(2).is_some_and(|p| kw(p, "VARiant")) => {
                    match parse_num(arg) {
                        _ if query => out = Out::Instr(Cmd::DbgSentVar(None)),
                        Some(v) if (0.0..=31.0).contains(&v) && v == (v as u8) as f32 => out = Out::Instr(Cmd::DbgSentVar(Some(v as u8))),
                        _ => self.err = Some("Illegal parameter value"),
                    }
                }
                _ if p0 == b"DBG" && query && parts.get(1).is_some_and(|p| kw(p, "SENTinel")) => out = Out::Instr(Cmd::DbgSent),
                _ if p0 == b"DBG" && query && parts.len() == 2 && kw(parts[1], "LEADs") => out = Out::Instr(Cmd::DbgLeads),
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
                // HY3131 bus self-test (src/hyspi.rs, main.rs).
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"HYSPI"[..]) => {
                    let a = trim(arg);
                    let mut it = a.split(|&b| b == b',').map(trim);
                    let first = it.next().unwrap_or(&[]);
                    let div = it.next().and_then(parse_num).map_or(0, |d| d as u32);
                    let div = if matches!(div, 8 | 16 | 32) { div as u8 } else { 0 };
                    let cpha = it.next().and_then(parse_num).is_some_and(|c| c != 0.0);
                    let d = match (parts.get(2).copied(), query) {
                        (Some(b"SELF"), true) => {
                            let n = parse_num(first).map_or(100, |v| v as u32).clamp(1, crate::hyspi::N_MAX);
                            Some(Dbg::HySpi(n as u16, div | (cpha as u8) << 7))
                        }
                        (Some(b"WR"), true) => Some(Dbg::HySpiWr(parse_num(first).map_or(1, |v| v as u32).min(3) as u8, div)),
                        _ => None,
                    };
                    match d {
                        Some(d) => out = Out::Dbg(d),
                        None => self.err = Some("Undefined header"),
                    }
                }
                // AD2 device experiments (src/ad2exp.rs, main.rs).
                _ if p0 == b"DBG" && parts.get(1) == Some(&&b"AD2"[..]) => {
                    let a = trim(arg);
                    let v = nums::<2>(a);
                    let third = a.split(|&b| b == b',').nth(2).map(trim);
                    let n = v[1].clamp(1, 2000) as u16; // ad2exp::N_MAX
                    let r26 = (v[0] & 0x3F) as u8;
                    let d = match (parts.get(2).copied(), query) {
                        (Some(b"TORN"), true) => Some(Dbg::Ad2(0, r26, n, third.is_some_and(|t| kw(t, "SYNChronous")) as u8)),
                        (Some(b"NOISE"), true) => {
                            Some(Dbg::Ad2(1, r26, n, third.and_then(parse_num).map_or(255, |c| (c as u8).min(3))))
                        }
                        (Some(b"DRAW"), false) => Some(Dbg::Ad2(2, (v[0].min(2)) as u8, 0, 0)),
                        (Some(b"DIV"), false) => Some(Dbg::Ad2(3, (v[0].min(255)) as u8, 0, (v[1] != 0) as u8)),
                        (Some(b"DMA"), false) => Some(Dbg::Ad2(4, (v[0].min(1)) as u8, 0, 0)),
                        (Some(b"DMA"), true) => Some(Dbg::Ad2(4, 255, 0, 0)),
                        // DCV? range,r26,n[,flags]: (5, r26, n | range << 8, flags).
                        (Some(b"DCV"), true) => Some(dcv_arg(a)),
                        _ => None,
                    };
                    match d {
                        Some(d) => out = Out::Dbg(d),
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
                        b"DIAG" => Dbg::Diag, b"TOUCH" => Dbg::Touch(!query),
                        _ if kw(sub, "UPLoad") => {
                            let op = parts.get(2).copied().unwrap_or(b"");
                            let mut v = [0u32; 2];
                            for (d, t) in v.iter_mut().zip(a.split(|&b| b == b',')) {
                                *d = core::str::from_utf8(trim(t)).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
                            }
                            if op.is_empty() && query {
                                Dbg::Upload(3, 0, 0)
                            } else if kw(op, "BEGin") {
                                Dbg::Upload(0, v[0], v[1])
                            } else if kw(op, "END") && query {
                                Dbg::Upload(1, 0, 0)
                            } else if kw(op, "ABORt") {
                                Dbg::Upload(2, 0, 0)
                            } else {
                                self.err = Some("Undefined header");
                                continue;
                            }
                        }
                        b"PROF" => Dbg::Prof(parts.get(2) == Some(&&b"RESET"[..])),
                        b"FLASH" => Dbg::Flash,
                        b"ISR" => Dbg::Isr,
                        b"RELEASE" => Dbg::Release,
                        b"RESET" => Dbg::Reset,
                        // DBG:UPDATE = DBG:RECOVER: zero the 12-byte loader
                        // marker at 0x08008000, then the deferred SYSRESETREQ ->
                        // vendor update screen + its USB disk (docs/native-boot.md).
                        // DBG:BOOT? = boot info.
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
                        b"BOOT" => {
                            crate::boot::put_info(self);
                            continue;
                        }
                        // The generated USB volume: EJECT n ("no medium" for n
                        // commands, then UNIT ATTENTION) | STAT? -> <files>,<next free
                        // cluster>,<files left out> | RO 0|1 (write-protect it: hosts
                        // mount it read-only, live updates while mounted; read at
                        // the host's next mount), RO? | WR? -> host writes (vwrite.rs):
                        // <accepted>,<refused>,<busy>,<last refused LBA>,<SPI files
                        // deleted>,<firmware files committed>,<rejected>,<root
                        // records>,<FAT pairs>,<staged clusters>,<frozen>,<staging>,
                        // <firmware state 0 none 1 ask 2 asked 3 declined 4 install 5 bad>.
                        #[cfg(feature = "vdisk")]
                        b"VDISK" => { self.len = 0; let v = nums::<1>(a); match parts.get(2).copied().unwrap_or(b"") { b"EJECT" => { crate::msc::DBG_EJECT.store(v[0].clamp(1, 1000) as u32, core::sync::atomic::Ordering::Relaxed); self.put(b"OK") } b"RD" => { use core::sync::atomic::Ordering::Relaxed; let r = &crate::vdisk::RD; let f = &crate::dlog::FR; for (i, x) in [r[0].load(Relaxed), (r[1].load(Relaxed) as u64 * 1024 / (crate::clock::HCLK_HZ as u64 / 1000)) as u32, r[2].load(Relaxed) / (crate::clock::HCLK_HZ / 1_000_000), r[3].load(Relaxed), f[0].load(Relaxed), f[1].load(Relaxed), f[2].load(Relaxed), f[3].load(Relaxed), f[4].load(Relaxed)].iter().enumerate() { if i > 0 { self.put(b","); } self.put_u32(*x); } } b"STAT" => { let (n, hw, out) = crate::vdisk::stats(); for (i, x) in [n as u32, hw, out as u32].iter().enumerate() { if i > 0 { self.put(b","); } self.put_u32(*x); } } b"RO" => { if query { self.put_u32(crate::vwrite::read_only() as u32); self.put(b","); self.put_u32(crate::vwrite::read_only_pending() as u32) } else { crate::vwrite::set_read_only(v[0] != 0); self.put(b"OK") } } b"WR" => { let n = crate::vwrite::stats(); let (rec, pairs, st, fz, stg, fw) = crate::vwrite::state(); for (i, x) in [n.ok, n.refused, n.busy, n.last_refused, n.deleted as u32, n.fw_ok as u32, n.fw_bad as u32, rec as u32, pairs as u32, st as u32, fz as u32, stg as u32, fw as u32].iter().enumerate() { if i > 0 { self.put(b","); } self.put_u32(*x); } } _ => self.err = Some("Undefined header") } continue; }
                        b"RECOVER" | b"UPDATE" => { if !crate::boot::enter_loader_update() { self.err = Some("Flash error"); continue; } Dbg::Reset }
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
                _ if kw(p0, "INRush") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    let c = if sub.is_empty() || kw(sub, "STATe") {
                        if query { Some(Inr::State(None)) } else { on_off(a).map(|b| Inr::State(Some(b))) }
                    } else if kw(sub, "ARM") && !query {
                        Some(Inr::Arm)
                    } else if kw(sub, "THReshold") {
                        if query { Some(Inr::Thr(None)) } else { parse_num(a).map(|v| Inr::Thr(Some(v))) }
                    } else if kw(sub, "CONDition") && query {
                        Some(Inr::Cond)
                    } else if kw(sub, "FETCh") && query {
                        Some(Inr::Fetch)
                    } else if kw(sub, "FILE") && query {
                        Some(Inr::File)
                    } else if kw(sub, "PREamble") && query {
                        Some(Inr::Pre)
                    } else if kw(sub, "DATA") && query {
                        let mut v = [0u32, 3000];
                        for (i, part) in a.split(|&b| b == b',').take(2).enumerate() {
                            if let Some(x) = parse_num(part) {
                                v[i] = x.max(0.0) as u32;
                            }
                        }
                        Some(Inr::Data(v[0], v[1]))
                    } else {
                        None
                    };
                    match c {
                        Some(c) => out = Out::Inr(c),
                        None => {
                            self.err = Some(if sub.is_empty() || kw(sub, "STATe") || kw(sub, "THReshold") { "Illegal parameter value" } else { "Undefined header" });
                            continue;
                        }
                    }
                }
                _ if kw(p0, "LOG") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    let a = trim(arg);
                    out = if kw(sub, "STATe") {
                        Out::Log(Log::State(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else if kw(sub, "SOURce") {
                        if query {
                            Out::Log(Log::Source(None))
                        } else if a == b"AD2" || a == b"AD1" {
                            Out::Log(Log::Source(Some(a == b"AD2")))
                        } else {
                            self.err = Some("Illegal parameter value");
                            continue;
                        }
                    } else if kw(sub, "SRATe") {
                        match (query, parse_num(a)) {
                            (true, _) => Out::Log(Log::SRate(None)),
                            (false, Some(v)) if v >= 1.0 && v <= 1.0e6 => Out::Log(Log::SRate(Some(v as u32))),
                            _ => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
                    } else if kw(sub, "GAPs") && query {
                        Out::Log(Log::Gaps)
                    } else if kw(sub, "OSR") {
                        Out::Log(Log::Osr(if query { None } else { Some(parse_num(a).unwrap_or(0.0) as u8) }))
                    } else if kw(sub, "CHOP") {
                        Out::Log(Log::Chop(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else if kw(sub, "RATE") && query {
                        Out::Log(Log::Rate)
                    } else if kw(sub, "COUNt") && query {
                        Out::Log(Log::Count)
                    } else if kw(sub, "PREamble") && query {
                        Out::Log(Log::Preamble)
                    } else if kw(sub, "DATA") && query {
                        Out::Log(Log::Data)
                    } else if kw(sub, "CSV") && parts.len() > 2 {
                        let s2 = parts[2];
                        // Seconds -> index of the allowed value (exact).
                        let idx = |set: &[u32]| parse_num(a).and_then(|v| set.iter().position(|&x| x as f32 == v)).map(|i| i as u8);
                        if kw(s2, "INTerval") || kw(s2, "DURation") {
                            let int = kw(s2, "INTerval");
                            let set: &[u32] = if int { &crate::csvlog::INTERVALS_S } else { &crate::csvlog::DURATIONS_S };
                            let v = if query { None } else {
                                match idx(set) {
                                    Some(i) => Some(i),
                                    None => {
                                        self.err = Some("Illegal parameter value");
                                        continue;
                                    }
                                }
                            };
                            Out::Log(if int { Log::CsvInt(v) } else { Log::CsvDur(v) })
                        } else if kw(s2, "FREE") && query {
                            Out::Log(Log::CsvFree)
                        } else {
                            self.err = Some("Undefined header");
                            continue;
                        }
                    } else if kw(sub, "CSV") {
                        Out::Log(Log::Csv(if query { None } else { Some(a == b"ON" || a == b"1") }))
                    } else if kw(sub, "DEST") {
                        match (query, a) {
                            (true, _) => Out::Log(Log::Dest(None)),
                            (false, x) if x.eq_ignore_ascii_case(b"DISK") => Out::Log(Log::Dest(Some(true))),
                            (false, x) if x.eq_ignore_ascii_case(b"HOST") => Out::Log(Log::Dest(Some(false))),
                            _ => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
                    } else if kw(sub, "DURation") {
                        match (query, parse_num(a)) {
                            (true, _) => Out::Log(Log::Dur(None)),
                            (false, Some(v)) if (0.0..=1.0e6).contains(&v) => Out::Log(Log::Dur(Some(v as u32))),
                            _ => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
                    } else if kw(sub, "DISK") && query {
                        Out::Log(Log::Disk)
                    } else {
                        self.err = Some("Undefined header");
                        continue;
                    };
                }
                _ if kw(p0, "MMEMory") => {
                    let sub = parts.get(1).copied().unwrap_or(&[]);
                    out = if kw(sub, "CATalog") && query {
                        Out::Mmem(Mmem::Cat)
                    } else if kw(sub, "DATA") && query {
                        // "<name>"[,<offset>[,<length>]]
                        let mut it = arg.split(|&c| c == b',');
                        let name = it.next().and_then(|n| crate::csvlog::parse_name(trim(n)));
                        let mut num = |d: u32| -> Option<u32> {
                            match it.next() {
                                None => Some(d),
                                Some(t) => core::str::from_utf8(trim(t)).ok()?.parse().ok(),
                            }
                        };
                        match (name, num(0), num(MMEM_DATA_MAX)) {
                            (Some(n), Some(off), Some(len)) => Out::Mmem(Mmem::Data(n, off, len.min(MMEM_DATA_MAX))),
                            _ => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
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
                _ if kw(p0, "FETCh") && query && parts.len() == 2 && kw(parts[1], "BURDen") => {
                    out = Out::Instr(Cmd::FetchBurden)
                }
                _ if kw(p0, "DISPlay") && parts.len() == 2 && kw(parts[1], "BURDen") => {
                    out = Out::Instr(Cmd::DispBurden(if query {
                        None
                    } else {
                        match on_off(trim(arg)) {
                            Some(b) => Some(b),
                            None => {
                                self.err = Some("Illegal parameter value");
                                continue;
                            }
                        }
                    }));
                }
                _ if kw(p0, "FETCh") && query => out = Out::Fetch,
                // [SENSe:]VOLTage:COUPling / CURRent:COUPling (fw-rs).
                _ if coup_cmd(parts).is_some() => {
                    let amp = coup_cmd(parts) == Some(true);
                    if query {
                        out = Out::Instr(Cmd::Coup(amp));
                        continue;
                    }
                    let a = trim(arg);
                    let f = |v, i| if amp { i } else { v };
                    let m = if a == b"DC" {
                        Some(f(1, 8))
                    } else if a == b"AC" {
                        Some(f(0, 5))
                    } else if a == b"ACDC" {
                        Some(f(16, 17))
                    } else if kw(a, "AUTO") {
                        Some(f(crate::dmm::FUNC_V_AUTO, crate::dmm::FUNC_A_AUTO))
                    } else {
                        None
                    };
                    match m {
                        Some(m) => out = Out::Configure(m),
                        None => self.err = Some("Illegal parameter value"),
                    }
                }
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

    /// CONFigure? reply for stock mode `mode` (Scpi::handle `cur`) and the
    /// latest reading of it: "\"VOLT +6.000000E+00,+1.000000E-04\"". Exact
    /// powers of ten (hosts take the resolution's exponent as the digits).
    pub fn put_conf(&mut self, mode: u8, r: Option<&Reading>) {
        let name = func_name(mode).as_bytes();
        self.put(&name[..name.len() - 1]);
        if matches!(mode, 11 | 12) {
            self.put(b"\"");
            return;
        }
        // Resolution exponent of the readout: -decimals + the unit prefix.
        let (exp, ext) = match r {
            Some(r) => {
                let pfx = match r.main.unit.chars().next() {
                    Some('m') => -3,
                    Some('\u{b5}') | Some('u') => -6,
                    Some('n') => -9,
                    Some('k') => 3,
                    Some('M') => 6,
                    _ => 0,
                };
                let pfx = if r.main.unit.chars().count() > 1 { pfx } else { 0 };
                (pfx - r.main.dec as i32, r.main.ext as i32)
            }
            None => (-4, 0),
        };
        fn pow10(s: &mut Scpi, m: u8, e: i32) {
            s.put(&[b'+', m, b'.', b'0', b'0', b'0', b'0', b'0', b'0', b'E', if e < 0 { b'-' } else { b'+' }]);
            let a = e.unsigned_abs();
            s.put(&[b'0' + (a / 10 % 10) as u8, b'0' + (a % 10) as u8]);
        }
        self.put(b" ");
        pow10(self, b'6', exp + 4);
        self.put(b",");
        pow10(self, b'1', exp - ext);
        self.put(b"\"");
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
