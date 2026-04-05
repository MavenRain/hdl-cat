//! The [`Hw`] trait: types with a hardware encoding.

use hdl_cat_error::{Error, Width};
use hdl_cat_bits::{Bits, SignedBits};

use crate::bit_seq::BitSeq;
use crate::ty_desc::TypeDesc;

/// A type representable as a finite-width sequence of hardware bits.
///
/// Every `Hw` implementation exposes:
///
/// - `WIDTH`: compile-time bit count
/// - [`Hw::type_desc`]: runtime [`TypeDesc`] describing the structure
/// - [`Hw::to_bits_seq`]: serialize to a [`BitSeq`] (LSB-first)
/// - [`Hw::from_bits_seq`]: deserialize, failing on width mismatch
///
/// # Laws
///
/// - `Self::WIDTH == Self::type_desc().width()`
/// - `v.to_bits_seq().len() == Self::WIDTH`
/// - `Self::from_bits_seq(&v.to_bits_seq()) == Ok(v)`  (round-trip)
pub trait Hw: Sized {
    /// The bit width of this type.
    const WIDTH: usize;

    /// Runtime description of the type's structure.
    #[must_use]
    fn type_desc() -> TypeDesc;

    /// Serialize the value to a [`BitSeq`] (LSB-first).
    fn to_bits_seq(&self) -> BitSeq;

    /// Deserialize a value from a [`BitSeq`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::WidthMismatch`] if `bits.len() != Self::WIDTH`.
    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error>;
}

/// Produce a `WidthMismatch` error from two `usize` widths.
///
/// Used by `from_bits_seq` implementations that need to report a
/// length mismatch without unwrapping the usize-to-u32 conversion.
pub(crate) fn width_mismatch(expected: usize, actual: usize) -> Error {
    let e = u32::try_from(expected).unwrap_or(u32::MAX);
    let a = u32::try_from(actual).unwrap_or(u32::MAX);
    Error::WidthMismatch {
        expected: Width::new(e),
        actual: Width::new(a),
    }
}

impl Hw for bool {
    const WIDTH: usize = 1;

    fn type_desc() -> TypeDesc {
        TypeDesc::Bool
    }

    fn to_bits_seq(&self) -> BitSeq {
        BitSeq::from_iter([*self])
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        (bits.len() == 1)
            .then(|| bits.bit(0))
            .ok_or_else(|| width_mismatch(1, bits.len()))
    }
}

impl Hw for () {
    const WIDTH: usize = 0;

    fn type_desc() -> TypeDesc {
        TypeDesc::Tuple(Vec::new())
    }

    fn to_bits_seq(&self) -> BitSeq {
        BitSeq::new()
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        bits.is_empty()
            .then_some(())
            .ok_or_else(|| width_mismatch(0, bits.len()))
    }
}

impl<const N: usize> Hw for Bits<N> {
    const WIDTH: usize = N;

    fn type_desc() -> TypeDesc {
        TypeDesc::Bits { n: N }
    }

    fn to_bits_seq(&self) -> BitSeq {
        BitSeq::from_iter(self.as_bits())
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        (bits.len() == N)
            .then_some(())
            .ok_or_else(|| width_mismatch(N, bits.len()))
            .map(|()| {
                let arr: [bool; N] = core::array::from_fn(|i| bits.bit(i));
                Self::from_bool_array(arr)
            })
    }
}

impl<const N: usize> Hw for SignedBits<N> {
    const WIDTH: usize = N;

    fn type_desc() -> TypeDesc {
        TypeDesc::Signed { n: N }
    }

    fn to_bits_seq(&self) -> BitSeq {
        BitSeq::from_iter(self.to_bits().as_bits())
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        let unsigned = Bits::<N>::from_bits_seq(bits)?;
        Ok(Self::from_bits(unsigned))
    }
}

#[cfg(test)]
mod tests {
    use super::Hw;
    use crate::ty_desc::TypeDesc;
    use hdl_cat_bits::{Bits, SignedBits};

    #[test]
    fn bool_round_trips_through_bits_seq() -> Result<(), hdl_cat_error::Error> {
        let v = true;
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 1);
        let back = bool::from_bits_seq(&seq)?;
        assert_eq!(back, v);
        Ok(())
    }

    #[test]
    fn unit_round_trips() -> Result<(), hdl_cat_error::Error> {
        let seq = ().to_bits_seq();
        assert_eq!(seq.len(), 0);
        <()>::from_bits_seq(&seq)?;
        Ok(())
    }

    #[test]
    fn bits_round_trip() -> Result<(), hdl_cat_error::Error> {
        let v = Bits::<8>::try_new(0xab)?;
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 8);
        let back = Bits::<8>::from_bits_seq(&seq)?;
        assert_eq!(back.to_u128(), 0xab);
        Ok(())
    }

    #[test]
    fn signed_bits_round_trip() -> Result<(), hdl_cat_error::Error> {
        let v = SignedBits::<8>::try_new(-42)?;
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 8);
        let back = SignedBits::<8>::from_bits_seq(&seq)?;
        assert_eq!(back.to_i128(), -42);
        Ok(())
    }

    #[test]
    fn bool_from_wrong_width_fails() {
        let too_long = bool::to_bits_seq(&true).concat(bool::to_bits_seq(&false));
        assert!(bool::from_bits_seq(&too_long).is_err());
    }

    #[test]
    fn bits_from_wrong_width_fails() {
        let seq = crate::bit_seq::BitSeq::from_iter([true, false, true]);
        assert!(Bits::<8>::from_bits_seq(&seq).is_err());
    }

    #[test]
    fn widths_match_type_desc() {
        assert_eq!(<bool as Hw>::WIDTH, TypeDesc::Bool.width());
        assert_eq!(<Bits<16> as Hw>::WIDTH, TypeDesc::Bits { n: 16 }.width());
        assert_eq!(<SignedBits<7> as Hw>::WIDTH, TypeDesc::Signed { n: 7 }.width());
        assert_eq!(<() as Hw>::WIDTH, 0);
    }
}
