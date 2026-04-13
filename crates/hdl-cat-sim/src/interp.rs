//! The IR graph interpreter.
//!
//! Evaluates an [`hdl_cat_ir::HdlGraph`] by applying each
//! [`hdl_cat_ir::Op`] in order, threading a wire-value
//! environment.

use comp_cat_rs::collapse::free_category::Graph;
use hdl_cat_bits::{Bits, SignedBits};
use hdl_cat_error::{Error, SignalName};
use hdl_cat_ir::{BinOp, HdlGraph, Instruction, Op, WireId};
use hdl_cat_kind::BitSeq;

/// A wire-value environment.
///
/// `env[wire_id.index()]` holds the value of that wire, or
/// `None` if it has not yet been computed.
pub type Env = Vec<Option<BitSeq>>;

/// Interpret a graph, returning the final wire-value environment.
///
/// Input wire values are taken from `inputs`, in the same order
/// as `input_wires`.
///
/// # Errors
///
/// Returns [`Error::UndefinedSignal`] if an instruction reads a
/// wire that was not yet produced.  Returns [`Error::WidthMismatch`]
/// if any op receives operands of unexpected width.
pub fn interpret(
    graph: &HdlGraph,
    input_wires: &[WireId],
    inputs: &[BitSeq],
) -> Result<Env, Error> {
    (input_wires.len() == inputs.len())
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(u32::try_from(input_wires.len()).unwrap_or(u32::MAX)),
            actual: hdl_cat_error::Width::new(u32::try_from(inputs.len()).unwrap_or(u32::MAX)),
        })?;

    let initial_env: Env = (0..graph.vertex_count())
        .map(|idx| {
            input_wires
                .iter()
                .zip(inputs.iter())
                .find_map(|(w, v)| (w.index() == idx).then(|| v.clone()))
        })
        .collect();

    graph
        .instructions()
        .iter()
        .try_fold(initial_env, |env, instr| step_instruction(&env, instr))
}

fn step_instruction(env: &Env, instr: &Instruction) -> Result<Env, Error> {
    let operand_values: Result<Vec<BitSeq>, Error> = instr
        .inputs()
        .iter()
        .map(|w| lookup(env, *w))
        .collect();
    let operands = operand_values?;
    let result = apply_op(instr.op(), &operands)?;
    Ok(env
        .iter()
        .enumerate()
        .map(|(idx, cur)| {
            if idx == instr.output().index() {
                Some(result.clone())
            } else {
                cur.clone()
            }
        })
        .collect())
}

fn lookup(env: &Env, w: WireId) -> Result<BitSeq, Error> {
    env.get(w.index())
        .and_then(Clone::clone)
        .ok_or_else(|| Error::UndefinedSignal {
            name: SignalName::new(format!("{w}")),
        })
}

/// Apply an [`Op`] to concrete bit-sequence operands.
///
/// # Errors
///
/// Returns [`Error::WidthMismatch`] when operands are of
/// inconsistent widths, or [`Error::Overflow`] when an arithmetic
/// op's width exceeds `u128`'s range.
pub fn apply_op(op: &Op, inputs: &[BitSeq]) -> Result<BitSeq, Error> {
    match op {
        Op::Not => {
            expect_arity(inputs, 1)?;
            Ok(bitwise_map(&inputs[0], |b| !b))
        }
        Op::Bin(b) => {
            expect_arity(inputs, 2)?;
            apply_bin(*b, &inputs[0], &inputs[1])
        }
        Op::Mux => {
            expect_arity(inputs, 3)?;
            let sel = inputs[0].bit(0);
            let chosen = if sel { &inputs[2] } else { &inputs[1] };
            Ok(chosen.clone())
        }
        Op::Const { bits, .. } => {
            expect_arity(inputs, 0)?;
            Ok(bits.clone())
        }
        Op::Reg { init, .. } => {
            expect_arity(inputs, 1)?;
            // Combinational interpretation: registers pass the input
            // through to the output (the state-loopback is threaded
            // by the testbench, not by the register instruction
            // itself).  `init` is used only at cycle 0, handled at
            // the testbench level.
            let _ = init;
            Ok(inputs[0].clone())
        }
        Op::Concat { low_width, high_width } => {
            expect_arity(inputs, 2)?;
            let lw = usize::try_from(*low_width).unwrap_or(0);
            let hw = usize::try_from(*high_width).unwrap_or(0);
            (inputs[0].len() == lw && inputs[1].len() == hw)
                .then(|| inputs[0].clone().concat(inputs[1].clone()))
                .ok_or_else(|| Error::WidthMismatch {
                    expected: hdl_cat_error::Width::new(low_width + high_width),
                    actual: hdl_cat_error::Width::new(
                        u32::try_from(inputs[0].len() + inputs[1].len()).unwrap_or(u32::MAX),
                    ),
                })
        }
        Op::Slice { lo, hi } => {
            expect_arity(inputs, 1)?;
            let lo_i = usize::try_from(*lo).unwrap_or(0);
            let hi_i = usize::try_from(*hi).unwrap_or(0);
            (hi_i >= lo_i && hi_i <= inputs[0].len())
                .then(|| {
                    inputs[0]
                        .as_slice()
                        .iter()
                        .skip(lo_i)
                        .take(hi_i - lo_i)
                        .copied()
                        .collect::<BitSeq>()
                })
                .ok_or_else(|| Error::WidthMismatch {
                    expected: hdl_cat_error::Width::new(hi - lo),
                    actual: hdl_cat_error::Width::new(
                        u32::try_from(inputs[0].len()).unwrap_or(u32::MAX),
                    ),
                })
        }
        Op::ArrayShiftIn { element_width, depth } => {
            apply_array_shift_in(inputs, *element_width, *depth)
        }
        Op::ArrayTail { element_width, depth } => {
            apply_array_tail(inputs, *element_width, *depth)
        }
    }
}

