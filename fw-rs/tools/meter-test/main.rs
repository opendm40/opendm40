// Host test crate for src/meter.rs and src/dmm.rs (FR-05 smoothing / rate,
// UF-21): the modules are compiled for the host with their register-level
// dependencies; the tests only call pure logic (no MMIO is touched).
#![allow(dead_code, unused_imports, clippy::all)]
#[path = "../../src/mmio.rs"]
mod mmio;
#[path = "../../src/hy3131.rs"]
mod hy3131;
#[path = "../../src/hy3131_tables.rs"]
mod hy3131_tables;
#[path = "../../src/clock.rs"]
mod clock;
#[path = "../../src/dmm.rs"]
mod dmm;
#[path = "../../src/meter.rs"]
mod meter;

fn main() {}
