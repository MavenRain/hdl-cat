//! Convert an [`hdl_cat_ir::HdlGraph`] to a Verilog [`Module`].

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::Error;
use hdl_cat_ir::{BinOp, HdlGraph, Instruction, Op, WireId, WireTy};
use hdl_cat_kind::BitSeq;

use crate::ast::{Expr, Module, Port, PortDirection, Stmt};

/// Produce a Verilog [`Module`] from an IR graph.
///
/// The `input_wires` list becomes the module's input ports and
/// the `output_wires` list becomes its output ports; all other
/// wires are internal declarations.  Each [`Instruction`] becomes
/// a continuous `assign` statement.
///
/// # Errors
///
/// Returns `Ok(...)` currently; reserved to fail if unsupported
/// ops (e.g. clocked registers in a non-clocked graph) are
/// encountered in future revisions.
#[must_use]
pub fn emit_graph(
    graph: &HdlGraph,
    name: &str,
    input_wires: &[WireId],
    output_wires: &[WireId],
) -> Io<Error, Module> {
    let name_owned = name.to_string();
    let graph_owned = graph.clone();
    let input_owned: Vec<WireId> = input_wires.to_vec();
    let output_owned: Vec<WireId> = output_wires.to_vec();

    Io::suspend(move || Ok(build_module(&graph_owned, &name_owned, &input_owned, &output_owned)))
}

fn build_module(
    graph: &HdlGraph,
    name: &str,
    inputs: &[WireId],
    outputs: &[WireId],
) -> Module {
    let ports: Vec<Port> = inputs
        .iter()
        .map(|w| make_port(graph, *w, PortDirection::Input))
        .chain(
            outputs
                .iter()
                .map(|w| make_port(graph, *w, PortDirection::Output)),
        )
        .collect();

    let internal_decls: Vec<Stmt> = graph
        .wires()
        .iter()
        .enumerate()
        .filter_map(|(idx, ty)| {
            let w = WireId::new(idx);
            let is_port = inputs.contains(&w) || outputs.contains(&w);
            if is_port {
                None
            } else {
                Some(Stmt::WireDecl {
                    name: wire_name(w),
                    width: ty.width(),
                })
            }
        })
        .collect();

    let assignments: Vec<Stmt> = graph
        .instructions()
        .iter()
        .map(instruction_to_stmt)
        .collect();

    let body = internal_decls.into_iter().chain(assignments).collect();
    Module::new(name, ports, body)
}

fn make_port(graph: &HdlGraph, w: WireId, dir: PortDirection) -> Port {
    let width = graph
        .wire_ty(w.index())
        .map_or(0, WireTy::width);
    Port::new(wire_name(w), dir, width)
}

fn wire_name(w: WireId) -> String {
    format!("w{}", w.index())
}

fn instruction_to_stmt(instr: &Instruction) -> Stmt {
    Stmt::Assign {
        lhs: wire_name(instr.output()),
        rhs: op_to_expr(instr.op(), instr.inputs()),
    }
}

fn op_to_expr(op: &Op, inputs: &[WireId]) -> Expr {
    match op {
        Op::Not => Expr::Not(Box::new(wire_expr(inputs[0]))),
        Op::Bin(b) => Expr::Binary {
            op: binop_token(*b),
            lhs: Box::new(wire_expr(inputs[0])),
            rhs: Box::new(wire_expr(inputs[1])),
        },
        Op::Mux => Expr::Mux {
            selector: Box::new(wire_expr(inputs[0])),
            false_arm: Box::new(wire_expr(inputs[1])),
            true_arm: Box::new(wire_expr(inputs[2])),
        },
        Op::Const { bits, ty } => Expr::Literal {
            width: ty.width(),
            value: bits_to_u128(bits),
        },
        Op::Reg { init, ty } => {
            // Combinational view of a register: the output equals
            // the input.  The initial value is emitted as part of
            // the surrounding always_ff block.
            let _ = (init, ty);
            wire_expr(inputs[0])
        }
        Op::Concat { .. } => Expr::Concat {
            high: Box::new(wire_expr(inputs[1])),
            low: Box::new(wire_expr(inputs[0])),
        },
        Op::Slice { lo, hi } => Expr::Slice {
            source: Box::new(wire_expr(inputs[0])),
            lo: *lo,
            hi: *hi,
        },
    }
}

