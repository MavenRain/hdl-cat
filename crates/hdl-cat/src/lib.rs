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
//! ## End-to-end example — stateful counter
//!
//! Build → simulate → emit Verilog.
//!
//! ```
//! # fn main() -> Result<(), hdl_cat::Error> {
//! use hdl_cat::prelude::*;
//! use hdl_cat_kind::BitSeq;
//!
//! let c = std_lib::counter::<4>()?;
//! let inputs: Vec<BitSeq> = (0..4).map(|_| BitSeq::new()).collect();
//! let samples = Testbench::new(c).run(inputs).run()?;
//! let values: Vec<u128> = samples
//!     .iter()
//!     .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
//!     .collect::<Result<Vec<_>, _>>()?;
//! assert_eq!(values, vec![0, 1, 2, 3]);
//!
//! let c = std_lib::counter::<4>()?;
//! let module = verilog::emit_sync_graph(
//!     c.graph(), "counter4",
//!     c.state_wire_count(), c.input_wires(), c.output_wires(),
//!     c.initial_state(),
//! ).run()?;
//! let text = module.render().run()?;
//! assert!(text.contains("always_ff @(posedge clk)"));
//! # Ok(()) }
//! ```
//!
//! ## End-to-end example — combinational half-adder
//!
//! ```
//! # fn main() -> Result<(), hdl_cat::Error> {
//! use hdl_cat::prelude::*;
//!
//! let ha = std_lib::half_adder()?;
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
