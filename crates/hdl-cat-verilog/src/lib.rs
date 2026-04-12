//! Verilog AST + emitter for [`hdl_cat_ir::HdlGraph`].
//!
//! Two emitters:
//!
//! - [`emit_graph`] — flat combinational view.  Treats every
//!   input wire as a module input port and every output wire as
//!   an output port.
//! - [`emit_sync_graph`] — stateful view.  Promotes state wires
//!   to `reg` declarations driven by `always_ff @(posedge clk)`
//!   blocks with synchronous reset from the machine's initial
//!   state.
//!
//! A typed Verilog AST ([`Module`], [`Port`], [`Stmt`], [`Expr`])
//! sits between the emitter and the renderer, so consumers can
//! inspect or transform the module before pretty-printing.
//!
//! # Example — combinational module
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_ir::{HdlGraphBuilder, Op, WireTy};
//! use hdl_cat_verilog::emit_graph;
//!
//! let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(8));
//! let (b, out) = b.with_wire(WireTy::Bits(8));
//! let b = b.with_instruction(Op::Not, vec![a], out)?;
//! let graph = b.build();
//!
//! let module = emit_graph(&graph, "inv8", &[a], &[out]).run()?;
//! let text = module.render().run()?;
//! assert!(text.contains("module inv8"));
//! assert!(text.contains("input [7:0] w0"));
//! assert!(text.contains("output [7:0] w1"));
//! assert!(text.contains("assign w1 = ~w0"));
//! # Ok(()) }
//! ```
//!
//! # Example — stateful module
//!
//! Emits a `counter4`-style module with `clk`/`rst` inputs and a
//! state `reg` driven by an `always_ff` block.
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
//! use hdl_cat_kind::BitSeq;
//! use hdl_cat_verilog::emit_sync_graph;
//!
//! let (b, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
//! let (b, one) = b.with_wire(WireTy::Bits(4));
//! let (b, next_state) = b.with_wire(WireTy::Bits(4));
//! let b = b.with_instruction(
//!     Op::Const { bits: BitSeq::from_vec(vec![true, false, false, false]), ty: WireTy::Bits(4) },
//!     vec![], one,
//! )?;
//! let b = b.with_instruction(Op::Bin(BinOp::Add), vec![state, one], next_state)?;
//! let graph = b.build();
//!
//! let init = BitSeq::from_vec(vec![false, false, false, false]);
//! // state_wire_count=1, input=[state], output=[next_state, state]
//! let module = emit_sync_graph(
//!     &graph, "counter4", 1,
//!     &[state], &[next_state, state], &init,
//! ).run()?;
//! let text = module.render().run()?;
//! assert!(text.contains("input clk"));
//! assert!(text.contains("input rst"));
//! assert!(text.contains("output reg [3:0] w0"));
//! assert!(text.contains("always_ff @(posedge clk)"));
//! # Ok(()) }
//! ```

pub mod ast;
pub mod emitter;
pub mod render;

pub use ast::{Expr, Module, Port, PortDirection, Stmt};
pub use emitter::{
    ArrayStrategy, emit_graph, emit_sync_graph, emit_sync_graph_with_arrays, StateArraySpec,
};