fn wire_expr(w: WireId) -> Expr {
    Expr::Wire(wire_name(w))
}

fn binop_token(b: BinOp) -> &'static str {
    match b {
        BinOp::And => "&",
        BinOp::Or => "|",
        BinOp::Xor => "^",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Eq => "==",
        BinOp::Lt => "<",
    }
}

fn bits_to_u128(bits: &hdl_cat_kind::BitSeq) -> u128 {
    bits.as_slice()
        .iter()
        .enumerate()
        .fold(0u128, |acc, (i, b)| acc | (u128::from(*b) << i))
}

/// Produce a Verilog [`Module`] from a stateful IR graph.
///
/// Unlike [`emit_graph`], this emitter treats the first
/// `state_wire_count` input wires as *state registers* (driven
/// by an `always_ff @(posedge clk)` block), and the first
/// `state_wire_count` output wires as their corresponding
/// *next-state* combinational feeds.  The remaining wires become
/// module data input/output ports.
///
/// Output ports that coincide with a state wire are rendered as
/// `output reg` so the same identifier can both be assigned inside
/// the `always_ff` block and exposed on the module interface.
///
/// The module has a fixed `clk` input and a fixed `rst` input
/// (synchronous reset).  The `initial_state` bit sequence is
/// split into per-state-wire chunks and emitted as each
/// register's reset value.
///
/// # Errors
///
/// Returns [`Error::WidthMismatch`] when `initial_state`'s bit
/// length does not match the total width of the state wires.
#[must_use]
pub fn emit_sync_graph(
    graph: &HdlGraph,
    name: &str,
    state_wire_count: usize,
    input_wires: &[WireId],
    output_wires: &[WireId],
    initial_state: &BitSeq,
) -> Io<Error, Module> {
    let name_owned = name.to_string();
    let graph_owned = graph.clone();
    let input_owned: Vec<WireId> = input_wires.to_vec();
    let output_owned: Vec<WireId> = output_wires.to_vec();
    let initial_owned = initial_state.clone();
    Io::suspend(move || {
        build_sync_module(
            &graph_owned,
            &name_owned,
            state_wire_count,
            &input_owned,
            &output_owned,
            &initial_owned,
        )
    })
}

/// Describes a contiguous range of state wires that form a shift-register
/// array.  The emitter collapses these into a single `RegArrayDecl` +
/// `AlwaysArrayShift` instead of individual registers.
///
/// State wire indices `start .. start + depth` (within the state wire
/// slice) are grouped.  Each element has the same bit `width`.  The
/// corresponding next-state wires must satisfy the shift-register
/// pattern: `next[0] = input_expr`, `next[i] = state[i-1]`.
#[derive(Clone, Debug)]
pub struct StateArraySpec {
    /// Name for the Verilog array (e.g. `"delay"`).
    name: String,
    /// Index of the first state wire in the state wire slice.
    start: usize,
    /// Number of elements (array depth).
    depth: usize,
    /// Bit width of each element.
    width: u32,
    /// The wire whose next-state feeds `arr[0]` each cycle.
    ///
    /// This is the output wire index (in the next-state slice) for
    /// element 0 of the array.  The emitter reads the corresponding
    /// instruction's input to discover the driving expression.
    input_next_wire: WireId,
    /// Reset value for every element (as a [`BitSeq`]).
    reset_bits: BitSeq,
}

impl StateArraySpec {
    /// Construct a new array specification.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        start: usize,
        depth: usize,
        width: u32,
        input_next_wire: WireId,
        reset_bits: BitSeq,
    ) -> Self {
        Self {
            name: name.into(),
            start,
            depth,
            width,
            input_next_wire,
            reset_bits,
        }
    }

    /// The array name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Index of the first state wire in the state wire slice.
    #[must_use]
    pub fn start(&self) -> usize {
        self.start
    }

    /// Number of elements.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Bit width of each element.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }
}

