//! One SCPI program message (a USBTMC DEV_DEP_MSG_OUT, a Bluetooth line) run
//! command by command, with its replies joined into one response (IEEE 488.2
//! 8.4 / 6.4.3: each query's reply in order, ';' between them, one
//! terminator).
//!
//! main.rs takes the commands (`next`) one at a time and runs each through
//! `Scpi::handle` and the dispatch that follows it, so a command the main
//! loop acts on (instrument settings, POW:*, LOG:*, MMEM:*, DBG:*) has run
//! before the next command of the same message is parsed: in
//! `POW:PHAS:CAL RES;:SYST:ERR?` the error query sees the refusal.
//! Formerly the whole message went through one `handle` call, which kept
//! only the last deferred command and dropped the text replies before it,
//! so such a message never got a reply and the host's read hung.
//!
//! A query that waits for a DMM reading (READ?, MEAS:<f>?, FETC?) pauses
//! the message; the reading is appended when it comes (or at the wait's
//! timeout) and the rest of the message then runs.
//!
//! A message with at least one query always gets a response, empty if no
//! query could be answered (the reasons are in SYSTem:ERRor?): the host's
//! read never hangs. A query without a reply contributes no reply unit.
//!
//! Replies built in the transport's own buffer (block data: MMEM:DATA?,
//! MMEM:CAT?, LOG:DATA?, DBG:LCD? / PROF? / TOUCH? / HYSPI? / AD2?) cannot be
//! joined: they are answered only as the sole query of a message; with other
//! queries they are refused ("Query error", no reply unit).
//!
//! Pure logic (no hardware): tools/scpi-msg-test runs it on the host.

/// Longest message (main.rs command buffer).
pub const MSG_MAX: usize = 256;
/// Joined reply (WAV:DATA? alone is 405 bytes).
pub const ACC_MAX: usize = 512;

pub struct Msg {
    msg: [u8; MSG_MAX],
    n: usize,
    pos: usize,
    active: bool,
    paused: bool,
    queries: u16,
    /// A query was answered (possibly with an empty reply) or a reply was
    /// sent from the transport's buffer.
    answered: bool,
    raw: bool,
    acc: [u8; ACC_MAX],
    len: usize,
}

/// A command (one ';' part) is a query: its header ends in '?'.
pub fn is_query(part: &[u8]) -> bool {
    let p = trim(part);
    let end = p.iter().position(|b| b.is_ascii_whitespace()).unwrap_or(p.len());
    p[..end].last() == Some(&b'?')
}

fn sep(b: u8) -> bool {
    b == b';' || b == b'\n' || b == b'\r'
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
    let e = s.iter().rposition(|b| !b.is_ascii_whitespace()).map(|i| i + 1).unwrap_or(a);
    &s[a..e.max(a)]
}

impl Msg {
    pub const fn new() -> Self {
        Msg {
            msg: [0; MSG_MAX],
            n: 0,
            pos: 0,
            active: false,
            paused: false,
            queries: 0,
            answered: false,
            raw: false,
            acc: [0; ACC_MAX],
            len: 0,
        }
    }

    /// Start a new message (drops any unfinished one).
    pub fn start(&mut self, m: &[u8]) {
        let n = m.len().min(MSG_MAX);
        self.msg[..n].copy_from_slice(&m[..n]);
        self.n = n;
        self.pos = 0;
        self.active = true;
        self.paused = false;
        self.answered = false;
        self.raw = false;
        self.len = 0;
        self.queries = self.msg[..n].split(|&b| sep(b)).filter(|p| is_query(p)).count() as u16;
    }

    /// Forget the message (device clear): no reply.
    pub fn reset(&mut self) {
        self.active = false;
        self.paused = false;
    }

    /// A message is in progress (commands left, waiting, or its reply not
    /// yet taken).
    pub fn active(&self) -> bool {
        self.active
    }

    /// In progress and not waiting for a reading: main.rs runs the rest.
    pub fn ready(&self) -> bool {
        self.active && !self.paused
    }

    /// More than one query in the message (block replies are refused).
    pub fn multi(&self) -> bool {
        self.queries > 1
    }

    /// Copy the next non-empty command into `out`; its length. None: no more
    /// commands now (finished, or waiting for a reading).
    pub fn next(&mut self, out: &mut [u8]) -> Option<usize> {
        if !self.active || self.paused {
            return None;
        }
        while self.pos < self.n {
            let s = self.pos;
            let e = self.msg[s..self.n].iter().position(|&b| sep(b)).map_or(self.n, |i| s + i);
            self.pos = (e + 1).min(self.n);
            if e == self.n {
                self.pos = self.n;
            }
            let p = &self.msg[s..e];
            if trim(p).is_empty() {
                continue;
            }
            let k = p.len().min(out.len());
            out[..k].copy_from_slice(&p[..k]);
            return Some(k);
        }
        None
    }

    /// The command just run waits for a reading: the message continues
    /// after `reply` gives it.
    pub fn pause(&mut self) {
        if self.active {
            self.paused = true;
        }
    }

    /// Append one command's reply (a reply unit; empty: none). False: no
    /// room (the unit is dropped, the query still counts as answered).
    pub fn reply(&mut self, r: &[u8]) -> bool {
        self.paused = false;
        self.answered = true;
        if r.is_empty() {
            return true;
        }
        let need = r.len() + (self.len > 0) as usize;
        if self.len + need > ACC_MAX {
            return false;
        }
        if self.len > 0 {
            self.acc[self.len] = b';';
            self.len += 1;
        }
        self.acc[self.len..self.len + r.len()].copy_from_slice(r);
        self.len += r.len();
        true
    }

    /// A reply went out from the transport's own buffer (block data).
    pub fn raw_sent(&mut self) {
        self.raw = true;
        self.answered = true;
    }

    /// The message is finished (`next` gave None and it is not waiting):
    /// the response to send, if any. Ends the message.
    pub fn finish(&mut self) -> Option<&[u8]> {
        if !self.active || self.paused || self.pos < self.n {
            return None;
        }
        self.active = false;
        if self.raw {
            // The block reply was the response (a non-query's text beside
            // it, e.g. a DBG "OK", is dropped: one response per message).
            return None;
        }
        if self.len > 0 || self.answered || self.queries > 0 {
            Some(&self.acc[..self.len])
        } else {
            None
        }
    }
}
