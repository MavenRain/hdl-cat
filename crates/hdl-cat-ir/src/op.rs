//! Primitive hardware operations.

use hdl_cat_kind::BitSeq;

use crate::wire::WireTy;

/// A binary gate operation over buses of matching width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// Bitwise AND.
    And,
    /// Bitwise OR.
    Or,
    /// Bitwise XOR.
    Xor,
    /// Unsigned wrap-around addition.
    Add,
    /// Unsigned wrap-around subtraction.
    Sub,
    /// Unsigned wrap-around multiplication.
    Mul,
    /// Equality comparison (output: 1 bit).
    Eq,
    /// Unsigned less-than comparison (output: 1 bit).
    Lt,
}

impl BinOp {
    /// Whether this op produces a 1-bit output regardless of input width.
    #[must_use]
    pub fn is_comparison(self) -> bool {
        matches!(self, Self::Eq | Self::Lt)
    }
}

impl core::fmt::Display for BinOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::And => f.write_str("and"),
            Self::Or => f.write_str("or"),
            Self::Xor => f.write_str("xor"),
            Self::Add => f.write_str("add"),
            Self::Sub => f.write_str("sub"),
            Self::Mul => f.write_str("mul"),
            Self::Eq => f.write_str("eq"),
            Self::Lt => f.write_str("lt"),
        }
    }
}

/// A primitive hardware operation.
///
/// Each `Op` is the atom of the IR: every circuit is expressible
/// as a sequence of `Op`-tagged [`crate::Instruction`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Bitwise NOT (single input, same-width output).
    Not,
    /// A binary gate (two inputs of matching width).
    Bin(BinOp),
    /// 2:1 multiplexer.  Inputs: `[selector: bit, false_arm, true_arm]`.
    Mux,
    /// A literal constant.  Zero inputs, one output.
    Const {
        /// The output bit pattern (LSB-first).
        bits: BitSeq,
        /// The output wire type.
        ty: WireTy,
    },
    /// A one-cycle delay register.  One input, one output.
    Reg {
        /// Initial value for cycle 0.
        init: BitSeq,
        /// Wire type (equal to both input and output).
        ty: WireTy,
    },
    /// Bus concatenation.  Inputs: `[low, high]`.
    Concat {
        /// The width of the low (first input) bus.
        low_width: u32,
        /// The width of the high (second input) bus.
        high_width: u32,
    },
    /// Bus slice: extract bits `[lo, hi)` of the single input.
    Slice {
        /// Low bit index (inclusive).
        lo: u32,
        /// High bit index (exclusive).
        hi: u32,
    },
}

impl Op {
    /// The number of inputs this op consumes.
    #[must_use]
    pub fn arity(&self) -> usize {
        match self {
            Self::Not | Self::Reg { .. } | Self::Slice { .. } => 1,
            Self::Bin(_) | Self::Concat { .. } => 2,
            Self::Mux => 3,
            Self::Const { .. } => 0,
        }
    }
}

impl core::fmt::Display for Op {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Not => f.write_str("not"),
            Self::Bin(b) => write!(f, "{b}"),
            Self::Mux => f.write_str("mux"),
            Self::Const { ty, .. } => write!(f, "const:{ty}"),
            Self::Reg { ty, .. } => write!(f, "reg:{ty}"),
            Self::Concat { low_width, high_width } => {
                write!(f, "concat<{low_width},{high_width}>")
            }
            Self::Slice { lo, hi } => write!(f, "slice[{lo}..{hi}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BinOp, Op};
    use crate::wire::WireTy;
    use hdl_cat_kind::BitSeq;

    #[test]
    fn binop_comparison_classification() {
        assert!(BinOp::Eq.is_comparison());
        assert!(BinOp::Lt.is_comparison());
        assert!(!BinOp::And.is_comparison());
        assert!(!BinOp::Add.is_comparison());
    }

    #[test]
    fn op_arity_matches_variant() {
        assert_eq!(Op::Not.arity(), 1);
        assert_eq!(Op::Bin(BinOp::And).arity(), 2);
        assert_eq!(Op::Mux.arity(), 3);
        assert_eq!(
            Op::Const {
                bits: BitSeq::new(),
                ty: WireTy::Bit,
            }
            .arity(),
            0
        );
        assert_eq!(
            Op::Reg {
                init: BitSeq::new(),
                ty: WireTy::Bits(8),
            }
            .arity(),
            1
        );
        assert_eq!(
            Op::Concat { low_width: 4, high_width: 4 }.arity(),
            2
        );
        assert_eq!(Op::Slice { lo: 0, hi: 4 }.arity(), 1);
    }

    #[test]
    fn op_display_contains_mnemonic() {
        assert_eq!(Op::Not.to_string(), "not");
        assert_eq!(Op::Bin(BinOp::Xor).to_string(), "xor");
        assert_eq!(Op::Slice { lo: 0, hi: 8 }.to_string(), "slice[0..8]");
    }
}
