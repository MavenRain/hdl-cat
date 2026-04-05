//! Half- and full-adder circuits.

use hdl_cat_circuit::{CircuitArrow, CircuitTensor, Obj};
use hdl_cat_error::Error;
use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};

/// A half-adder arrow: `(bool ⊗ bool) -> (bool ⊗ bool)`.
/// Output is `(sum, carry)` packed as a tensor.
pub type HalfAdderArrow = CircuitArrow<
    CircuitTensor<Obj<bool>, Obj<bool>>,
    CircuitTensor<Obj<bool>, Obj<bool>>,
>;

/// A full-adder arrow:
/// `((bool ⊗ bool) ⊗ bool) -> (bool ⊗ bool)`.
/// Inputs: `((a, b), carry_in)`.  Outputs: `(sum, carry_out)`.
pub type FullAdderArrow = CircuitArrow<
    CircuitTensor<CircuitTensor<Obj<bool>, Obj<bool>>, Obj<bool>>,
    CircuitTensor<Obj<bool>, Obj<bool>>,
>;

/// Construct a single-bit half-adder.
///
/// Two inputs (`a`, `b`) drive both an XOR gate (the sum) and an
/// AND gate (the carry), wired directly at the IR level.
///
/// # Errors
///
/// Infallible in practice; [`Error`] is returned only if the
/// IR builder rejects an instruction.
pub fn half_adder() -> Result<HalfAdderArrow, Error> {
    let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
    let (bld, b) = bld.with_wire(WireTy::Bit);
    let (bld, sum) = bld.with_wire(WireTy::Bit);
    let (bld, carry) = bld.with_wire(WireTy::Bit);
    let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![a, b], sum)?;
    let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![a, b], carry)?;
    Ok(CircuitArrow::from_raw_parts(
        bld.build(),
        vec![a, b],
        vec![sum, carry],
    ))
}

/// Construct a single-bit full-adder.
///
/// Three inputs (`a`, `b`, `cin`) produce `sum = a ^ b ^ cin` and
/// `cout = (a & b) | (cin & (a ^ b))`.
///
/// # Errors
///
/// Infallible in practice; [`Error`] is returned only if the
/// IR builder rejects an instruction.
pub fn full_adder() -> Result<FullAdderArrow, Error> {
    let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
    let (bld, b) = bld.with_wire(WireTy::Bit);
    let (bld, cin) = bld.with_wire(WireTy::Bit);
    let (bld, ab) = bld.with_wire(WireTy::Bit);     // a ^ b
    let (bld, ab_and) = bld.with_wire(WireTy::Bit); // a & b
    let (bld, c_and) = bld.with_wire(WireTy::Bit);  // cin & (a ^ b)
    let (bld, sum) = bld.with_wire(WireTy::Bit);    // ab ^ cin
    let (bld, cout) = bld.with_wire(WireTy::Bit);   // ab_and | c_and
    let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![a, b], ab)?;
    let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![a, b], ab_and)?;
    let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![cin, ab], c_and)?;
    let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![ab, cin], sum)?;
    let bld = bld.with_instruction(Op::Bin(BinOp::Or), vec![ab_and, c_and], cout)?;
    Ok(CircuitArrow::from_raw_parts(
        bld.build(),
        vec![a, b, cin],
        vec![sum, cout],
    ))
}

#[cfg(test)]
mod tests {
    use super::{full_adder, half_adder};

    #[test]
    fn half_adder_builds() -> Result<(), hdl_cat_error::Error> {
        let ha = half_adder()?;
        assert_eq!(ha.inputs().len(), 2);
        assert_eq!(ha.outputs().len(), 2);
        assert_eq!(ha.graph().instructions().len(), 2);
        Ok(())
    }

    #[test]
    fn half_adder_sum_and_carry_exist() -> Result<(), hdl_cat_error::Error> {
        let ha = half_adder()?;
        assert_eq!(ha.graph().wires().len(), 4);
        Ok(())
    }

    #[test]
    fn full_adder_builds() -> Result<(), hdl_cat_error::Error> {
        let fa = full_adder()?;
        assert_eq!(fa.inputs().len(), 3);
        assert_eq!(fa.outputs().len(), 2);
        assert_eq!(fa.graph().instructions().len(), 5);
        Ok(())
    }
}
