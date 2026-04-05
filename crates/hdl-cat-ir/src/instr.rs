//! Instructions: gate invocations with multiple inputs and one output.

use crate::op::Op;
use crate::wire::WireId;

/// A single gate invocation in an [`crate::HdlGraph`].
///
/// Each `Instruction` reads zero-or-more input wires, applies an
/// [`Op`], and writes to exactly one output wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    op: Op,
    inputs: Vec<WireId>,
    output: WireId,
}

impl Instruction {
    /// Build an instruction, validating arity.
    ///
    /// # Panics
    ///
    /// Never panics; invalid arity returns `None` instead.
    #[must_use]
    pub fn new(op: Op, inputs: Vec<WireId>, output: WireId) -> Option<Self> {
        (inputs.len() == op.arity()).then_some(Self { op, inputs, output })
    }

    /// The operation performed by this instruction.
    #[must_use]
    pub fn op(&self) -> &Op {
        &self.op
    }

    /// The input wires in declaration order.
    #[must_use]
    pub fn inputs(&self) -> &[WireId] {
        &self.inputs
    }

    /// The output wire.
    #[must_use]
    pub fn output(&self) -> WireId {
        self.output
    }

    /// The "primary" input wire — the one that acts as the `source`
    /// vertex when the instruction is viewed as a graph edge.  For
    /// zero-input ops (constants), the source equals the output.
    #[must_use]
    pub fn primary_source(&self) -> WireId {
        self.inputs.first().copied().unwrap_or(self.output)
    }
}

impl core::fmt::Display for Instruction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} = {}", self.output, self.op)?;
        self.inputs.iter().try_fold(true, |first, w| {
            let sep = if first { " " } else { ", " };
            write!(f, "{sep}{w}")?;
            Ok(false)
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Instruction;
    use crate::op::{BinOp, Op};
    use crate::wire::WireId;

    #[test]
    fn new_accepts_matching_arity() {
        let i = Instruction::new(
            Op::Bin(BinOp::And),
            vec![WireId::new(0), WireId::new(1)],
            WireId::new(2),
        );
        assert!(i.is_some());
    }

    #[test]
    fn new_rejects_wrong_arity() {
        let too_few = Instruction::new(
            Op::Bin(BinOp::Or),
            vec![WireId::new(0)],
            WireId::new(1),
        );
        assert!(too_few.is_none());

        let too_many = Instruction::new(
            Op::Not,
            vec![WireId::new(0), WireId::new(1)],
            WireId::new(2),
        );
        assert!(too_many.is_none());
    }

    #[test]
    fn primary_source_is_first_input() {
        let i = Instruction::new(
            Op::Mux,
            vec![WireId::new(5), WireId::new(6), WireId::new(7)],
            WireId::new(8),
        );
        assert!(matches!(i.as_ref().map(Instruction::primary_source), Some(w) if w == WireId::new(5)));
    }

    #[test]
    fn primary_source_is_output_for_const() {
        let i = Instruction::new(
            Op::Const {
                bits: hdl_cat_kind::BitSeq::new(),
                ty: crate::wire::WireTy::Bit,
            },
            vec![],
            WireId::new(42),
        );
        assert!(matches!(i.as_ref().map(Instruction::primary_source), Some(w) if w == WireId::new(42)));
    }

    #[test]
    fn display_formats_instruction() {
        let i = Instruction::new(
            Op::Bin(BinOp::Add),
            vec![WireId::new(0), WireId::new(1)],
            WireId::new(2),
        );
        assert_eq!(i.map(|i| i.to_string()).as_deref(), Some("w2 = add w0, w1"));
    }
}