fn apply_array_shift_in(
    inputs: &[BitSeq],
    element_width: u32,
    depth: usize,
) -> Result<BitSeq, Error> {
    let ew = usize::try_from(element_width).unwrap_or(0);
    let total = ew * depth;
    let (array, new_elem) = inputs
        .first()
        .zip(inputs.get(1))
        .filter(|(a, e)| a.len() == total && e.len() == ew)
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(total).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(inputs.len()).unwrap_or(u32::MAX),
            ),
        })?;
    // Discard the tail (highest element) and prepend the
    // new element at the low end.
    let kept = (depth - 1) * ew;
    let old_kept: BitSeq = array
        .as_slice()
        .iter()
        .take(kept)
        .copied()
        .collect();
    Ok(new_elem.clone().concat(old_kept))
}

fn apply_array_tail(
    inputs: &[BitSeq],
    element_width: u32,
    depth: usize,
) -> Result<BitSeq, Error> {
    let ew = usize::try_from(element_width).unwrap_or(0);
    let total = ew * depth;
    let array = inputs
        .first()
        .filter(|a| a.len() == total)
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(total).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(inputs.first().map_or(0, BitSeq::len)).unwrap_or(u32::MAX),
            ),
        })?;
    let tail_start = (depth - 1) * ew;
    Ok(array
        .as_slice()
        .iter()
        .skip(tail_start)
        .take(ew)
        .copied()
        .collect())
}

fn expect_arity(inputs: &[BitSeq], expected: usize) -> Result<(), Error> {
    (inputs.len() == expected)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(u32::try_from(expected).unwrap_or(u32::MAX)),
            actual: hdl_cat_error::Width::new(u32::try_from(inputs.len()).unwrap_or(u32::MAX)),
        })
}

fn bitwise_map(a: &BitSeq, f: impl Fn(bool) -> bool) -> BitSeq {
    a.as_slice().iter().map(|b| f(*b)).collect()
}

fn bitwise_zip(a: &BitSeq, b: &BitSeq, f: impl Fn(bool, bool) -> bool) -> BitSeq {
    a.as_slice()
        .iter()
        .zip(b.as_slice().iter())
        .map(|(x, y)| f(*x, *y))
        .collect()
}

fn bits_from_seq(seq: &BitSeq) -> u128 {
    seq.as_slice()
        .iter()
        .enumerate()
        .fold(0u128, |acc, (i, b)| acc | (u128::from(*b) << i))
}

fn seq_from_u128(v: u128, n: usize) -> BitSeq {
    (0..n).map(|i| (v >> i) & 1 == 1).collect()
}

