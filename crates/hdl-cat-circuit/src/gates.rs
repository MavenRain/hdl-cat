//! Primitive gate arrow constructors.
//!
//! Each function returns a [`crate::CircuitArrow`] embodying one
//! hardware gate.  Compose them via `Category::comp` and
//! `MonoidalCategory::tensor_map` to build larger circuits.

use hdl_cat_bits::Bits;
use hdl_cat_error::Error;
use hdl_cat_ir::{BinOp, Op, WireTy};

use crate::arrow::{primitive_arrow, CircuitArrow};
use crate::object::{CircuitTensor, Obj};

/// A binary `Bits<N>` gate arrow: `(Bits<N> ⊗ Bits<N>) -> Bits<N>`.
pub type BinGate<const N: usize> =
    CircuitArrow<CircuitTensor<Obj<Bits<N>>, Obj<Bits<N>>>, Obj<Bits<N>>>;

/// A comparator arrow: `(Bits<N> ⊗ Bits<N>) -> bool`.
pub type CmpGate<const N: usize> =
    CircuitArrow<CircuitTensor<Obj<Bits<N>>, Obj<Bits<N>>>, Obj<bool>>;

/// A binary single-bit gate arrow: `(bool ⊗ bool) -> bool`.
pub type BitBinGate = CircuitArrow<CircuitTensor<Obj<bool>, Obj<bool>>, Obj<bool>>;

fn bits_width<const N: usize>() -> Result<u32, Error> {
    u32::try_from(N).map_err(|_| Error::Overflow {
        width: hdl_cat_error::Width::new(u32::MAX),
    })
}

/// Bitwise NOT on a `Bits<N>` input.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn not_bits<const N: usize>() -> Result<CircuitArrow<Obj<Bits<N>>, Obj<Bits<N>>>, Error> {
    let w = bits_width::<N>()?;
    primitive_arrow(vec![WireTy::Bits(w)], Op::Not, WireTy::Bits(w))
}

fn binary_bits<const N: usize>(op: BinOp) -> Result<BinGate<N>, Error> {
    let w = bits_width::<N>()?;
    primitive_arrow(
        vec![WireTy::Bits(w), WireTy::Bits(w)],
        Op::Bin(op),
        WireTy::Bits(w),
    )
}

/// Bitwise AND of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn and_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::And)
}

/// Bitwise OR of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn or_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::Or)
}

/// Bitwise XOR of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn xor_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::Xor)
}

/// Unsigned wrap-around addition of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn add_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::Add)
}

/// Unsigned wrap-around subtraction of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn sub_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::Sub)
}

/// Unsigned wrap-around multiplication of two `Bits<N>` inputs.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn mul_bits<const N: usize>() -> Result<BinGate<N>, Error> {
    binary_bits::<N>(BinOp::Mul)
}

/// Equality comparator on two `Bits<N>` inputs; output is one bit.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn eq_bits<const N: usize>() -> Result<CmpGate<N>, Error> {
    let w = bits_width::<N>()?;
    primitive_arrow(
        vec![WireTy::Bits(w), WireTy::Bits(w)],
        Op::Bin(BinOp::Eq),
        WireTy::Bit,
    )
}

/// Less-than comparator on two `Bits<N>` inputs; output is one bit.
///
/// # Errors
///
/// Returns [`Error::Overflow`] when `N` exceeds `u32::MAX`.
pub fn lt_bits<const N: usize>() -> Result<CmpGate<N>, Error> {
    let w = bits_width::<N>()?;
    primitive_arrow(
        vec![WireTy::Bits(w), WireTy::Bits(w)],
        Op::Bin(BinOp::Lt),
        WireTy::Bit,
    )
}

/// Bitwise NOT on a single `bool`.
///
/// # Errors
///
/// Infallible in practice; signature matches the rest of the module.
pub fn not_bit() -> Result<CircuitArrow<Obj<bool>, Obj<bool>>, Error> {
    primitive_arrow(vec![WireTy::Bit], Op::Not, WireTy::Bit)
}

/// Bitwise AND on two `bool` inputs.
///
/// # Errors
///
/// Infallible in practice; signature matches the rest of the module.
pub fn and_bit() -> Result<BitBinGate, Error> {
    primitive_arrow(
        vec![WireTy::Bit, WireTy::Bit],
        Op::Bin(BinOp::And),
        WireTy::Bit,
    )
}

/// Bitwise OR on two `bool` inputs.
///
/// # Errors
///
/// Infallible in practice; signature matches the rest of the module.
pub fn or_bit() -> Result<BitBinGate, Error> {
    primitive_arrow(
        vec![WireTy::Bit, WireTy::Bit],
        Op::Bin(BinOp::Or),
        WireTy::Bit,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        add_bits, and_bit, and_bits, eq_bits, lt_bits, mul_bits, not_bit, not_bits, or_bit,
        or_bits, sub_bits, xor_bits,
    };

    #[test]
    fn not_bits_builds() -> Result<(), hdl_cat_error::Error> {
        let n = not_bits::<8>()?;
        assert_eq!(n.inputs().len(), 1);
        assert_eq!(n.outputs().len(), 1);
        assert_eq!(n.graph().instructions().len(), 1);
        Ok(())
    }

    #[test]
    fn and_bits_builds() -> Result<(), hdl_cat_error::Error> {
        let a = and_bits::<4>()?;
        assert_eq!(a.inputs().len(), 2);
        assert_eq!(a.outputs().len(), 1);
        Ok(())
    }

    #[test]
    fn all_binary_gates_build() -> Result<(), hdl_cat_error::Error> {
        let _ = and_bits::<4>()?;
        let _ = or_bits::<4>()?;
        let _ = xor_bits::<4>()?;
        let _ = add_bits::<4>()?;
        let _ = sub_bits::<4>()?;
        let _ = mul_bits::<4>()?;
        Ok(())
    }

    #[test]
    fn comparators_produce_bool_output() -> Result<(), hdl_cat_error::Error> {
        let eq = eq_bits::<8>()?;
        let lt = lt_bits::<8>()?;
        // Both have two inputs and one output; the output is 1-bit.
        assert_eq!(eq.outputs().len(), 1);
        assert_eq!(lt.outputs().len(), 1);
        Ok(())
    }

    #[test]
    fn bit_gates_build() -> Result<(), hdl_cat_error::Error> {
        let _ = not_bit()?;
        let _ = and_bit()?;
        let _ = or_bit()?;
        Ok(())
    }
}
