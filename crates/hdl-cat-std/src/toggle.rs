//! 1-bit toggle flip-flop.
//!
//! The output alternates `false → true → false → …` each cycle,
//! starting from `false`.

use hdl_cat_circuit::{CircuitUnit, Obj};
use hdl_cat_error::Error;
use hdl_cat_ir::{HdlGraphBuilder, Op, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::{machine, Sync};

/// A 1-bit toggle flip-flop.
///
/// State: `bool` starting at `false`.
/// Input: [`CircuitUnit`] (no data input).
/// Output: the current state; next cycle's state is its negation.
pub type ToggleSync = Sync<Obj<bool>, CircuitUnit, Obj<bool>>;

/// Construct a 1-bit toggle flip-flop.
///
/// # Errors
///
/// Infallible in practice; propagates any IR builder error.
pub fn toggle_ff() -> Result<ToggleSync, Error> {
    let (bld, state) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
    let (bld, next_state) = bld.with_wire(WireTy::Bit);
    let bld = bld.with_instruction(Op::Not, vec![state], next_state)?;
    let graph = bld.build();
    Ok(machine::from_raw(
        graph,
        vec![state],
        vec![next_state, state],
        BitSeq::from_iter([false]),
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::toggle_ff;

    #[test]
    fn toggle_has_one_state_wire() -> Result<(), hdl_cat_error::Error> {
        let t = toggle_ff()?;
        assert_eq!(t.state_wire_count(), 1);
        Ok(())
    }

    #[test]
    fn toggle_initial_state_is_false() -> Result<(), hdl_cat_error::Error> {
        let t = toggle_ff()?;
        assert_eq!(t.initial_state().len(), 1);
        assert!(!t.initial_state().bit(0));
        Ok(())
    }

    #[test]
    fn toggle_has_one_not_instruction() -> Result<(), hdl_cat_error::Error> {
        let t = toggle_ff()?;
        assert_eq!(t.graph().instructions().len(), 1);
        Ok(())
    }
}