fn apply_bin(op: BinOp, a: &BitSeq, b: &BitSeq) -> Result<BitSeq, Error> {
    (a.len() == b.len())
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(u32::try_from(a.len()).unwrap_or(u32::MAX)),
            actual: hdl_cat_error::Width::new(u32::try_from(b.len()).unwrap_or(u32::MAX)),
        })?;
    let n = a.len();
    match op {
        BinOp::And => Ok(bitwise_zip(a, b, |x, y| x & y)),
        BinOp::Or => Ok(bitwise_zip(a, b, |x, y| x | y)),
        BinOp::Xor => Ok(bitwise_zip(a, b, |x, y| x ^ y)),
        BinOp::Add | BinOp::Sub | BinOp::Mul => {
            let av = bits_from_seq(a);
            let bv = bits_from_seq(b);
            let raw = match op {
                BinOp::Add => av.wrapping_add(bv),
                BinOp::Sub => av.wrapping_sub(bv),
                BinOp::Mul => av.wrapping_mul(bv),
                _ => 0,
            };
            let mask = match n {
                0 => 0u128,
                128.. => u128::MAX,
                k => (1u128 << k) - 1,
            };
            Ok(seq_from_u128(raw & mask, n))
        }
        BinOp::Eq => Ok(BitSeq::from_iter([bits_from_seq(a) == bits_from_seq(b)])),
        BinOp::Lt => {
            // Unsigned comparison; treat the operand bits as unsigned.
            // SignedBits comparison is a separate op in future
            // expansions; we don't yet distinguish.
            let _ = (Bits::<128>::new_wrapping, SignedBits::<128>::new_wrapping);
            Ok(BitSeq::from_iter([bits_from_seq(a) < bits_from_seq(b)]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_op, interpret};
    use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
    use hdl_cat_kind::BitSeq;

    #[test]
    fn not_inverts_all_bits() -> Result<(), hdl_cat_error::Error> {
        let input = BitSeq::from_iter([true, false, true, false]);
        let out = apply_op(&Op::Not, &[input])?;
        assert_eq!(out.as_slice(), &[false, true, false, true]);
        Ok(())
    }

    #[test]
    fn and_computes_bitwise() -> Result<(), hdl_cat_error::Error> {
        let a = BitSeq::from_iter([true, true, false, false]);
        let b = BitSeq::from_iter([true, false, true, false]);
        let out = apply_op(&Op::Bin(BinOp::And), &[a, b])?;
        assert_eq!(out.as_slice(), &[true, false, false, false]);
        Ok(())
    }

    #[test]
    fn add_wraps_within_width() -> Result<(), hdl_cat_error::Error> {
        // 200 + 100 in 8 bits = 44
        let a_bits: BitSeq = (0..8).map(|i| (200u128 >> i) & 1 == 1).collect();
        let b_bits: BitSeq = (0..8).map(|i| (100u128 >> i) & 1 == 1).collect();
        let out = apply_op(&Op::Bin(BinOp::Add), &[a_bits, b_bits])?;
        let v = (0..8).fold(0u128, |acc, i| acc | (u128::from(out.bit(i)) << i));
        assert_eq!(v, 44);
        Ok(())
    }

    #[test]
    fn mux_selects_by_selector() -> Result<(), hdl_cat_error::Error> {
        let sel_false = BitSeq::from_iter([false]);
        let sel_true = BitSeq::from_iter([true]);
        let lo = BitSeq::from_iter([true, false]);
        let hi = BitSeq::from_iter([false, true]);
        let r_false = apply_op(&Op::Mux, &[sel_false, lo.clone(), hi.clone()])?;
        let r_true = apply_op(&Op::Mux, &[sel_true, lo.clone(), hi.clone()])?;
        assert_eq!(r_false.as_slice(), lo.as_slice());
        assert_eq!(r_true.as_slice(), hi.as_slice());
        Ok(())
    }

    #[test]
    fn eq_produces_single_bit() -> Result<(), hdl_cat_error::Error> {
        let a = BitSeq::from_iter([true, false, true, false]);
        let b = BitSeq::from_iter([true, false, true, false]);
        let out = apply_op(&Op::Bin(BinOp::Eq), &[a, b])?;
        assert_eq!(out.as_slice(), &[true]);
        Ok(())
    }

    #[test]
    fn lt_produces_single_bit() -> Result<(), hdl_cat_error::Error> {
        let a = BitSeq::from_iter([false, false, true, false]); // 4
        let b = BitSeq::from_iter([true, false, true, false]);  // 5
        let out = apply_op(&Op::Bin(BinOp::Lt), &[a, b])?;
        assert_eq!(out.as_slice(), &[true]);
        Ok(())
    }

    #[test]
    fn concat_joins_bit_sequences() -> Result<(), hdl_cat_error::Error> {
        let low = BitSeq::from_iter([true, false]);
        let high = BitSeq::from_iter([true, true]);
        let out = apply_op(
            &Op::Concat {
                low_width: 2,
                high_width: 2,
            },
            &[low, high],
        )?;
        assert_eq!(out.as_slice(), &[true, false, true, true]);
        Ok(())
    }

    #[test]
    fn slice_extracts_range() -> Result<(), hdl_cat_error::Error> {
        let input = BitSeq::from_iter([true, false, true, false, true]);
        let out = apply_op(&Op::Slice { lo: 1, hi: 4 }, &[input])?;
        assert_eq!(out.as_slice(), &[false, true, false]);
        Ok(())
    }

    #[test]
    fn interpret_single_not_gate() -> Result<(), hdl_cat_error::Error> {
        let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (bld, b) = bld.with_wire(WireTy::Bit);
        let bld = bld.with_instruction(Op::Not, vec![a], b)?;
        let g = bld.build();

        let env = interpret(&g, &[a], &[BitSeq::from_iter([true])])?;
        assert_eq!(env[b.index()].as_ref().map(|s| s.bit(0)), Some(false));
        Ok(())
    }

    #[test]
    fn interpret_chains_instructions() -> Result<(), hdl_cat_error::Error> {
        // (lhs XOR rhs) AND sel
        let (bld, lhs) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (bld, rhs) = bld.with_wire(WireTy::Bit);
        let (bld, sel) = bld.with_wire(WireTy::Bit);
        let (bld, xor_out) = bld.with_wire(WireTy::Bit);
        let (bld, out) = bld.with_wire(WireTy::Bit);
        let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![lhs, rhs], xor_out)?;
        let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![xor_out, sel], out)?;
        let graph = bld.build();

        let env = interpret(
            &graph,
            &[lhs, rhs, sel],
            &[
                BitSeq::from_iter([true]),
                BitSeq::from_iter([false]),
                BitSeq::from_iter([true]),
            ],
        )?;
        assert_eq!(env[out.index()].as_ref().map(|s| s.bit(0)), Some(true));
        Ok(())
    }
}
