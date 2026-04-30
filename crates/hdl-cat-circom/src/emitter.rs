//! Convert an [`hdl_cat_ir::HdlGraph`] to a Circom [`Template`].
//!
//! The backend is combinational and bit-level.  Every hdl-cat wire
//! of width `N` becomes `N` Circom signals, each constrained to
//! `{0, 1}` via a boolean constraint emitted on every input bit.
//!
//! Op coverage:
//!
//! - Bitwise `Not`/`And`/`Or`/`Xor`, `Mux`, `Const`, `Slice`,
//!   `Concat`: per-bit assignments, no external includes.
//! - `Add`/`Sub`/`Mul`: the operand bits are packed via `Bits2Num`
//!   into a field sum/difference/product, fed through circomlib's
//!   `Num2Bits(k)` gadget (with `k = N+1` for add/sub and `k = 2N`
//!   for mul), and the low `N` bits become the output.  Subtraction
//!   adds a `2^N` bias so the `Num2Bits` input stays non-negative.
//!   Emits `include "circomlib/circuits/bitify.circom";`.
//!   At `N == 1` the emitter takes a fast path: `Add`/`Sub` reduce
//!   to per-bit `XOR` and `Mul` reduces to per-bit `AND`, skipping
//!   the `Bits2Num`/`Num2Bits` round-trip and the `bitify` include.
//! - `Eq`/`Lt`: circomlib's `IsEqual`/`LessThan` gadgets.  Emits
//!   `include "circomlib/circuits/comparators.circom";`.
//! - `Reg`, `ArrayShiftIn`, `ArrayTail`: stateful ops with no
//!   combinational Circom lowering.  These return
//!   [`hdl_cat_error::Error::UnsupportedInCircom`] so the Verilog
//!   backend remains the authoritative target for stateful designs.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::{Error, SignalName};
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
/// The `public_inputs` list selects which input wires are exposed
/// as public on the rendered `component main` line.  Pass `&[]`
/// for a fully-private interface.  Every entry must already appear
/// in `input_wires`.
///
/// # Errors
///
/// - Returns [`Error::UnsupportedInCircom`] when a stateful op
///   (`Reg`, `ArrayShiftIn`, `ArrayTail`) appears in the graph,
///   since those have no combinational Circom lowering.
/// - Returns [`Error::UndefinedSignal`] when a wire in
///   `public_inputs` does not appear in `input_wires`.
#[must_use]
pub fn emit_template(
    graph: &HdlGraph,
    name: &str,
    input_wires: &[WireId],
    output_wires: &[WireId],
    public_inputs: &[WireId],
) -> Io<Error, Template> {
    let graph_owned = graph.clone();
    let name_owned = name.to_string();
    let inputs_owned: Vec<WireId> = input_wires.to_vec();
    let outputs_owned: Vec<WireId> = output_wires.to_vec();
    let publics_owned: Vec<WireId> = public_inputs.to_vec();
    Io::suspend(move || {
        build_template(
            &graph_owned,
            &name_owned,
            &inputs_owned,
            &outputs_owned,
            &publics_owned,
        )
    })
}