/// Produce a Verilog [`Module`] from a stateful IR graph, collapsing
/// designated state wire ranges into register arrays.
///
/// Behaves like [`emit_sync_graph`] but accepts a list of
/// [`StateArraySpec`]s.  State wires covered by an array spec are
/// emitted as `RegArrayDecl` + `AlwaysArrayShift` instead of
/// individual `RegDecl` / `AlwaysFf` pairs.
///
/// # Errors
///
/// Returns [`Error::WidthMismatch`] when `initial_state`'s bit
/// length does not match the total width of the state wires.
#[must_use]
pub fn emit_sync_graph_with_arrays(
    graph: &HdlGraph,
    name: &str,
    state_wire_count: usize,
    input_wires: &[WireId],
    output_wires: &[WireId],
    initial_state: &BitSeq,
    arrays: &[StateArraySpec],
) -> Io<Error, Module> {
    let name_owned = name.to_string();
    let graph_owned = graph.clone();
    let input_owned: Vec<WireId> = input_wires.to_vec();
    let output_owned: Vec<WireId> = output_wires.to_vec();
    let initial_owned = initial_state.clone();
    let arrays_owned: Vec<StateArraySpec> = arrays.to_vec();
    Io::suspend(move || {
        build_sync_module_with_arrays(
            &graph_owned,
            &name_owned,
            state_wire_count,
            &input_owned,
            &output_owned,
            &initial_owned,
            &arrays_owned,
        )
    })
}

