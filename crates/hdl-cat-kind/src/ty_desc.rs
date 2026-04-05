//! Runtime type descriptors for [`crate::Hw`] types.
//!
//! A [`TypeDesc`] is the runtime witness of a hardware type's
//! structure.  Unlike the compile-time `WIDTH` constant, the
//! descriptor carries enough information to drive introspection-
//! heavy passes: VCD trace naming, Verilog port generation,
//! waveform tooltips, etc.

/// A named field inside a struct descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructField {
    name: String,
    ty: TypeDesc,
}

impl StructField {
    /// Construct a new field descriptor.
    #[must_use]
    pub fn new(name: impl Into<String>, ty: TypeDesc) -> Self {
        Self { name: name.into(), ty }
    }

    /// The field's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The field's type descriptor.
    #[must_use]
    pub fn ty(&self) -> &TypeDesc {
        &self.ty
    }
}

/// A variant inside an enum descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumVariant {
    name: String,
    payload: Vec<TypeDesc>,
}

impl EnumVariant {
    /// Construct a new variant descriptor.
    #[must_use]
    pub fn new(name: impl Into<String>, payload: Vec<TypeDesc>) -> Self {
        Self { name: name.into(), payload }
    }

    /// The variant's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The payload types (empty for a unit variant).
    #[must_use]
    pub fn payload(&self) -> &[TypeDesc] {
        &self.payload
    }
}

/// A runtime-visible description of a hardware type's structure.
///
/// # Examples
///
/// ```
/// use hdl_cat_kind::TypeDesc;
/// let t = TypeDesc::Bits { n: 8 };
/// assert_eq!(t.width(), 8);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDesc {
    /// A single wire carrying one boolean bit.
    Bool,
    /// An unsigned `n`-bit integer.
    Bits {
        /// The bit count.
        n: usize,
    },
    /// A two's-complement signed `n`-bit integer.
    Signed {
        /// The bit count.
        n: usize,
    },
    /// A fixed-arity tuple of hardware types.
    Tuple(Vec<TypeDesc>),
    /// A fixed-length array of identical elements.
    Array {
        /// The element type.
        elem: Box<TypeDesc>,
        /// The number of elements.
        len: usize,
    },
    /// A named struct with named fields.
    Struct {
        /// The struct's declared name.
        name: String,
        /// The named fields in declaration order.
        fields: Vec<StructField>,
    },
    /// A tagged union (sum type).
    Enum {
        /// The enum's declared name.
        name: String,
        /// Variants in declaration order.
        variants: Vec<EnumVariant>,
    },
}

impl TypeDesc {
    /// The total bit width of a value of this type.
    ///
    /// Struct and enum widths are the sum of their payload widths
    /// plus, for enums, the log-ceil discriminant width.
    #[must_use]
    pub fn width(&self) -> usize {
        match self {
            Self::Bool => 1,
            Self::Bits { n } | Self::Signed { n } => *n,
            Self::Tuple(ts) => ts.iter().map(Self::width).sum(),
            Self::Array { elem, len } => elem.width() * len,
            Self::Struct { fields, .. } => fields.iter().map(|f| f.ty().width()).sum(),
            Self::Enum { variants, .. } => {
                let discriminant = discriminant_width(variants.len());
                let payload = variants
                    .iter()
                    .map(|v| v.payload().iter().map(Self::width).sum::<usize>())
                    .max()
                    .unwrap_or(0);
                discriminant + payload
            }
        }
    }
}

fn discriminant_width(n_variants: usize) -> usize {
    match n_variants {
        0 | 1 => 0,
        n => {
            let leading = (n - 1).leading_zeros();
            let total = usize::BITS - leading;
            usize::try_from(total).unwrap_or(0)
        }
    }
}

impl core::fmt::Display for TypeDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bool => f.write_str("bool"),
            Self::Bits { n } => write!(f, "Bits<{n}>"),
            Self::Signed { n } => write!(f, "SignedBits<{n}>"),
            Self::Tuple(ts) => {
                f.write_str("(")?;
                ts.iter().try_fold(true, |first, t| {
                    if !first {
                        f.write_str(", ")?;
                    }
                    write!(f, "{t}")?;
                    Ok(false)
                })?;
                f.write_str(")")
            }
            Self::Array { elem, len } => write!(f, "[{elem}; {len}]"),
            Self::Struct { name, .. } | Self::Enum { name, .. } => f.write_str(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EnumVariant, StructField, TypeDesc};

    #[test]
    fn bool_width_is_one() {
        assert_eq!(TypeDesc::Bool.width(), 1);
    }

    #[test]
    fn bits_width_matches_parameter() {
        assert_eq!(TypeDesc::Bits { n: 12 }.width(), 12);
        assert_eq!(TypeDesc::Signed { n: 32 }.width(), 32);
    }

    #[test]
    fn tuple_width_sums_components() {
        let t = TypeDesc::Tuple(vec![
            TypeDesc::Bool,
            TypeDesc::Bits { n: 4 },
            TypeDesc::Signed { n: 3 },
        ]);
        assert_eq!(t.width(), 8);
    }

    #[test]
    fn array_width_is_product() {
        let t = TypeDesc::Array {
            elem: Box::new(TypeDesc::Bits { n: 8 }),
            len: 4,
        };
        assert_eq!(t.width(), 32);
    }

    #[test]
    fn struct_width_sums_fields() {
        let t = TypeDesc::Struct {
            name: "Packet".to_string(),
            fields: vec![
                StructField::new("header", TypeDesc::Bits { n: 8 }),
                StructField::new("payload", TypeDesc::Bits { n: 16 }),
            ],
        };
        assert_eq!(t.width(), 24);
    }

    #[test]
    fn enum_width_discriminant_plus_max_payload() {
        let t = TypeDesc::Enum {
            name: "Msg".to_string(),
            variants: vec![
                EnumVariant::new("Ping", vec![]),
                EnumVariant::new("Data", vec![TypeDesc::Bits { n: 16 }]),
                EnumVariant::new("Ack", vec![TypeDesc::Bits { n: 4 }]),
            ],
        };
        // 3 variants -> 2-bit discriminant, max payload = 16 -> total 18
        assert_eq!(t.width(), 18);
    }

    #[test]
    fn empty_enum_has_zero_width() {
        let t = TypeDesc::Enum {
            name: "Never".to_string(),
            variants: vec![],
        };
        assert_eq!(t.width(), 0);
    }

    #[test]
    fn single_variant_enum_has_no_discriminant() {
        let t = TypeDesc::Enum {
            name: "Only".to_string(),
            variants: vec![EnumVariant::new("A", vec![TypeDesc::Bool])],
        };
        assert_eq!(t.width(), 1);
    }

    #[test]
    fn display_formats_nested_tuple() {
        let t = TypeDesc::Tuple(vec![
            TypeDesc::Bits { n: 4 },
            TypeDesc::Bool,
        ]);
        assert_eq!(format!("{t}"), "(Bits<4>, bool)");
    }

    #[test]
    fn display_formats_array() {
        let t = TypeDesc::Array {
            elem: Box::new(TypeDesc::Bool),
            len: 8,
        };
        assert_eq!(format!("{t}"), "[bool; 8]");
    }
}
