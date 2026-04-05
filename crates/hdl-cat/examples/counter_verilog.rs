//! Emit the 4-bit counter as stateful `SystemVerilog`.
//!
//! Uses `emit_sync_graph`, which promotes state wires into
//! `always_ff @(posedge clk)` blocks with synchronous reset.

use hdl_cat::prelude::*;

fn main() -> Result<(), hdl_cat_error::Error> {
    let c = std_lib::counter::<4>()?;
    let module = verilog::emit_sync_graph(
        c.graph(),
        "counter4",
        c.state_wire_count(),
        c.input_wires(),
        c.output_wires(),
        c.initial_state(),
    )
    .run()?;
    let text = module.render().run()?;
    print!("{text}");
    Ok(())
}
