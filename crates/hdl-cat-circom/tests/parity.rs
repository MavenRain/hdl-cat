//! Cross-backend parity proptest.
//!
//! Pairs `hdl_cat_sim::interp::interpret` (the reference IR
//! semantics) against `simulate_circom` (a Rust mirror of what the
//! Circom emitter renders) and asserts bit-equal outputs on
//! randomly generated inputs.  Catches drift between the two
//! implementations of the same per-op math.
//!
//! Each proptest also exercises `emit_template` end-to-end so the
//! emitter's lowering of the same graph is shown to succeed at
//! render time.

use hdl_cat_circom::emit_template;
use hdl_cat_error::{Error, SignalName, Width};
use hdl_cat_ir::{BinOp, HdlGraph, HdlGraphBuilder, Op, WireId, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sim::interp::interpret;
use proptest::prelude::*;

type Env = Vec<Option<BitSeq>>;

const MAX_WIDTH: u32 = 8;

// --- helpers ---

fn bits_to_u128(seq: &BitSeq) -> u128 {
    seq.as_slice()
        .iter()
        .enumerate()
        .fold(0u128, |acc, (i, b)| acc | (u128::from(*b) << i))
}

fn u128_to_bits(v: u128, width: u32) -> BitSeq {
    let n = usize::try_from(width).unwrap_or(0);
    (0..n).map(|i| (v >> i) & 1 == 1).collect()
}

fn low_mask(width: u32) -> u128 {
    match width {
        0 => 0u128,
        128.. => u128::MAX,
        k => 1u128.checked_shl(k).map_or(u128::MAX, |v| v - 1),
    }
}

fn missing_operand_error(arity: u32, got: usize) -> Error {
    Error::WidthMismatch {
        expected: Width::new(arity),
        actual: Width::new(u32::try_from(got).unwrap_or(u32::MAX)),
    }
}

// --- Circom-shape op simulation ---

fn simulate_circom_op(
    op: &Op,
    operands: &[BitSeq],
    out_width: u32,
) -> Result<BitSeq, Error> {
    match op {
        Op::Not => operands
            .first()
            .map(|a| a.as_slice().iter().map(|b| !b).collect())
            .ok_or_else(|| missing_operand_error(1, operands.len())),
        Op::Bin(BinOp::And) => bin_per_bit(operands, |a, b| a & b),
        Op::Bin(BinOp::Or) => bin_per_bit(operands, |a, b| a | b),
        Op::Bin(BinOp::Xor) => bin_per_bit(operands, |a, b| a ^ b),
        Op::Bin(BinOp::Add) => {
            bin_arith(operands, out_width, u128::wrapping_add)
        }
        Op::Bin(BinOp::Sub) => {
            bin_arith(operands, out_width, u128::wrapping_sub)
        }
        Op::Bin(BinOp::Mul) => {
            bin_arith(operands, out_width, u128::wrapping_mul)
        }
        Op::Bin(BinOp::Eq) => bin_compare(operands, |a, b| a == b),
        Op::Bin(BinOp::Lt) => bin_compare(operands, |a, b| a < b),
        Op::Mux => mux_op(operands),
        Op::Const { bits, .. } => Ok(bits.clone()),
        Op::Slice { lo, hi } => slice_op(operands, *lo, *hi),
        Op::Concat { low_width, high_width } => {
            concat_op(operands, *low_width, *high_width)
        }
        Op::Reg { .. } => Err(Error::UnsupportedInCircom("reg")),
        Op::ArrayShiftIn { .. } => {
            Err(Error::UnsupportedInCircom("array_shift_in"))
        }
        Op::ArrayTail { .. } => Err(Error::UnsupportedInCircom("array_tail")),
    }
}

fn bin_per_bit(
    operands: &[BitSeq],
    f: impl Fn(bool, bool) -> bool,
) -> Result<BitSeq, Error> {
    operands
        .first()
        .zip(operands.get(1))
        .map(|(a, b)| {
            a.as_slice()
                .iter()
                .zip(b.as_slice().iter())
                .map(|(x, y)| f(*x, *y))
                .collect()
        })
        .ok_or_else(|| missing_operand_error(2, operands.len()))
}

fn bin_arith(
    operands: &[BitSeq],
    out_width: u32,
    op: impl Fn(u128, u128) -> u128,
) -> Result<BitSeq, Error> {
    operands
        .first()
        .zip(operands.get(1))
        .map(|(a, b)| {
            // Mirrors the emitter: pack each operand via Bits2Num,
            // perform the field op, take the low `out_width` bits via
            // Num2Bits.
            let av = bits_to_u128(a);
            let bv = bits_to_u128(b);
            u128_to_bits(op(av, bv) & low_mask(out_width), out_width)
        })
        .ok_or_else(|| missing_operand_error(2, operands.len()))
}

fn bin_compare(
    operands: &[BitSeq],
    pred: impl Fn(u128, u128) -> bool,
) -> Result<BitSeq, Error> {
    operands
        .first()
        .zip(operands.get(1))
        .map(|(a, b)| BitSeq::from_iter([pred(bits_to_u128(a), bits_to_u128(b))]))
        .ok_or_else(|| missing_operand_error(2, operands.len()))
}

fn mux_op(operands: &[BitSeq]) -> Result<BitSeq, Error> {
    operands
        .first()
        .zip(operands.get(1))
        .zip(operands.get(2))
        .map(|((sel, f_arm), t_arm)| {
            let s = sel.bit(0);
            f_arm
                .as_slice()
                .iter()
                .zip(t_arm.as_slice().iter())
                .map(|(fb, tb)| if s { *tb } else { *fb })
                .collect()
        })
        .ok_or_else(|| missing_operand_error(3, operands.len()))
}

fn slice_op(operands: &[BitSeq], lo: u32, hi: u32) -> Result<BitSeq, Error> {
    operands
        .first()
        .map(|a| {
            let lo_i = usize::try_from(lo).unwrap_or(0);
            let hi_i = usize::try_from(hi).unwrap_or(0);
            a.as_slice()
                .iter()
                .skip(lo_i)
                .take(hi_i.saturating_sub(lo_i))
                .copied()
                .collect()
        })
        .ok_or_else(|| missing_operand_error(1, operands.len()))
}

fn concat_op(
    operands: &[BitSeq],
    _low_width: u32,
    _high_width: u32,
) -> Result<BitSeq, Error> {
    operands
        .first()
        .zip(operands.get(1))
        .map(|(low, high)| low.clone().concat(high.clone()))
        .ok_or_else(|| missing_operand_error(2, operands.len()))
}

fn simulate_circom(
    graph: &HdlGraph,
    input_wires: &[WireId],
    inputs: &[BitSeq],
) -> Result<Env, Error> {
    let initial: Env = (0..graph.wires().len())
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
        .try_fold(initial, |env, instr| {
            let operand_values: Vec<BitSeq> = instr
                .inputs()
                .iter()
                .map(|w| {
                    env.get(w.index())
                        .and_then(Clone::clone)
                        .ok_or_else(|| Error::UndefinedSignal {
                            name: SignalName::new(format!("{w}")),
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let out_width = graph
                .wire_ty(instr.output().index())
                .map_or(0, WireTy::width);
            let result =
                simulate_circom_op(instr.op(), &operand_values, out_width)?;
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
        })
}

fn extract(env: &Env, w: WireId) -> Option<BitSeq> {
    env.get(w.index()).cloned().flatten()
}

fn parity_outputs(
    graph: &HdlGraph,
    in_wires: &[WireId],
    inputs: &[BitSeq],
    out: WireId,
) -> (Option<BitSeq>, Option<BitSeq>) {
    let interp_out = interpret(graph, in_wires, inputs)
        .ok()
        .and_then(|env| extract(&env, out));
    let circom_out = simulate_circom(graph, in_wires, inputs)
        .ok()
        .and_then(|env| extract(&env, out));
    (interp_out, circom_out)
}

fn emit_renders(
    graph: &HdlGraph,
    name: &str,
    in_wires: &[WireId],
    out_wires: &[WireId],
) -> bool {
    emit_template(graph, name, in_wires, out_wires, &[])
        .run()
        .and_then(|t| t.render().run())
        .is_ok()
}

// --- per-op graph builders ---

fn build_unary_graph(
    op: Op,
    in_ty: WireTy,
    out_ty: WireTy,
) -> Result<(HdlGraph, WireId, WireId), Error> {
    let (b, a) = HdlGraphBuilder::new().with_wire(in_ty);
    let (b, out) = b.with_wire(out_ty);
    let b = b.with_instruction(op, vec![a], out)?;
    Ok((b.build(), a, out))
}

fn build_binary_graph(
    op: Op,
    lhs_ty: WireTy,
    rhs_ty: WireTy,
    out_ty: WireTy,
) -> Result<(HdlGraph, WireId, WireId, WireId), Error> {
    let (b, a) = HdlGraphBuilder::new().with_wire(lhs_ty);
    let (b, c) = b.with_wire(rhs_ty);
    let (b, out) = b.with_wire(out_ty);
    let b = b.with_instruction(op, vec![a, c], out)?;
    Ok((b.build(), a, c, out))
}

fn build_mux_graph(
    width: u32,
) -> Result<(HdlGraph, WireId, WireId, WireId, WireId), Error> {
    let (b, sel) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
    let (b, f_arm) = b.with_wire(WireTy::Bits(width));
    let (b, t_arm) = b.with_wire(WireTy::Bits(width));
    let (b, out) = b.with_wire(WireTy::Bits(width));
    let b = b.with_instruction(Op::Mux, vec![sel, f_arm, t_arm], out)?;
    Ok((b.build(), sel, f_arm, t_arm, out))
}

fn build_const_graph(
    bits: BitSeq,
    width: u32,
) -> Result<(HdlGraph, WireId), Error> {
    let (b, out) = HdlGraphBuilder::new().with_wire(WireTy::Bits(width));
    let b = b.with_instruction(
        Op::Const {
            bits,
            ty: WireTy::Bits(width),
        },
        Vec::new(),
        out,
    )?;
    Ok((b.build(), out))
}

// --- per-op proptests ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn parity_not(
        width in 1u32..=MAX_WIDTH,
        seed in any::<u64>(),
    ) {
        let bits = u128_to_bits(u128::from(seed), width);
        let (g, a, out) = build_unary_graph(
            Op::Not,
            WireTy::Bits(width),
            WireTy::Bits(width),
        ).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(&g, &[a], &[bits], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "not_t", &[a], &[out]));
    }

    #[test]
    fn parity_bitwise(
        which in 0u8..3,
        width in 1u32..=MAX_WIDTH,
        a in any::<u64>(),
        b in any::<u64>(),
    ) {
        let op = match which {
            0 => BinOp::And,
            1 => BinOp::Or,
            _ => BinOp::Xor,
        };
        let av = u128_to_bits(u128::from(a), width);
        let bv = u128_to_bits(u128::from(b), width);
        let (g, w_a, w_b, out) = build_binary_graph(
            Op::Bin(op),
            WireTy::Bits(width),
            WireTy::Bits(width),
            WireTy::Bits(width),
        ).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(&g, &[w_a, w_b], &[av, bv], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "bw_t", &[w_a, w_b], &[out]));
    }

    #[test]
    fn parity_arith(
        which in 0u8..3,
        width in 1u32..=MAX_WIDTH,
        a in any::<u64>(),
        b in any::<u64>(),
    ) {
        let op = match which {
            0 => BinOp::Add,
            1 => BinOp::Sub,
            _ => BinOp::Mul,
        };
        let av = u128_to_bits(u128::from(a), width);
        let bv = u128_to_bits(u128::from(b), width);
        let (g, w_a, w_b, out) = build_binary_graph(
            Op::Bin(op),
            WireTy::Bits(width),
            WireTy::Bits(width),
            WireTy::Bits(width),
        ).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(&g, &[w_a, w_b], &[av, bv], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "ar_t", &[w_a, w_b], &[out]));
    }

    #[test]
    fn parity_compare(
        is_lt in any::<bool>(),
        width in 1u32..=MAX_WIDTH,
        a in any::<u64>(),
        b in any::<u64>(),
    ) {
        let op = if is_lt { BinOp::Lt } else { BinOp::Eq };
        let av = u128_to_bits(u128::from(a), width);
        let bv = u128_to_bits(u128::from(b), width);
        let (g, w_a, w_b, out) = build_binary_graph(
            Op::Bin(op),
            WireTy::Bits(width),
            WireTy::Bits(width),
            WireTy::Bits(1),
        ).map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(&g, &[w_a, w_b], &[av, bv], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "cmp_t", &[w_a, w_b], &[out]));
    }

    #[test]
    fn parity_mux(
        width in 1u32..=MAX_WIDTH,
        sel_bit in any::<bool>(),
        f_seed in any::<u64>(),
        t_seed in any::<u64>(),
    ) {
        let sel = BitSeq::from_iter([sel_bit]);
        let f_arm = u128_to_bits(u128::from(f_seed), width);
        let t_arm = u128_to_bits(u128::from(t_seed), width);
        let (g, w_sel, w_f, w_t, out) = build_mux_graph(width)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(
            &g,
            &[w_sel, w_f, w_t],
            &[sel, f_arm, t_arm],
            out,
        );
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "mux_t", &[w_sel, w_f, w_t], &[out]));
    }

    #[test]
    fn parity_const(
        width in 1u32..=MAX_WIDTH,
        seed in any::<u64>(),
    ) {
        let bits = u128_to_bits(u128::from(seed), width);
        let (g, out) = build_const_graph(bits, width)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let (lhs, rhs) = parity_outputs(&g, &[], &[], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "k_t", &[], &[out]));
    }

    #[test]
    fn parity_slice(
        full_width in 2u32..=MAX_WIDTH,
        lo_pick in 0u32..MAX_WIDTH,
        len_pick in 1u32..=MAX_WIDTH,
        seed in any::<u64>(),
    ) {
        let lo = lo_pick % full_width;
        let max_len = full_width - lo;
        let len = (len_pick % max_len).max(1);
        let hi = lo + len;
        let bits = u128_to_bits(u128::from(seed), full_width);
        let (b, src) = HdlGraphBuilder::new()
            .with_wire(WireTy::Bits(full_width));
        let (b, out) = b.with_wire(WireTy::Bits(len));
        let b = b
            .with_instruction(Op::Slice { lo, hi }, vec![src], out)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let g = b.build();
        let (lhs, rhs) = parity_outputs(&g, &[src], &[bits], out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "sl_t", &[src], &[out]));
    }

    #[test]
    fn parity_concat(
        low_width in 1u32..=MAX_WIDTH,
        high_width in 1u32..=MAX_WIDTH,
        low_seed in any::<u64>(),
        high_seed in any::<u64>(),
    ) {
        let low_bits = u128_to_bits(u128::from(low_seed), low_width);
        let high_bits = u128_to_bits(u128::from(high_seed), high_width);
        let (b, low) = HdlGraphBuilder::new()
            .with_wire(WireTy::Bits(low_width));
        let (b, high) = b.with_wire(WireTy::Bits(high_width));
        let (b, out) = b.with_wire(WireTy::Bits(low_width + high_width));
        let b = b
            .with_instruction(
                Op::Concat { low_width, high_width },
                vec![low, high],
                out,
            )
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let g = b.build();
        let (lhs, rhs) = parity_outputs(
            &g,
            &[low, high],
            &[low_bits, high_bits],
            out,
        );
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "cc_t", &[low, high], &[out]));
    }
}

