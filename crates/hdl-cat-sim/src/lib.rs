//! Cycle-accurate simulator for [`hdl_cat_sync::Sync`] machines.
//!
//! The simulator interprets a `Sync` machine's IR graph on each
//! clock cycle, threading state from one cycle to the next.  The
//! public entry point is [`Testbench`]; under the hood it builds
//! an `Io<Error, Vec<TimedSample<BitSeq>>>` and leaves the user
//! to call `.run()` at the simulation boundary.
//!
//! # Public interface
//!
//! - [`Testbench::new`] — wrap a `Sync<S, I, O>` for driving
//! - [`Testbench::run`] — produce an `Io` that simulates N cycles
//! - [`TimedSample`] — per-cycle output value tagged with its cycle index
//! - [`trace_to_string`] — VCD trace emission
//! - [`interp::interpret`] — raw IR graph interpreter (used by both)
//!
//! # Example — stateless pipeline
//!
//! Simulate a combinational 4-bit inverter for three cycles:
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_bits::Bits;
//! use hdl_cat_circuit::{gates, CircuitUnit, Obj};
//! use hdl_cat_kind::Hw;
//! use hdl_cat_sim::Testbench;
//! use hdl_cat_sync::Sync;
//!
//! let inv = gates::not_bits::<4>()?;
//! let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
//!
//! let inputs = vec![
//!     Bits::<4>::try_new(0x0)?.to_bits_seq(),
//!     Bits::<4>::try_new(0xf)?.to_bits_seq(),
//!     Bits::<4>::try_new(0xa)?.to_bits_seq(),
//! ];
//! let samples = Testbench::new(m).run(inputs).run()?;
//! assert_eq!(samples.len(), 3);
//! assert_eq!(Bits::<4>::from_bits_seq(samples[0].value())?.to_u128(), 0xf);
//! assert_eq!(Bits::<4>::from_bits_seq(samples[1].value())?.to_u128(), 0x0);
//! assert_eq!(Bits::<4>::from_bits_seq(samples[2].value())?.to_u128(), 0x5);
//! # Ok(()) }
//! ```
//!
//! # Stateful simulation
//!
//! The simulator threads `next_state` wires back to `state` wires
//! between cycles automatically, using the `Sync`'s
//! `initial_state` for cycle 0.  See the `hdl-cat-std` and
//! umbrella `hdl-cat` crates for worked stateful examples
//! (counters, accumulators, shift registers).

pub mod interp;
pub mod sample;
pub mod testbench;
pub mod vcd;

pub use sample::TimedSample;
pub use testbench::Testbench;
pub use vcd::trace_to_string;
