//! [`CircuitArrow<A, B>`]: a typed morphism in the circuit category.

use core::marker::PhantomData;

use hdl_cat_ir::{HdlGraph, HdlGraphBuilder, Op, WireId, WireTy};

use crate::object::Object;

/// A morphism `A -> B` in the [`crate::Circuit`] category.
///
/// Wraps an [`HdlGraph`] together with lists of input and output
/// wire identifiers.  Typed by phantom `A` and `B` parameters to
/// match the `Category::Hom` GAT shape.
///
/// The graph owns all wires and instructions belonging to this
/// arrow.  Composition creates a new arrow holding a merged graph.
#[derive(Clone, Debug)]
#[must_use]
pub struct CircuitArrow<A, B> {
    graph: HdlGraph,
    inputs: Vec<WireId>,
    outputs: Vec<WireId>,
    _phantom: PhantomData<(A, B)>,
}

impl<A, B> CircuitArrow<A, B> {
    /// Construct from parts.  Internal-only — use the category
    /// combinators or primitive builders instead.
    pub(crate) fn from_parts(
        graph: HdlGraph,
        inputs: Vec<WireId>,
        outputs: Vec<WireId>,
    ) -> Self {
        Self {
            graph,
            inputs,
            outputs,
            _phantom: PhantomData,
        }
    }

    /// The underlying IR graph.
    pub fn graph(&self) -> &HdlGraph {
        &self.graph
    }

    /// The input wire identifiers, in declaration order.
    #[must_use]
    pub fn inputs(&self) -> &[WireId] {
        &self.inputs
    }

    /// The output wire identifiers, in declaration order.
    #[must_use]
    pub fn outputs(&self) -> &[WireId] {
        &self.outputs
    }

    /// Consume the arrow into its component parts.
    pub(crate) fn into_parts(self) -> (HdlGraph, Vec<WireId>, Vec<WireId>) {
        (self.graph, self.inputs, self.outputs)
    }

    /// Consume the arrow into its raw IR parts.  Public escape
    /// hatch for downstream crates (`hdl-cat-sync`,
    /// `hdl-cat-sim`, `hdl-cat-verilog`) that need to drive the
    /// underlying graph directly.
    pub fn into_raw_parts(self) -> (HdlGraph, Vec<WireId>, Vec<WireId>) {
        (self.graph, self.inputs, self.outputs)
    }

    /// Construct an arrow from raw IR parts.  The caller is
    /// responsible for ensuring the wire indices exist in
    /// the graph and that input/output counts match the
    /// phantom `A` and `B` types' wire layouts.
    pub fn from_raw_parts(
        graph: HdlGraph,
        inputs: Vec<WireId>,
        outputs: Vec<WireId>,
    ) -> Self {
        Self::from_parts(graph, inputs, outputs)
    }
}

/// Build a "pass-through" identity arrow for an object `A`.
///
/// The graph declares one wire per component type in `A`'s
/// layout; inputs and outputs point to the same wires.
pub(crate) fn identity_arrow<A: Object>() -> CircuitArrow<A, A> {
    let layout = A::wire_layout();
    let (graph_bld, wires) = layout.into_iter().fold(
        (HdlGraphBuilder::new(), Vec::<WireId>::new()),
        |(bld, acc), ty| {
            let (next_bld, id) = bld.with_wire(ty);
            let next_acc = acc
                .into_iter()
                .chain(core::iter::once(id))
                .collect();
            (next_bld, next_acc)
        },
    );
    let graph = graph_bld.build();
    CircuitArrow::from_parts(graph, wires.clone(), wires)
}

fn append_wires(
    bld: HdlGraphBuilder,
    tys: impl IntoIterator<Item = WireTy>,
) -> HdlGraphBuilder {
    tys.into_iter().fold(bld, |bld, ty| bld.with_wire(ty).0)
}