fn build_template(
    graph: &HdlGraph,
    name: &str,
    inputs: &[WireId],
    outputs: &[WireId],
    publics: &[WireId],
) -> Result<Template, Error> {
    let public_names: Vec<String> = publics
        .iter()
        .map(|w| {
            if inputs.contains(w) {
                Ok(wire_name(*w))
            } else {
                Err(Error::UndefinedSignal {
                    name: SignalName::new(format!(
                        "public input {} not in input list",
                        wire_name(*w),
                    )),
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

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
        public_names,
    ))
}

#[derive(Clone, Debug, Default)]
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
        let stmts = self.stmts.into_iter().chain(other.stmts).collect();
        let includes =
            other.includes.into_iter().fold(self.includes, |acc, inc| {
                if acc.contains(&inc) {
                    acc
                } else {
                    acc.into_iter().chain(std::iter::once(inc)).collect()
                }
            });
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
        Op::Bin(BinOp::Add) => Ok(lower_add(instr, graph)),
        Op::Bin(BinOp::Sub) => Ok(lower_sub(instr, graph)),
        Op::Bin(BinOp::Mul) => Ok(lower_mul(instr, graph)),
        Op::Bin(BinOp::Eq) => Ok(lower_eq(instr, graph)),
        Op::Bin(BinOp::Lt) => Ok(lower_lt(instr, graph)),
        Op::Reg { .. } => Err(Error::UnsupportedInCircom(
            "reg (stateful; combinational-only backend)",
        )),
        Op::ArrayShiftIn { .. } => Err(Error::UnsupportedInCircom(
            "array_shift_in (stateful; combinational-only backend)",
        )),
        Op::ArrayTail { .. } => Err(Error::UnsupportedInCircom(
            "array_tail (stateful; combinational-only backend)",
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
        .map(|(a, b)| bitwise_assigns(out, a, b, width, op))
        .unwrap_or_default()
}

fn bitwise_assigns(
    out: WireId,
    a: WireId,
    b: WireId,
    width: u32,
    op: BitwiseOp,
) -> Vec<Stmt> {
    (0..width)
        .map(|i| Stmt::Assign {
            lhs: wire_name(out),
            index: Some(i),
            rhs: bitwise_expr(op, a, b, i),
        })
        .collect()
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

fn lower_add(instr: &Instruction, graph: &HdlGraph) -> Lowered {
    let out = instr.output();
    let width = graph_width(graph, out);
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| match () {
            // a + b mod 2 == a XOR b: emit a single per-bit XOR
            // and skip the Bits2Num/Num2Bits round-trip and the
            // bitify include.
            () if width == 1 => {
                Lowered::of_stmts(bitwise_assigns(out, a, b, 1, BitwiseOp::Xor))
            }
            () => {
                let sum = Expr::Add(
                    Box::new(bits_to_num(a, width)),
                    Box::new(bits_to_num(b, width)),
                );
                num2bits_wrap("add", out, width, width.saturating_add(1), sum)
            }
        })
        .unwrap_or_default()
}

fn lower_sub(instr: &Instruction, graph: &HdlGraph) -> Lowered {
    let out = instr.output();
    let width = graph_width(graph, out);
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| match () {
            // a - b mod 2 == a + b mod 2 == a XOR b: same
            // short-circuit as add.
            () if width == 1 => {
                Lowered::of_stmts(bitwise_assigns(out, a, b, 1, BitwiseOp::Xor))
            }
            () => {
                // a - b wraps on 2^width.  Adding 2^width keeps the Num2Bits
                // input non-negative without disturbing the low `width` bits.
                let bias = 1u128.checked_shl(width).unwrap_or(0);
                let diff = Expr::Sub(
                    Box::new(bits_to_num(a, width)),
                    Box::new(bits_to_num(b, width)),
                );
                let shifted = Expr::Add(
                    Box::new(diff),
                    Box::new(Expr::FieldLiteral(bias)),
                );
                num2bits_wrap("sub", out, width, width.saturating_add(1), shifted)
            }
        })
        .unwrap_or_default()
}

fn lower_mul(instr: &Instruction, graph: &HdlGraph) -> Lowered {
    let out = instr.output();
    let width = graph_width(graph, out);
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| match () {
            // a * b mod 2 == a AND b: emit a single per-bit AND
            // and skip the Bits2Num/Num2Bits round-trip and the
            // bitify include.
            () if width == 1 => {
                Lowered::of_stmts(bitwise_assigns(out, a, b, 1, BitwiseOp::And))
            }
            () => {
                let prod = Expr::Mul(
                    Box::new(bits_to_num(a, width)),
                    Box::new(bits_to_num(b, width)),
                );
                num2bits_wrap("mul", out, width, width.saturating_mul(2), prod)
            }
        })
        .unwrap_or_default()
}

fn lower_eq(instr: &Instruction, graph: &HdlGraph) -> Lowered {
    let out = instr.output();
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| {
            let a_width = graph_width(graph, a);
            let b_width = graph_width(graph, b);
            let comp = component_name("eq", out);
            let decl = Stmt::Component {
                name: comp.clone(),
                template: "IsEqual".to_string(),
                args: Vec::new(),
            };
            let drive_a = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(0),
                rhs: bits_to_num(a, a_width),
            };
            let drive_b = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(1),
                rhs: bits_to_num(b, b_width),
            };
            let assign = Stmt::Assign {
                lhs: wire_name(out),
                index: Some(0),
                rhs: Expr::CompOutField {
                    comp,
                    port: "out".to_string(),
                },
            };
            Lowered {
                stmts: vec![decl, drive_a, drive_b, assign],
                includes: vec![comparators_include()],
            }
        })
        .unwrap_or_default()
}

