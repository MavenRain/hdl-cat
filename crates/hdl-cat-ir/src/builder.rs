//! Functional builder for [`crate::HdlGraph`].
//!
//! Every method consumes `self` and returns a new builder — no
//! mutation.  Compose via chained calls or the `?` operator.

use hdl_cat_error::{Error, TypeName};

use crate::graph::HdlGraph;
use crate::instr::Instruction;
use crate::op::Op;
use crate::wire::{WireId, WireTy};

/// An immutable, accumulating builder for an [`crate::HdlGraph`].
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct HdlGraphBuilder {
    wires: Vec<WireTy>,
    instructions: Vec<Instruction>,
}

impl HdlGraphBuilder {
    /// Start an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a new wire of the given type, returning its id.
    pub fn with_wire(self, ty: WireTy) -> (Self, WireId) {
        let id = WireId::new(self.wires.len());
        let wires = self.wires.into_iter().chain(core::iter::once(ty)).collect();
        (
            Self {
                wires,
                instructions: self.instructions,
            },
            id,
        )
    }

    /// Append an instruction, validating arity.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TypeMismatch`] when the number of inputs
    /// does not match the op's declared arity.
    pub fn with_instruction(
        self,
        op: Op,
        inputs: Vec<WireId>,
        output: WireId,
    ) -> Result<Self, Error> {
        let arity = op.arity();
        let provided = inputs.len();
        Instruction::new(op, inputs, output)
            .ok_or_else(|| Error::TypeMismatch {
                expected: TypeName::new(format!("{arity} inputs")),
                actual: TypeName::new(format!("{provided} inputs")),
            })
            .map(|instr| Self {
                wires: self.wires,
                instructions: self
                    .instructions
                    .into_iter()
                    .chain(core::iter::once(instr))
                    .collect(),
            })
    }

    /// Finalize the builder into an [`HdlGraph`].
    pub fn build(self) -> HdlGraph {
        HdlGraph {
            wires: self.wires,
            instructions: self.instructions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HdlGraphBuilder;
    use crate::op::{BinOp, Op};
    use crate::wire::WireTy;

    #[test]
    fn empty_builder_yields_empty_graph() {
        let g = HdlGraphBuilder::new().build();
        assert_eq!(g.wires().len(), 0);
        assert_eq!(g.instructions().len(), 0);
    }

    #[test]
    fn wires_are_assigned_sequential_ids() {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (_, d) = b.with_wire(WireTy::Signed(8));
        assert_eq!(a.index(), 0);
        assert_eq!(c.index(), 1);
        assert_eq!(d.index(), 2);
    }

    #[test]
    fn with_instruction_accepts_matching_arity() -> Result<(), hdl_cat_error::Error> {
        let (bld, lhs) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (bld, rhs) = bld.with_wire(WireTy::Bit);
        let (bld, out) = bld.with_wire(WireTy::Bit);
        let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![lhs, rhs], out)?;
        let graph = bld.build();
        assert_eq!(graph.instructions().len(), 1);
        Ok(())
    }

    #[test]
    fn with_instruction_rejects_wrong_arity() {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (b, out) = b.with_wire(WireTy::Bit);
        let result = b.with_instruction(Op::Bin(BinOp::And), vec![a], out);
        assert!(result.is_err());
    }

    #[test]
    fn chained_construction() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (b, n_a) = b.with_wire(WireTy::Bit);
        let b = b.with_instruction(Op::Not, vec![a], n_a)?;
        let g = b.build();
        assert_eq!(g.wires().len(), 2);
        assert_eq!(g.instructions().len(), 1);
        Ok(())
    }
}
