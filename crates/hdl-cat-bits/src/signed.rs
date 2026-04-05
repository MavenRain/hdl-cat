//! Signed `N`-bit integer newtype (two's complement).
//!
//! See the [crate-level docs](crate) for an overview.

use core::ops::{Add, Mul, Neg, Sub};

use hdl_cat_error::{Error, Width};

use crate::bits::Bits;
use crate::mask::{mask, sign_bit, sign_extend};

/// A two's-complement signed `N`-bit integer, `0 <= N <= 128`.
///
/// Stored internally as an `i128` sign-extended from the `N`-bit
/// value.  Arithmetic wraps modulo `2^N`, matching fixed-width
/// hardware signed arithmetic.
///
/// # Examples
///
/// ```
/// use hdl_cat_bits::SignedBits;
///
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// let a = SignedBits::<4>::try_new(-8)?;     // min value in 4-bit signed
/// let b = SignedBits::<4>::try_new(-1)?;
/// assert_eq!((a + b).to_i128(), 7);          // -9 wraps to +7
/// # Ok(()) }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[must_use]
pub struct SignedBits<const N: usize>(i128);

impl<const N: usize> SignedBits<N> {
    /// Compile-time width bound — triggers a monomorphization error
    /// if `N > 128`.
    const WIDTH_VALID: () = assert!(N <= 128, "SignedBits<N> requires N <= 128");

    /// The width in bits.
    pub const WIDTH: usize = N;

    /// The zero value.
    pub const ZERO: Self = Self(0);

    /// Construct from an `i128`, wrapping on overflow.
    ///
    /// The value is masked to the low `N` bits and then sign-extended.
    pub fn new_wrapping(v: i128) -> Self {
        let () = Self::WIDTH_VALID;
        let u = u128::from_ne_bytes(v.to_ne_bytes());
        let masked = u & mask(N);
        Self(sign_extend(masked, N))
    }

    /// Construct from an `i128`, failing on overflow.
    ///
    /// # Errors
    ///
    /// Returns [`Error::WidthMismatch`] when `v` is outside
    /// `[min_value(), max_value()]`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), hdl_cat_error::Error> {
    /// use hdl_cat_bits::SignedBits;
    /// let ok = SignedBits::<8>::try_new(-128)?;
    /// assert_eq!(ok.to_i128(), -128);
    /// assert!(SignedBits::<8>::try_new(128).is_err());
    /// # Ok(()) }
    /// ```
    pub fn try_new(v: i128) -> Result<Self, Error> {
        let () = Self::WIDTH_VALID;
        let min = Self::min_i128();
        let max = Self::max_i128();
        (min <= v && v <= max)
            .then_some(Self(v))
            .ok_or(Error::WidthMismatch {
                expected: Width::new(
                    u32::try_from(N).map_err(|_| Error::Overflow {
                        width: Width::new(u32::MAX),
                    })?,
                ),
                actual: Width::new(128),
            })
    }

    /// The raw sign-extended value.
    #[must_use]
    pub const fn to_i128(self) -> i128 {
        self.0
    }

    /// The minimum representable value: `-2^(N-1)` for `N > 0`, else `0`.
    pub fn min_value() -> Self {
        let () = Self::WIDTH_VALID;
        Self(Self::min_i128())
    }

    /// The maximum representable value: `2^(N-1) - 1` for `N > 0`, else `0`.
    pub fn max_value() -> Self {
        let () = Self::WIDTH_VALID;
        Self(Self::max_i128())
    }

    const fn min_i128() -> i128 {
        if N == 0 {
            0
        } else {
            let sb_i = i128::from_ne_bytes(sign_bit(N).to_ne_bytes());
            sb_i.wrapping_neg()
        }
    }

    const fn max_i128() -> i128 {
        if N == 0 {
            0
        } else {
            let sb_i = i128::from_ne_bytes(sign_bit(N).to_ne_bytes());
            sb_i.wrapping_sub(1)
        }
    }

    /// Reinterpret the underlying bits as an unsigned [`Bits<N>`].
    ///
    /// The two's-complement bit pattern is preserved: for instance,
    /// `SignedBits::<4>(-1).to_bits()` yields `Bits::<4>(0xf)`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), hdl_cat_error::Error> {
    /// use hdl_cat_bits::{Bits, SignedBits};
    /// let s = SignedBits::<4>::try_new(-1)?;
    /// assert_eq!(s.to_bits().to_u128(), 0xf);
    /// # Ok(()) }
    /// ```
    pub fn to_bits(self) -> Bits<N> {
        let u = u128::from_ne_bytes(self.0.to_ne_bytes());
        Bits::<N>::new_wrapping(u & mask(N))
    }

    /// Reinterpret a [`Bits<N>`] as `SignedBits<N>` (sign-extending).
    ///
    /// The bit pattern is preserved; the value is interpreted in
    /// two's complement.
    ///
    /// # Examples
    ///
    /// ```
    /// use hdl_cat_bits::{Bits, SignedBits};
    /// let u = Bits::<4>::new_wrapping(0xf);
    /// let s = SignedBits::<4>::from_bits(u);
    /// assert_eq!(s.to_i128(), -1);
    /// ```
    pub fn from_bits(b: Bits<N>) -> Self {
        let () = Self::WIDTH_VALID;
        Self(sign_extend(b.to_u128(), N))
    }
}

impl<const N: usize> Default for SignedBits<N> {
    fn default() -> Self {
        Self::ZERO
    }
}

