//! Bit-precise integer newtypes.
//!
//! This crate provides two const-generic hardware integer types:
//!
//! - [`Bits<N>`] — unsigned `N`-bit integer, `0 <= N <= 128`
//! - [`SignedBits<N>`] — two's-complement signed `N`-bit integer,
//!   `0 <= N <= 128`
//!
//! Both types are newtype wrappers, enforce the `N`-bit invariant on
//! construction, and implement the standard `core::ops` arithmetic
//! traits with **wrap-around** semantics — the same semantics FPGAs
//! and ASICs use when an operation exceeds the declared width.
//!
//! # Width bound
//!
//! `N <= 128` is checked at monomorphization time via an associated
//! const assertion.  Attempting to name e.g. `Bits<256>` is a
//! compile-time (post-monomorphization) error.
//!
//! # Conversions
//!
//! - `Bits::<N>::try_new(v)` — fallible (`Err` if `v` overflows `N`)
//! - `Bits::<N>::new_wrapping(v)` — masks `v` to the low `N` bits
//! - `bits.to_u128()` — extracts the raw value
//! - `bits.as_bits()` — exploded `[bool; N]` array (LSB-first)
//! - `Bits::from_bool_array(arr)` — rebuilds from a `[bool; N]`
//!
//! # Examples
//!
//! ```
//! use hdl_cat_bits::Bits;
//!
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! let a = Bits::<8>::try_new(200)?;
//! let b = Bits::<8>::try_new(100)?;
//! let sum = a + b;                 // 300 wraps to 44 in 8 bits
//! assert_eq!(sum.to_u128(), 44);
//! # Ok(()) }
//! ```

pub mod mask;
pub mod bits;
pub mod signed;

pub use bits::Bits;
pub use signed::SignedBits;