fn lower_lt(instr: &Instruction, graph: &HdlGraph) -> Lowered {
    let out = instr.output();
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| {
            let a_width = graph_width(graph, a);
            let b_width = graph_width(graph, b);
            let comp = component_name("lt", out);
            let decl = Stmt::Component {
                name: comp.clone(),
                template: "LessThan".to_string(),
                args: vec![a_width],
            };
            let drive_a = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(0),
                rhs: bits_to_num(a, a_width),
            };
            let drive_b = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(1),
                rhs: bits_to_num(b, b_width),
            };
            let assign = Stmt::Assign {
                lhs: wire_name(out),
                index: Some(0),
                rhs: Expr::CompOutField {
                    comp,
                    port: "out".to_string(),
                },
            };
            Lowered {
                stmts: vec![decl, drive_a, drive_b, assign],
                includes: vec![comparators_include()],
            }
        })
        .unwrap_or_default()
}

fn num2bits_wrap(
    tag: &str,
    out: WireId,
    out_width: u32,
    n_bits: u32,
    rhs: Expr,
) -> Lowered {
    let comp = component_name(tag, out);
    let decl = Stmt::Component {
        name: comp.clone(),
        template: "Num2Bits".to_string(),
        args: vec![n_bits],
    };
    let drive = Stmt::ComponentDrive {
        comp: comp.clone(),
        port: "in".to_string(),
        index: None,
        rhs,
    };
    let assigns = (0..out_width).map(|i| Stmt::Assign {
        lhs: wire_name(out),
        index: Some(i),
        rhs: Expr::CompOutBit {
            comp: comp.clone(),
            port: "out".to_string(),
            index: i,
        },
    });
    let stmts: Vec<Stmt> = std::iter::once(decl)
        .chain(std::iter::once(drive))
        .chain(assigns)
        .collect();
    Lowered {
        stmts,
        includes: vec![bitify_include()],
    }
}

fn bits_to_num(w: WireId, width: u32) -> Expr {
    Expr::Bits2Num((0..width).map(|i| bit_ref(w, i)).collect())
}

fn component_name(tag: &str, out: WireId) -> String {
    format!("{tag}_w{}", out.index())
}

fn bitify_include() -> String {
    "circomlib/circuits/bitify.circom".to_string()
}