// --- chained-graph proptest ---
//
// Chains a small sequence of in-scope ops on a shared width and
// checks that interpreter and Circom-shape semantics agree end to
// end.  Width is fixed per case to keep the strategy simple while
// still exercising operand routing.

fn arb_chain_step() -> impl Strategy<Value = BinOp> {
    prop_oneof![
        Just(BinOp::And),
        Just(BinOp::Or),
        Just(BinOp::Xor),
        Just(BinOp::Add),
        Just(BinOp::Sub),
        Just(BinOp::Mul),
    ]
}

fn build_chain_graph(
    width: u32,
    steps: &[BinOp],
) -> Result<(HdlGraph, WireId, WireId, WireId), Error> {
    let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(width));
    let (b, c) = b.with_wire(WireTy::Bits(width));
    let initial = (b, a);
    let (final_b, last) = steps.iter().try_fold(
        initial,
        |(builder, acc), step| -> Result<(HdlGraphBuilder, WireId), Error> {
            let (next_b, out) = builder.with_wire(WireTy::Bits(width));
            let next_b = next_b
                .with_instruction(Op::Bin(*step), vec![acc, c], out)?;
            Ok((next_b, out))
        },
    )?;
    Ok((final_b.build(), a, c, last))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn parity_chain(
        width in 1u32..=MAX_WIDTH,
        steps in prop::collection::vec(arb_chain_step(), 1..=4),
        a_seed in any::<u64>(),
        b_seed in any::<u64>(),
    ) {
        let (g, w_a, w_b, w_out) = build_chain_graph(width, &steps)
            .map_err(|e| TestCaseError::fail(format!("{e}")))?;
        let av = u128_to_bits(u128::from(a_seed), width);
        let bv = u128_to_bits(u128::from(b_seed), width);
        let (lhs, rhs) = parity_outputs(&g, &[w_a, w_b], &[av, bv], w_out);
        prop_assert_eq!(lhs, rhs);
        prop_assert!(emit_renders(&g, "chain_t", &[w_a, w_b], &[w_out]));
    }
}
