//! `N`-bit free-running down-counter.
//!
//! Starts at `2^N - 1` and decrements by 1 each cycle, wrapping
//! back to `2^N - 1` after zero.

use hdl_cat_bits::Bits;
use hdl_cat_circuit::{CircuitUnit, Obj};
use hdl_cat_error::{Error, Width};
use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::{machine, Sync};

/// A free-running `N`-bit down-counter.
pub type DownCounterSync<const N: usize> = Sync<Obj<Bits<N>>, CircuitUnit, Obj<Bits<N>>>;

/// Construct a free-running `N`-bit down-counter.
///
/// Initial state is all-ones (`2^N - 1`); each cycle the state
/// decrements by 1 and wraps at zero.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn down_counter<const N: usize>() -> Result<DownCounterSync<N>, Error> {
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
    let bld = bld.with_instruction(Op::Bin(BinOp::Sub), vec![state, one], next_state)?;
    let graph = bld.build();

    // Initial state = all ones.
    let initial_state: BitSeq = (0..N).map(|_| true).collect();
    Ok(machine::from_raw(
        graph,
        vec![state],
        vec![next_state, state],
        initial_state,
        1,
    ))
}

fn one_bitseq(n: usize) -> BitSeq {
    (0..n).map(|i| i == 0).collect()
}

#[cfg(test)]
mod tests {
    use super::down_counter;

    #[test]
    fn down_counter_initial_state_is_all_ones() -> Result<(), hdl_cat_error::Error> {
        let d = down_counter::<4>()?;
        assert_eq!(d.initial_state().len(), 4);
        assert!(d.initial_state().as_slice().iter().all(|b| *b));
        Ok(())
    }

    #[test]
    fn down_counter_has_const_and_sub() -> Result<(), hdl_cat_error::Error> {
        let d = down_counter::<8>()?;
        assert_eq!(d.graph().instructions().len(), 2);
        Ok(())
    }
}
