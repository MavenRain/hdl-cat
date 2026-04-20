//! Convert an [`hdl_cat_ir::HdlGraph`] to a Circom [`Template`].
//!
//! v1 is combinational and bit-level.  Every hdl-cat wire of width
//! `N` becomes `N` Circom signals, each constrained to `{0, 1}` via
//! the boolean constraint emitted on every input bit.  Bitwise ops
//! (`Not`, `And`, `Or`, `Xor`, `Mux`), `Const`, `Slice`, and `Concat`
//! lower directly to per-bit assignments.  Arithmetic (`Add`, `Sub`,
//! `Mul`), comparisons (`Eq`, `Lt`), and stateful ops (`Reg`,
//! `ArrayShiftIn`, `ArrayTail`) return
//! [`hdl_cat_error::Error::UnsupportedInCircom`] in v1 and will be
//! wired up by a follow-up change.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::Error;
use hdl_cat_ir::{BinOp, HdlGraph, Instruction, Op, WireId, WireTy};
use hdl_cat_kind::BitSeq;

use crate::ast::{Expr, Field, Signal, SignalDir, Stmt, Template};

/// Produce a Circom [`Template`] from an IR graph.
///
/// The `input_wires` list becomes the template's `signal input`
/// ports and the `output_wires` list becomes its `signal output`
/// ports; every other wire becomes a local `signal` intermediate.
/// Each input port emits per-bit boolean constraints on entry.
///
/// # Errors
///
/// Returns [`Error::UnsupportedInCircom`] when an op outside the
/// v1 scope appears (arithmetic, comparisons, or stateful ops).
#[must_use]
pub fn emit_template(
    graph: &HdlGraph,
    name: &str,
    input_wires: &[WireId],
    output_wires: &[WireId],
) -> Io<Error, Template> {
    let graph_owned = graph.clone();
    let name_owned = name.to_string();
    let inputs_owned: Vec<WireId> = input_wires.to_vec();
    let outputs_owned: Vec<WireId> = output_wires.to_vec();
    Io::suspend(move || {
        build_template(&graph_owned, &name_owned, &inputs_owned, &outputs_owned)
    })
}

fn build_template(
    graph: &HdlGraph,
    name: &str,
    inputs: &[WireId],
    outputs: &[WireId],
) -> Result<Template, Error> {
    let input_ports = inputs.iter().map(|w| {
        Signal::new(wire_name(*w), SignalDir::Input, graph_width(graph, *w))
    });
    let output_ports = outputs.iter().map(|w| {
        Signal::new(wire_name(*w), SignalDir::Output, graph_width(graph, *w))
    });
    let ports: Vec<Signal> = input_ports.chain(output_ports).collect();

    let intermediates: Vec<Signal> = graph
        .wires()
        .iter()
        .enumerate()
        .filter_map(|(idx, ty)| {
            let w = WireId::new(idx);
            let is_port = inputs.contains(&w) || outputs.contains(&w);
            if is_port {
                None
            } else {
                Some(Signal::new(
                    wire_name(w),
                    SignalDir::Intermediate,
                    ty.width(),
                ))
            }
        })
        .collect();

    let boolean_constraints: Vec<Stmt> = inputs
        .iter()
        .flat_map(|w| {
            let width = graph_width(graph, *w);
            (0..width).map(move |i| boolean_constraint(*w, i))
        })
        .collect();

    let lowered = graph
        .instructions()
        .iter()
        .try_fold(Lowered::empty(), |acc, instr| {
            lower_instruction(instr, graph)
                .map(|delta| acc.extend(delta))
        })?;

    let body: Vec<Stmt> = boolean_constraints
        .into_iter()
        .chain(lowered.stmts)
        .collect();

    Ok(Template::new(
        Field::Bn254,
        name,
        lowered.includes,
        ports,
        intermediates,
        body,
    ))
}

#[derive(Clone, Debug)]
struct Lowered {
    stmts: Vec<Stmt>,
    includes: Vec<String>,
}

impl Lowered {
    fn empty() -> Self {
        Self {
            stmts: Vec::new(),
            includes: Vec::new(),
        }
    }

    fn of_stmts(stmts: Vec<Stmt>) -> Self {
        Self {
            stmts,
            includes: Vec::new(),
        }
    }

