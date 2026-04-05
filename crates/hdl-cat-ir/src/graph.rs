//! The IR graph: typed wires + gate instructions, viewable as a
//! `comp_cat_rs::collapse::free_category::Graph`.

use comp_cat_rs::collapse::free_category::{Edge, FreeCategoryError, Graph, Vertex};

use crate::instr::Instruction;
use crate::wire::WireTy;

/// The IR graph.
///
/// Stores a flat list of typed wires and a flat list of
/// instructions.  The `Graph` trait implementation maps each
/// wire to a vertex and each instruction to an edge whose
/// source is the instruction's primary input (or output, for
/// constants) and whose target is the instruction's output.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// use hdl_cat_ir::{HdlGraphBuilder, Op, BinOp, WireTy};
/// use comp_cat_rs::collapse::free_category::Graph;
///
/// let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(8));
/// let (b, c) = b.with_wire(WireTy::Bits(8));
/// let (b, d) = b.with_wire(WireTy::Bits(8));
/// let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], d)?;
/// let g = b.build();
/// assert_eq!(g.vertex_count(), 3);
/// assert_eq!(g.edge_count(), 1);
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct HdlGraph {
    pub(crate) wires: Vec<WireTy>,
    pub(crate) instructions: Vec<Instruction>,
}

impl HdlGraph {
    /// Access the wire types by index.
    #[must_use]
    pub fn wires(&self) -> &[WireTy] {
        &self.wires
    }

    /// Access the instructions in declaration order.
    #[must_use]
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Look up a wire's type by index.
    #[must_use]
    pub fn wire_ty(&self, idx: usize) -> Option<&WireTy> {
        self.wires.get(idx)
    }

    /// The total bit count across all declared wires.
    #[must_use]
    pub fn total_bit_width(&self) -> usize {
        self.wires.iter().map(|w| w.width() as usize).sum()
    }
}

impl Graph for HdlGraph {
    fn vertex_count(&self) -> usize {
        self.wires.len()
    }

    fn edge_count(&self) -> usize {
        self.instructions.len()
    }

    fn source(&self, edge: Edge) -> Result<Vertex, FreeCategoryError> {
        self.instructions
            .get(edge.index())
            .map(|i| Vertex::new(i.primary_source().index()))
            .ok_or(FreeCategoryError::EdgeOutOfBounds {
                edge,
                count: self.instructions.len(),
            })
    }

    fn target(&self, edge: Edge) -> Result<Vertex, FreeCategoryError> {
        self.instructions
            .get(edge.index())
            .map(|i| Vertex::new(i.output().index()))
            .ok_or(FreeCategoryError::EdgeOutOfBounds {
                edge,
                count: self.instructions.len(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::HdlGraph;
    use crate::builder::HdlGraphBuilder;
    use crate::op::{BinOp, Op};
    use crate::wire::WireTy;
    use comp_cat_rs::collapse::free_category::{Edge, Graph, Vertex};

    fn example_graph() -> Result<HdlGraph, hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (b, c) = b.with_wire(WireTy::Bit);
        let (b, d) = b.with_wire(WireTy::Bit);
        let b = b.with_instruction(Op::Bin(BinOp::And), vec![a, c], d)?;
        Ok(b.build())
    }

    #[test]
    fn vertex_count_matches_wires() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        assert_eq!(g.vertex_count(), 3);
        Ok(())
    }

    #[test]
    fn edge_count_matches_instructions() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        assert_eq!(g.edge_count(), 1);
        Ok(())
    }

    #[test]
    fn source_is_primary_input() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        let src = g.source(Edge::new(0))?;
        assert_eq!(src, Vertex::new(0));
        Ok(())
    }

    #[test]
    fn target_is_output() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        let tgt = g.target(Edge::new(0))?;
        assert_eq!(tgt, Vertex::new(2));
        Ok(())
    }

    #[test]
    fn source_out_of_bounds_errors() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        assert!(g.source(Edge::new(10)).is_err());
        Ok(())
    }

    #[test]
    fn wire_ty_lookup() -> Result<(), hdl_cat_error::Error> {
        let g = example_graph()?;
        assert_eq!(g.wire_ty(0), Some(&WireTy::Bit));
        assert_eq!(g.wire_ty(5), None);
        Ok(())
    }

    #[test]
    fn total_bit_width_sums_wires() {
        let (bld, _) = HdlGraphBuilder::new().with_wire(WireTy::Bits(8));
        let (bld, _) = bld.with_wire(WireTy::Bits(4));
        let (bld, _) = bld.with_wire(WireTy::Bit);
        let graph = bld.build();
        assert_eq!(graph.total_bit_width(), 13);
    }
}
