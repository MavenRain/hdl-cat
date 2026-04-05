//! The canonical bit buffer used by [`crate::Hw`].
//!
//! A [`BitSeq`] is an ordered, LSB-first sequence of booleans.  It
//! serves as the universal intermediate representation for
//! hardware values during simulation, serialization, and codegen.

/// An ordered, LSB-first sequence of bits.
///
/// Newtype over `Vec<bool>` so that "a sequence of hardware bits" is
/// a distinct type from "a general bool vector."
///
/// # Examples
///
/// ```
/// use hdl_cat_kind::BitSeq;
///
/// let s = BitSeq::from_iter([true, false, true, false]);
/// assert_eq!(s.len(), 4);
/// assert!(s.bit(0));
/// assert!(!s.bit(1));
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct BitSeq(Vec<bool>);

impl BitSeq {
    /// Construct an empty sequence.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Construct from a preallocated `Vec<bool>`.
    pub fn from_vec(v: Vec<bool>) -> Self {
        Self(v)
    }

    /// The number of bits in the sequence.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the sequence is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return the bit at index `i`, or `false` if out of bounds.
    #[must_use]
    pub fn bit(&self, i: usize) -> bool {
        self.0.get(i).copied().unwrap_or(false)
    }

    /// Access the underlying slice.
    #[must_use]
    pub fn as_slice(&self) -> &[bool] {
        &self.0
    }

    /// Consume `self` and produce its underlying `Vec<bool>`.
    #[must_use]
    pub fn into_vec(self) -> Vec<bool> {
        self.0
    }

    /// Append the bits of `other` to `self`, in place.
    ///
    /// Consumes both and returns the concatenation.
    pub fn concat(self, other: Self) -> Self {
        Self(self.0.into_iter().chain(other.0).collect())
    }

    /// Split into a prefix of length `at` and the remaining suffix.
    ///
    /// If `at > self.len()`, the prefix is the whole sequence and
    /// the suffix is empty.
    pub fn split_at(self, at: usize) -> (Self, Self) {
        let v = self.0;
        let take = at.min(v.len());
        let tail = v[take..].to_vec();
        let head = v.into_iter().take(take).collect();
        (Self(head), Self(tail))
    }
}

impl Default for BitSeq {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<bool> for BitSeq {
    fn from_iter<I: IntoIterator<Item = bool>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl core::fmt::Debug for BitSeq {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("BitSeq[")?;
        self.0
            .iter()
            .try_fold(true, |first, b| {
                if !first {
                    f.write_str(", ")?;
                }
                let s = if *b { "1" } else { "0" };
                f.write_str(s)?;
                Ok(false)
            })?;
        f.write_str("]")
    }
}

#[cfg(test)]
mod tests {
    use super::BitSeq;

    #[test]
    fn empty_sequence_has_zero_length() {
        assert!(BitSeq::new().is_empty());
        assert_eq!(BitSeq::new().len(), 0);
    }

    #[test]
    fn from_iter_preserves_order() {
        let s = BitSeq::from_iter([true, false, true]);
        assert_eq!(s.len(), 3);
        assert!(s.bit(0));
        assert!(!s.bit(1));
        assert!(s.bit(2));
    }

    #[test]
    fn bit_out_of_bounds_is_false() {
        let s = BitSeq::from_iter([true]);
        assert!(!s.bit(5));
    }

    #[test]
    fn concat_joins_sequences() {
        let a = BitSeq::from_iter([true, false]);
        let b = BitSeq::from_iter([false, true]);
        let c = a.concat(b);
        assert_eq!(c.len(), 4);
        assert_eq!(c.as_slice(), &[true, false, false, true]);
    }

    #[test]
    fn split_at_yields_matching_lengths() {
        let s = BitSeq::from_iter([true, false, true, false]);
        let (head, tail) = s.split_at(2);
        assert_eq!(head.len(), 2);
        assert_eq!(tail.len(), 2);
        assert_eq!(head.as_slice(), &[true, false]);
        assert_eq!(tail.as_slice(), &[true, false]);
    }

    #[test]
    fn split_at_clamps_to_length() {
        let s = BitSeq::from_iter([true, false]);
        let (head, tail) = s.split_at(10);
        assert_eq!(head.len(), 2);
        assert_eq!(tail.len(), 0);
    }

    #[test]
    fn debug_shows_bits() {
        let s = BitSeq::from_iter([true, false, true]);
        assert_eq!(format!("{s:?}"), "BitSeq[1, 0, 1]");
    }
}