#[allow(clippy::too_many_lines)]
fn build_sync_module_with_arrays(
    graph: &HdlGraph,
    name: &str,
    state_wire_count: usize,
    input_wires: &[WireId],
    output_wires: &[WireId],
    initial_state: &BitSeq,
    arrays: &[StateArraySpec],
) -> Result<Module, Error> {
    let (state_wires_in, data_inputs) = input_wires.split_at(state_wire_count);
    let (next_state_wires, data_outputs) = output_wires.split_at(state_wire_count);

    // Split initial_state into per-state-wire chunks.
    let state_widths: Vec<usize> = state_wires_in
        .iter()
        .map(|w| graph_wire_width_usize(graph, *w))
        .collect();
    let expected_bits: usize = state_widths.iter().sum();
    (initial_state.len() == expected_bits)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(expected_bits).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(initial_state.len()).unwrap_or(u32::MAX),
            ),
        })?;
    let init_chunks = split_state_bits(initial_state, &state_widths);

    // Build a set of state wire indices that belong to arrays.
    let array_indices: Vec<usize> = arrays
        .iter()
        .flat_map(|a| a.start..a.start + a.depth)
        .collect();

    // Ports: clk, rst, data inputs, data outputs.
    let data_input_ports = data_inputs.iter().map(|w| {
        Port::new(
            wire_name(*w),
            PortDirection::Input,
            graph_wire_width_u32(graph, *w),
        )
    });
    let data_output_ports = data_outputs.iter().map(|w| {
        let direction = if state_wires_in.contains(w) {
            PortDirection::OutputReg
        } else {
            PortDirection::Output
        };
        Port::new(wire_name(*w), direction, graph_wire_width_u32(graph, *w))
    });
    let ports: Vec<Port> = core::iter::once(Port::new("clk", PortDirection::Input, 1))
        .chain(core::iter::once(Port::new("rst", PortDirection::Input, 1)))
        .chain(data_input_ports)
        .chain(data_output_ports)
        .collect();

    let port_wire_ids: Vec<WireId> = data_inputs
        .iter()
        .copied()
        .chain(data_outputs.iter().copied())
        .collect();

    // Individual state reg declarations (excluding array members).
    let state_reg_decls: Vec<Stmt> = state_wires_in
        .iter()
        .enumerate()
        .filter(|(i, w)| !array_indices.contains(i) && !data_outputs.contains(w))
        .map(|(_, w)| Stmt::RegDecl {
            name: wire_name(*w),
            width: graph_wire_width_u32(graph, *w),
        })
        .collect();

    // Array declarations.
    let array_decls: Vec<Stmt> = arrays
        .iter()
        .map(|a| Stmt::RegArrayDecl {
            name: a.name.clone(),
            width: a.width,
            depth: a.depth,
        })
        .collect();

    // Internal wire declarations (excluding state and port wires).
    // Also exclude next-state wires that belong to arrays (indices 1..depth)
    // since those are handled by the shift logic.
    let array_next_skip: Vec<WireId> = arrays
        .iter()
        .flat_map(|a| (1..a.depth).map(move |i| next_state_wires[a.start + i]))
        .collect();
    let internal_wire_decls: Vec<Stmt> = graph
        .wires()
        .iter()
        .enumerate()
        .filter_map(|(idx, ty)| {
            let w = WireId::new(idx);
            let is_state = state_wires_in.contains(&w);
            let is_port = port_wire_ids.contains(&w);
            let is_array_next = array_next_skip.contains(&w);
            if is_state || is_port || is_array_next {
                None
            } else {
                Some(Stmt::WireDecl {
                    name: wire_name(w),
                    width: ty.width(),
                })
            }
        })
        .collect();

    // Filter out assignments that write to array shift wires (next[1..depth]).
    let assignments: Vec<Stmt> = graph
        .instructions()
        .iter()
        .filter(|instr| !array_next_skip.contains(&instr.output()))
        .map(instruction_to_stmt)
        .collect();

    // Individual always_ff blocks for non-array state wires.
    let always_blocks: Vec<Stmt> = state_wires_in
        .iter()
        .enumerate()
        .zip(next_state_wires.iter())
        .zip(init_chunks.iter())
        .filter(|(((i, _), _), _)| !array_indices.contains(i))
        .map(|(((_i, state_w), next_w), init_bits)| {
            let init_width = graph_wire_width_u32(graph, *state_w);
            Stmt::AlwaysFf {
                clock: "clk".to_string(),
                reset: Some("rst".to_string()),
                reg: wire_name(*state_w),
                reset_value: Expr::Literal {
                    width: init_width,
                    value: bits_to_u128(init_bits),
                },
                next: Expr::Wire(wire_name(*next_w)),
            }
        })
        .collect();

    // Array shift blocks.
    let array_shifts: Vec<Stmt> = arrays
        .iter()
        .map(|a| Stmt::AlwaysArrayShift {
            clock: "clk".to_string(),
            reset: "rst".to_string(),
            array: a.name.clone(),
            depth: a.depth,
            width: a.width,
            reset_value: Expr::Literal {
                width: a.width,
                value: bits_to_u128(&a.reset_bits),
            },
            input: Expr::Wire(wire_name(a.input_next_wire)),
        })
        .collect();

    // Wire up array tail outputs: assign w{tail} = array[depth-1];
    let array_tail_assigns: Vec<Stmt> = arrays
        .iter()
        .map(|a| {
            let tail_state_idx = a.start + a.depth - 1;
            let tail_wire = state_wires_in[tail_state_idx];
            Stmt::Assign {
                lhs: wire_name(tail_wire),
                rhs: Expr::ArrayIndex {
                    array: a.name.clone(),
                    index: a.depth - 1,
                },
            }
        })
        .collect();

    let body: Vec<Stmt> = state_reg_decls
        .into_iter()
        .chain(array_decls)
        .chain(internal_wire_decls)
        .chain(assignments)
        .chain(array_tail_assigns)
        .chain(always_blocks)
        .chain(array_shifts)
        .collect();

    Ok(Module::new(name, ports, body))
}