/// Sequentially compose `f: A -> B` with `g: B -> C` to produce
/// `A -> C`.
///
/// The implementation merges the two graphs, shifting `g`'s wire
/// indices by `f`'s wire count, and substitutes `g`'s input
/// indices for `f`'s output indices.
pub(crate) fn compose_arrows<A, B, C>(
    f: CircuitArrow<A, B>,
    g: CircuitArrow<B, C>,
) -> CircuitArrow<A, C> {
    let (f_graph, f_inputs, f_outputs) = f.into_parts();
    let (g_graph, g_inputs, g_outputs) = g.into_parts();
    let offset = f_graph.wires().len();

    // Mapping: for each of g's shifted inputs, substitute the
    // corresponding f output.
    let substitution: Vec<(WireId, WireId)> = g_inputs
        .iter()
        .zip(f_outputs.iter())
        .map(|(g_in, f_out)| (WireId::new(g_in.index() + offset), *f_out))
        .collect();

    let remap = |w: WireId| -> WireId {
        let shifted = WireId::new(w.index() + offset);
        substitution
            .iter()
            .find_map(|(from, to)| (*from == shifted).then_some(*to))
            .unwrap_or(shifted)
    };

    let bld = append_wires(HdlGraphBuilder::new(), f_graph.wires().iter().cloned());
    let bld = append_wires(bld, g_graph.wires().iter().cloned());

    let bld_with_f = f_graph.instructions().iter().try_fold(bld, |bld, instr| {
        bld.with_instruction(
            instr.op().clone(),
            instr.inputs().to_vec(),
            instr.output(),
        )
    });

    let bld_with_both = bld_with_f.and_then(|bld| {
        g_graph.instructions().iter().try_fold(bld, |bld, instr| {
            let new_inputs: Vec<WireId> =
                instr.inputs().iter().copied().map(remap).collect();
            let new_output = remap(instr.output());
            bld.with_instruction(instr.op().clone(), new_inputs, new_output)
        })
    });

    let graph = bld_with_both
        .map_or_else(|_| HdlGraphBuilder::new().build(), HdlGraphBuilder::build);
    let combined_outputs: Vec<WireId> = g_outputs.into_iter().map(remap).collect();
    CircuitArrow::from_parts(graph, f_inputs, combined_outputs)
}

/// Parallel composition: place two arrows side by side.
///
/// Given `f: A -> B` and `g: C -> D`, produce
/// `f ⊗ g: (A ⊗ C) -> (B ⊗ D)`.  The two sub-graphs are
/// concatenated; no wire sharing occurs between them.
pub(crate) fn tensor_arrows<A, B, C, D>(
    f: CircuitArrow<A, B>,
    g: CircuitArrow<C, D>,
) -> CircuitArrow<crate::object::CircuitTensor<A, C>, crate::object::CircuitTensor<B, D>> {
    let (f_graph, f_inputs, f_outputs) = f.into_parts();
    let (g_graph, g_inputs, g_outputs) = g.into_parts();
    let offset = f_graph.wires().len();

    let bld = append_wires(HdlGraphBuilder::new(), f_graph.wires().iter().cloned());
    let bld = append_wires(bld, g_graph.wires().iter().cloned());

    let bld_with_f = f_graph.instructions().iter().try_fold(bld, |bld, instr| {
        bld.with_instruction(
            instr.op().clone(),
            instr.inputs().to_vec(),
            instr.output(),
        )
    });

    let shift = |w: WireId| WireId::new(w.index() + offset);
    let bld_full = bld_with_f.and_then(|bld| {
        g_graph.instructions().iter().try_fold(bld, |bld, instr| {
            let new_inputs: Vec<WireId> =
                instr.inputs().iter().copied().map(shift).collect();
            let new_output = shift(instr.output());
            bld.with_instruction(instr.op().clone(), new_inputs, new_output)
        })
    });

    let graph = bld_full
        .map_or_else(|_| HdlGraphBuilder::new().build(), HdlGraphBuilder::build);

    let combined_inputs: Vec<WireId> = f_inputs
        .into_iter()
        .chain(g_inputs.into_iter().map(shift))
        .collect();
    let combined_outputs: Vec<WireId> = f_outputs
        .into_iter()
        .chain(g_outputs.into_iter().map(shift))
        .collect();

    CircuitArrow::from_parts(graph, combined_inputs, combined_outputs)
}

