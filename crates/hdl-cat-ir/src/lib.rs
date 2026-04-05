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
