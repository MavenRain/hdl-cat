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
//! - `ArrayShiftIn`, `ArrayTail`: lowered as per-bit assignments
//!   over the array's flat storage (`element_width * depth` bits).
//!   These ops only contribute non-trivial state semantics when
//!   wrapped in a [`hdl_cat_sync::Sync`] machine and unrolled with
//!   [`emit_unrolled_template`]; standalone they read and write the
//!   array as a flat bit-vector.
//! - `Reg`: still returns
//!   [`hdl_cat_error::Error::UnsupportedInCircom`] (its semantics
//!   differ across cycles, which the time-unrolled emitter would
//!   need to rewrite explicitly; not yet implemented).
//!
//! The time-unrolled emitter [`emit_unrolled_template`] replicates a
//! [`hdl_cat_sync::Sync`] machine's combinational `arrow_ir` `K`
//! times, pinning cycle-0 state wires to the machine's initial state
//! and plumbing each later cycle's state wires from the previous
//! cycle's next-state wires.  This produces a single Circom template
//! that captures `K` cycles of the Mealy machine.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::{Error, SignalName};
use hdl_cat_ir::{BinOp, HdlGraph, Instruction, Op, WireId, WireTy};
use hdl_cat_kind::BitSeq;

use crate::ast::{Expr, Field, Signal, SignalDir, Stmt, Template};

/// Naming context for wire and component identifiers in a lowered
/// Circom template.
///
/// In the plain backend (single combinational template) wires render
/// as `w<idx>`.  In the time-unrolled backend each wire is replicated
/// per cycle and renders as `w<idx>_c<cycle>` so that `K` copies of
/// the same combinational graph share no signal names.
#[derive(Clone, Copy, Debug)]
struct NamingContext {
    cycle: Option<usize>,
}

impl NamingContext {
    const fn plain() -> Self {
        Self { cycle: None }
    }

    const fn cycle(k: usize) -> Self {
        Self { cycle: Some(k) }
    }
}

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
/// - Returns [`Error::UnsupportedInCircom`] when an op without a
///   combinational Circom lowering (currently `Reg`) appears in
///   the graph.  `ArrayShiftIn` and `ArrayTail` are accepted and
///   lower to per-bit assignments on the array's flat storage.
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

