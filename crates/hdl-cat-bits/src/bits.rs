//! Unsigned `N`-bit integer newtype.
//!
//! See the [crate-level docs](crate) for an overview.

use core::ops::{Add, BitAnd, BitOr, BitXor, Mul, Not, Shl, Shr, Sub};

use hdl_cat_error::{Error, Width};

use crate::mask::mask;

/// An unsigned `N`-bit integer, `0 <= N <= 128`.
///
/// Stored internally as a `u128` masked to the low `N` bits.
/// The type-level `N` witnesses the bit width; no runtime width is
/// carried.
///
/// Arithmetic operations wrap modulo `2^N`, matching the semantics
/// of fixed-width hardware adders.
///
/// # Examples
///
/// ```
/// use hdl_cat_bits::Bits;
///
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// let a = Bits::<4>::try_new(0xF)?;
/// let b = Bits::<4>::try_new(0x1)?;
/// assert_eq!((a + b).to_u128(), 0x0);  // 16 wraps to 0 in 4 bits
/// # Ok(()) }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[must_use]
pub struct Bits<const N: usize>(u128);

impl<const N: usize> Bits<N> {
    /// Compile-time width bound — triggers a monomorphization error
    /// if `N > 128`.
    const WIDTH_VALID: () = assert!(N <= 128, "Bits<N> requires N <= 128");

    /// The width in bits.
    pub const WIDTH: usize = N;

    /// The zero value: all bits clear.
    pub const ZERO: Self = Self(0);

    /// Construct a `Bits<N>` from a raw `u128`, masking overflow.
    ///
    /// The high `128 - N` bits of `v` are discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use hdl_cat_bits::Bits;
    /// let b = Bits::<4>::new_wrapping(0xff);  // 0xff & 0xf == 0xf
    /// assert_eq!(b.to_u128(), 0xf);
    /// ```
    pub const fn new_wrapping(v: u128) -> Self {
        let () = Self::WIDTH_VALID;
        Self(v & mask(N))
    }

    /// Construct a `Bits<N>` from a raw `u128`, failing on overflow.
    ///
    /// # Errors
    ///
    /// Returns [`Error::WidthMismatch`] when `v` has any bit set
    /// above bit `N-1`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), hdl_cat_error::Error> {
    /// use hdl_cat_bits::Bits;
    /// let ok = Bits::<8>::try_new(200)?;
    /// assert_eq!(ok.to_u128(), 200);
    /// assert!(Bits::<8>::try_new(300).is_err());
    /// # Ok(()) }
    /// ```
    pub fn try_new(v: u128) -> Result<Self, Error> {
        let () = Self::WIDTH_VALID;
        let m = mask(N);
        let fits = v & !m == 0;
        fits.then_some(Self(v))
            .ok_or(Error::WidthMismatch {
                expected: Width::new(
                    u32::try_from(N).map_err(|_| Error::Overflow {
                        width: Width::new(u32::MAX),
                    })?,
                ),
                actual: Width::new(128),
            })
    }

    /// The raw underlying value, with high bits zeroed.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }

    /// The maximum representable value: all `N` bits set.
    pub const fn max_value() -> Self {
        let () = Self::WIDTH_VALID;
        Self(mask(N))
    }

    /// Explode the value into a `[bool; N]`, LSB-first.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), hdl_cat_error::Error> {
    /// use hdl_cat_bits::Bits;
    /// let b = Bits::<4>::try_new(0b1010)?;
    /// assert_eq!(b.as_bits(), [false, true, false, true]);
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn as_bits(self) -> [bool; N] {
        let () = Self::WIDTH_VALID;
        core::array::from_fn(|i| (self.0 >> i) & 1 == 1)
    }

    /// Rebuild a `Bits<N>` from a `[bool; N]`, LSB-first.
    ///
    /// # Examples
    ///
    /// ```
    /// use hdl_cat_bits::Bits;
    /// let b = Bits::<4>::from_bool_array([true, false, true, false]);
    /// assert_eq!(b.to_u128(), 0b0101);
    /// ```
    pub fn from_bool_array(arr: [bool; N]) -> Self {
        let () = Self::WIDTH_VALID;
        let v = arr
            .into_iter()
            .enumerate()
            .fold(0u128, |acc, (i, b)| acc | (u128::from(b) << i));
        Self(v)
    }
}

impl<const N: usize> Default for Bits<N> {
    fn default() -> Self {
        Self::ZERO
    }
}

impl<const N: usize> core::fmt::Debug for Bits<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Bits<{N}>({:#x})", self.0)
    }
}

impl<const N: usize> core::fmt::Display for Bits<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<const N: usize> Add for Bits<N> {
    type Output = Self;
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0) & mask(N))
    }
}

impl<const N: usize> Sub for Bits<N> {
    type Output = Self;
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0) & mask(N))
    }
}

impl<const N: usize> Mul for Bits<N> {
    type Output = Self;
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn mul(self, rhs: Self) -> Self {
        Self(self.0.wrapping_mul(rhs.0) & mask(N))
    }
}

