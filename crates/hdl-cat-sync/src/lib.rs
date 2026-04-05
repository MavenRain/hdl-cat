//! Synchronous (Mealy) machines as stateful circuit arrows.
//!
//! A [`Sync<S, I, O>`] represents a Mealy machine:
//!
//! - `S`: state object (carried across cycles)
//! - `I`: input object (one sample per cycle)
//! - `O`: output object (one sample per cycle)
//!
//! Internally it wraps a combinational
//! [`hdl_cat_circuit::CircuitArrow`]-shaped IR whose input list is
//! `state_wires ++ input_wires` and whose output list is
//! `next_state_wires ++ output_wires`, together with an
//! initial-state [`BitSeq`].  On each simulated cycle the IR is
//! interpreted with the current state and input wires, producing
//! the next state and current output.
//!
//! No closures are stored; everything is IR.  Actual stepping
//! (interpreting the IR on concrete bits) is provided by the
//! `hdl-cat-sim` crate.
//!
//! # Constructors
//!
//! - [`Sync::lift_comb`] — embed a stateless combinational
//!   arrow as a `Sync<CircuitUnit, I, O>`.
//! - [`Sync::from_arrow`] — assemble a `Sync` directly from an
//!   arrow whose inputs are `(state ⊗ input)` and outputs are
//!   `(next_state ⊗ output)`.
//! - [`machine::from_raw`] — hand-assemble from raw IR parts
//!   (used by hand-built state machines like the counter).
//!
//! # Composition
//!
//! - [`compose_sync<S1, S2, I, M, O>`] — sequential composition;
//!   state becomes the product `(S1, S2)` and `f`'s data output
//!   wires are routed into `g`'s data input wires at the IR
//!   level.
//! - [`par_sync<S1, S2, I1, I2, O1, O2>`] — parallel
//!   composition; state becomes `(S1, S2)` and the two sub-
//!   machines operate on independent inputs/outputs.
//! - [`feedback_sync<S, I, O>`] — close a one-cycle feedback
//!   loop; promotes the data output to an additional state wire.
//!
//! # Example — lifting and composing stateless machines
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_bits::Bits;
//! use hdl_cat_circuit::{gates, CircuitUnit, Obj};
//! use hdl_cat_sync::{compose_sync, Sync};
//!
//! // Two stateless inverters composed sequentially = identity.
//! let inv_a = gates::not_bits::<4>()?;
//! let inv_b = gates::not_bits::<4>()?;
//! let ma: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_a);
//! let mb: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_b);
//! let composed = compose_sync(ma, mb);
//!
//! assert_eq!(composed.state_wire_count(), 0);
//! // Two NOT instructions, 1 input wire (after state-thread merge), 1 output wire.
//! assert_eq!(composed.graph().instructions().len(), 2);
//! assert_eq!(composed.input_wires().len(), 1);
//! assert_eq!(composed.output_wires().len(), 1);
//! # Ok(()) }
//! ```

pub mod compose;
pub mod machine;

pub use compose::{compose_sync, feedback_sync, par_sync};
pub use machine::Sync;

pub use hdl_cat_kind::BitSeq;
