//! [`crate::Hw`] implementations for tuples and arrays.
//!
//! - 2-, 3-, and 4-arity tuples of `Hw` components
//! - fixed-length arrays `[T; N]` of `Hw` elements

use hdl_cat_error::Error;

use crate::bit_seq::BitSeq;
use crate::hw::{Hw, width_mismatch};
use crate::ty_desc::TypeDesc;

impl<A: Hw, B: Hw> Hw for (A, B) {
    const WIDTH: usize = A::WIDTH + B::WIDTH;

    fn type_desc() -> TypeDesc {
        TypeDesc::Tuple(vec![A::type_desc(), B::type_desc()])
    }

    fn to_bits_seq(&self) -> BitSeq {
        self.0.to_bits_seq().concat(self.1.to_bits_seq())
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        let expected = Self::WIDTH;
        (bits.len() == expected)
            .then_some(())
            .ok_or_else(|| width_mismatch(expected, bits.len()))
            .and_then(|()| {
                let (head, tail) = bits.clone().split_at(A::WIDTH);
                let a = A::from_bits_seq(&head)?;
                let b = B::from_bits_seq(&tail)?;
                Ok((a, b))
            })
    }
}

impl<A: Hw, B: Hw, C: Hw> Hw for (A, B, C) {
    const WIDTH: usize = A::WIDTH + B::WIDTH + C::WIDTH;

    fn type_desc() -> TypeDesc {
        TypeDesc::Tuple(vec![A::type_desc(), B::type_desc(), C::type_desc()])
    }

    fn to_bits_seq(&self) -> BitSeq {
        self.0
            .to_bits_seq()
            .concat(self.1.to_bits_seq())
            .concat(self.2.to_bits_seq())
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        let expected = Self::WIDTH;
        (bits.len() == expected)
            .then_some(())
            .ok_or_else(|| width_mismatch(expected, bits.len()))
            .and_then(|()| {
                let (head, rest) = bits.clone().split_at(A::WIDTH);
                let (mid, tail) = rest.split_at(B::WIDTH);
                let a = A::from_bits_seq(&head)?;
                let b = B::from_bits_seq(&mid)?;
                let c = C::from_bits_seq(&tail)?;
                Ok((a, b, c))
            })
    }
}

impl<A: Hw, B: Hw, C: Hw, D: Hw> Hw for (A, B, C, D) {
    const WIDTH: usize = A::WIDTH + B::WIDTH + C::WIDTH + D::WIDTH;

    fn type_desc() -> TypeDesc {
        TypeDesc::Tuple(vec![
            A::type_desc(),
            B::type_desc(),
            C::type_desc(),
            D::type_desc(),
        ])
    }

    fn to_bits_seq(&self) -> BitSeq {
        self.0
            .to_bits_seq()
            .concat(self.1.to_bits_seq())
            .concat(self.2.to_bits_seq())
            .concat(self.3.to_bits_seq())
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        let expected = Self::WIDTH;
        (bits.len() == expected)
            .then_some(())
            .ok_or_else(|| width_mismatch(expected, bits.len()))
            .and_then(|()| {
                let (ha, r1) = bits.clone().split_at(A::WIDTH);
                let (hb, r2) = r1.split_at(B::WIDTH);
                let (hc, hd) = r2.split_at(C::WIDTH);
                let a = A::from_bits_seq(&ha)?;
                let b = B::from_bits_seq(&hb)?;
                let c = C::from_bits_seq(&hc)?;
                let d = D::from_bits_seq(&hd)?;
                Ok((a, b, c, d))
            })
    }
}

impl<T: Hw, const N: usize> Hw for [T; N] {
    const WIDTH: usize = T::WIDTH * N;

    fn type_desc() -> TypeDesc {
        TypeDesc::Array {
            elem: Box::new(T::type_desc()),
            len: N,
        }
    }

    fn to_bits_seq(&self) -> BitSeq {
        self.iter()
            .fold(BitSeq::new(), |acc, elem| acc.concat(elem.to_bits_seq()))
    }

