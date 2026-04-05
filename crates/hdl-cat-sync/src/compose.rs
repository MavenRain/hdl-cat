//! Composition primitives for [`crate::Sync`] machines.
//!
//! - [`compose_sync`] — sequential composition; state becomes the
//!   product `(S1, S2)` and `f`'s data output is wired into `g`'s
//!   data input.
//! - [`par_sync`] — parallel composition; state becomes `(S1, S2)`
//!   and the two machines operate on independent inputs/outputs.
//! - [`feedback_sync`] — close a one-cycle feedback loop; promotes
//!   the data output to a state wire whose value is read back in
//!   the next cycle.

use hdl_cat_circuit::CircuitTensor;
use hdl_cat_ir::{HdlGraph, HdlGraphBuilder, Instruction, WireId};
use hdl_cat_kind::BitSeq;

use crate::machine::{self, Sync};

/// Sequentially compose two `Sync` machines.
///
/// Produces a machine whose state is the product `(S1, S2)` and
/// whose data pipe is `f` then `g`: `f`'s data output `M` is
/// routed directly into `g`'s data input.
///
/// At each cycle, with combined state `(s1, s2)` and input `i`:
///
/// 1. `f` runs with `(s1, i)`, producing `(s1', m)`.
/// 2. `g` runs with `(s2, m)`, producing `(s2', o)`.
/// 3. The machine yields state `(s1', s2')` and output `o`.
#[allow(clippy::similar_names)]
pub fn compose_sync<S1, S2, I, M, O>(
    f: Sync<S1, I, M>,
    g: Sync<S2, M, O>,
) -> Sync<CircuitTensor<S1, S2>, I, O> {
    let (f_graph, f_inputs, f_outputs, f_init, f_sc) = f.into_parts();
    let (g_graph, g_inputs, g_outputs, g_init, g_sc) = g.into_parts();
    let f_wire_count = f_graph.wires().len();

    let (state_f_input, data_f_input) = f_inputs.split_at(f_sc);
    let (state_f_output, data_f_output) = f_outputs.split_at(f_sc);
    let (state_g_input, data_g_input) = g_inputs.split_at(g_sc);
    let (state_g_output, data_g_output) = g_outputs.split_at(g_sc);

    let shift = |w: WireId| WireId::new(w.index() + f_wire_count);

    // g's data inputs get rewritten to f's data outputs.
    let substitution: Vec<(WireId, WireId)> = data_g_input
        .iter()
        .zip(data_f_output.iter())
        .map(|(g_m, f_m)| (shift(*g_m), *f_m))
        .collect();

    let remap_g = move |w: WireId| -> WireId {
        let shifted = WireId::new(w.index() + f_wire_count);
        substitution
            .iter()
            .find_map(|(from, to)| (*from == shifted).then_some(*to))
            .unwrap_or(shifted)
    };

    let merged = merge_graphs(&f_graph, &g_graph, remap_g);

    let combined_inputs: Vec<WireId> = state_f_input
        .iter()
        .copied()
        .chain(state_g_input.iter().copied().map(shift))
        .chain(data_f_input.iter().copied())
        .collect();

    let combined_outputs: Vec<WireId> = state_f_output
        .iter()
        .copied()
        .chain(state_g_output.iter().copied().map(shift))
        .chain(data_g_output.iter().copied().map(shift))
        .collect();

    let combined_state = f_init.concat(g_init);
    let combined_state_count = f_sc + g_sc;

    machine::from_raw(
        merged,
        combined_inputs,
        combined_outputs,
        combined_state,
        combined_state_count,
    )
}

/// Parallel composition: place two `Sync` machines side by side.
///
/// Produces a machine whose state is `(S1, S2)`, whose input is
/// `(I1, I2)`, and whose output is `(O1, O2)`.  The two sub-
/// machines operate independently — no wire sharing between
/// them.
#[allow(clippy::similar_names)]
pub fn par_sync<S1, S2, I1, I2, O1, O2>(
    f: Sync<S1, I1, O1>,
    g: Sync<S2, I2, O2>,
) -> Sync<CircuitTensor<S1, S2>, CircuitTensor<I1, I2>, CircuitTensor<O1, O2>> {
    let (f_graph, f_inputs, f_outputs, f_init, f_sc) = f.into_parts();
    let (g_graph, g_inputs, g_outputs, g_init, g_sc) = g.into_parts();
    let f_wire_count = f_graph.wires().len();

    let (state_f_input, data_f_input) = f_inputs.split_at(f_sc);
    let (state_f_output, data_f_output) = f_outputs.split_at(f_sc);
    let (state_g_input, data_g_input) = g_inputs.split_at(g_sc);
    let (state_g_output, data_g_output) = g_outputs.split_at(g_sc);

    let shift = |w: WireId| WireId::new(w.index() + f_wire_count);
    let merged = merge_graphs(&f_graph, &g_graph, shift);

    let combined_inputs: Vec<WireId> = state_f_input
        .iter()
        .copied()
        .chain(state_g_input.iter().copied().map(shift))
        .chain(data_f_input.iter().copied())
        .chain(data_g_input.iter().copied().map(shift))
        .collect();

    let combined_outputs: Vec<WireId> = state_f_output
        .iter()
        .copied()
        .chain(state_g_output.iter().copied().map(shift))
        .chain(data_f_output.iter().copied())
        .chain(data_g_output.iter().copied().map(shift))
        .collect();

    let combined_state = f_init.concat(g_init);
    let combined_state_count = f_sc + g_sc;

    machine::from_raw(
        merged,
        combined_inputs,
        combined_outputs,
        combined_state,
        combined_state_count,
    )
}

