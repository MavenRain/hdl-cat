//! Circuits as a symmetric monoidal category.
//!
//! [`Circuit`] is a marker type implementing
//! `comp_cat_rs::foundation::Category`,
//! `MonoidalCategory`, `Braided`, and `Symmetric`.  Its objects
//! are *hardware wire bundles* and its morphisms are compiled
//! [`CircuitArrow`]s — typed wrappers around an [`hdl_cat_ir`]
//! graph.
//!
//! # Objects
//!
//! Three object constructors:
//!
//! - [`Obj<T>`] — a bundle carrying a single [`hdl_cat_kind::Hw`]
//!   value.
//! - [`CircuitTensor<A, B>`] — the tensor product (parallel
//!   composition) of two object bundles.
//! - [`CircuitUnit`] — the unit object (the empty bundle).
//!
//! Each implements [`Object`], which exposes a compile-time
//! width.  Each implements `From<Self> for Circuit` so it can
//! witness the `Into<Circuit>` trait bound used throughout
//! comp-cat-rs's `Category` / `MonoidalCategory` traits.
//!
//! # Morphisms
//!
//! A [`CircuitArrow<A, B>`] is a typed wrapper around an IR
//! graph and its primary data-flow path.  Sequential composition
//! (`Category::comp`) splices one arrow's output into another's
//! input; parallel composition (`MonoidalCategory::tensor_map`)
//! places two arrows side by side.
//!
//! # Coherence
//!
//! The associator, unitors, and braiding are implemented as
//! pure wire-permutation identity arrows — no gate emission.
//! The Lean 4 spec's coherence theorems certify these rewrites
//! preserve semantics.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_circuit::{Circuit, gates};
//! use comp_cat_rs::foundation::category::Category;
//!
//! // Two inverters in series on a 4-bit bus.
//! let inv_a = gates::not_bits::<4>()?;
//! let inv_b = gates::not_bits::<4>()?;
//! let roundtrip = Circuit::comp(inv_a, inv_b);
//! assert_eq!(roundtrip.graph().instructions().len(), 2);
//! # Ok(()) }
//! ```

pub mod arrow;
pub mod category_impl;
pub mod coherence;
pub mod gates;
pub mod object;

pub use arrow::CircuitArrow;
pub use category_impl::Circuit;
pub use coherence::{wired_associator, wired_braid, wired_left_unitor, wired_right_unitor};
pub use object::{CircuitTensor, CircuitUnit, Obj, Object};
