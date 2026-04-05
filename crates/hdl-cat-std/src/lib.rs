//! Standard components built from `hdl-cat-circuit` primitives
//! and `hdl-cat-sync` state machinery.
//!
//! # Component catalogue
//!
//! | Component | Shape | Summary |
//! |---|---|---|
//! | [`half_adder`] | `CircuitArrow<bool⊗bool, bool⊗bool>` | `(a, b) → (sum, carry)` |
//! | [`full_adder`] | `CircuitArrow<(bool⊗bool)⊗bool, bool⊗bool>` | `(a, b, cin) → (sum, cout)` |
//! | [`counter()`] | `Sync<Obj<Bits<N>>, CircuitUnit, Obj<Bits<N>>>` | `state + 1` each cycle |
//! | [`down_counter()`] | `Sync<Obj<Bits<N>>, CircuitUnit, Obj<Bits<N>>>` | `state - 1` each cycle |
//! | [`accumulator()`] | `Sync<Obj<Bits<N>>, Obj<Bits<N>>, Obj<Bits<N>>>` | `state + input` each cycle |
//! | [`toggle_ff`] | `Sync<Obj<bool>, CircuitUnit, Obj<bool>>` | flips the state each cycle |
//! | [`shift_register_left()`] | `Sync<Obj<Bits<N>>, Obj<bool>, Obj<Bits<N>>>` | shift left, new bit at bit 0 |
//!
//! # Examples
//!
//! ## Accumulating a stream of values
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_std::accumulator;
//!
//! let acc = accumulator::<8>()?;
//! assert_eq!(acc.state_wire_count(), 1);
//! assert_eq!(acc.input_wires().len(), 2);  // state + input
//! # Ok(()) }
//! ```
//!
//! ## Building a half-adder from primitive gates
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_std::half_adder;
//!
//! let ha = half_adder()?;
//! assert_eq!(ha.inputs().len(), 2);   // a, b
//! assert_eq!(ha.outputs().len(), 2);  // sum, carry
//! assert_eq!(ha.graph().instructions().len(), 2);  // XOR + AND
//! # Ok(()) }
//! ```

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
