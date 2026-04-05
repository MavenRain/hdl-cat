//! Synchronous (Mealy) machines as stateful circuit arrows.
//!
//! A [`Sync<S, I, O>`] represents a Mealy machine:
//!
//! - `S`: state object (carried across cycles)
//! - `I`: input object (one sample per cycle)
//! - `O`: output object (one sample per cycle)
//!
//! Internally it wraps a combinational [`hdl_cat_circuit::CircuitArrow`]
//! of type `(S ⊗ I) → (S ⊗ O)`, together with an initial-state
//! [`BitSeq`].  On each simulated cycle the arrow consumes the
//! current state plus the current input and yields the next
//! state plus the current output.
//!
//! No closures are stored; everything is IR.  Actual stepping
//! (interpreting the arrow on concrete bits) is provided by the
//! `hdl-cat-sim` crate.
//!
//! # Constructors
//!
//! - [`Sync::lift_comb`] — embed a stateless combinational
//!   arrow as a `Sync<CircuitUnit, I, O>`.
//! - [`Sync::from_parts`] — assemble a `Sync` directly from an
//!   arrow and an initial-state bit pattern.
//!
//! # Composition primitives
//!
//! - `Sync` composition operators that respect the Mealy
//!   product-of-states rule are intentionally minimal here;
//!   richer composition (sequential, parallel, feedback) is
//!   deferred to `hdl-cat-std`, where common wiring patterns
//!   over flat object layouts are packaged as library
//!   components.

pub mod compose;
pub mod machine;

pub use compose::{compose_sync, feedback_sync, par_sync};
pub use machine::Sync;

pub use hdl_cat_kind::BitSeq;
