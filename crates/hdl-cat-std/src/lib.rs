//! Standard combinational components built from `hdl-cat-circuit`
//! primitives.
//!
//! This crate is intentionally small for the first cut: it
//! provides single-bit half- and full-adders and a few other
//! bus-manipulation utilities, all expressed as
//! [`hdl_cat_circuit::CircuitArrow`]s.  Stateful components
//! (counters, FIFOs, RAMs, FSMs) will be added once
//! `hdl-cat-sync` exposes a sufficiently rich wire-plumbing API.

pub mod accumulator;
pub mod adder;
pub mod counter;
pub mod down_counter;
pub mod shift_reg;
pub mod toggle;

pub use accumulator::{accumulator, AccumulatorSync};
pub use adder::{half_adder, full_adder, HalfAdderArrow, FullAdderArrow};
pub use counter::{counter, CounterSync};
pub use down_counter::{down_counter, DownCounterSync};
pub use shift_reg::{shift_register_left, ShiftRegisterLeftSync};
pub use toggle::{toggle_ff, ToggleSync};
