//! The scope record's tail (docs/ram-pools-audit.md candidate B; pool.rs
//! `TAIL`, scope.rs "record tail"): the lenders' side. A lender's start
//! asks the scope for its segment first (`tail_lend`); an idle lender's
//! segment goes back to the pool every pass (`tail_reclaim`), for the
//! scope's next acquisition (`Scope::grab_tail`).

use super::*;

/// The tail segments in address order (`Scope::yield_tail`): the CH2
/// store, the logger ring's slots, the CSV row FIFO.
pub const SEG_CH2: usize = 0;
pub const SEG_RING: usize = 1;
pub const SEG_FIFO: usize = 2;

impl App {
    /// A lender of segment `k` starts now: the scope gives it and the
    /// segments after it back (a running acquisition restarts shorter, a
    /// stopped record is cut). False: a long stored record is being read
    /// (WAV:DATA?, a scope CSV save): the lender waits (LOG:STATe ON, CH2)
    /// or is refused (a CSV recording).
    #[inline(always)]
    pub fn tail_lend(&mut self, k: usize) -> bool {
        self.scope.yield_tail(k)
    }

    /// Idle lenders give their segment back (main.rs, every pass, with
    /// `Logger::slots_release` for the ring's slots): CH2 (`ch2_holds`),
    /// the FIFO (`csvlog::fifo_release`).
    #[inline(never)]
    pub fn tail_reclaim(&mut self) {
        use crate::pool::{Tenant, TAIL};
        if TAIL.holds(Tenant::Ch2Store) && !self.ch2_holds() {
            crate::scope_ch2::store().clear();
            TAIL.give(Tenant::Ch2Store);
        }
        crate::csvlog::fifo_release();
    }
}