    fn from_bits_seq(bits: &BitSeq) -> Result<Self, Error> {
        let expected = Self::WIDTH;
        (bits.len() == expected)
            .then_some(())
            .ok_or_else(|| width_mismatch(expected, bits.len()))
            .and_then(|()| {
                let elem_width = T::WIDTH;
                let slice = bits.as_slice();
                (0..N)
                    .map(|i| {
                        let start = i * elem_width;
                        let end = start + elem_width;
                        let chunk: BitSeq =
                            slice[start..end].iter().copied().collect();
                        T::from_bits_seq(&chunk)
                    })
                    .collect::<Result<Vec<T>, Error>>()
            })
            .and_then(|elems| {
                <[T; N]>::try_from(elems)
                    .map_err(|v: Vec<T>| width_mismatch(N, v.len()))
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::hw::Hw;
    use crate::ty_desc::TypeDesc;
    use hdl_cat_bits::Bits;

    #[test]
    fn pair_bool_bits_round_trips() -> Result<(), hdl_cat_error::Error> {
        let v: (bool, Bits<4>) = (true, Bits::<4>::try_new(0xa)?);
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 5);
        let back: (bool, Bits<4>) = Hw::from_bits_seq(&seq)?;
        assert!(back.0);
        assert_eq!(back.1.to_u128(), 0xa);
        Ok(())
    }

    #[test]
    fn triple_round_trips() -> Result<(), hdl_cat_error::Error> {
        let v: (Bits<2>, Bits<3>, bool) =
            (Bits::<2>::try_new(0b10)?, Bits::<3>::try_new(0b101)?, true);
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 6);
        let back: (Bits<2>, Bits<3>, bool) = Hw::from_bits_seq(&seq)?;
        assert_eq!(back.0.to_u128(), 0b10);
        assert_eq!(back.1.to_u128(), 0b101);
        assert!(back.2);
        Ok(())
    }

    #[test]
    fn quad_round_trips() -> Result<(), hdl_cat_error::Error> {
        let v: (bool, bool, bool, bool) = (true, false, true, false);
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 4);
        let back: (bool, bool, bool, bool) = Hw::from_bits_seq(&seq)?;
        assert_eq!(back, v);
        Ok(())
    }

    #[test]
    fn array_of_bits_round_trips() -> Result<(), hdl_cat_error::Error> {
        let v: [Bits<4>; 3] = [
            Bits::<4>::try_new(0x1)?,
            Bits::<4>::try_new(0x2)?,
            Bits::<4>::try_new(0x3)?,
        ];
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 12);
        let back: [Bits<4>; 3] = Hw::from_bits_seq(&seq)?;
        assert_eq!(back[0].to_u128(), 0x1);
        assert_eq!(back[1].to_u128(), 0x2);
        assert_eq!(back[2].to_u128(), 0x3);
        Ok(())
    }

    #[test]
    fn empty_array_has_zero_width() -> Result<(), hdl_cat_error::Error> {
        let v: [bool; 0] = [];
        let seq = v.to_bits_seq();
        assert_eq!(seq.len(), 0);
        let _back: [bool; 0] = Hw::from_bits_seq(&seq)?;
        Ok(())
    }

    #[test]
    fn array_of_bool_width_matches() {
        assert_eq!(<[bool; 8] as Hw>::WIDTH, 8);
    }

    #[test]
    fn tuple_type_desc_is_tuple_variant() {
        let d = <(bool, Bits<4>) as Hw>::type_desc();
        assert!(matches!(d, TypeDesc::Tuple(_)));
        assert_eq!(d.width(), 5);
    }

    #[test]
    fn array_type_desc_is_array_variant() {
        let d = <[Bits<8>; 4] as Hw>::type_desc();
        assert!(matches!(d, TypeDesc::Array { len: 4, .. }));
        assert_eq!(d.width(), 32);
    }

    #[test]
    fn wrong_width_fails_tuple() {
        let seq = crate::bit_seq::BitSeq::from_iter([true, false, true]);
        let r: Result<(bool, bool), _> = Hw::from_bits_seq(&seq);
        assert!(r.is_err());
    }
}
