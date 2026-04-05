//! Cycle-accurate simulator for [`hdl_cat_sync::Sync`] machines.
//!
//! The simulator interprets a `Sync` machine's IR graph on each
//! clock cycle, threading state from one cycle to the next.  The
//! public entry point is [`Testbench`]; under the hood it builds
//! an `Io<Error, Vec<TimedSample<BitSeq>>>` and leaves the user
//! to call `.run()` at the simulation boundary.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_bits::Bits;
//! use hdl_cat_circuit::{gates, CircuitUnit, Obj};
//! use hdl_cat_kind::{BitSeq, Hw};
//! use hdl_cat_sim::Testbench;
//! use hdl_cat_sync::Sync;
//!
//! // Stateless 4-bit inverter.
//! let inv = gates::not_bits::<4>()?;
//! let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
//!
//! // Drive with three inputs.
//! let inputs = vec![
//!     Bits::<4>::try_new(0x0)?.to_bits_seq(),
//!     Bits::<4>::try_new(0xf)?.to_bits_seq(),
//!     Bits::<4>::try_new(0xa)?.to_bits_seq(),
//! ];
//! let samples = Testbench::new(m).run(inputs).run()?;
//! assert_eq!(samples.len(), 3);
//! // 0x0 inverted (mod 16) = 0xf
//! let first = Bits::<4>::from_bits_seq(samples[0].value())?;
//! assert_eq!(first.to_u128(), 0xf);
//! # Ok(()) }
//! ```

pub mod interp;
pub mod sample;
pub mod testbench;
pub mod vcd;

pub use sample::TimedSample;
pub use testbench::Testbench;
pub use vcd::trace_to_string;
