//! Wire types and identifiers.

/// An identifier for a wire in an [`crate::HdlGraph`].
///
/// Newtype over `usize` so wire identifiers cannot be confused
/// with instruction indices or any other position-style value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WireId(usize);

impl WireId {
    /// Construct a `WireId` from a raw index.
    #[must_use]
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    /// The underlying index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

impl core::fmt::Display for WireId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "w{}", self.0)
    }
}

/// The type carried on a hardware wire.
///
/// Narrower than [`hdl_cat_kind::TypeDesc`]: wires in the IR
/// carry only primitive bit buses, not aggregates.  Aggregates
/// in the surface language lower to concat / slice instructions
/// over flat wires.
///
/// The [`Array`](Self::Array) variant represents a fixed-depth
/// collection of identically-typed elements (e.g. a delay line).
/// A single [`WireId`] suffices to name the entire array, keeping
/// the graph compact regardless of depth.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WireTy {
    /// A single bit.
    Bit,
    /// An unsigned `n`-bit bus.
    Bits(u32),
    /// A two's-complement signed `n`-bit bus.
    Signed(u32),
    /// A fixed-depth array of `element_width`-bit elements.
    ///
    /// Used for delay lines and shift registers.  [`width`](Self::width)
    /// returns `element_width` (the per-element width), not the total
    /// storage.  Use [`storage_bits`](Self::storage_bits) when the
    /// full `element_width * depth` size is needed.
    Array {
        /// Bit width of each element.
        element_width: u32,
        /// Number of elements.
        depth: usize,
    },
}

impl WireTy {
    /// The per-element bit width of this wire.
    ///
    /// For scalar types this is the bus width.  For
    /// [`Array`](Self::Array) this is the element width, not the
    /// total storage.
    #[must_use]
    pub fn width(&self) -> u32 {
        match self {
            Self::Bit => 1,
            Self::Bits(n) | Self::Signed(n) => *n,
            Self::Array { element_width, .. } => *element_width,
        }
    }

    /// The depth of an array wire, or `None` for scalar wires.
    #[must_use]
    pub fn depth(&self) -> Option<usize> {
        match self {
            Self::Bit | Self::Bits(_) | Self::Signed(_) => None,
            Self::Array { depth, .. } => Some(*depth),
        }
    }

    /// Total storage in bits.
    ///
    /// For scalar wires this equals [`width`](Self::width).  For
    /// arrays it is `element_width * depth`.
    #[must_use]
    pub fn storage_bits(&self) -> usize {
        match self {
            Self::Bit => 1,
            Self::Bits(n) | Self::Signed(n) => usize::try_from(*n).unwrap_or(0),
            Self::Array { element_width, depth } => {
                usize::try_from(*element_width).unwrap_or(0) * depth
            }
        }
    }
}

impl core::fmt::Display for WireTy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bit => f.write_str("bit"),
            Self::Bits(n) => write!(f, "Bits<{n}>"),
            Self::Signed(n) => write!(f, "SignedBits<{n}>"),
            Self::Array { element_width, depth } => {
                write!(f, "Array<Bits<{element_width}>, {depth}>")
            }
        }
    }
}

/// A declared wire: an identifier paired with its type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Wire {
    id: WireId,
    ty: WireTy,
}

impl Wire {
    /// Construct a new declared wire.
    #[must_use]
    pub fn new(id: WireId, ty: WireTy) -> Self {
        Self { id, ty }
    }

    /// The wire's identifier.
    #[must_use]
    pub fn id(&self) -> WireId {
        self.id
    }

    /// The wire's type.
    #[must_use]
    pub fn ty(&self) -> &WireTy {
        &self.ty
    }
}

#[cfg(test)]
mod tests {
    use super::{Wire, WireId, WireTy};

    #[test]
    fn wire_id_round_trips() {
        assert_eq!(WireId::new(7).index(), 7);
    }

    #[test]
    fn wire_ty_widths() {
        assert_eq!(WireTy::Bit.width(), 1);
        assert_eq!(WireTy::Bits(8).width(), 8);
        assert_eq!(WireTy::Signed(32).width(), 32);
        assert_eq!(
            WireTy::Array { element_width: 64, depth: 1024 }.width(),
            64,
        );
    }

    #[test]
    fn wire_ty_displays() {
        assert_eq!(WireTy::Bit.to_string(), "bit");
        assert_eq!(WireTy::Bits(12).to_string(), "Bits<12>");
        assert_eq!(WireTy::Signed(7).to_string(), "SignedBits<7>");
        assert_eq!(
            WireTy::Array { element_width: 64, depth: 8 }.to_string(),
            "Array<Bits<64>, 8>",
        );
    }

    #[test]
    fn wire_ty_depth() {
        assert_eq!(WireTy::Bit.depth(), None);
        assert_eq!(WireTy::Bits(8).depth(), None);
        assert_eq!(WireTy::Signed(16).depth(), None);
        assert_eq!(
            WireTy::Array { element_width: 64, depth: 512 }.depth(),
            Some(512),
        );
    }

    #[test]
    fn wire_ty_storage_bits() {
        assert_eq!(WireTy::Bit.storage_bits(), 1);
        assert_eq!(WireTy::Bits(8).storage_bits(), 8);
        assert_eq!(WireTy::Signed(32).storage_bits(), 32);
        assert_eq!(
            WireTy::Array { element_width: 64, depth: 8 }.storage_bits(),
            512,
        );
    }

    #[test]
    fn wire_id_displays_as_w_prefix() {
        assert_eq!(WireId::new(3).to_string(), "w3");
    }

    #[test]
    fn wire_holds_id_and_ty() {
        let w = Wire::new(WireId::new(0), WireTy::Bits(4));
        assert_eq!(w.id(), WireId::new(0));
        assert_eq!(w.ty(), &WireTy::Bits(4));
    }
}