/// Produce a Circom [`Template`] from a [`hdl_cat_sync::Sync`]
/// machine's combinational `arrow_ir`, time-unrolled across
/// `num_cycles` cycles.
///
/// The graph is expected to be the combinational `arrow_ir` of a
/// Mealy machine where:
///
/// - `input_wires = state_wires ++ data_input_wires` and
/// - `output_wires = next_state_wires ++ data_output_wires`,
///
/// with the first `state_wire_count` entries on each side carrying
/// state.  `initial_state` is in the same compact form used by
/// [`hdl_cat_sync::Sync::initial_state`]: one slice per state wire of
/// `element_width` bits (depth-replicated below the surface).
///
/// The emitter renders `num_cycles` cycle-renamed copies of the
/// graph (`w<idx>_c<k>` instead of `w<idx>`), drives each cycle-0
/// state wire from `initial_state` via per-bit `<==` assignments,
/// and plumbs each cycle-`k>0` state wire from cycle-`(k-1)`'s
/// next-state wire via per-bit copy assignments.  Per-cycle data
/// inputs become `signal input` ports and per-cycle data outputs
/// become `signal output` ports; everything else (state wires,
/// next-state wires, intermediates) becomes a local `signal` per
/// cycle.
///
/// # Errors
///
/// - Returns [`Error::UnsupportedInCircom`] when `num_cycles == 0`,
///   when `state_wire_count` exceeds either wire-list length, or
///   when `Op::Reg` appears in the graph (state plumbing for `Reg`
///   is not yet rewritten into the unrolled form).
#[must_use]
pub fn emit_unrolled_template(
    graph: &HdlGraph,
    name: &str,
    input_wires: &[WireId],
    output_wires: &[WireId],
    state_wire_count: usize,
    initial_state: &BitSeq,
    num_cycles: usize,
) -> Io<Error, Template> {
    let graph_owned = graph.clone();
    let name_owned = name.to_string();
    let inputs_owned: Vec<WireId> = input_wires.to_vec();
    let outputs_owned: Vec<WireId> = output_wires.to_vec();
    let initial_owned = initial_state.clone();
    Io::suspend(move || {
        build_unrolled_template(
            &graph_owned,
            &name_owned,
            &inputs_owned,
            &outputs_owned,
            state_wire_count,
            &initial_owned,
            num_cycles,
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
    let ctx = NamingContext::plain();
    let public_names: Vec<String> = publics
        .iter()
        .map(|w| {
            if inputs.contains(w) {
                Ok(wire_name(*w, ctx))
            } else {
                Err(Error::UndefinedSignal {
                    name: SignalName::new(format!(
                        "public input {} not in input list",
                        wire_name(*w, ctx),
                    )),
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let input_ports = inputs.iter().map(|w| {
        Signal::new(wire_name(*w, ctx), SignalDir::Input, graph_width(graph, *w))
    });
    let output_ports = outputs.iter().map(|w| {
        Signal::new(wire_name(*w, ctx), SignalDir::Output, graph_width(graph, *w))
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
                    wire_name(w, ctx),
                    SignalDir::Intermediate,
                    u32::try_from(ty.storage_bits()).unwrap_or(u32::MAX),
                ))
            }
        })
        .collect();

    let boolean_constraints: Vec<Stmt> = inputs
        .iter()
        .flat_map(|w| {
            let width = graph_width(graph, *w);
            (0..width).map(move |i| boolean_constraint(*w, i, ctx))
        })
        .collect();

    let lowered = graph
        .instructions()
        .iter()
        .try_fold(Lowered::empty(), |acc, instr| {
            lower_instruction(instr, graph, ctx)
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

fn build_unrolled_template(
    graph: &HdlGraph,
    name: &str,
    input_wires: &[WireId],
    output_wires: &[WireId],
    state_wire_count: usize,
    initial_state: &BitSeq,
    num_cycles: usize,
) -> Result<Template, Error> {
    let split_inputs = input_wires.split_at_checked(state_wire_count).ok_or(
        Error::UnsupportedInCircom(
            "state_wire_count exceeds input_wires length",
        ),
    )?;
    let split_outputs = output_wires.split_at_checked(state_wire_count).ok_or(
        Error::UnsupportedInCircom(
            "state_wire_count exceeds output_wires length",
        ),
    )?;
    let (state_inputs, data_inputs) = split_inputs;
    let (next_states, data_outputs) = split_outputs;

    match () {
        () if num_cycles == 0 => Err(Error::UnsupportedInCircom(
            "unrolled template requires at least one cycle",
        )),
        () => assemble_unrolled_template(
            graph,
            name,
            state_inputs,
            data_inputs,
            next_states,
            data_outputs,
            initial_state,
            num_cycles,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn assemble_unrolled_template(
    graph: &HdlGraph,
    name: &str,
    state_inputs: &[WireId],
    data_inputs: &[WireId],
    next_states: &[WireId],
    data_outputs: &[WireId],
    initial_state: &BitSeq,
    num_cycles: usize,
) -> Result<Template, Error> {
    let input_ports: Vec<Signal> = (0..num_cycles)
        .flat_map(|k| {
            let ctx = NamingContext::cycle(k);
            data_inputs.iter().map(move |w| {
                Signal::new(
                    wire_name(*w, ctx),
                    SignalDir::Input,
                    graph_width(graph, *w),
                )
            })
        })
        .collect();
    let output_ports: Vec<Signal> = (0..num_cycles)
        .flat_map(|k| {
            let ctx = NamingContext::cycle(k);
            data_outputs.iter().map(move |w| {
                Signal::new(
                    wire_name(*w, ctx),
                    SignalDir::Output,
                    graph_width(graph, *w),
                )
            })
        })
        .collect();
    let ports: Vec<Signal> =
        input_ports.into_iter().chain(output_ports).collect();

    let intermediates: Vec<Signal> = (0..num_cycles)
        .flat_map(|k| {
            let ctx = NamingContext::cycle(k);
            graph.wires().iter().enumerate().filter_map(move |(idx, ty)| {
                let w = WireId::new(idx);
                let is_data_port =
                    data_inputs.contains(&w) || data_outputs.contains(&w);
                match () {
                    () if is_data_port => None,
                    () => Some(Signal::new(
                        wire_name(w, ctx),
                        SignalDir::Intermediate,
                        u32::try_from(ty.storage_bits()).unwrap_or(u32::MAX),
                    )),
                }
            })
        })
        .collect();

    let initial_assigns =
        cycle_zero_state_assigns(graph, state_inputs, initial_state);

    let plumbing_assigns: Vec<Stmt> = (1..num_cycles)
        .flat_map(|k| {
            state_inputs
                .iter()
                .zip(next_states.iter())
                .flat_map(move |(state_in, next_state)| {
                    let storage = graph_width(graph, *state_in);
                    let prev = NamingContext::cycle(k - 1);
                    let curr = NamingContext::cycle(k);
                    (0..storage).map(move |p| Stmt::Assign {
                        lhs: wire_name(*state_in, curr),
                        index: Some(p),
                        rhs: bit_ref(*next_state, p, prev),
                    })
                })
        })
        .collect();

    let boolean_constraints: Vec<Stmt> = (0..num_cycles)
        .flat_map(|k| {
            let ctx = NamingContext::cycle(k);
            data_inputs.iter().flat_map(move |w| {
                let width = graph_width(graph, *w);
                (0..width).map(move |i| boolean_constraint(*w, i, ctx))
            })
        })
        .collect();

    let lowered =
        (0..num_cycles).try_fold(Lowered::empty(), |acc, k| {
            let ctx = NamingContext::cycle(k);
            graph.instructions().iter().try_fold(acc, |inner, instr| {
                lower_instruction(instr, graph, ctx)
                    .map(|delta| inner.extend(delta))
            })
        })?;

    let body: Vec<Stmt> = initial_assigns
        .into_iter()
        .chain(plumbing_assigns)
        .chain(boolean_constraints)
        .chain(lowered.stmts)
        .collect();

    Ok(Template::new(
        Field::Bn254,
        name,
        lowered.includes,
        ports,
        intermediates,
        body,
        Vec::new(),
    ))
}

/// Emit per-bit `<==` assignments that drive every cycle-0 state
/// wire from `initial_state`.
///
/// `initial_state` is in the compact form documented on
/// [`hdl_cat_sync::Sync::initial_state`]: the concatenation of one
/// slice per state wire whose length equals that wire's
/// [`WireTy::width`] (the per-element width).  For an array-typed
/// state wire of `element_width * depth` storage bits this slice is
/// replicated `depth` times so the cycle-0 array starts with every
/// element equal to the same compact value, matching what
/// `hdl-cat-sim`'s testbench expansion does.
fn cycle_zero_state_assigns(
    graph: &HdlGraph,
    state_inputs: &[WireId],
    initial_state: &BitSeq,
) -> Vec<Stmt> {
    let bits = initial_state.as_slice();
    state_inputs
        .iter()
        .fold(
            (0usize, Vec::<Stmt>::new()),
            |(off, acc), w| {
                let element_width =
                    graph.wire_ty(w.index()).map_or(0, WireTy::width);
                let storage = graph
                    .wire_ty(w.index())
                    .map_or(0, WireTy::storage_bits);
                let element_width_usize =
                    usize::try_from(element_width).unwrap_or(0);
                let assigns = (0..storage).filter_map(|bit_pos| {
                    u32::try_from(bit_pos).ok().map(|bit_idx| {
                        let modulus = element_width_usize.max(1);
                        let compact_idx = off + (bit_pos % modulus);
                        let value =
                            bits.get(compact_idx).copied().unwrap_or(false);
                        Stmt::Assign {
                            lhs: wire_name(*w, NamingContext::cycle(0)),
                            index: Some(bit_idx),
                            rhs: Expr::BitLiteral(value),
                        }
                    })
                });
                let combined: Vec<Stmt> =
                    acc.into_iter().chain(assigns).collect();
                (off + element_width_usize, combined)
            },
        )
        .1
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
    ctx: NamingContext,
) -> Result<Lowered, Error> {
    match instr.op() {
        Op::Not => Ok(Lowered::of_stmts(lower_not(instr, graph, ctx))),
        Op::Bin(BinOp::And) => Ok(Lowered::of_stmts(lower_bitwise(
            instr,
            graph,
            BitwiseOp::And,
            ctx,
        ))),
        Op::Bin(BinOp::Or) => Ok(Lowered::of_stmts(lower_bitwise(
            instr,
            graph,
            BitwiseOp::Or,
            ctx,
        ))),
        Op::Bin(BinOp::Xor) => Ok(Lowered::of_stmts(lower_bitwise(
            instr,
            graph,
            BitwiseOp::Xor,
            ctx,
        ))),
        Op::Mux => Ok(Lowered::of_stmts(lower_mux(instr, graph, ctx))),
        Op::Const { bits, .. } => {
            Ok(Lowered::of_stmts(lower_const(instr, bits, ctx)))
        }
        Op::Slice { lo, hi } => {
            Ok(Lowered::of_stmts(lower_slice(instr, *lo, *hi, ctx)))
        }
        Op::Concat { low_width, high_width } => Ok(Lowered::of_stmts(
            lower_concat(instr, *low_width, *high_width, ctx),
        )),
        Op::Bin(BinOp::Add) => Ok(lower_add(instr, graph, ctx)),
        Op::Bin(BinOp::Sub) => Ok(lower_sub(instr, graph, ctx)),
        Op::Bin(BinOp::Mul) => Ok(lower_mul(instr, graph, ctx)),
        Op::Bin(BinOp::Eq) => Ok(lower_eq(instr, graph, ctx)),
        Op::Bin(BinOp::Lt) => Ok(lower_lt(instr, graph, ctx)),
        Op::ArrayShiftIn { element_width, depth } => Ok(Lowered::of_stmts(
            lower_array_shift_in(instr, *element_width, *depth, ctx),
        )),
        Op::ArrayTail { element_width, depth } => Ok(Lowered::of_stmts(
            lower_array_tail(instr, *element_width, *depth, ctx),
        )),
        Op::Reg { .. } => Err(Error::UnsupportedInCircom(
            "reg (cross-cycle stateful; not yet rewritten by the unrolled emitter)",
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitwiseOp {
    And,
    Or,
    Xor,
}

fn lower_not(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    let width = graph_width(graph, out);
    instr
        .inputs()
        .first()
        .copied()
        .map(|a| {
            (0..width)
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out, ctx),
                    index: Some(i),
                    rhs: Expr::Sub(
                        Box::new(Expr::BitLiteral(true)),
                        Box::new(bit_ref(a, i, ctx)),
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
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    let width = graph_width(graph, out);
    instr
        .inputs()
        .first()
        .copied()
        .zip(instr.inputs().get(1).copied())
        .map(|(a, b)| bitwise_assigns(out, a, b, width, op, ctx))
        .unwrap_or_default()
}

fn bitwise_assigns(
    out: WireId,
    a: WireId,
    b: WireId,
    width: u32,
    op: BitwiseOp,
    ctx: NamingContext,
) -> Vec<Stmt> {
    (0..width)
        .map(|i| Stmt::Assign {
            lhs: wire_name(out, ctx),
            index: Some(i),
            rhs: bitwise_expr(op, a, b, i, ctx),
        })
        .collect()
}

fn bitwise_expr(
    op: BitwiseOp,
    a: WireId,
    b: WireId,
    i: u32,
    ctx: NamingContext,
) -> Expr {
    let a_bit = bit_ref(a, i, ctx);
    let b_bit = bit_ref(b, i, ctx);
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

fn lower_mux(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Vec<Stmt> {
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
                    let sel_bit = bit_ref(sel, 0, ctx);
                    let t_bit = bit_ref(t, i, ctx);
                    let f_bit = bit_ref(f, i, ctx);
                    let diff = Expr::Sub(
                        Box::new(t_bit),
                        Box::new(f_bit.clone()),
                    );
                    let prod = Expr::Mul(Box::new(sel_bit), Box::new(diff));
                    Stmt::Assign {
                        lhs: wire_name(out, ctx),
                        index: Some(i),
                        rhs: Expr::Add(Box::new(prod), Box::new(f_bit)),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_const(
    instr: &Instruction,
    bits: &BitSeq,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    bits.as_slice()
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            u32::try_from(i).ok().map(|idx| Stmt::Assign {
                lhs: wire_name(out, ctx),
                index: Some(idx),
                rhs: Expr::BitLiteral(*b),
            })
        })
        .collect()
}

fn lower_slice(
    instr: &Instruction,
    lo: u32,
    hi: u32,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    instr
        .inputs()
        .first()
        .copied()
        .map(|src| {
            (0..hi.saturating_sub(lo))
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out, ctx),
                    index: Some(i),
                    rhs: bit_ref(src, lo + i, ctx),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_concat(
    instr: &Instruction,
    low_width: u32,
    high_width: u32,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    instr
        .inputs()
        .first()
        .copied()
        .zip(instr.inputs().get(1).copied())
        .map(|(low, high)| {
            let low_bits = (0..low_width).map(|i| Stmt::Assign {
                lhs: wire_name(out, ctx),
                index: Some(i),
                rhs: bit_ref(low, i, ctx),
            });
            let high_bits = (0..high_width).map(|j| Stmt::Assign {
                lhs: wire_name(out, ctx),
                index: Some(low_width + j),
                rhs: bit_ref(high, j, ctx),
            });
            low_bits.chain(high_bits).collect()
        })
        .unwrap_or_default()
}

fn lower_array_shift_in(
    instr: &Instruction,
    element_width: u32,
    depth: usize,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    let kept = u32::try_from(depth.saturating_sub(1))
        .unwrap_or(u32::MAX)
        .saturating_mul(element_width);
    instr
        .inputs()
        .first()
        .copied()
        .zip(instr.inputs().get(1).copied())
        .map(|(array, new_elem)| {
            let new_bits = (0..element_width).map(|i| Stmt::Assign {
                lhs: wire_name(out, ctx),
                index: Some(i),
                rhs: bit_ref(new_elem, i, ctx),
            });
            let shifted = (0..kept).map(|j| Stmt::Assign {
                lhs: wire_name(out, ctx),
                index: Some(element_width + j),
                rhs: bit_ref(array, j, ctx),
            });
            new_bits.chain(shifted).collect()
        })
        .unwrap_or_default()
}

fn lower_array_tail(
    instr: &Instruction,
    element_width: u32,
    depth: usize,
    ctx: NamingContext,
) -> Vec<Stmt> {
    let out = instr.output();
    let tail_start = u32::try_from(depth.saturating_sub(1))
        .unwrap_or(u32::MAX)
        .saturating_mul(element_width);
    instr
        .inputs()
        .first()
        .copied()
        .map(|array| {
            (0..element_width)
                .map(|i| Stmt::Assign {
                    lhs: wire_name(out, ctx),
                    index: Some(i),
                    rhs: bit_ref(array, tail_start + i, ctx),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_add(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Lowered {
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
            () if width == 1 => Lowered::of_stmts(bitwise_assigns(
                out,
                a,
                b,
                1,
                BitwiseOp::Xor,
                ctx,
            )),
            () => {
                let sum = Expr::Add(
                    Box::new(bits_to_num(a, width, ctx)),
                    Box::new(bits_to_num(b, width, ctx)),
                );
                num2bits_wrap(
                    "add",
                    out,
                    width,
                    width.saturating_add(1),
                    sum,
                    ctx,
                )
            }
        })
        .unwrap_or_default()
}

fn lower_sub(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Lowered {
    let out = instr.output();
    let width = graph_width(graph, out);
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| match () {
            // a - b mod 2 == a + b mod 2 == a XOR b: same
            // short-circuit as add.
            () if width == 1 => Lowered::of_stmts(bitwise_assigns(
                out,
                a,
                b,
                1,
                BitwiseOp::Xor,
                ctx,
            )),
            () => {
                // a - b wraps on 2^width.  Adding 2^width keeps the Num2Bits
                // input non-negative without disturbing the low `width` bits.
                let bias = 1u128.checked_shl(width).unwrap_or(0);
                let diff = Expr::Sub(
                    Box::new(bits_to_num(a, width, ctx)),
                    Box::new(bits_to_num(b, width, ctx)),
                );
                let shifted = Expr::Add(
                    Box::new(diff),
                    Box::new(Expr::FieldLiteral(bias)),
                );
                num2bits_wrap(
                    "sub",
                    out,
                    width,
                    width.saturating_add(1),
                    shifted,
                    ctx,
                )
            }
        })
        .unwrap_or_default()
}

fn lower_mul(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Lowered {
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
            () if width == 1 => Lowered::of_stmts(bitwise_assigns(
                out,
                a,
                b,
                1,
                BitwiseOp::And,
                ctx,
            )),
            () => {
                let prod = Expr::Mul(
                    Box::new(bits_to_num(a, width, ctx)),
                    Box::new(bits_to_num(b, width, ctx)),
                );
                num2bits_wrap(
                    "mul",
                    out,
                    width,
                    width.saturating_mul(2),
                    prod,
                    ctx,
                )
            }
        })
        .unwrap_or_default()
}

fn lower_eq(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Lowered {
    let out = instr.output();
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| {
            let a_width = graph_width(graph, a);
            let b_width = graph_width(graph, b);
            let comp = component_name("eq", out, ctx);
            let decl = Stmt::Component {
                name: comp.clone(),
                template: "IsEqual".to_string(),
                args: Vec::new(),
            };
            let drive_a = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(0),
                rhs: bits_to_num(a, a_width, ctx),
            };
            let drive_b = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(1),
                rhs: bits_to_num(b, b_width, ctx),
            };
            let assign = Stmt::Assign {
                lhs: wire_name(out, ctx),
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

fn lower_lt(
    instr: &Instruction,
    graph: &HdlGraph,
    ctx: NamingContext,
) -> Lowered {
    let out = instr.output();
    let ins = instr.inputs();
    ins.first()
        .copied()
        .zip(ins.get(1).copied())
        .map(|(a, b)| {
            let a_width = graph_width(graph, a);
            let b_width = graph_width(graph, b);
            let comp = component_name("lt", out, ctx);
            let decl = Stmt::Component {
                name: comp.clone(),
                template: "LessThan".to_string(),
                args: vec![a_width],
            };
            let drive_a = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(0),
                rhs: bits_to_num(a, a_width, ctx),
            };
            let drive_b = Stmt::ComponentDrive {
                comp: comp.clone(),
                port: "in".to_string(),
                index: Some(1),
                rhs: bits_to_num(b, b_width, ctx),
            };
            let assign = Stmt::Assign {
                lhs: wire_name(out, ctx),
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
    ctx: NamingContext,
) -> Lowered {
    let comp = component_name(tag, out, ctx);
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
        lhs: wire_name(out, ctx),
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

fn bits_to_num(w: WireId, width: u32, ctx: NamingContext) -> Expr {
    Expr::Bits2Num((0..width).map(|i| bit_ref(w, i, ctx)).collect())
}

fn component_name(tag: &str, out: WireId, ctx: NamingContext) -> String {
    ctx.cycle.map_or_else(
        || format!("{tag}_w{}", out.index()),
        |k| format!("{tag}_w{}_c{}", out.index(), k),
    )
}

fn bitify_include() -> String {
    "circomlib/circuits/bitify.circom".to_string()
}

fn comparators_include() -> String {
    "circomlib/circuits/comparators.circom".to_string()
}

fn boolean_constraint(w: WireId, bit: u32, ctx: NamingContext) -> Stmt {
    let bit_expr = bit_ref(w, bit, ctx);
    Stmt::Constraint(Expr::Mul(
        Box::new(bit_expr.clone()),
        Box::new(Expr::Sub(
            Box::new(bit_expr),
            Box::new(Expr::BitLiteral(true)),
        )),
    ))
}

fn bit_ref(w: WireId, index: u32, ctx: NamingContext) -> Expr {
    Expr::Bit {
        sig: wire_name(w, ctx),
        index,
    }
}

fn wire_name(w: WireId, ctx: NamingContext) -> String {
    ctx.cycle.map_or_else(
        || format!("w{}", w.index()),
        |k| format!("w{}_c{}", w.index(), k),
    )
}

fn graph_width(graph: &HdlGraph, w: WireId) -> u32 {
    graph
        .wire_ty(w.index())
        .map_or(0, |ty| u32::try_from(ty.storage_bits()).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::{emit_template, emit_unrolled_template};
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

    #[test]
    fn emits_array_shift_in_per_bit() -> Result<(), hdl_cat_error::Error> {
        // Array of 2 elements of 4 bits = 8 storage bits.  ArrayShiftIn
        // prepends a fresh 4-bit element and drops the oldest, so the
        // 8-bit output is `new ++ array[0..4]`.
        let (b, arr) = HdlGraphBuilder::new()
            .with_wire(WireTy::Array { element_width: 4, depth: 2 });
        let (b, new_elem) = b.with_wire(WireTy::Bits(4));
        let (b, out) =
            b.with_wire(WireTy::Array { element_width: 4, depth: 2 });
        let b = b.with_instruction(
            Op::ArrayShiftIn { element_width: 4, depth: 2 },
            vec![arr, new_elem],
            out,
        )?;
        let graph = b.build();

        let t = emit_template(
            &graph,
            "shift",
            &[arr, new_elem],
            &[out],
            &[],
        )
        .run()?;
        let text = t.render().run()?;
        // First 4 bits come from the new element.
        assert!(text.contains("w2[0] <== w1[0];"));
        assert!(text.contains("w2[3] <== w1[3];"));
        // Next 4 bits come from the array's first 4 storage bits.
        assert!(text.contains("w2[4] <== w0[0];"));
        assert!(text.contains("w2[7] <== w0[3];"));
        Ok(())
    }

    #[test]
    fn emits_array_tail_per_bit() -> Result<(), hdl_cat_error::Error> {
        // Tail of an Array<4, 2>: bits at storage positions 4..8.
        let (b, arr) = HdlGraphBuilder::new()
            .with_wire(WireTy::Array { element_width: 4, depth: 2 });
        let (b, out) = b.with_wire(WireTy::Bits(4));
        let b = b.with_instruction(
            Op::ArrayTail { element_width: 4, depth: 2 },
            vec![arr],
            out,
        )?;
        let graph = b.build();

        let t =
            emit_template(&graph, "tail", &[arr], &[out], &[]).run()?;
        let text = t.render().run()?;
        assert!(text.contains("w1[0] <== w0[4];"));
        assert!(text.contains("w1[3] <== w0[7];"));
        Ok(())
    }

    #[test]
    fn unrolled_emits_cycle_renamed_signals(
    ) -> Result<(), hdl_cat_error::Error> {
        // XOR accumulator: state' = state XOR data_in, data_out = state.
        let (b, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, data_in) = b.with_wire(WireTy::Bits(1));
        let (b, next_state) = b.with_wire(WireTy::Bits(1));
        let (b, data_out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(
            Op::Bin(BinOp::Xor),
            vec![state, data_in],
            next_state,
        )?;
        let b = b.with_instruction(
            Op::Slice { lo: 0, hi: 1 },
            vec![state],
            data_out,
        )?;
        let graph = b.build();

        let initial = BitSeq::from_vec(vec![false]);
        let t = emit_unrolled_template(
            &graph,
            "xor_acc",
            &[state, data_in],
            &[next_state, data_out],
            1,
            &initial,
            3,
        )
        .run()?;
        let text = t.render().run()?;

        // Per-cycle data inputs and outputs become template ports.
        assert!(text.contains("signal input w1_c0[1];"));
        assert!(text.contains("signal input w1_c1[1];"));
        assert!(text.contains("signal input w1_c2[1];"));
        assert!(text.contains("signal output w3_c0[1];"));
        assert!(text.contains("signal output w3_c1[1];"));
        assert!(text.contains("signal output w3_c2[1];"));
        // State wires are local signals at every cycle.
        assert!(text.contains("signal w0_c0[1];"));
        assert!(text.contains("signal w0_c1[1];"));
        assert!(text.contains("signal w0_c2[1];"));
        Ok(())
    }

    #[test]
    fn unrolled_initializes_cycle_zero_state_from_initial(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, data_in) = b.with_wire(WireTy::Bits(1));
        let (b, next_state) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(
            Op::Bin(BinOp::Xor),
            vec![state, data_in],
            next_state,
        )?;
        let graph = b.build();

        let initial = BitSeq::from_vec(vec![true]);
        let t = emit_unrolled_template(
            &graph,
            "init",
            &[state, data_in],
            &[next_state],
            1,
            &initial,
            1,
        )
        .run()?;
        let text = t.render().run()?;
        assert!(text.contains("w0_c0[0] <== 1;"));
        Ok(())
    }

    #[test]
    fn unrolled_plumbs_state_from_previous_next_state(
    ) -> Result<(), hdl_cat_error::Error> {
        // Two cycles: cycle-1 state input == cycle-0 next-state output.
        let (b, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, data_in) = b.with_wire(WireTy::Bits(1));
        let (b, next_state) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(
            Op::Bin(BinOp::Xor),
            vec![state, data_in],
            next_state,
        )?;
        let graph = b.build();

        let initial = BitSeq::from_vec(vec![false]);
        let t = emit_unrolled_template(
            &graph,
            "plumb",
            &[state, data_in],
            &[next_state],
            1,
            &initial,
            2,
        )
        .run()?;
        let text = t.render().run()?;
        assert!(text.contains("w0_c1[0] <== w2_c0[0];"));
        Ok(())
    }

    #[test]
    fn unrolled_replicates_array_initial_across_storage(
    ) -> Result<(), hdl_cat_error::Error> {
        // Array<4, 2>: 8 storage bits, 4 compact bits.  Initial value
        // 0b1010 (LSB-first [false, true, false, true]) must repeat
        // twice across the 8-bit storage.
        let (b, state) = HdlGraphBuilder::new()
            .with_wire(WireTy::Array { element_width: 4, depth: 2 });
        let (b, new_elem) = b.with_wire(WireTy::Bits(4));
        let (b, next_state) =
            b.with_wire(WireTy::Array { element_width: 4, depth: 2 });
        let b = b.with_instruction(
            Op::ArrayShiftIn { element_width: 4, depth: 2 },
            vec![state, new_elem],
            next_state,
        )?;
        let graph = b.build();

        let initial = BitSeq::from_vec(vec![false, true, false, true]);
        let t = emit_unrolled_template(
            &graph,
            "delay",
            &[state, new_elem],
            &[next_state],
            1,
            &initial,
            1,
        )
        .run()?;
        let text = t.render().run()?;
        assert!(text.contains("w0_c0[0] <== 0;"));
        assert!(text.contains("w0_c0[1] <== 1;"));
        assert!(text.contains("w0_c0[2] <== 0;"));
        assert!(text.contains("w0_c0[3] <== 1;"));
        // Bits 4..8 are a replication of bits 0..4.
        assert!(text.contains("w0_c0[4] <== 0;"));
        assert!(text.contains("w0_c0[5] <== 1;"));
        assert!(text.contains("w0_c0[6] <== 0;"));
        assert!(text.contains("w0_c0[7] <== 1;"));
        Ok(())
    }

    #[test]
    fn unrolled_dedupes_includes_across_cycles(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(2));
        let (b, c) = b.with_wire(WireTy::Bits(2));
        let (b, out) = b.with_wire(WireTy::Bits(2));
        let b = b.with_instruction(Op::Bin(BinOp::Add), vec![a, c], out)?;
        let graph = b.build();

        let t = emit_unrolled_template(
            &graph,
            "add2",
            &[a, c],
            &[out],
            0,
            &BitSeq::new(),
            2,
        )
        .run()?;
        let text = t.render().run()?;
        let bitify_hits = text
            .matches("include \"circomlib/circuits/bitify.circom\"")
            .count();
        assert_eq!(bitify_hits, 1);
        Ok(())
    }

    #[test]
    fn unrolled_zero_state_is_pure_replication(
    ) -> Result<(), hdl_cat_error::Error> {
        // state_wire_count == 0: every input wire is a data input.
        // The unrolled emitter renders K independent copies of the
        // combinational graph with no state plumbing.
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Not, vec![a], out)?;
        let graph = b.build();

        let t = emit_unrolled_template(
            &graph,
            "inv",
            &[a],
            &[out],
            0,
            &BitSeq::new(),
            2,
        )
        .run()?;
        let text = t.render().run()?;
        assert!(text.contains("signal input w0_c0[1];"));
        assert!(text.contains("signal input w0_c1[1];"));
        assert!(text.contains("signal output w1_c0[1];"));
        assert!(text.contains("signal output w1_c1[1];"));
        assert!(text.contains("w1_c0[0] <== (1 - w0_c0[0]);"));
        assert!(text.contains("w1_c1[0] <== (1 - w0_c1[0]);"));
        Ok(())
    }

    #[test]
    fn unrolled_rejects_zero_cycles() -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Not, vec![a], out)?;
        let graph = b.build();

        let result = emit_unrolled_template(
            &graph,
            "z",
            &[a],
            &[out],
            0,
            &BitSeq::new(),
            0,
        )
        .run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }

    #[test]
    fn unrolled_rejects_state_count_overflowing_inputs(
    ) -> Result<(), hdl_cat_error::Error> {
        let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(1));
        let (b, out) = b.with_wire(WireTy::Bits(1));
        let b = b.with_instruction(Op::Not, vec![a], out)?;
        let graph = b.build();

        let result = emit_unrolled_template(
            &graph,
            "z",
            &[a],
            &[out],
            2,
            &BitSeq::new(),
            1,
        )
        .run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }

    #[test]
    fn unrolled_still_rejects_reg() -> Result<(), hdl_cat_error::Error> {
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

        let result = emit_unrolled_template(
            &graph,
            "r",
            &[a],
            &[out],
            0,
            &BitSeq::new(),
            1,
        )
        .run();
        let is_unsupported = matches!(
            result,
            Err(hdl_cat_error::Error::UnsupportedInCircom(_))
        );
        assert!(is_unsupported);
        Ok(())
    }
}