    fn extend(self, other: Self) -> Self {
        let stmts =
            self.stmts.into_iter().chain(other.stmts).collect();
        let includes = self
            .includes
            .into_iter()
            .chain(
                other.includes.into_iter().filter(|inc| {
                    // Preserved include-set dedupe kept for future
                    // gadget emissions; v1 emits no includes so the
                    // filter is a no-op here.
                    !inc.is_empty()
                }),
            )
            .collect();
        Self { stmts, includes }
    }
}

fn lower_instruction(
    instr: &Instruction,
    graph: &HdlGraph,
) -> Result<Lowered, Error> {
    match instr.op() {
        Op::Not => Ok(Lowered::of_stmts(lower_not(instr, graph))),
        Op::Bin(BinOp::And) => {
            Ok(Lowered::of_stmts(lower_bitwise(instr, graph, BitwiseOp::And)))
        }
        Op::Bin(BinOp::Or) => {
            Ok(Lowered::of_stmts(lower_bitwise(instr, graph, BitwiseOp::Or)))
        }
        Op::Bin(BinOp::Xor) => {
            Ok(Lowered::of_stmts(lower_bitwise(instr, graph, BitwiseOp::Xor)))
        }
        Op::Mux => Ok(Lowered::of_stmts(lower_mux(instr, graph))),
        Op::Const { bits, .. } => {
            Ok(Lowered::of_stmts(lower_const(instr, bits)))
        }
        Op::Slice { lo, hi } => {
            Ok(Lowered::of_stmts(lower_slice(instr, *lo, *hi)))
        }
        Op::Concat { low_width, high_width } => Ok(Lowered::of_stmts(
            lower_concat(instr, *low_width, *high_width),
        )),
        Op::Bin(BinOp::Add) => Err(Error::UnsupportedInCircom(
            "add (Num2Bits-based arithmetic lowering pending)",
        )),
        Op::Bin(BinOp::Sub) => Err(Error::UnsupportedInCircom(
            "sub (Num2Bits-based arithmetic lowering pending)",
        )),
        Op::Bin(BinOp::Mul) => Err(Error::UnsupportedInCircom(
            "mul (Num2Bits-based arithmetic lowering pending)",
        )),
        Op::Bin(BinOp::Eq) => Err(Error::UnsupportedInCircom(
            "eq (IsEqual gadget wiring pending)",
        )),
        Op::Bin(BinOp::Lt) => Err(Error::UnsupportedInCircom(
            "lt (LessThan gadget wiring pending)",
        )),
        Op::Reg { .. } => Err(Error::UnsupportedInCircom(
            "reg (stateful; combinational-only in v1)",
        )),
        Op::ArrayShiftIn { .. } => Err(Error::UnsupportedInCircom(
            "array_shift_in (stateful; combinational-only in v1)",
        )),
        Op::ArrayTail { .. } => Err(Error::UnsupportedInCircom(
            "array_tail (stateful; combinational-only in v1)",
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitwiseOp {
    And,
    Or,
    Xor,
}

fn lower_not(instr: &Instruction, graph: &HdlGraph) -> Vec<Stmt> {
    let out = instr.output();
    let width = graph_width(graph, out);
    instr
        .inputs()
        .first()
        .copied()
        .map(|a| {
            (0..width)
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out),
                    index: Some(i),
                    rhs: Expr::Sub(
                        Box::new(Expr::BitLiteral(true)),
                        Box::new(bit_ref(a, i)),
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_bitwise(
    instr: &Instruction,
    graph: &HdlGraph,
    op: BitwiseOp,
) -> Vec<Stmt> {
    let out = instr.output();
    let width = graph_width(graph, out);
    instr
        .inputs()
        .first()
        .copied()
        .zip(instr.inputs().get(1).copied())
        .map(|(a, b)| {
            (0..width)
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out),
                    index: Some(i),
                    rhs: bitwise_expr(op, a, b, i),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn bitwise_expr(op: BitwiseOp, a: WireId, b: WireId, i: u32) -> Expr {
    let a_bit = bit_ref(a, i);
    let b_bit = bit_ref(b, i);
    match op {
        BitwiseOp::And => Expr::Mul(Box::new(a_bit), Box::new(b_bit)),
        BitwiseOp::Or => Expr::Sub(
            Box::new(Expr::Add(
                Box::new(a_bit.clone()),
                Box::new(b_bit.clone()),
            )),
            Box::new(Expr::Mul(Box::new(a_bit), Box::new(b_bit))),
        ),
        BitwiseOp::Xor => Expr::Sub(
            Box::new(Expr::Add(
                Box::new(a_bit.clone()),
                Box::new(b_bit.clone()),
            )),
            Box::new(Expr::Mul(
                Box::new(Expr::FieldLiteral(2)),
                Box::new(Expr::Mul(Box::new(a_bit), Box::new(b_bit))),
            )),
        ),
    }
}

fn lower_mux(instr: &Instruction, graph: &HdlGraph) -> Vec<Stmt> {
    let out = instr.output();
    let width = graph_width(graph, out);
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .zip(ins.get(2).copied())
        .map(|((sel, f), t)| {
            (0..width)
                .map(|i| {
                    let sel_bit = bit_ref(sel, 0);
                    let t_bit = bit_ref(t, i);
                    let f_bit = bit_ref(f, i);
                    let diff = Expr::Sub(
                        Box::new(t_bit),
                        Box::new(f_bit.clone()),
                    );
                    let prod = Expr::Mul(Box::new(sel_bit), Box::new(diff));
                    Stmt::Assign {
                        lhs: wire_name(out),
                        index: Some(i),
                        rhs: Expr::Add(Box::new(prod), Box::new(f_bit)),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_const(instr: &Instruction, bits: &BitSeq) -> Vec<Stmt> {
    let out = instr.output();
    bits.as_slice()
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            u32::try_from(i).ok().map(|idx| Stmt::Assign {
                lhs: wire_name(out),
                index: Some(idx),
                rhs: Expr::BitLiteral(*b),
            })
        })
        .collect()
}

fn lower_slice(instr: &Instruction, lo: u32, hi: u32) -> Vec<Stmt> {
    let out = instr.output();
    instr
        .inputs()
        .first()
        .copied()
        .map(|src| {
            (0..hi.saturating_sub(lo))
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out),
                    index: Some(i),
                    rhs: bit_ref(src, lo + i),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_concat(
    instr: &Instruction,
    low_width: u32,
    high_width: u32,
) -> Vec<Stmt> {
    let out = instr.output();
    instr
        .inputs()
        .first()
        .copied()
        .zip(instr.inputs().get(1).copied())
        .map(|(low, high)| {
            let low_bits = (0..low_width).map(|i| Stmt::Assign {
                lhs: wire_name(out),
                index: Some(i),
                rhs: bit_ref(low, i),
            });
            let high_bits = (0..high_width).map(|j| Stmt::Assign {
                lhs: wire_name(out),
                index: Some(low_width + j),
                rhs: bit_ref(high, j),
            });
            low_bits.chain(high_bits).collect()
        })
        .unwrap_or_default()
}

fn boolean_constraint(w: WireId, bit: u32) -> Stmt {
    let bit_expr = bit_ref(w, bit);
    Stmt::Constraint(Expr::Mul(
        Box::new(bit_expr.clone()),
        Box::new(Expr::Sub(
            Box::new(bit_expr),
            Box::new(Expr::BitLiteral(true)),
        )),
    ))
}

fn bit_ref(w: WireId, index: u32) -> Expr {
    Expr::Bit {
        sig: wire_name(w),
        index,
    }
}

fn wire_name(w: WireId) -> String {
    format!("w{}", w.index())
}

fn graph_width(graph: &HdlGraph, w: WireId) -> u32 {
    graph.wire_ty(w.index()).map_or(0, WireTy::width)
}

#[cfg(test)]
mod tests {
    use super::emit_template;
    use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
    use hdl_cat_kind::BitSeq;

    #[test]
    fn emits_inverter_template() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Not, vec![a], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "inv4", &[a], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("template inv4()"));
        assert!(text.contains("signal input w0[4];"));
        assert!(text.contains("signal output w1[4];"));
        assert!(text.contains("w1[0] <== (1 - w0[0]);"));
        assert!(text.contains("w1[3] <== (1 - w0[3]);"));
        assert!(text.contains("(w0[0] * (w0[0] - 1)) === 0;"));
        Ok(())
    }

    #[test]
    fn emits_xor_per_bit() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(2));
        let (b, c) = b.with_wire(WireTy::Bits(2));
        let (b, out) = b.with_wire(WireTy::Bits(2));
        let b = b.with_instruction(Op::Bin(BinOp::Xor), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "xor2", &[a, c], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== ((w0[0] + w1[0]) - (2 * (w0[0] * w1[0])));"));
        Ok(())
    }

    #[test]
    fn emits_and_per_bit() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, c) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::And), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "and1", &[a, c], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== (w0[0] * w1[0]);"));
        Ok(())
    }

    #[test]
    fn emits_or_per_bit() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, c) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Or), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "or1", &[a, c], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== ((w0[0] + w1[0]) - (w0[0] * w1[0]));"));
        Ok(())
    }

    #[test]
    fn emits_mux_per_bit() -> Result<(), hdl_cat_error::Error> {
        let (b, sel) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
        let (b, f) = b.with_wire(WireTy::Bits(1));
        let (b, t_arm) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b =
            b.with_instruction(Op::Mux, vec![sel, f, t_arm], out)?;
        let graph = b.build();

        let tpl =
            emit_template(&graph, "mux1", &[sel, f, t_arm], &[out]).run()?;
        let text = tpl.render().run()?;
        assert!(text.contains("w3[0] <== ((w0[0] * (w2[0] - w1[0])) + w1[0]);"));
        Ok(())
    }

    #[test]
    fn emits_const_as_bit_literals() -> Result<(), hdl_cat_error::Error> {
        let bits = BitSeq::from_vec(vec![true, false, true, false]);
        let (b, out) =
            HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let b = b.with_instruction(
            Op::Const {
                bits: bits.clone(),
                ty: WireTy::Bits(4),
            },
            Vec::new(),
            out,
        )?;
        let graph = b.build();

        let t = emit_template(&graph, "c4", &[], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w0[0] <== 1;"));
        assert!(text.contains("w0[1] <== 0;"));
        assert!(text.contains("w0[2] <== 1;"));
        assert!(text.contains("w0[3] <== 0;"));
        Ok(())
    }

    #[test]
    fn emits_slice_reindexes_bits() -> Result<(), hdl_cat_error::Error> {
        let (b, src) =
            HdlGraphBuilder::new().with_wire(WireTy::Bits(8));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(
            Op::Slice { lo: 2, hi: 6 },
            vec![src],
            out,
        )?;
        let graph = b.build();

        let t = emit_template(&graph, "s", &[src], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w1[0] <== w0[2];"));
        assert!(text.contains("w1[3] <== w0[5];"));
        Ok(())
    }

    #[test]
    fn emits_concat_low_then_high() -> Result<(), hdl_cat_error::Error> {
        let (b, low) =
            HdlGraphBuilder::new().with_wire(WireTy::Bits(2));
        let (b, high) = b.with_wire(WireTy::Bits(2));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(
            Op::Concat {
                low_width: 2,
                high_width: 2,
            },
            vec![low, high],
            out,
        )?;
        let graph = b.build();

        let t = emit_template(&graph, "cat", &[low, high], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== w0[0];"));
        assert!(text.contains("w2[1] <== w0[1];"));
        assert!(text.contains("w2[2] <== w1[0];"));
        assert!(text.contains("w2[3] <== w1[1];"));
        Ok(())
    }

    #[test]
    fn intermediate_wires_become_local_signals(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, tmp) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Not, vec![a], tmp)?;
        let b = b.with_instruction(Op::Not, vec![tmp], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "inv_inv", &[a], &[out]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("signal w1[4];"));
        Ok(())
    }

    #[test]
    fn rejects_add_with_unsupported_in_circom(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let result = emit_template(&graph, "add", &[a, c], &[out]).run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }

    #[test]
    fn rejects_reg_with_unsupported_in_circom(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(
            Op::Reg {
                init: BitSeq::from_vec(vec![false; 4]),
                ty: WireTy::Bits(4),
            },
            vec![a],
            out,
        )?;
        let graph = b.build();

        let result = emit_template(&graph, "r", &[a], &[out]).run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }
}
