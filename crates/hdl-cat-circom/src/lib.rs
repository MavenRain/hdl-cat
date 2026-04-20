//! Circom AST + emitter for [`hdl_cat_ir::HdlGraph`].
//!
//! v1 is combinational and bit-level: every hdl-cat wire of width
//! `N` becomes `N` Circom signals, each with a per-bit boolean
//! constraint emitted on entry for input ports.  Bitwise ops
//! (`Not`, `And`, `Or`, `Xor`, `Mux`), `Const`, `Slice`, and `Concat`
//! lower directly to per-bit assignments.  Arithmetic (`Add`, `Sub`,
//! `Mul`), comparisons (`Eq`, `Lt`), and stateful ops (`Reg`,
//! `ArrayShiftIn`, `ArrayTail`) are scheduled for a follow-up and
//! presently return [`hdl_cat_error::Error::UnsupportedInCircom`].
//!
//! Three modules sit between the IR and the emitted Circom file:
//!
//! - [`ast`] — typed Circom AST ([`Template`], [`Signal`], [`Stmt`],
//!   [`Expr`]) so consumers can inspect or transform the template
//!   before pretty-printing.
//! - [`emitter`] — lowers a [`hdl_cat_ir::HdlGraph`] to a
//!   [`Template`] via [`emit_template`].
//! - [`render`] — pretty-prints a [`Template`] to a self-contained
//!   `circom 2.0.0` file (`pragma`, `include`s, template body, and
//!   `component main`).
//!
//! # Example
//!
//! Emit a 4-bit bitwise inverter and assert on the rendered text.
//!
//! ```
//! # fn main() -> Result<(), hdl_cat_error::Error> {
//! use hdl_cat_circom::emit_template;
//! use hdl_cat_ir::{HdlGraphBuilder, Op, WireTy};
//!
//! let (b, a) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
//! let (b, out) = b.with_wire(WireTy::Bits(4));
//! let b = b.with_instruction(Op::Not, vec![a], out)?;
//! let graph = b.build();
//!
//! let template = emit_template(&graph, "inv4", &[a], &[out]).run()?;
//! let text = template.render().run()?;
//! assert!(text.starts_with("pragma circom 2.0.0;"));
//! assert!(text.contains("template inv4()"));
//! assert!(text.contains("signal input w0[4];"));
//! assert!(text.contains("signal output w1[4];"));
//! assert!(text.contains("w1[0] <== (1 - w0[0]);"));
//! assert!(text.contains("component main = inv4();"));
//! # Ok(()) }
//! ```

pub mod ast;
pub mod emitter;
pub mod render;

pub use ast::{Expr, Field, Signal, SignalDir, Stmt, Template};
pub use emitter::emit_template;