/// Close a one-cycle feedback loop on a `Sync` machine.
///
/// Transforms `f: Sync<S, (I ⊗ O), O>` — a machine whose input
/// mixes an external `I` with a fed-back `O` — into
/// `Sync<(S ⊗ O), I, O>` where the fed-back `O` is stored in a
/// new state wire initialized from `initial_feedback`.
///
/// At cycle `k`, with state `(s_k, o_{k-1})` and input `i_k`:
///
/// 1. `f` runs with `(s_k, (i_k, o_{k-1}))`, producing
///    `(s_{k+1}, o_k)`.
/// 2. The new state is `(s_{k+1}, o_k)`.
/// 3. The output is `o_k`.
///
/// # Errors
///
/// Returns [`hdl_cat_error::Error::WidthMismatch`] when
/// `initial_feedback`'s bit length does not match the sum of
/// widths of `f`'s data-output wires.
pub fn feedback_sync<S, I, O>(
    f: Sync<S, CircuitTensor<I, O>, O>,
    initial_feedback: BitSeq,
) -> Result<Sync<CircuitTensor<S, O>, I, O>, hdl_cat_error::Error> {
    let (f_graph, f_inputs, f_outputs, f_init, f_sc) = f.into_parts();
    let total_inputs = f_inputs.len();
    let total_outputs = f_outputs.len();
    let o_wire_count = total_outputs.saturating_sub(f_sc);
    let i_wire_count = total_inputs.saturating_sub(f_sc).saturating_sub(o_wire_count);

    let (state_pre, rest) = f_inputs.split_at(f_sc);
    let (i_wires, o_feedback_wires) = rest.split_at(i_wire_count);
    let (state_post_next, data_output_wires) = f_outputs.split_at(f_sc);

    let expected_feedback_bits: usize = o_feedback_wires
        .iter()
        .map(|w| wire_width(&f_graph, *w))
        .sum();
    (initial_feedback.len() == expected_feedback_bits)
        .then_some(())
        .ok_or_else(|| hdl_cat_error::Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(expected_feedback_bits).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(initial_feedback.len()).unwrap_or(u32::MAX),
            ),
        })?;

    let new_inputs: Vec<WireId> = state_pre
        .iter()
        .copied()
        .chain(o_feedback_wires.iter().copied())
        .chain(i_wires.iter().copied())
        .collect();
    let new_outputs: Vec<WireId> = state_post_next
        .iter()
        .copied()
        .chain(data_output_wires.iter().copied())
        .chain(data_output_wires.iter().copied())
        .collect();
    let new_state = f_init.concat(initial_feedback);
    let new_state_count = f_sc + o_wire_count;

    Ok(machine::from_raw(
        f_graph,
        new_inputs,
        new_outputs,
        new_state,
        new_state_count,
    ))
}

fn merge_graphs<F>(f_graph: &HdlGraph, g_graph: &HdlGraph, remap_g: F) -> HdlGraph
where
    F: Fn(WireId) -> WireId + Clone,
{
    let bld = HdlGraphBuilder::new();
    let bld = f_graph
        .wires()
        .iter()
        .cloned()
        .fold(bld, |b, ty| b.with_wire(ty).0);
    let bld = g_graph
        .wires()
        .iter()
        .cloned()
        .fold(bld, |b, ty| b.with_wire(ty).0);

    let bld_with_f = f_graph
        .instructions()
        .iter()
        .try_fold(bld, instruction_copier);
    let bld_with_both = bld_with_f.and_then(|b| {
        g_graph.instructions().iter().try_fold(b, |b, instr| {
            let new_inputs: Vec<WireId> =
                instr.inputs().iter().copied().map(remap_g.clone()).collect();
            let new_output = remap_g(instr.output());
            b.with_instruction(instr.op().clone(), new_inputs, new_output)
        })
    });
    bld_with_both
        .map_or_else(|_| HdlGraphBuilder::new().build(), HdlGraphBuilder::build)
}

