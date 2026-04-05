//! Hardware intermediate representation.
//!
//! The central type is [`HdlGraph`] — a directed, typed, flat
//! dataflow graph of hardware [`Instruction`]s that also implements
//! `comp_cat_rs::collapse::free_category::Graph`.
//!
//! A compiled circuit is a sequence of gate invocations reading
//! named [`WireId`]s and writing one new [`WireId`] each.  Each
//! [`Wire`] is tagged with a [`WireTy`].  Interpretation
//! (simulation, Verilog emission) walks the instruction list.
//!
//! # Design
//!
//! comp-cat-rs's `Graph` models single-edge-per-morphism — each
//! edge has exactly one source and one target vertex.  Our
//! [`Instruction`]s are multi-input: the graph view takes the
//! first input as the source vertex and the output as the
//! target.  Secondary inputs live on the instruction itself.
//! This keeps the `Graph` trait satisfiable without forcing
//! tensor-product vertex types into the IR.
//!
//! Both representations are coherent: a `Path` through the
//! `HdlGraph` corresponds to a sequential data-flow thread,
//! while the full multi-port circuit structure is recoverable
//! from the instruction list.
//!
//! # Example — building a graph by hand
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
//!
//! // Compute `(a XOR b) AND c` using three input wires.
//! let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bit);
//! let (b, bb) = b.with_wire(WireTy::Bit);
//! let (b, c) = b.with_wire(WireTy::Bit);
//! let (b, xor_out) = b.with_wire(WireTy::Bit);
//! let (b, out) = b.with_wire(WireTy::Bit);
//! let b = b.with_instruction(Op::Bin(BinOp::Xor), vec![a, bb], xor_out)?;
//! let b = b.with_instruction(Op::Bin(BinOp::And), vec![xor_out, c], out)?;
//! let graph = b.build();
//!
//! assert_eq!(graph.wires().len(), 5);
//! assert_eq!(graph.instructions().len(), 2);
//! # Ok(()) }
//! ```
//!
//! # Example — interpreting the graph
//!
//! The `hdl-cat-sim` crate provides `interpret` which walks the
//! instruction list, computing each wire's value from a supplied
//! input environment.

pub mod wire;
pub mod op;
pub mod instr;
pub mod graph;
pub mod builder;

pub use builder::HdlGraphBuilder;
pub use graph::HdlGraph;
pub use instr::Instruction;
pub use op::{BinOp, Op};
pub use wire::{Wire, WireId, WireTy};