impl<const N: usize> BitAnd for Bits<N> {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl<const N: usize> BitOr for Bits<N> {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl<const N: usize> BitXor for Bits<N> {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl<const N: usize> Not for Bits<N> {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0 & mask(N))
    }
}

impl<const N: usize> Shl<usize> for Bits<N> {
    type Output = Self;
    fn shl(self, rhs: usize) -> Self {
        match rhs {
            128.. => Self::ZERO,
            r => Self((self.0 << r) & mask(N)),
        }
    }
}

impl<const N: usize> Shr<usize> for Bits<N> {
    type Output = Self;
    fn shr(self, rhs: usize) -> Self {
        match rhs {
            128.. => Self::ZERO,
            r => Self((self.0 >> r) & mask(N)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Bits;
    use proptest::prelude::*;

    fn any_u128_masked<const N: usize>() -> impl Strategy<Value = u128> {
        use crate::mask::mask;
        (0..=u128::MAX).prop_map(move |v| v & mask(N))
    }

    #[test]
    fn zero_is_zero() {
        assert_eq!(Bits::<8>::ZERO.to_u128(), 0);
    }

    #[test]
    fn max_value_is_all_ones() {
        assert_eq!(Bits::<8>::max_value().to_u128(), 0xff);
        assert_eq!(Bits::<4>::max_value().to_u128(), 0xf);
        assert_eq!(Bits::<1>::max_value().to_u128(), 0x1);
        assert_eq!(Bits::<0>::max_value().to_u128(), 0);
    }

    #[test]
    fn try_new_rejects_overflow() {
        assert!(Bits::<4>::try_new(16).is_err());
        assert!(Bits::<4>::try_new(0xffff).is_err());
    }

    #[test]
    fn try_new_accepts_in_range() -> Result<(), hdl_cat_error::Error> {
        let b = Bits::<4>::try_new(15)?;
        assert_eq!(b.to_u128(), 15);
        Ok(())
    }

    #[test]
    fn new_wrapping_masks_high_bits() {
        assert_eq!(Bits::<4>::new_wrapping(0xff).to_u128(), 0xf);
        assert_eq!(Bits::<8>::new_wrapping(0x1_0000).to_u128(), 0);
    }

    #[test]
    fn display_shows_decimal() -> Result<(), hdl_cat_error::Error> {
        let b = Bits::<8>::try_new(42)?;
        assert_eq!(format!("{b}"), "42");
        Ok(())
    }

    #[test]
    fn debug_shows_hex_with_width() -> Result<(), hdl_cat_error::Error> {
        let b = Bits::<8>::try_new(0xab)?;
        assert_eq!(format!("{b:?}"), "Bits<8>(0xab)");
        Ok(())
    }

    proptest! {
        #[test]
        fn add_wraps_modulo_2_pow_n(a in any_u128_masked::<8>(), b in any_u128_masked::<8>()) {
            let x = Bits::<8>::new_wrapping(a);
            let y = Bits::<8>::new_wrapping(b);
            prop_assert_eq!((x + y).to_u128(), (a.wrapping_add(b)) & 0xff);
        }

        #[test]
        fn sub_wraps_modulo_2_pow_n(a in any_u128_masked::<8>(), b in any_u128_masked::<8>()) {
            let x = Bits::<8>::new_wrapping(a);
            let y = Bits::<8>::new_wrapping(b);
            prop_assert_eq!((x - y).to_u128(), (a.wrapping_sub(b)) & 0xff);
        }

        #[test]
        fn mul_wraps_modulo_2_pow_n(a in any_u128_masked::<8>(), b in any_u128_masked::<8>()) {
            let x = Bits::<8>::new_wrapping(a);
            let y = Bits::<8>::new_wrapping(b);
            prop_assert_eq!((x * y).to_u128(), (a.wrapping_mul(b)) & 0xff);
        }

        #[test]
        fn not_toggles_all_bits(a in any_u128_masked::<8>()) {
            let x = Bits::<8>::new_wrapping(a);
            prop_assert_eq!((!x).to_u128(), (!a) & 0xff);
        }

        #[test]
        fn bits_array_round_trips(a in any_u128_masked::<12>()) {
            let x = Bits::<12>::new_wrapping(a);
            let arr = x.as_bits();
            let y = Bits::<12>::from_bool_array(arr);
            prop_assert_eq!(x.to_u128(), y.to_u128());
        }

        #[test]
        fn shl_by_128_or_more_is_zero(a in any_u128_masked::<16>(), r in 128usize..=256) {
            let x = Bits::<16>::new_wrapping(a);
            prop_assert_eq!((x << r).to_u128(), 0);
        }

        #[test]
        fn shr_by_128_or_more_is_zero(a in any_u128_masked::<16>(), r in 128usize..=256) {
            let x = Bits::<16>::new_wrapping(a);
            prop_assert_eq!((x >> r).to_u128(), 0);
        }

        #[test]
        fn shl_shr_are_inverses_when_no_overflow(a in 0u128..=0xff, r in 0usize..=7) {
            let x = Bits::<8>::new_wrapping(a);
            let shifted = x << r;
            let back = shifted >> r;
            // Bits that would have overflowed are lost — only low (8-r) bits preserved
            let preserved_mask = crate::mask::mask(8 - r);
            prop_assert_eq!(back.to_u128(), a & preserved_mask);
        }

        #[test]
        fn and_is_idempotent(a in any_u128_masked::<16>()) {
            let x = Bits::<16>::new_wrapping(a);
            prop_assert_eq!((x & x).to_u128(), a);
        }

        #[test]
        fn xor_with_self_is_zero(a in any_u128_masked::<16>()) {
            let x = Bits::<16>::new_wrapping(a);
            prop_assert_eq!((x ^ x).to_u128(), 0);
        }
    }
}
