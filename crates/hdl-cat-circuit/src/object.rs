//! Object types in the circuit category.

use core::marker::PhantomData;

use hdl_cat_bits::{Bits, SignedBits};
use hdl_cat_ir::WireTy;
use hdl_cat_kind::Hw;

use crate::category_impl::Circuit;

/// A [`Hw`] type that maps to a single IR wire.
///
/// The [`Obj<T>`] constructor is parameterized by `T: Scalar`,
/// so only primitive (single-wire) types are admissible as
/// flat circuit objects.  Aggregates must be expressed
/// structurally via [`CircuitTensor`].
pub trait Scalar: Hw {
    /// The IR wire type corresponding to this scalar.
    #[must_use]
    fn wire_ty() -> WireTy;
}

impl Scalar for bool {
    fn wire_ty() -> WireTy {
        WireTy::Bit
    }
}

impl<const N: usize> Scalar for Bits<N> {
    fn wire_ty() -> WireTy {
        WireTy::Bits(u32::try_from(N).unwrap_or(u32::MAX))
    }
}

impl<const N: usize> Scalar for SignedBits<N> {
    fn wire_ty() -> WireTy {
        WireTy::Signed(u32::try_from(N).unwrap_or(u32::MAX))
    }
}

/// A trait witnessing that a type is an object in the [`Circuit`]
/// category, carrying a compile-time total bit width and a runtime
/// wire-type layout.
pub trait Object: Sized + Into<Circuit> {
    /// The total bit width of the wire bundle this object represents.
    const WIDTH: usize;

    /// The list of primitive wire types this bundle flattens to.
    fn wire_layout() -> Vec<WireTy>;
}

/// A wire bundle carrying a single [`Hw`] value of type `T`.
#[derive(Debug, Default)]
#[must_use]
pub struct Obj<T>(PhantomData<T>);

impl<T> Obj<T> {
    /// Construct the phantom object marker.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Clone for Obj<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Obj<T> {}

impl<T> From<Obj<T>> for Circuit {
    fn from(_: Obj<T>) -> Self {
        Self
    }
}

impl<T: Scalar> Object for Obj<T> {
    const WIDTH: usize = T::WIDTH;

    fn wire_layout() -> Vec<WireTy> {
        vec![T::wire_ty()]
    }
}

/// The tensor product of two objects: the parallel composition
/// of wire bundles.
#[derive(Debug, Default)]
#[must_use]
pub struct CircuitTensor<A, B>(PhantomData<(A, B)>);

impl<A, B> CircuitTensor<A, B> {
    /// Construct the phantom tensor marker.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<A, B> Clone for CircuitTensor<A, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A, B> Copy for CircuitTensor<A, B> {}

impl<A, B> From<CircuitTensor<A, B>> for Circuit {
    fn from(_: CircuitTensor<A, B>) -> Self {
        Self
    }
}

impl<A: Object, B: Object> Object for CircuitTensor<A, B> {
    const WIDTH: usize = A::WIDTH + B::WIDTH;

    fn wire_layout() -> Vec<WireTy> {
        A::wire_layout()
            .into_iter()
            .chain(B::wire_layout())
            .collect()
    }
}

/// The unit object: the empty wire bundle.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct CircuitUnit;

impl CircuitUnit {
    /// Construct the unit marker.
    pub fn new() -> Self {
        Self
    }
}

impl From<CircuitUnit> for Circuit {
    fn from(_: CircuitUnit) -> Self {
        Self
    }
}

impl Object for CircuitUnit {
    const WIDTH: usize = 0;

    fn wire_layout() -> Vec<WireTy> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{CircuitTensor, CircuitUnit, Obj, Object};
    use crate::category_impl::Circuit;
    use hdl_cat_bits::Bits;

    #[test]
    fn obj_width_reads_hw_width() {
        assert_eq!(<Obj<Bits<8>> as Object>::WIDTH, 8);
        assert_eq!(<Obj<bool> as Object>::WIDTH, 1);
    }

    #[test]
    fn tensor_width_sums_operands() {
        type T = CircuitTensor<Obj<Bits<4>>, Obj<Bits<4>>>;
        assert_eq!(<T as Object>::WIDTH, 8);
    }

    #[test]
    fn nested_tensor_sums_recursively() {
        type T = CircuitTensor<
            CircuitTensor<Obj<Bits<2>>, Obj<Bits<2>>>,
            Obj<Bits<4>>,
        >;
        assert_eq!(<T as Object>::WIDTH, 8);
    }

    #[test]
    fn unit_has_zero_width() {
        assert_eq!(<CircuitUnit as Object>::WIDTH, 0);
    }

    #[test]
    fn tensor_with_unit_preserves_width() {
        type T1 = CircuitTensor<Obj<Bits<8>>, CircuitUnit>;
        type T2 = CircuitTensor<CircuitUnit, Obj<Bits<8>>>;
        assert_eq!(<T1 as Object>::WIDTH, 8);
        assert_eq!(<T2 as Object>::WIDTH, 8);
    }

    #[test]
    fn objects_convert_into_circuit() {
        let _: Circuit = Obj::<Bits<8>>::new().into();
        let _: Circuit = CircuitTensor::<Obj<Bits<4>>, Obj<Bits<4>>>::new().into();
        let _: Circuit = CircuitUnit::new().into();
    }
}
