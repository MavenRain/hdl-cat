//! The [`Circuit`] marker and its `Category`/`MonoidalCategory`
//! /`Braided`/`Symmetric` implementations.

use comp_cat_rs::foundation::category::Category;
use comp_cat_rs::foundation::iso::Iso;
use comp_cat_rs::foundation::monoidal::{Braided, MonoidalCategory, Symmetric};

use crate::arrow::{compose_arrows, identity_arrow, tensor_arrows, CircuitArrow};
use crate::object::{CircuitTensor, CircuitUnit, Object};

/// The circuit category marker.
///
/// Instances of [`Object`] provide this type with
/// `From<ObjType> for Circuit` witnesses, so every legal object
/// type satisfies `Into<Circuit>`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Circuit;

impl Category for Circuit {
    type Hom<A: Into<Self>, B: Into<Self>> = CircuitArrow<A, B>;

    fn id<A: Into<Self> + Clone>(_a: &A) -> Self::Hom<A, A> {
        // All identities are represented by a phantom pass-through
        // arrow.  Since A: Into<Circuit> does not expose width or
        // layout here, we produce an empty arrow — the downstream
        // `identity_arrow` constructor is available for callers
        // who need a wired identity and can supply `A: Object`.
        CircuitArrow::from_parts(
            hdl_cat_ir::HdlGraphBuilder::new().build(),
            Vec::new(),
            Vec::new(),
        )
    }

    fn comp<A, B, C>(f: Self::Hom<A, B>, g: Self::Hom<B, C>) -> Self::Hom<A, C>
    where
        A: Into<Self>,
        B: Into<Self>,
        C: Into<Self>,
    {
        compose_arrows(f, g)
    }
}

impl MonoidalCategory for Circuit {
    type Tensor<A: Into<Self>, B: Into<Self>> = CircuitTensor<A, B>;
    type Unit = CircuitUnit;

    fn tensor_map<A, B, C, D>(
        f: Self::Hom<A, B>,
        g: Self::Hom<C, D>,
    ) -> Self::Hom<Self::Tensor<A, C>, Self::Tensor<B, D>>
    where
        A: Into<Self>,
        B: Into<Self>,
        C: Into<Self>,
        D: Into<Self>,
    {
        tensor_arrows(f, g)
    }

    fn associator<A, B, C>() -> Iso<
        Self,
        Self::Tensor<Self::Tensor<A, B>, C>,
        Self::Tensor<A, Self::Tensor<B, C>>,
    >
    where
        A: Into<Self>,
        B: Into<Self>,
        C: Into<Self>,
    {
        // Wire permutation: `(A ⊗ B) ⊗ C ≅ A ⊗ (B ⊗ C)`.
        // The underlying flat wire list is identical; only the
        // object typing changes.  A pair of phantom arrows
        // witnesses the iso with no IR instructions.
        let forward = CircuitArrow::from_parts(
            hdl_cat_ir::HdlGraphBuilder::new().build(),
            Vec::new(),
            Vec::new(),
        );
        let backward = CircuitArrow::from_parts(
            hdl_cat_ir::HdlGraphBuilder::new().build(),
            Vec::new(),
            Vec::new(),
        );
        Iso::new(forward, backward)
    }

    fn left_unitor<A>() -> Iso<Self, Self::Tensor<Self::Unit, A>, A>
    where
        A: Into<Self>,
    {
        Iso::new(
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
        )
    }

    fn right_unitor<A>() -> Iso<Self, Self::Tensor<A, Self::Unit>, A>
    where
        A: Into<Self>,
    {
        Iso::new(
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
        )
    }
}

impl Braided for Circuit {
    fn braid<A, B>() -> Iso<Self, Self::Tensor<A, B>, Self::Tensor<B, A>>
    where
        A: Into<Self>,
        B: Into<Self>,
    {
        Iso::new(
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
            CircuitArrow::from_parts(
                hdl_cat_ir::HdlGraphBuilder::new().build(),
                Vec::new(),
                Vec::new(),
            ),
        )
    }
}

impl Symmetric for Circuit {}

/// Typed identity arrow for objects with a known layout.
///
/// `Category::id` cannot access an object's wire layout (its
/// bound is only `Into<Circuit> + Clone`), so use this
/// function when you need an identity arrow with real wires.
pub fn wired_identity<A: Object>() -> CircuitArrow<A, A> {
    identity_arrow::<A>()
}

#[cfg(test)]
mod tests {
    use super::{wired_identity, Circuit};
    use crate::arrow::CircuitArrow;
    use crate::object::{CircuitTensor, Obj};
    use comp_cat_rs::foundation::category::Category;
    use comp_cat_rs::foundation::monoidal::MonoidalCategory;
    use hdl_cat_bits::Bits;

    #[test]
    fn category_id_produces_empty_arrow() {
        let marker = Obj::<Bits<4>>::new();
        let id: CircuitArrow<Obj<Bits<4>>, Obj<Bits<4>>> = Circuit::id(&marker);
        assert_eq!(id.graph().instructions().len(), 0);
    }

    #[test]
    fn wired_identity_has_real_wires() {
        let id = wired_identity::<Obj<Bits<8>>>();
        assert_eq!(id.inputs().len(), 1);
        assert_eq!(id.graph().wires().len(), 1);
    }

    type Pair4 = CircuitTensor<Obj<Bits<4>>, Obj<Bits<4>>>;

    #[test]
    fn monoidal_tensor_of_wired_ids_has_two_wires() {
        let a = wired_identity::<Obj<Bits<4>>>();
        let b = wired_identity::<Obj<Bits<4>>>();
        let paired: CircuitArrow<Pair4, Pair4> = Circuit::tensor_map(a, b);
        assert_eq!(paired.inputs().len(), 2);
        assert_eq!(paired.outputs().len(), 2);
    }
}