impl<const N: usize> core::fmt::Debug for SignedBits<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SignedBits<{N}>({})", self.0)
    }
}

impl<const N: usize> core::fmt::Display for SignedBits<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<const N: usize> Add for SignedBits<N> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new_wrapping(self.0.wrapping_add(rhs.0))
    }
}

impl<const N: usize> Sub for SignedBits<N> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new_wrapping(self.0.wrapping_sub(rhs.0))
    }
}

impl<const N: usize> Mul for SignedBits<N> {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new_wrapping(self.0.wrapping_mul(rhs.0))
    }
}

impl<const N: usize> Neg for SignedBits<N> {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new_wrapping(self.0.wrapping_neg())
    }
}

#[cfg(test)]
mod tests {
    use super::SignedBits;
    use crate::bits::Bits;
    use proptest::prelude::*;

    #[test]
    fn zero_is_zero() {
        assert_eq!(SignedBits::<8>::ZERO.to_i128(), 0);
    }

    #[test]
    fn min_max_for_various_widths() {
        assert_eq!(SignedBits::<4>::min_value().to_i128(), -8);
        assert_eq!(SignedBits::<4>::max_value().to_i128(), 7);
        assert_eq!(SignedBits::<8>::min_value().to_i128(), -128);
        assert_eq!(SignedBits::<8>::max_value().to_i128(), 127);
        assert_eq!(SignedBits::<0>::min_value().to_i128(), 0);
        assert_eq!(SignedBits::<0>::max_value().to_i128(), 0);
    }

    #[test]
    fn try_new_rejects_overflow() {
        assert!(SignedBits::<4>::try_new(8).is_err());
        assert!(SignedBits::<4>::try_new(-9).is_err());
        assert!(SignedBits::<8>::try_new(128).is_err());
        assert!(SignedBits::<8>::try_new(-129).is_err());
    }

    #[test]
    fn try_new_accepts_boundaries() -> Result<(), hdl_cat_error::Error> {
        let lo = SignedBits::<4>::try_new(-8)?;
        let hi = SignedBits::<4>::try_new(7)?;
        assert_eq!(lo.to_i128(), -8);
        assert_eq!(hi.to_i128(), 7);
        Ok(())
    }

    #[test]
    fn to_bits_round_trips() -> Result<(), hdl_cat_error::Error> {
        let s = SignedBits::<4>::try_new(-1)?;
        let u = s.to_bits();
        assert_eq!(u.to_u128(), 0xf);
        let back = SignedBits::<4>::from_bits(u);
        assert_eq!(back.to_i128(), -1);
        Ok(())
    }

    #[test]
    fn from_bits_sign_extends() {
        assert_eq!(SignedBits::<4>::from_bits(Bits::<4>::new_wrapping(0x8)).to_i128(), -8);
        assert_eq!(SignedBits::<4>::from_bits(Bits::<4>::new_wrapping(0x7)).to_i128(), 7);
        assert_eq!(SignedBits::<4>::from_bits(Bits::<4>::new_wrapping(0x0)).to_i128(), 0);
    }

    #[test]
    fn neg_of_min_wraps_to_min() -> Result<(), hdl_cat_error::Error> {
        let min = SignedBits::<4>::try_new(-8)?;
        assert_eq!((-min).to_i128(), -8);  // -(-8) = 8 wraps back to -8 in 4 bits
        Ok(())
    }

    #[test]
    fn new_wrapping_masks_and_sign_extends() {
        assert_eq!(SignedBits::<4>::new_wrapping(0x7).to_i128(), 7);
        assert_eq!(SignedBits::<4>::new_wrapping(0x8).to_i128(), -8);
        assert_eq!(SignedBits::<4>::new_wrapping(0xff).to_i128(), -1);
        assert_eq!(SignedBits::<4>::new_wrapping(0x10).to_i128(), 0);
    }

    #[test]
    fn debug_shows_signed_value() -> Result<(), hdl_cat_error::Error> {
        let s = SignedBits::<8>::try_new(-42)?;
        assert_eq!(format!("{s:?}"), "SignedBits<8>(-42)");
        Ok(())
    }

    proptest! {
        #[test]
        fn add_wraps_in_nbits(a in -128i128..=127, b in -128i128..=127) {
            let x = SignedBits::<8>::new_wrapping(a);
            let y = SignedBits::<8>::new_wrapping(b);
            let sum = (a.wrapping_add(b)) & 0xff;
            let expected = if sum & 0x80 != 0 { sum | !0xffi128 } else { sum };
            prop_assert_eq!((x + y).to_i128(), expected);
        }

        #[test]
        fn sub_wraps_in_nbits(a in -128i128..=127, b in -128i128..=127) {
            let x = SignedBits::<8>::new_wrapping(a);
            let y = SignedBits::<8>::new_wrapping(b);
            let diff = (a.wrapping_sub(b)) & 0xff;
            let expected = if diff & 0x80 != 0 { diff | !0xffi128 } else { diff };
            prop_assert_eq!((x - y).to_i128(), expected);
        }

        #[test]
        fn to_bits_from_bits_round_trip(a in -8i128..=7) {
            let s = SignedBits::<4>::new_wrapping(a);
            let u = s.to_bits();
            let back = SignedBits::<4>::from_bits(u);
            prop_assert_eq!(s.to_i128(), back.to_i128());
        }

        #[test]
        fn try_new_succeeds_inside_range(a in -128i128..=127) {
            prop_assert!(SignedBits::<8>::try_new(a).is_ok());
        }
    }
}
