//! Wired monoidal coherence arrows.
//!
//! `comp_cat_rs::foundation::monoidal` declares `associator`,
//! `left_unitor`, `right_unitor`, and `braid` taking only
//! `A: Into<Self>` — the `Object` bound is not available there,
//! so the [`crate::Circuit`] trait implementations of those
//! methods return empty arrows.
//!
//! This module provides **wired** counterparts with `A: Object`,
//! `B: Object`, `C: Object` bounds.  They build real
//! [`crate::CircuitArrow`]s that implement the coherence
//! isomorphisms at the IR level:
//!
//! - [`wired_associator`], [`wired_left_unitor`],
//!   [`wired_right_unitor`] are pure type-cast identities
//!   (the underlying flat wire layouts match on both sides).
//! - [`wired_braid`] permutes its `A`-block and `B`-block
//!   wires to witness `A ⊗ B ≅ B ⊗ A`.

use comp_cat_rs::foundation::iso::Iso;
use hdl_cat_ir::{HdlGraphBuilder, WireId, WireTy};

use crate::arrow::CircuitArrow;
use crate::category_impl::Circuit;
use crate::object::{CircuitTensor, CircuitUnit, Object};

/// Build a graph whose wires follow the concatenation of the
/// provided layouts, returning the graph together with the
/// wire ids in declaration order.
fn declare_wires(layouts: &[&[WireTy]]) -> (hdl_cat_ir::HdlGraph, Vec<WireId>) {
    let all = layouts
        .iter()
        .flat_map(|layout| layout.iter().cloned());
    let (bld, ids) = all.fold(
        (HdlGraphBuilder::new(), Vec::<WireId>::new()),
        |(b, acc), ty| {
            let (next_b, id) = b.with_wire(ty);
            let next_acc = acc
                .into_iter()
                .chain(core::iter::once(id))
                .collect();
            (next_b, next_acc)
        },
    );
    (bld.build(), ids)
}

/// Construct a pass-through arrow between two object types whose
/// flat wire layouts are pointwise identical.  The caller asserts
/// compatibility; no runtime check is performed.
fn pass_through<Src, Tgt>(layout: &[WireTy]) -> CircuitArrow<Src, Tgt> {
    let (graph, ids) = declare_wires(&[layout]);
    CircuitArrow::from_raw_parts(graph, ids.clone(), ids)
}

/// `(A ⊗ B) ⊗ C` as a single type alias, for the associator's
/// left-associated side.
pub type LeftAssoc<A, B, C> = CircuitTensor<CircuitTensor<A, B>, C>;

/// `A ⊗ (B ⊗ C)` as a single type alias, for the associator's
/// right-associated side.
pub type RightAssoc<A, B, C> = CircuitTensor<A, CircuitTensor<B, C>>;

/// Wired associator: `(A ⊗ B) ⊗ C ≅ A ⊗ (B ⊗ C)`.
///
/// The flat wire layouts on both sides are equal
/// (`A ++ B ++ C`), so this returns a pass-through identity
/// with no IR instructions.
pub fn wired_associator<A, B, C>() -> Iso<Circuit, LeftAssoc<A, B, C>, RightAssoc<A, B, C>>
where
    A: Object,
    B: Object,
    C: Object,
{
    let layout: Vec<WireTy> = A::wire_layout()
        .into_iter()
        .chain(B::wire_layout())
        .chain(C::wire_layout())
        .collect();
    let forward = pass_through::<LeftAssoc<A, B, C>, RightAssoc<A, B, C>>(&layout);
    let backward = pass_through::<RightAssoc<A, B, C>, LeftAssoc<A, B, C>>(&layout);
    Iso::new(forward, backward)
}

/// Wired left unitor: `I ⊗ A ≅ A`.
///
/// `CircuitUnit::wire_layout()` is empty, so the flat layouts
/// are both `A::wire_layout()`.  Pass-through identity.
pub fn wired_left_unitor<A>() -> Iso<Circuit, CircuitTensor<CircuitUnit, A>, A>
where
    A: Object,
{
    let layout = A::wire_layout();
    let forward = pass_through::<CircuitTensor<CircuitUnit, A>, A>(&layout);
    let backward = pass_through::<A, CircuitTensor<CircuitUnit, A>>(&layout);
    Iso::new(forward, backward)
}

/// Wired right unitor: `A ⊗ I ≅ A`.
///
/// Pass-through identity, symmetrically to [`wired_left_unitor`].
pub fn wired_right_unitor<A>() -> Iso<Circuit, CircuitTensor<A, CircuitUnit>, A>
where
    A: Object,
{
    let layout = A::wire_layout();
    let forward = pass_through::<CircuitTensor<A, CircuitUnit>, A>(&layout);
    let backward = pass_through::<A, CircuitTensor<A, CircuitUnit>>(&layout);
    Iso::new(forward, backward)
}