/// Build an arrow from a single primitive instruction.
pub(crate) fn primitive_arrow<A: Object, B: Object>(
    input_tys: Vec<WireTy>,
    op: Op,
    output_ty: WireTy,
) -> Result<CircuitArrow<A, B>, hdl_cat_error::Error> {
    let (bld, inputs) = input_tys.into_iter().fold(
        (HdlGraphBuilder::new(), Vec::<WireId>::new()),
        |(bld, acc), ty| {
            let (next_bld, id) = bld.with_wire(ty);
            let next_acc = acc
                .into_iter()
                .chain(core::iter::once(id))
                .collect();
            (next_bld, next_acc)
        },
    );
    let (bld, output) = bld.with_wire(output_ty);
    let bld = bld.with_instruction(op, inputs.clone(), output)?;
    Ok(CircuitArrow::from_parts(bld.build(), inputs, vec![output]))
}

#[cfg(test)]
mod tests {
    use super::{compose_arrows, identity_arrow, primitive_arrow, tensor_arrows, CircuitArrow};
    use crate::object::{CircuitTensor, Obj};
    use hdl_cat_bits::Bits;
    use hdl_cat_ir::{BinOp, Op, WireTy};

    type And4 = CircuitArrow<CircuitTensor<Obj<Bits<4>>, Obj<Bits<4>>>, Obj<Bits<4>>>;
    type Not4 = CircuitArrow<Obj<Bits<4>>, Obj<Bits<4>>>;

    fn and4() -> Result<And4, hdl_cat_error::Error> {
        primitive_arrow(
            vec![WireTy::Bits(4), WireTy::Bits(4)],
            Op::Bin(BinOp::And),
            WireTy::Bits(4),
        )
    }

    fn not4() -> Result<Not4, hdl_cat_error::Error> {
        primitive_arrow(vec![WireTy::Bits(4)], Op::Not, WireTy::Bits(4))
    }

    #[test]
    fn identity_arrow_has_matching_io() {
        let id = identity_arrow::<Obj<Bits<8>>>();
        assert_eq!(id.inputs().len(), 1);
        assert_eq!(id.outputs().len(), 1);
        assert_eq!(id.inputs()[0], id.outputs()[0]);
        assert_eq!(id.graph().instructions().len(), 0);
    }

    #[test]
    fn primitive_and_has_correct_shape() -> Result<(), hdl_cat_error::Error> {
        let g = and4()?;
        assert_eq!(g.inputs().len(), 2);
        assert_eq!(g.outputs().len(), 1);
        assert_eq!(g.graph().instructions().len(), 1);
        assert_eq!(g.graph().wires().len(), 3);
        Ok(())
    }

    #[test]
    fn compose_not_not_merges_graphs() -> Result<(), hdl_cat_error::Error> {
        let nf = not4()?;
        let ng = not4()?;
        let composed = compose_arrows(nf, ng);
        assert_eq!(composed.inputs().len(), 1);
        assert_eq!(composed.outputs().len(), 1);
        assert_eq!(composed.graph().instructions().len(), 2);
        Ok(())
    }

    #[test]
    fn tensor_of_primitives_pairs_inputs() -> Result<(), hdl_cat_error::Error> {
        let a = not4()?;
        let b = not4()?;
        let paired = tensor_arrows(a, b);
        assert_eq!(paired.inputs().len(), 2);
        assert_eq!(paired.outputs().len(), 2);
        assert_eq!(paired.graph().instructions().len(), 2);
        Ok(())
    }

    #[test]
    fn identity_then_primitive_keeps_primitive() -> Result<(), hdl_cat_error::Error> {
        let id: CircuitArrow<Obj<Bits<4>>, Obj<Bits<4>>> = identity_arrow();
        let n = not4()?;
        let composed = compose_arrows(id, n);
        assert_eq!(composed.graph().instructions().len(), 1);
        assert_eq!(composed.inputs().len(), 1);
        assert_eq!(composed.outputs().len(), 1);
        Ok(())
    }
}