fn instruction_copier(
    bld: HdlGraphBuilder,
    instr: &Instruction,
) -> Result<HdlGraphBuilder, hdl_cat_error::Error> {
    bld.with_instruction(
        instr.op().clone(),
        instr.inputs().to_vec(),
        instr.output(),
    )
}

fn wire_width(graph: &HdlGraph, w: WireId) -> usize {
    graph
        .wire_ty(w.index())
        .map_or(0, |ty| usize::try_from(ty.width()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::{compose_sync, feedback_sync, par_sync};
    use crate::Sync;
    use hdl_cat_bits::Bits;
    use hdl_cat_circuit::{gates, CircuitTensor, CircuitUnit, Obj};
    use hdl_cat_kind::{BitSeq, Hw};

    #[test]
    fn compose_two_stateless_inverters_flattens() -> Result<(), hdl_cat_error::Error> {
        let inv_a = gates::not_bits::<4>()?;
        let inv_b = gates::not_bits::<4>()?;
        let ma: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_a);
        let mb: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_b);
        let composed = compose_sync(ma, mb);
        assert_eq!(composed.state_wire_count(), 0);
        assert_eq!(composed.graph().instructions().len(), 2);
        assert_eq!(composed.input_wires().len(), 1);
        assert_eq!(composed.output_wires().len(), 1);
        Ok(())
    }

    #[test]
    fn par_two_stateless_halves_doubles_ports() -> Result<(), hdl_cat_error::Error> {
        let inv_a = gates::not_bits::<4>()?;
        let inv_b = gates::not_bits::<4>()?;
        let ma: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_a);
        let mb: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_b);
        let paired = par_sync(ma, mb);
        assert_eq!(paired.state_wire_count(), 0);
        assert_eq!(paired.input_wires().len(), 2);
        assert_eq!(paired.output_wires().len(), 2);
        assert_eq!(paired.graph().instructions().len(), 2);
        Ok(())
    }

    #[test]
    fn compose_preserves_state_widths() -> Result<(), hdl_cat_error::Error> {
        use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
        let (bld, s) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, inp) = bld.with_wire(WireTy::Bits(4));
        let (bld, s_next) = bld.with_wire(WireTy::Bits(4));
        let (bld, out) = bld.with_wire(WireTy::Bits(4));
        let (bld, mask) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(
            Op::Const {
                bits: BitSeq::from_iter([true, true, true, true]),
                ty: WireTy::Bits(4),
            },
            vec![],
            mask,
        )?;
        let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![s, mask], s_next)?;
        let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![inp, mask], out)?;
        let graph = bld.build();
        let m1: Sync<Obj<Bits<4>>, Obj<Bits<4>>, Obj<Bits<4>>> = crate::machine::from_raw(
            graph.clone(),
            vec![s, inp],
            vec![s_next, out],
            Bits::<4>::try_new(5)?.to_bits_seq(),
            1,
        );
        let m2: Sync<Obj<Bits<4>>, Obj<Bits<4>>, Obj<Bits<4>>> = crate::machine::from_raw(
            graph,
            vec![s, inp],
            vec![s_next, out],
            Bits::<4>::try_new(9)?.to_bits_seq(),
            1,
        );
        let composed = compose_sync(m1, m2);
        assert_eq!(composed.state_wire_count(), 2);
        assert_eq!(composed.initial_state().len(), 8);
        Ok(())
    }

    #[test]
    fn feedback_wires_output_back_to_input() -> Result<(), hdl_cat_error::Error> {
        use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
        let (bld, i_in) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (bld, o_fb) = bld.with_wire(WireTy::Bit);
        let (bld, out) = bld.with_wire(WireTy::Bit);
        let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![i_in, o_fb], out)?;
        let graph = bld.build();
        let f: Sync<CircuitUnit, CircuitTensor<Obj<bool>, Obj<bool>>, Obj<bool>> =
            crate::machine::from_raw(
                graph,
                vec![i_in, o_fb],
                vec![out],
                BitSeq::new(),
                0,
            );
        let closed = feedback_sync(f, BitSeq::from_iter([true]))?;
        assert_eq!(closed.state_wire_count(), 1);
        assert_eq!(closed.initial_state().len(), 1);
        assert_eq!(closed.input_wires().len(), 2);
        assert_eq!(closed.output_wires().len(), 2);
        Ok(())
    }
}
