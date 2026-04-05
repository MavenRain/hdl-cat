//! The README's quick-start, as a runnable example.
//!
//! Build → simulate → emit Verilog for an 8-bit counter.

use hdl_cat::prelude::*;

fn main() -> Result<(), hdl_cat_error::Error> {
    // 1. Build an 8-bit counter (state + 1 each cycle).
    let counter = std_lib::counter::<8>()?;

    // 2. Simulate 4 cycles with empty input (counter takes no data input).
    let inputs = vec![
        hdl_cat_kind::BitSeq::new(),
        hdl_cat_kind::BitSeq::new(),
        hdl_cat_kind::BitSeq::new(),
        hdl_cat_kind::BitSeq::new(),
    ];
    let samples = Testbench::new(counter).run(inputs).run()?;

    // 3. Read back: the first 4 counts.
    let counts: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<8>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(counts, vec![0, 1, 2, 3]);
    println!("counts: {counts:?}");

    // 4. Emit synthesizable SystemVerilog.
    let counter_for_verilog = std_lib::counter::<8>()?;
    let module = verilog::emit_sync_graph(
        counter_for_verilog.graph(),
        "counter8",
        counter_for_verilog.state_wire_count(),
        counter_for_verilog.input_wires(),
        counter_for_verilog.output_wires(),
        counter_for_verilog.initial_state(),
    )
    .run()?;
    let verilog_text = module.render().run()?;
    println!("---");
    print!("{verilog_text}");
    Ok(())
}
