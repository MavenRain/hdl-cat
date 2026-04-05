//! The [`Sync<S, I, O>`] machine type.

use core::marker::PhantomData;

use hdl_cat_circuit::{CircuitArrow, Object};
use hdl_cat_error::{Error, Width};
use hdl_cat_ir::{HdlGraph, HdlGraphBuilder, WireId};
use hdl_cat_kind::BitSeq;

/// Phantom type marker for (state, input, output) trios in
/// [`Sync`].  Wrapping as `fn() -> T` makes the marker `Send + Sync`
/// regardless of the wrapped types' auto-traits.
type Tag<S, I, O> = PhantomData<fn() -> (S, I, O)>;

/// A Mealy machine: a combinational IR graph with one cycle of
/// looped-back state.
///
/// `Sync<S, I, O>` stores:
///
/// - a combinational [`HdlGraph`]
/// - the wire ids that constitute its `(state ⊗ input)` inputs
///   (first `S::WIDTH` wires are state; remainder are input)
/// - the wire ids that constitute its `(state ⊗ output)` outputs
///   (first `S::WIDTH` wires are next-state; remainder are output)
/// - an initial-state bit pattern for cycle 0
///
/// Each simulated cycle, the machine is driven by interpreting
/// the IR graph with concrete bit values on the input wires,
/// producing concrete values on the output wires.  The
/// `next_state` slice becomes the following cycle's `state`.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// use hdl_cat_sync::Sync;
/// use hdl_cat_circuit::{gates, CircuitUnit, Obj};
/// use hdl_cat_bits::Bits;
///
/// // Stateless 4-bit inverter lifted to a Sync machine.
/// let inv = gates::not_bits::<4>()?;
/// let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
/// assert_eq!(m.initial_state().len(), 0);
/// assert_eq!(m.input_wires().len(), 1);
/// assert_eq!(m.output_wires().len(), 1);
/// # Ok(()) }
/// ```
#[must_use]
pub struct Sync<S, I, O> {
    arrow_ir: HdlGraph,
    input_wires: Vec<WireId>,
    output_wires: Vec<WireId>,
    initial_state: BitSeq,
    state_wire_count: usize,
    _phantom: Tag<S, I, O>,
}

impl<S, I, O> Sync<S, I, O> {
    /// The combinational IR graph driving this machine.
    pub fn graph(&self) -> &HdlGraph {
        &self.arrow_ir
    }

    /// Input wires: `state_wires ++ input_wires` in that order.
    #[must_use]
    pub fn input_wires(&self) -> &[WireId] {
        &self.input_wires
    }

    /// Output wires: `next_state_wires ++ output_wires` in that
    /// order.
    #[must_use]
    pub fn output_wires(&self) -> &[WireId] {
        &self.output_wires
    }

    /// The initial-state bit pattern (LSB-first).
    pub fn initial_state(&self) -> &BitSeq {
        &self.initial_state
    }

    /// The number of state wires.  The first `state_wire_count`
    /// entries of both [`Self::input_wires`] and
    /// [`Self::output_wires`] are the state-carrying wires; the
    /// remainder are the input/output wires respectively.
    #[must_use]
    pub fn state_wire_count(&self) -> usize {
        self.state_wire_count
    }

    /// Consume the machine and return its owned parts.
    pub fn into_parts(self) -> (HdlGraph, Vec<WireId>, Vec<WireId>, BitSeq, usize) {
        (
            self.arrow_ir,
            self.input_wires,
            self.output_wires,
            self.initial_state,
            self.state_wire_count,
        )
    }
}

impl<S, I, O> Sync<S, I, O>
where
    S: Object,
    I: Object,
    O: Object,
{
    /// Assemble a `Sync` from an explicit combinational arrow and
    /// initial state.
    ///
    /// The arrow's inputs must be interpreted as `state ⊗ input`
    /// (state wires first, then input wires).  Its outputs must
    /// be interpreted as `next_state ⊗ output`.  The initial
    /// state's bit length must equal `S::WIDTH`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::WidthMismatch`] when the initial state's
    /// bit length does not equal `S::WIDTH`, or the arrow's
    /// wires do not match the expected layout sizes.
    pub fn from_arrow<AI, AO>(
        arrow: CircuitArrow<AI, AO>,
        initial_state: BitSeq,
    ) -> Result<Self, Error> {
        // Compare in terms of wire counts, derived from each
        // object's layout.  `Object::WIDTH` is a *bit* count;
        // `Object::wire_layout().len()` is a *wire* count, which
        // matches `arrow.inputs()`/`arrow.outputs()`.
        let state_wires = S::wire_layout().len();
        let input_wires = I::wire_layout().len();
        let output_wires = O::wire_layout().len();
        let expected_inputs = state_wires + input_wires;
        let expected_outputs = state_wires + output_wires;

        (arrow.inputs().len() == expected_inputs && arrow.outputs().len() == expected_outputs)
            .then_some(())
            .ok_or_else(|| Error::WidthMismatch {
                expected: Width::new(width_u32(expected_inputs + expected_outputs)),
                actual: Width::new(width_u32(arrow.inputs().len() + arrow.outputs().len())),
            })?;

        (initial_state.len() == S::WIDTH)
            .then_some(())
            .ok_or_else(|| Error::WidthMismatch {
                expected: Width::new(width_u32(S::WIDTH)),
                actual: Width::new(width_u32(initial_state.len())),
            })?;

        let (graph, inputs, outputs) = arrow.into_raw_parts();
        Ok(Self {
            arrow_ir: graph,
            input_wires: inputs,
            output_wires: outputs,
            initial_state,
            state_wire_count: state_wires,
            _phantom: PhantomData,
        })
    }
}

