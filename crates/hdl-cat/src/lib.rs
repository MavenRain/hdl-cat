//! # hdl-cat
//!
//! A hardware description library re-architected around
//! [`comp_cat_rs`](https://github.com/MavenRain/comp-cat-rs)'s
//! categorical effect system.
//!
//! This umbrella crate re-exports the full workspace so a single
//! `use hdl_cat::*;` (or a trimmed [`prelude`]) is enough to get
//! going.
//!
//! ## Layers
//!
//! | Concept | Crate |
//! |---|---|
//! | Errors | `hdl_cat_error` |
//! | Bit-precise ints | `hdl_cat_bits` |
//! | Hardware-typable types | `hdl_cat_kind` |
//! | Domain-indexed signals | `hdl_cat_signal` |
//! | IR (free-category graph) | `hdl_cat_ir` |
//! | Circuit category | `hdl_cat_circuit` |
//! | Sync machines | `hdl_cat_sync` |
//! | Simulator | `hdl_cat_sim` |
//! | Verilog emitter | `hdl_cat_verilog` |
//! | Standard components | `hdl_cat_std` |
//!
//! ## End-to-end example
//!
//! ```
//! # fn main() -> Result<(), hdl_cat::Error> {
//! use hdl_cat::prelude::*;
//!
//! // Build a 1-bit half-adder.
//! let ha = std_lib::half_adder()?;
//!
//! // Emit Verilog for it.
//! let module = verilog::emit_graph(
//!     ha.graph(),
//!     "half_adder",
//!     ha.inputs(),
//!     ha.outputs(),
//! ).run()?;
//! let text = module.render().run()?;
//! assert!(text.contains("module half_adder"));
//! assert!(text.contains("assign"));
//! # Ok(()) }
//! ```

pub use hdl_cat_bits as bits;
pub use hdl_cat_macros::kernel;
pub use hdl_cat_circuit as circuit;
pub use hdl_cat_error::{Cycle, Error, SignalName, TypeName, Width};
pub use hdl_cat_ir as ir;
pub use hdl_cat_kind as kind;
pub use hdl_cat_sim as sim;
pub use hdl_cat_signal as signal;
pub use hdl_cat_std as std_lib;
pub use hdl_cat_sync as sync;
pub use hdl_cat_verilog as verilog;

/// Curated re-exports for quick imports.
pub mod prelude {
    pub use crate::{bits, circuit, ir, kind, sim, signal, std_lib, sync, verilog};
    pub use crate::{Cycle, Error, SignalName, TypeName, Width};
    pub use hdl_cat_bits::{Bits, SignedBits};
    pub use hdl_cat_circuit::{
        gates, Circuit, CircuitArrow, CircuitTensor, CircuitUnit, Obj, Object,
    };
    pub use hdl_cat_kind::{BitSeq, Hw, TypeDesc};
    pub use hdl_cat_sim::Testbench;
    pub use hdl_cat_sync::Sync;
}
