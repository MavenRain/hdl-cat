//! Verilog AST + emitter for [`hdl_cat_ir::HdlGraph`].
//!
//! Exposes a typed Verilog AST ([`Module`], [`Port`], [`Stmt`],
//! [`Expr`]) and an emitter that walks an IR graph producing a
//! `Module` inside `Io<Error, Module>`.  The pretty-printer
//! serializes a `Module` to a `String`, again inside `Io`.
//!
//! # Examples
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
//! assert!(text.contains("assign"));
//! # Ok(()) }
//! ```

pub mod ast;
pub mod emitter;
pub mod render;

pub use ast::{Expr, Module, Port, PortDirection, Stmt};
pub use emitter::{emit_graph, emit_sync_graph};
