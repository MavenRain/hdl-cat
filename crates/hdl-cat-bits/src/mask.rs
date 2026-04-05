//! Width-parameterized masking helpers.
//!
//! Shared between [`crate::bits`] and [`crate::signed`].  Every
//! constructor and arithmetic operation applies a mask derived from
//! the const-generic `N` parameter to uphold the "only the low `N`
//! bits are populated" invariant.

/// The `N`-bit mask as a `u128`.
///
/// - `mask(0)` is `0`
/// - `mask(1..=127)` is `(1 << N) - 1`
/// - `mask(128..)` is `u128::MAX`
///
/// Total over `usize`: no panics, no UB for any input.
///
/// # Examples
///
/// ```
/// use hdl_cat_bits::mask::mask;
/// assert_eq!(mask(0),  0);
/// assert_eq!(mask(1),  1);
/// assert_eq!(mask(8),  0xff);
/// assert_eq!(mask(64), 0xffff_ffff_ffff_ffff);
/// assert_eq!(mask(128), u128::MAX);
/// assert_eq!(mask(200), u128::MAX);
/// ```
#[must_use]
pub const fn mask(n: usize) -> u128 {
    match n {
        0 => 0,
        1..=127 => (1u128 << n) - 1,
        128.. => u128::MAX,
    }
}

/// The `N`-bit sign bit mask: only bit `N-1` set.
///
/// - `sign_bit(0)` is `0` (no sign bit)
/// - `sign_bit(1..=128)` is `1 << (N-1)`
/// - `sign_bit(129..)` is `1 << 127`
#[must_use]
pub const fn sign_bit(n: usize) -> u128 {
    match n {
        0 => 0,
        1..=128 => 1u128 << (n - 1),
        129.. => 1u128 << 127,
    }
}

/// Sign-extend the low `n` bits of `low` into a full `i128`.
///
/// Given a `u128` whose low `n` bits encode a two's-complement
/// `n`-bit signed value (and whose high bits are zero), this
/// returns the signed value as an `i128`, preserving the numeric
/// meaning.
///
/// # Examples
///
/// ```
/// use hdl_cat_bits::mask::sign_extend;
/// assert_eq!(sign_extend(0b0111, 4), 7);
/// assert_eq!(sign_extend(0b1000, 4), -8);
/// assert_eq!(sign_extend(0b1111, 4), -1);
/// assert_eq!(sign_extend(0, 0), 0);
/// ```
#[must_use]
pub const fn sign_extend(low: u128, n: usize) -> i128 {
    let sign = sign_bit(n);
    let is_negative = sign != 0 && (low & sign) != 0;
    let lifted = if is_negative { low | !mask(n) } else { low & mask(n) };
    i128::from_ne_bytes(lifted.to_ne_bytes())
}