impl<I, O> Sync<hdl_cat_circuit::CircuitUnit, I, O>
where
    I: Object,
    O: Object,
{
    /// Lift a stateless combinational arrow into a
    /// `Sync<CircuitUnit, I, O>` machine.
    ///
    /// Because `CircuitUnit::WIDTH` is zero, the underlying graph
    /// has the same wires as the combinational arrow — there is
    /// no state layer to prepend.
    pub fn lift_comb(arrow: CircuitArrow<I, O>) -> Self {
        let (graph, inputs, outputs) = arrow.into_raw_parts();
        Self {
            arrow_ir: graph,
            input_wires: inputs,
            output_wires: outputs,
            initial_state: BitSeq::new(),
            state_wire_count: 0,
            _phantom: PhantomData,
        }
    }
}

/// Assemble a raw `Sync` from IR pieces.  Useful for
/// constructing machines from hand-built IR graphs.
pub fn from_raw<S, I, O>(
    graph: HdlGraph,
    input_wires: Vec<WireId>,
    output_wires: Vec<WireId>,
    initial_state: BitSeq,
    state_wire_count: usize,
) -> Sync<S, I, O> {
    Sync {
        arrow_ir: graph,
        input_wires,
        output_wires,
        initial_state,
        state_wire_count,
        _phantom: PhantomData,
    }
}

/// An empty `Sync` machine with no wires or instructions.
pub fn empty_sync<S, I, O>() -> Sync<S, I, O> {
    Sync {
        arrow_ir: HdlGraphBuilder::new().build(),
        input_wires: Vec::new(),
        output_wires: Vec::new(),
        initial_state: BitSeq::new(),
        state_wire_count: 0,
        _phantom: PhantomData,
    }
}

fn width_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{empty_sync, from_raw, Sync};
    use hdl_cat_bits::Bits;
    use hdl_cat_circuit::{gates, CircuitUnit, Obj};
    use hdl_cat_ir::HdlGraphBuilder;
    use hdl_cat_kind::BitSeq;

    #[test]
    fn lift_comb_preserves_arrow_shape() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        assert_eq!(m.initial_state().len(), 0);
        assert_eq!(m.input_wires().len(), 1);
        assert_eq!(m.output_wires().len(), 1);
        assert_eq!(m.graph().instructions().len(), 1);
        Ok(())
    }

    #[test]
    fn empty_sync_has_empty_graph() {
        let m: Sync<CircuitUnit, Obj<bool>, Obj<bool>> = empty_sync();
        assert_eq!(m.graph().wires().len(), 0);
        assert_eq!(m.graph().instructions().len(), 0);
        assert_eq!(m.input_wires().len(), 0);
        assert_eq!(m.output_wires().len(), 0);
    }

    #[test]
    fn from_raw_stores_provided_data() {
        let g = HdlGraphBuilder::new().build();
        let m: Sync<CircuitUnit, Obj<bool>, Obj<bool>> =
            from_raw(g, Vec::new(), Vec::new(), BitSeq::new(), 0);
        assert_eq!(m.initial_state().len(), 0);
        assert_eq!(m.state_wire_count(), 0);
    }

    #[test]
    fn from_arrow_rejects_wrong_state_width() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        // CircuitUnit has width 0, so a non-empty initial state should fail.
        let result = Sync::<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>>::from_arrow(
            inv,
            BitSeq::from_iter([true, false]),
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn from_arrow_accepts_matching_widths() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m = Sync::<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>>::from_arrow(
            inv,
            BitSeq::new(),
        )?;
        assert_eq!(m.initial_state().len(), 0);
        // inputs = 0 (state) + 1 wire (Bits<4>) = 1
        assert_eq!(m.input_wires().len(), 1);
        assert_eq!(m.output_wires().len(), 1);
        Ok(())
    }
}
