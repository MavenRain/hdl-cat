//! `N`-bit left-shift register.
//!
//! Each cycle, shifts the state left by one and feeds the input
//! bit into bit 0.  Output is the *current* state (pre-shift).

use hdl_cat_bits::Bits;
use hdl_cat_circuit::Obj;
use hdl_cat_error::{Error, Width};
use hdl_cat_ir::{HdlGraphBuilder, Op, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::{machine, Sync};

/// An `N`-bit left-shift register, `N >= 2`.
///
/// State: `Bits<N>`.
/// Input: `bool` (new bit shifted into bit 0).
/// Output: the current `Bits<N>` state before the shift.
pub type ShiftRegisterLeftSync<const N: usize> = Sync<Obj<Bits<N>>, Obj<bool>, Obj<Bits<N>>>;

/// Construct an `N`-bit left-shift register.  Requires `N >= 2`.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`, or
/// [`Error::WidthMismatch`] when `N < 2` (unsupported).
pub fn shift_register_left<const N: usize>() -> Result<ShiftRegisterLeftSync<N>, Error> {
    (N >= 2)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: Width::new(2),
            actual: Width::new(u32::try_from(N).unwrap_or(u32::MAX)),
        })?;

    let width_n = u32::try_from(N).map_err(|_| Error::Overflow {
        width: Width::new(u32::MAX),
    })?;
    let width_n_minus_1 = width_n - 1;

    let (bld, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(width_n));
    let (bld, new_bit) = bld.with_wire(WireTy::Bit);
    let (bld, low_part) = bld.with_wire(WireTy::Bits(width_n_minus_1));
    let (bld, next_state) = bld.with_wire(WireTy::Bits(width_n));

    // low_part = state[0..N-1]  (the low N-1 bits)
    let bld = bld.with_instruction(
        Op::Slice {
            lo: 0,
            hi: width_n_minus_1,
        },
        vec![state],
        low_part,
    )?;
    // next_state = concat(new_bit, low_part)  — new_bit at bit 0, low_part at bits 1..N
    let bld = bld.with_instruction(
        Op::Concat {
            low_width: 1,
            high_width: width_n_minus_1,
        },
        vec![new_bit, low_part],
        next_state,
    )?;

    let graph = bld.build();
    let initial_state: BitSeq = (0..N).map(|_| false).collect();
    Ok(machine::from_raw(
        graph,
        vec![state, new_bit],
        vec![next_state, state],
        initial_state,
        1,
    ))
}

#[cfg(test)]
mod tests {
    use super::shift_register_left;

    #[test]
    fn shift_register_rejects_width_below_two() {
        assert!(shift_register_left::<1>().is_err());
        assert!(shift_register_left::<0>().is_err());
    }

    #[test]
    fn shift_register_has_two_instructions() -> Result<(), hdl_cat_error::Error> {
        let s = shift_register_left::<8>()?;
        assert_eq!(s.graph().instructions().len(), 2);
        Ok(())
    }

    #[test]
    fn shift_register_state_is_n_bits() -> Result<(), hdl_cat_error::Error> {
        let s = shift_register_left::<4>()?;
        assert_eq!(s.initial_state().len(), 4);
        Ok(())
    }
}
