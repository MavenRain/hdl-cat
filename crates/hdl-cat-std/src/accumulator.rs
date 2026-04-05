//! `N`-bit running accumulator.
//!
//! Adds the input to a running total each cycle.  The output is
//! the *new* accumulator value (post-increment) — i.e. the sum
//! of all inputs through cycle `k`.

use hdl_cat_bits::Bits;
use hdl_cat_circuit::Obj;
use hdl_cat_error::{Error, Width};
use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::{machine, Sync};

/// An `N`-bit running accumulator.
///
/// State: `Bits<N>` starting at 0.
/// Input: `Bits<N>` (value to add).
/// Output: accumulator value AFTER adding the cycle's input.
pub type AccumulatorSync<const N: usize> = Sync<Obj<Bits<N>>, Obj<Bits<N>>, Obj<Bits<N>>>;

/// Construct an `N`-bit accumulator.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn accumulator<const N: usize>() -> Result<AccumulatorSync<N>, Error> {
    let width = u32::try_from(N).map_err(|_| Error::Overflow {
        width: Width::new(u32::MAX),
    })?;
    let ty = WireTy::Bits(width);

    let (bld, state) = HdlGraphBuilder::new().with_wire(ty.clone());
    let (bld, input) = bld.with_wire(ty.clone());
    let (bld, next_state) = bld.with_wire(ty);
    let bld = bld.with_instruction(Op::Bin(BinOp::Add), vec![state, input], next_state)?;
    let graph = bld.build();

    let initial_state: BitSeq = (0..N).map(|_| false).collect();
    Ok(machine::from_raw(
        graph,
        vec![state, input],
        vec![next_state, next_state],
        initial_state,
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::accumulator;

    #[test]
    fn accumulator_has_one_state_and_one_input_wire() -> Result<(), hdl_cat_error::Error> {
        let acc = accumulator::<8>()?;
        assert_eq!(acc.state_wire_count(), 1);
        assert_eq!(acc.input_wires().len(), 2);  // state + input
        assert_eq!(acc.output_wires().len(), 2); // next_state + output
        Ok(())
    }

    #[test]
    fn accumulator_initial_state_is_zero() -> Result<(), hdl_cat_error::Error> {
        let acc = accumulator::<8>()?;
        assert_eq!(acc.initial_state().len(), 8);
        assert!(acc.initial_state().as_slice().iter().all(|b| !*b));
        Ok(())
    }

    #[test]
    fn accumulator_has_one_add_instruction() -> Result<(), hdl_cat_error::Error> {
        let acc = accumulator::<4>()?;
        assert_eq!(acc.graph().instructions().len(), 1);
        Ok(())
    }
}
