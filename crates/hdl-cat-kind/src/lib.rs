//! The [`Hw`] trait: types representable in hardware.
//!
//! This crate is the analog of RHDL's `Digital` trait.  A type
//! implements [`Hw`] when it has a well-defined finite-width bit
//! encoding.  The trait provides:
//!
//! - a compile-time width (`const WIDTH: usize`)
//! - a runtime [`TypeDesc`] type-witness
//! - serialization to and from a [`BitSeq`] (the canonical bit buffer)
//!
//! Impls are provided for:
//!
//! - `bool`, `()` (unit)
//! - [`hdl_cat_bits::Bits<N>`] and [`hdl_cat_bits::SignedBits<N>`]
//! - 2-, 3-, and 4-element tuples whose elements are `Hw`
//! - fixed-size arrays `[T; N]` of `Hw` elements
//!
//! # Examples
//!
//! ```
//! use hdl_cat_kind::{Hw, TypeDesc};
//! use hdl_cat_bits::Bits;
//!
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! let v = Bits::<8>::try_new(0xab)?;
//! let bits = v.to_bits_seq();
//! assert_eq!(bits.len(), 8);
//! let back = Bits::<8>::from_bits_seq(&bits)?;
//! assert_eq!(back.to_u128(), 0xab);
//! assert!(matches!(Bits::<8>::type_desc(), TypeDesc::Bits { n: 8 }));
//! # Ok(()) }
//! ```

pub mod bit_seq;
pub mod ty_desc;
pub mod hw;
pub mod aggregate;

pub use bit_seq::BitSeq;
pub use hw::Hw;
pub use ty_desc::{TypeDesc, StructField, EnumVariant};