fn build_sync_module(
    graph: &HdlGraph,
    name: &str,
    state_wire_count: usize,
    input_wires: &[WireId],
    output_wires: &[WireId],
    initial_state: &BitSeq,
) -> Result<Module, Error> {
    let (state_wires_in, data_inputs) = input_wires.split_at(state_wire_count);
    let (next_state_wires, data_outputs) = output_wires.split_at(state_wire_count);

    // Split initial_state into per-state-wire chunks.
    let state_widths: Vec<usize> = state_wires_in
        .iter()
        .map(|w| graph_wire_width_usize(graph, *w))
        .collect();
    let expected_bits: usize = state_widths.iter().sum();
    (initial_state.len() == expected_bits)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(expected_bits).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(initial_state.len()).unwrap_or(u32::MAX),
            ),
        })?;
    let init_chunks = split_state_bits(initial_state, &state_widths);

    // Build the port list: clk, rst, data inputs, data outputs.
    let data_input_ports = data_inputs.iter().map(|w| {
        Port::new(
            wire_name(*w),
            PortDirection::Input,
            graph_wire_width_u32(graph, *w),
        )
    });
    let data_output_ports = data_outputs.iter().map(|w| {
        let direction = if state_wires_in.contains(w) {
            PortDirection::OutputReg
        } else {
            PortDirection::Output
        };
        Port::new(wire_name(*w), direction, graph_wire_width_u32(graph, *w))
    });
    let ports: Vec<Port> = core::iter::once(Port::new("clk", PortDirection::Input, 1))
        .chain(core::iter::once(Port::new("rst", PortDirection::Input, 1)))
        .chain(data_input_ports)
        .chain(data_output_ports)
        .collect();

    // Determine which wires are ports (by id) so we can skip
    // declaring them as internal wires/regs.
    let port_wire_ids: Vec<WireId> = data_inputs
        .iter()
        .copied()
        .chain(data_outputs.iter().copied())
        .collect();

    // Internal declarations.
    let state_reg_decls: Vec<Stmt> = state_wires_in
        .iter()
        .filter(|w| !data_outputs.contains(w))
        .map(|w| Stmt::RegDecl {
            name: wire_name(*w),
            width: graph_wire_width_u32(graph, *w),
        })
        .collect();

    let internal_wire_decls: Vec<Stmt> = graph
        .wires()
        .iter()
        .enumerate()
        .filter_map(|(idx, ty)| {
            let w = WireId::new(idx);
            let is_state = state_wires_in.contains(&w);
            let is_port = port_wire_ids.contains(&w);
            if is_state || is_port {
                None
            } else {
                Some(Stmt::WireDecl {
                    name: wire_name(w),
                    width: ty.width(),
                })
            }
        })
        .collect();

    // Assigns for each instruction (instructions never write to state wires).
    let assignments: Vec<Stmt> = graph.instructions().iter().map(instruction_to_stmt).collect();

    // always_ff blocks for state wires.
    let always_blocks: Vec<Stmt> = state_wires_in
        .iter()
        .zip(next_state_wires.iter())
        .zip(init_chunks.iter())
        .map(|((state_w, next_w), init_bits)| {
            let init_width = graph_wire_width_u32(graph, *state_w);
            Stmt::AlwaysFf {
                clock: "clk".to_string(),
                reset: Some("rst".to_string()),
                reg: wire_name(*state_w),
                reset_value: Expr::Literal {
                    width: init_width,
                    value: bits_to_u128(init_bits),
                },
                next: Expr::Wire(wire_name(*next_w)),
            }
        })
        .collect();

    let body: Vec<Stmt> = state_reg_decls
        .into_iter()
        .chain(internal_wire_decls)
        .chain(assignments)
        .chain(always_blocks)
        .collect();

    Ok(Module::new(name, ports, body))
}

fn split_state_bits(bits: &BitSeq, widths: &[usize]) -> Vec<BitSeq> {
    let (chunks, _) = widths.iter().fold(
        (Vec::<BitSeq>::new(), 0usize),
        |(acc, offset), &w| {
            let chunk: BitSeq = bits
                .as_slice()
                .iter()
                .skip(offset)
                .take(w)
                .copied()
                .collect();
            let new_acc = acc
                .into_iter()
                .chain(core::iter::once(chunk))
                .collect();
            (new_acc, offset + w)
        },
    );
    chunks
}

fn graph_wire_width_usize(graph: &HdlGraph, w: WireId) -> usize {
    graph
        .wire_ty(w.index())
        .map_or(0, |ty| usize::try_from(ty.width()).unwrap_or(0))
}

fn graph_wire_width_u32(graph: &HdlGraph, w: WireId) -> u32 {
    graph.wire_ty(w.index()).map_or(0, WireTy::width)
}

#[cfg(test)]
mod tests {
    use super::{emit_graph, emit_sync_graph};
    use crate::ast::{PortDirection, Stmt};
    use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
    use hdl_cat_kind::BitSeq;