/// Wired braid: `A ⊗ B ≅ B ⊗ A`.
///
/// Permutes the two blocks of wires.  No IR instructions — the
/// wire lists on the input and output sides are the same set of
/// wire ids in swapped order.
pub fn wired_braid<A, B>() -> Iso<Circuit, CircuitTensor<A, B>, CircuitTensor<B, A>>
where
    A: Object,
    B: Object,
{
    let a_layout = A::wire_layout();
    let b_layout = B::wire_layout();
    let a_count = a_layout.len();

    let (forward_graph, ids_ab) = declare_wires(&[&a_layout, &b_layout]);
    let forward_inputs: Vec<WireId> = ids_ab.clone();
    let forward_outputs: Vec<WireId> = ids_ab[a_count..]
        .iter()
        .copied()
        .chain(ids_ab[..a_count].iter().copied())
        .collect();
    let forward = CircuitArrow::from_raw_parts(forward_graph, forward_inputs, forward_outputs);

    // Backward braid: declared wires in B-then-A order, permuted
    // back to A-then-B.
    let b_count = b_layout.len();
    let (backward_graph, ids_ba) = declare_wires(&[&b_layout, &a_layout]);
    let backward_inputs: Vec<WireId> = ids_ba.clone();
    let backward_outputs: Vec<WireId> = ids_ba[b_count..]
        .iter()
        .copied()
        .chain(ids_ba[..b_count].iter().copied())
        .collect();
    let backward = CircuitArrow::from_raw_parts(backward_graph, backward_inputs, backward_outputs);

    Iso::new(forward, backward)
}

#[cfg(test)]
mod tests {
    use super::{wired_associator, wired_braid, wired_left_unitor, wired_right_unitor};
    use crate::object::{CircuitTensor, Obj, Object};
    use hdl_cat_bits::Bits;

    type A4 = Obj<Bits<4>>;
    type A8 = Obj<Bits<8>>;
    type AB = CircuitTensor<Obj<Bits<4>>, Obj<Bits<8>>>;

    #[test]
    fn associator_has_no_instructions() {
        let iso = wired_associator::<A4, A8, A4>();
        assert_eq!(iso.forward().graph().instructions().len(), 0);
        assert_eq!(iso.backward().graph().instructions().len(), 0);
    }

    #[test]
    fn associator_declares_all_wires() {
        let iso = wired_associator::<A4, A8, A4>();
        // Three objects, each Obj<Bits<_>> has one wire → 3 wires.
        assert_eq!(iso.forward().graph().wires().len(), 3);
        assert_eq!(iso.forward().inputs().len(), 3);
        assert_eq!(iso.forward().outputs().len(), 3);
    }

    #[test]
    fn associator_input_equals_output() {
        let iso = wired_associator::<A4, A4, A4>();
        assert_eq!(iso.forward().inputs(), iso.forward().outputs());
    }

    #[test]
    fn left_unitor_is_passthrough_on_a() {
        let iso = wired_left_unitor::<A8>();
        assert_eq!(iso.forward().inputs().len(), <A8 as Object>::WIDTH / 8);
        assert_eq!(iso.forward().inputs(), iso.forward().outputs());
    }

    #[test]
    fn right_unitor_is_passthrough_on_a() {
        let iso = wired_right_unitor::<A8>();
        assert_eq!(iso.forward().inputs().len(), 1);
        assert_eq!(iso.forward().inputs(), iso.forward().outputs());
    }

    #[test]
    fn braid_swaps_single_wire_halves() {
        let iso = wired_braid::<A4, A8>();
        let inputs = iso.forward().inputs();
        let outputs = iso.forward().outputs();
        // Both sides have 2 wires: [A-wire, B-wire].
        assert_eq!(inputs.len(), 2);
        assert_eq!(outputs.len(), 2);
        // Output is swapped: outputs[0] should equal inputs[1] (B wire)
        // and outputs[1] should equal inputs[0] (A wire).
        assert_eq!(outputs[0], inputs[1]);
        assert_eq!(outputs[1], inputs[0]);
    }

    #[test]
    fn braid_backward_swaps_opposite_direction() {
        let iso = wired_braid::<A4, A8>();
        let inputs = iso.backward().inputs();
        let outputs = iso.backward().outputs();
        // Backward braid's inputs are in B-then-A order
        // (that is, inputs for Tensor<B, A>).
        assert_eq!(inputs.len(), 2);
        assert_eq!(outputs.len(), 2);
        // Output restores A-then-B: outputs[0] is the 2nd input, outputs[1] the 1st.
        assert_eq!(outputs[0], inputs[1]);
        assert_eq!(outputs[1], inputs[0]);
    }

    #[test]
    fn braid_on_tensor_objects_swaps_blocks() {
        // For CircuitTensor<Obj<Bits<4>>, Obj<Bits<8>>> vs Obj<Bits<4>>:
        // A = CircuitTensor<Obj<Bits<4>>, Obj<Bits<8>>> has 2 wires
        // B = Obj<Bits<4>> has 1 wire
        // inputs  = [A0, A1, B0]
        // outputs = [B0, A0, A1]
        let iso = wired_braid::<AB, A4>();
        let inputs = iso.forward().inputs();
        let outputs = iso.forward().outputs();
        assert_eq!(inputs.len(), 3);
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], inputs[2]); // first output = B's wire
        assert_eq!(outputs[1], inputs[0]); // then A's wires
        assert_eq!(outputs[2], inputs[1]);
    }
}