fn comparators_include() -> String {
    "circomlib/circuits/comparators.circom".to_string()
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

        let t = emit_template(&graph, "inv4", &[a], &[out], &[]).run()?;
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

        let t = emit_template(&graph, "xor2", &[a, c], &[out], &[]).run()?;
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

        let t = emit_template(&graph, "and1", &[a, c], &[out], &[]).run()?;
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

        let t = emit_template(&graph, "or1", &[a, c], &[out], &[]).run()?;
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

        let tpl = emit_template(
            &graph,
            "mux1",
            &[sel, f, t_arm],
            &[out],
            &[],
        )
        .run()?;
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

        let t = emit_template(&graph, "c4", &[], &[out], &[]).run()?;
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

        let t = emit_template(&graph, "s", &[src], &[out], &[]).run()?;
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

        let t =
            emit_template(&graph, "cat", &[low, high], &[out], &[]).run()?;
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

        let t =
            emit_template(&graph, "inv_inv", &[a], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("signal w1[4];"));
        Ok(())
    }

    #[test]
    fn emits_add_via_num2bits() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "add4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("include \"circomlib/circuits/bitify.circom\";"));
        assert!(text.contains("component add_w2 = Num2Bits(5);"));
        assert!(text.contains("add_w2.in <=="));
        assert!(text.contains("w2[0] <== add_w2.out[0];"));
        assert!(text.contains("w2[3] <== add_w2.out[3];"));
        // The overflow bit at index 4 is discarded.
        assert!(!text.contains("w2[4] <=="));
        Ok(())
    }

    #[test]
    fn emits_sub_with_two_to_the_width_bias(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Sub), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "sub4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component sub_w2 = Num2Bits(5);"));
        // 2^4 == 16 bias keeps the Num2Bits input non-negative.
        assert!(text.contains(" + 16)"));
        assert!(text.contains("w2[0] <== sub_w2.out[0];"));
        Ok(())
    }

    #[test]
    fn emits_mul_via_num2bits_double_width(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Mul), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "mul4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component mul_w2 = Num2Bits(8);"));
        assert!(text.contains("w2[0] <== mul_w2.out[0];"));
        assert!(text.contains("w2[3] <== mul_w2.out[3];"));
        assert!(!text.contains("w2[4] <=="));
        Ok(())
    }

    #[test]
    fn emits_eq_via_is_equal() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Eq), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "eq4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(
            text.contains("include \"circomlib/circuits/comparators.circom\";"),
        );
        assert!(text.contains("component eq_w2 = IsEqual();"));
        assert!(text.contains("eq_w2.in[0] <=="));
        assert!(text.contains("eq_w2.in[1] <=="));
        assert!(text.contains("w2[0] <== eq_w2.out;"));
        Ok(())
    }

    #[test]
    fn emits_lt_via_less_than() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Lt), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "lt4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(
            text.contains("include \"circomlib/circuits/comparators.circom\";"),
        );
        assert!(text.contains("component lt_w2 = LessThan(4);"));
        assert!(text.contains("lt_w2.in[0] <=="));
        assert!(text.contains("lt_w2.in[1] <=="));
        assert!(text.contains("w2[0] <== lt_w2.out;"));
        Ok(())
    }

    #[test]
    fn dedupes_repeated_includes() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, tmp) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], tmp)?;
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![tmp, a], out)?;
        let graph = b.build();

        let t =
            emit_template(&graph, "add_twice", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        let bitify_hits =
            text.matches("include \"circomlib/circuits/bitify.circom\"").count();
        assert_eq!(bitify_hits, 1);
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

        let result = emit_template(&graph, "r", &[a], &[out], &[]).run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }

    #[test]
    fn marks_single_public_input() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "add4", &[a, c], &[out], &[a]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component main { public [w0] } = add4();"));
        Ok(())
    }

    #[test]
    fn marks_multiple_public_inputs(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t =
            emit_template(&graph, "add4", &[a, c], &[out], &[a, c]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component main { public [w0, w1] } = add4();"));
        Ok(())
    }

    #[test]
    fn empty_public_list_keeps_plain_main(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "add4", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component main = add4();"));
        assert!(!text.contains("public ["));
        Ok(())
    }

    #[test]
    fn rejects_public_input_not_in_input_list(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (b, c) = b.with_wire(WireTy::Bits(4));
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        // Wire `out` is an output, not an input — passing it as public
        // must fail.
        let result =
            emit_template(&graph, "add4", &[a, c], &[out], &[out]).run();
        let is_undefined =
            matches!(result, Err(hdl_cat_error::Error::UndefinedSignal { .. }));
        assert!(is_undefined);
        Ok(())
    }

    #[test]
    fn width_1_add_short_circuits_to_xor(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, c) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "add1", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== ((w0[0] + w1[0]) - (2 * (w0[0] * w1[0])));"));
        assert!(!text.contains("Num2Bits"));
        assert!(!text.contains("circomlib/circuits/bitify.circom"));
        Ok(())
    }

    #[test]
    fn width_1_sub_short_circuits_to_xor(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, c) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Sub), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "sub1", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== ((w0[0] + w1[0]) - (2 * (w0[0] * w1[0])));"));
        assert!(!text.contains("Num2Bits"));
        assert!(!text.contains("circomlib/circuits/bitify.circom"));
        Ok(())
    }

    #[test]
    fn width_1_mul_short_circuits_to_and(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, c) = b.with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Bin(BinOp::Mul), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "mul1", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w2[0] <== (w0[0] * w1[0]);"));
        assert!(!text.contains("Num2Bits"));
        assert!(!text.contains("circomlib/circuits/bitify.circom"));
        Ok(())
    }

    #[test]
    fn width_2_add_still_uses_num2bits(
    ) -> Result<(), hdl_cat_error::Error> {
        // Sanity: the short-circuit applies only at width 1.  At width
        // 2 the emitter must still go through Bits2Num/Num2Bits.
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(2));
        let (b, c) = b.with_wire(WireTy::Bits(2));
        let (b, out) = b.with_wire(WireTy::Bits(2));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_template(&graph, "add2", &[a, c], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("component add_w2 = Num2Bits(3);"));
        assert!(text.contains("circomlib/circuits/bitify.circom"));
        Ok(())
    }
}
