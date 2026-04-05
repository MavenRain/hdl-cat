//! `N`-bit free-running counter.
//!
//! Builds a [`hdl_cat_sync::Sync`] machine whose state is a
//! `Bits<N>` counter that increments by 1 each cycle.  The
//! machine has no data input and outputs the current count.

use hdl_cat_bits::Bits;
use hdl_cat_circuit::{CircuitUnit, Obj};
use hdl_cat_error::{Error, Width};
use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::{machine, Sync};

/// A free-running `N`-bit counter.
///
/// State: `Bits<N>` starting at 0.
/// Input: nothing ([`CircuitUnit`]).
/// Output: the current count (before increment).
pub type CounterSync<const N: usize> = Sync<Obj<Bits<N>>, CircuitUnit, Obj<Bits<N>>>;

/// Construct a free-running `N`-bit counter.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// use hdl_cat_std::counter;
/// let c = counter::<8>()?;
/// assert_eq!(c.state_wire_count(), 1);
/// assert_eq!(c.initial_state().len(), 8);
/// # Ok(()) }
/// ```
pub fn counter<const N: usize>() -> Result<CounterSync<N>, Error> {
    let width = u32::try_from(N).map_err(|_| Error::Overflow {
        width: Width::new(u32::MAX),
    })?;
    let ty = WireTy::Bits(width);

    let (bld, state) = HdlGraphBuilder::new().with_wire(ty.clone());
    let (bld, one) = bld.with_wire(ty.clone());
    let (bld, next_state) = bld.with_wire(ty.clone());

    let bld = bld.with_instruction(
        Op::Const {
            bits: one_bitseq(N),
            ty: ty.clone(),
        },
        vec![],
        one,
    )?;
    let bld = bld.with_instruction(Op::Bin(BinOp::Add), vec![state, one], next_state)?;

    let graph = bld.build();
    let initial_state = zero_bitseq(N);

    // input_wires: [state]  (no data input; CircuitUnit contributes 0 wires)
    // output_wires: [next_state, state]  (state itself serves as the data output;
    //   it is an input wire so its value is taken from the incoming state each cycle)
    Ok(machine::from_raw(
        graph,
        vec![state],
        vec![next_state, state],
        initial_state,
        1,
    ))
}

fn zero_bitseq(n: usize) -> BitSeq {
    (0..n).map(|_| false).collect()
}

fn one_bitseq(n: usize) -> BitSeq {
    (0..n).map(|i| i == 0).collect()
}

#[cfg(test)]
mod tests {
    use super::counter;

    #[test]
    fn counter_has_one_state_wire() -> Result<(), hdl_cat_error::Error> {
        let c = counter::<8>()?;
        assert_eq!(c.state_wire_count(), 1);
        Ok(())
    }

    #[test]
    fn counter_initial_state_is_zero() -> Result<(), hdl_cat_error::Error> {
        let c = counter::<8>()?;
        assert_eq!(c.initial_state().len(), 8);
        assert!(c.initial_state().as_slice().iter().all(|b| !*b));
        Ok(())
    }

    #[test]
    fn counter_has_two_instructions() -> Result<(), hdl_cat_error::Error> {
        let c = counter::<4>()?;
        // Const + Add = 2 instructions.
        assert_eq!(c.graph().instructions().len(), 2);
        Ok(())
    }
}