    #[test]
    fn emits_module_for_inverter() -> Result<(), hdl_cat_error::Error> {
        let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(8));
        let (bld, out) = bld.with_wire(WireTy::Bits(8));
        let bld = bld.with_instruction(Op::Not, vec![a], out)?;
        let graph = bld.build();

        let module = emit_graph(&graph, "inv8", &[a], &[out]).run()?;
        assert_eq!(module.name(), "inv8");
        assert_eq!(module.ports().len(), 2);
        assert_eq!(module.body().len(), 1);
        Ok(())
    }

    #[test]
    fn emits_assign_for_binary_op() -> Result<(), hdl_cat_error::Error> {
        let (bld, lhs) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, rhs) = bld.with_wire(WireTy::Bits(4));
        let (bld, out) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(Op::Bin(BinOp::Xor), vec![lhs, rhs], out)?;
        let graph = bld.build();

        let module = emit_graph(&graph, "xor4", &[lhs, rhs], &[out]).run()?;
        assert_eq!(module.ports().len(), 3);
        Ok(())
    }

    #[test]
    fn internal_wires_become_decls() -> Result<(), hdl_cat_error::Error> {
        let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, tmp) = bld.with_wire(WireTy::Bits(4));
        let (bld, out) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(Op::Not, vec![a], tmp)?;
        let bld = bld.with_instruction(Op::Not, vec![tmp], out)?;
        let graph = bld.build();

        let module = emit_graph(&graph, "inv_inv", &[a], &[out]).run()?;
        // tmp is neither input nor output — it becomes a wire decl.
        let decl_count = module
            .body()
            .iter()
            .filter(|s| matches!(s, crate::ast::Stmt::WireDecl { .. }))
            .count();
        assert_eq!(decl_count, 1);
        Ok(())
    }

    #[test]
    fn sync_emitter_adds_clk_and_rst_ports() -> Result<(), hdl_cat_error::Error> {
        // Build a trivial 4-bit counter graph: state + 1 = next_state.
        let (bld, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, one) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_state) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(
            Op::Const {
                bits: BitSeq::from_vec(vec![true, false, false, false]),
                ty: WireTy::Bits(4),
            },
            vec![],
            one,
        )?;
        let bld = bld.with_instruction(Op::Bin(BinOp::Add), vec![state, one], next_state)?;
        let graph = bld.build();

        let init = BitSeq::from_vec(vec![false, false, false, false]);
        let module = emit_sync_graph(
            &graph,
            "counter4",
            1,
            &[state],
            &[next_state, state],
            &init,
        )
        .run()?;

        // Ports: clk, rst, output reg [3:0] w0 (state-as-output).
        let port_names: Vec<_> = module.ports().iter().map(|p| p.name().to_string()).collect();
        assert!(port_names.contains(&"clk".to_string()));
        assert!(port_names.contains(&"rst".to_string()));
        assert!(port_names.contains(&"w0".to_string()));

        // The port for w0 should be OutputReg since w0 is a state wire AND a data output.
        let w0_port = module.ports().iter().find(|p| p.name() == "w0");
        assert!(matches!(
            w0_port.map(crate::ast::Port::direction),
            Some(PortDirection::OutputReg)
        ));

        // Body must contain exactly one AlwaysFf block.
        let always_count = module
            .body()
            .iter()
            .filter(|s| matches!(s, Stmt::AlwaysFf { .. }))
            .count();
        assert_eq!(always_count, 1);
        Ok(())
    }

    #[test]
    fn sync_emitter_rejects_mismatched_initial_state() -> Result<(), hdl_cat_error::Error> {
        let (bld, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, next_state) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(Op::Not, vec![state], next_state)?;
        let graph = bld.build();

        // Wrong initial state width: 2 bits instead of 4.
        let init = BitSeq::from_vec(vec![true, false]);
        let result = emit_sync_graph(&graph, "bad", 1, &[state], &[next_state], &init).run();
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn array_emitter_produces_reg_array_and_shift() -> Result<(), hdl_cat_error::Error> {
        // Build a graph with 4 state wires (shift register) + 1 state wire (counter).
        // State: [d0, d1, d2, d3, counter]
        // Next:  [next_d0, next_d1, next_d2, next_d3, next_counter]
        // Shift: next_d0 = data_in (via identity), next_d1 = d0, etc.
        let (bld, d0) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, d1) = bld.with_wire(WireTy::Bits(4));
        let (bld, d2) = bld.with_wire(WireTy::Bits(4));
        let (bld, d3) = bld.with_wire(WireTy::Bits(4));
        let (bld, ctr) = bld.with_wire(WireTy::Bits(4));
        // Data input
        let (bld, data_in) = bld.with_wire(WireTy::Bits(4));
        // Next-state wires
        let (bld, next_d0) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_d1) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_d2) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_d3) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_ctr) = bld.with_wire(WireTy::Bits(4));
        // Data output: tail of shift register
        let (bld, data_out) = bld.with_wire(WireTy::Bits(4));

        // Instructions: next_d0 = data_in, next_d1 = d0, etc.
        let bld = bld.with_instruction(
            Op::Slice { lo: 0, hi: 4 }, vec![data_in], next_d0,
        )?;
        let bld = bld.with_instruction(
            Op::Slice { lo: 0, hi: 4 }, vec![d0], next_d1,
        )?;
        let bld = bld.with_instruction(
            Op::Slice { lo: 0, hi: 4 }, vec![d1], next_d2,
        )?;
        let bld = bld.with_instruction(
            Op::Slice { lo: 0, hi: 4 }, vec![d2], next_d3,
        )?;
        // counter: next_ctr = ctr + 1 (simplified as NOT for the test)
        let bld = bld.with_instruction(Op::Not, vec![ctr], next_ctr)?;
        // data_out = d3
        let bld = bld.with_instruction(
            Op::Slice { lo: 0, hi: 4 }, vec![d3], data_out,
        )?;
        let graph = bld.build();

        // State: [d0, d1, d2, d3, ctr] — 5 state wires
        let input_wires = [d0, d1, d2, d3, ctr, data_in];
        let output_wires = [next_d0, next_d1, next_d2, next_d3, next_ctr, data_out];
        let init = BitSeq::from_vec(vec![false; 4 * 5]); // 5 x 4-bit = 20 bits

        let array_spec = super::StateArraySpec::new(
            "delay",
            0,    // start
            4,    // depth
            4,    // width
            next_d0,
            BitSeq::from_vec(vec![false; 4]),
        );

        let module = super::emit_sync_graph_with_arrays(
            &graph, "shift_test", 5,
            &input_wires, &output_wires, &init,
            &[array_spec],
        ).run()?;

        let text = module.render().run()?;

        // Should have a reg array declaration
        assert!(text.contains("reg [3:0] delay [0:3];"));
        // Should have the array shift block
        assert!(text.contains("delay[0] <="));
        assert!(text.contains("delay[1] <= delay[0];"));
        assert!(text.contains("delay[3] <= delay[2];"));
        // Counter should still have its own AlwaysFf
        let always_ff_count = module
            .body()
            .iter()
            .filter(|s| matches!(s, Stmt::AlwaysFf { .. }))
            .count();
        assert_eq!(always_ff_count, 1); // only for counter
        // Should have one AlwaysArrayShift
        let array_shift_count = module
            .body()
            .iter()
            .filter(|s| matches!(s, Stmt::AlwaysArrayShift { .. }))
            .count();
        assert_eq!(array_shift_count, 1);
        Ok(())
    }

    #[test]
    fn sync_emitter_handles_pure_combinational() -> Result<(), hdl_cat_error::Error> {
        // state_wire_count = 0 should produce a module with no clk/rst in body
        // (but clk/rst are still in the port list for uniformity).
        let (bld, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, out) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(Op::Not, vec![a], out)?;
        let graph = bld.build();

        let module = emit_sync_graph(&graph, "inv", 0, &[a], &[out], &BitSeq::new()).run()?;
        let always_count = module
            .body()
            .iter()
            .filter(|s| matches!(s, Stmt::AlwaysFf { .. }))
            .count();
        assert_eq!(always_count, 0);
        Ok(())
    }
}
