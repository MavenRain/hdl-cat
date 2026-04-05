//! End-to-end integration: build → simulate → emit Verilog.

use hdl_cat::prelude::*;
use hdl_cat_kind::BitSeq;

#[test]
fn inverter_simulates_and_emits_verilog() -> Result<(), hdl_cat_error::Error> {
    // Build a 4-bit inverter.
    let inv = gates::not_bits::<4>()?;

    // Simulate three input samples and verify outputs.
    let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv.clone());
    let inputs = vec![
        Bits::<4>::try_new(0x0)?.to_bits_seq(),
        Bits::<4>::try_new(0xf)?.to_bits_seq(),
        Bits::<4>::try_new(0xa)?.to_bits_seq(),
    ];
    let samples = Testbench::new(m).run(inputs).run()?;
    assert_eq!(samples.len(), 3);
    let v0 = Bits::<4>::from_bits_seq(samples[0].value())?;
    assert_eq!(v0.to_u128(), 0xf);

    // Emit Verilog.
    let module = verilog::emit_graph(inv.graph(), "inv4", inv.inputs(), inv.outputs())
        .run()?;
    let text = module.render().run()?;
    assert!(text.contains("module inv4"));
    assert!(text.contains("assign"));
    assert!(text.contains("input"));
    assert!(text.contains("output"));
    Ok(())
}

#[test]
fn half_adder_emits_and_simulates() -> Result<(), hdl_cat_error::Error> {
    let ha = std_lib::half_adder()?;
    assert_eq!(ha.inputs().len(), 2);
    assert_eq!(ha.outputs().len(), 2);
    assert_eq!(ha.graph().instructions().len(), 2);

    // Interpret the graph directly with (a, b) = (true, true):
    // sum should be false (1 XOR 1), carry should be true (1 AND 1).
    let env = hdl_cat_sim::interp::interpret(
        ha.graph(),
        ha.inputs(),
        &[BitSeq::from_iter([true]), BitSeq::from_iter([true])],
    )?;
    let sum = &env[ha.outputs()[0].index()];
    let carry = &env[ha.outputs()[1].index()];
    assert_eq!(sum.as_ref().map(|s| s.bit(0)), Some(false));
    assert_eq!(carry.as_ref().map(|s| s.bit(0)), Some(true));

    // Verilog.
    let module = verilog::emit_graph(ha.graph(), "half_adder", ha.inputs(), ha.outputs())
        .run()?;
    let text = module.render().run()?;
    assert!(text.contains("module half_adder"));
    Ok(())
}

#[test]
fn full_adder_has_five_instructions() -> Result<(), hdl_cat_error::Error> {
    let fa = std_lib::full_adder()?;
    assert_eq!(fa.graph().instructions().len(), 5);
    Ok(())
}

#[test]
fn compose_sync_of_two_inverters_is_identity() -> Result<(), hdl_cat_error::Error> {
    use hdl_cat_sync::compose_sync;
    let inv_a = gates::not_bits::<4>()?;
    let inv_b = gates::not_bits::<4>()?;
    let ma: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_a);
    let mb: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_b);
    let composed = compose_sync(ma, mb);

    // Two inverters = identity: every input should come out unchanged.
    let inputs = vec![
        Bits::<4>::try_new(0x3)?.to_bits_seq(),
        Bits::<4>::try_new(0x5)?.to_bits_seq(),
        Bits::<4>::try_new(0xa)?.to_bits_seq(),
    ];
    let samples = Testbench::new(composed).run(inputs).run()?;
    assert_eq!(samples.len(), 3);
    assert_eq!(Bits::<4>::from_bits_seq(samples[0].value())?.to_u128(), 0x3);
    assert_eq!(Bits::<4>::from_bits_seq(samples[1].value())?.to_u128(), 0x5);
    assert_eq!(Bits::<4>::from_bits_seq(samples[2].value())?.to_u128(), 0xa);
    Ok(())
}

#[test]
fn counter_increments_each_cycle() -> Result<(), hdl_cat_error::Error> {
    // 8-bit free-running counter.  Input per cycle is empty (unit).
    // Expected outputs: [0, 1, 2, ..., 15] over 16 cycles.
    let c = std_lib::counter::<8>()?;
    let empty_input = BitSeq::new();
    let inputs: Vec<BitSeq> = (0..16).map(|_| empty_input.clone()).collect();
    let samples = Testbench::new(c).run(inputs).run()?;
    assert_eq!(samples.len(), 16);

    let values: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<8>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values, (0u128..16).collect::<Vec<_>>());
    Ok(())
}

#[test]
fn counter_wraps_at_bit_width() -> Result<(), hdl_cat_error::Error> {
    // 4-bit counter wraps back to 0 after 15.
    let c = std_lib::counter::<4>()?;
    let inputs: Vec<BitSeq> = (0..18).map(|_| BitSeq::new()).collect();
    let samples = Testbench::new(c).run(inputs).run()?;
    let values: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    // Expected: 0, 1, 2, ..., 15, 0, 1
    let expected: Vec<u128> = (0..16).chain(0..2).collect();
    assert_eq!(values, expected);
    Ok(())
}

#[test]
fn counter_emits_verilog_combinational_view() -> Result<(), hdl_cat_error::Error> {
    // Combinational view: state wires appear on both sides of the
    // port list.  Preserved as a regression check on `emit_graph`.
    let c = std_lib::counter::<8>()?;
    let module = verilog::emit_graph(c.graph(), "counter8", c.input_wires(), c.output_wires())
        .run()?;
    let text = module.render().run()?;
    assert!(text.contains("module counter8"));
    assert!(text.contains("assign"));
    assert!(text.contains('+'));
    Ok(())
}

#[test]
fn counter_emits_stateful_verilog() -> Result<(), hdl_cat_error::Error> {
    // Stateful view via emit_sync_graph: state wire becomes an
    // always_ff-driven reg, with clk/rst input ports.
    let c = std_lib::counter::<8>()?;
    let module = verilog::emit_sync_graph(
        c.graph(),
        "counter8",
        c.state_wire_count(),
        c.input_wires(),
        c.output_wires(),
        c.initial_state(),
    )
    .run()?;
    let text = module.render().run()?;
    assert!(text.contains("module counter8"));
    assert!(text.contains("input clk"));
    assert!(text.contains("input rst"));
    assert!(text.contains("output reg [7:0] w0"));
    assert!(text.contains("always_ff @(posedge clk)"));
    assert!(text.contains("w0 <= 8'd0"));  // reset value
    assert!(text.contains("w0 <= w2"));    // normal assignment
    // State wire should NOT be listed as both input and output.
    let input_w0_count = text.matches("input [7:0] w0").count();
    assert_eq!(input_w0_count, 0);
    Ok(())
}

#[test]
fn toggle_ff_flips_each_cycle() -> Result<(), hdl_cat_error::Error> {
    let t = std_lib::toggle_ff()?;
    let inputs: Vec<BitSeq> = (0..5).map(|_| BitSeq::new()).collect();
    let samples = Testbench::new(t).run(inputs).run()?;
    let bits: Vec<bool> = samples.iter().map(|s| s.value().bit(0)).collect();
    // Output at each cycle is the *current* state before the toggle:
    // cycle 0: false (initial)
    // cycle 1: true (was toggled from false)
    // cycle 2: false
    // cycle 3: true
    // cycle 4: false
    assert_eq!(bits, vec![false, true, false, true, false]);
    Ok(())
}

#[test]
fn accumulator_sums_inputs() -> Result<(), hdl_cat_error::Error> {
    let acc = std_lib::accumulator::<8>()?;
    let inputs: Vec<BitSeq> = [3u128, 5, 10, 2]
        .iter()
        .map(|&v| Bits::<8>::try_new(v).map(|b| b.to_bits_seq()))
        .collect::<Result<_, _>>()?;
    let samples = Testbench::new(acc).run(inputs).run()?;
    let values: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<8>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    // Cumulative: 3, 8, 18, 20
    assert_eq!(values, vec![3, 8, 18, 20]);
    Ok(())
}

#[test]
fn down_counter_wraps_correctly() -> Result<(), hdl_cat_error::Error> {
    let d = std_lib::down_counter::<4>()?;
    let inputs: Vec<BitSeq> = (0..18).map(|_| BitSeq::new()).collect();
    let samples = Testbench::new(d).run(inputs).run()?;
    let values: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    // Starts at 0xf and decrements; wraps 0 -> 0xf.
    let expected: Vec<u128> = (0..16u128).rev().chain((0..16u128).rev().take(2)).collect();
    assert_eq!(values, expected);
    Ok(())
}

#[test]
fn shift_register_shifts_in_bits() -> Result<(), hdl_cat_error::Error> {
    let s = std_lib::shift_register_left::<4>()?;
    // Shift in 1, 0, 1, 1 successively.  Output reflects the *previous* state.
    let inputs: Vec<BitSeq> = [true, false, true, true]
        .iter()
        .map(|b| BitSeq::from_iter([*b]))
        .collect();
    let samples = Testbench::new(s).run(inputs).run()?;
    let values: Vec<u128> = samples
        .iter()
        .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
        .collect::<Result<Vec<_>, _>>()?;
    // Cycle 0: output = state (initial 0000 = 0); shifted state → 0001
    // Cycle 1: output = 0001 = 1; shifted → 0010 (shift in 0)
    // Cycle 2: output = 0010 = 2; shifted → 0101 (shift in 1)
    // Cycle 3: output = 0101 = 5; shifted → 1011
    assert_eq!(values, vec![0, 1, 2, 5]);
    Ok(())
}

#[test]
fn braid_swaps_tensor_halves_under_simulation() -> Result<(), hdl_cat_error::Error> {
    // Wire the braid A4 ⊗ A8 → A8 ⊗ A4 through a Sync and simulate.
    use hdl_cat_circuit::wired_braid;
    type A4 = Obj<Bits<4>>;
    type A8 = Obj<Bits<8>>;
    let braid_iso = wired_braid::<A4, A8>();
    let braid_arrow = braid_iso.into_forward();
    let m: Sync<
        CircuitUnit,
        hdl_cat_circuit::CircuitTensor<A4, A8>,
        hdl_cat_circuit::CircuitTensor<A8, A4>,
    > = Sync::lift_comb(braid_arrow);

    // Input bits: A4_value (4 bits, LSB-first) ++ A8_value (8 bits).
    // For A4 = 0xa = 0b1010, A8 = 0x42 = 0b0100_0010:
    let a4_bits = Bits::<4>::try_new(0xa)?.to_bits_seq();
    let a8_bits = Bits::<8>::try_new(0x42)?.to_bits_seq();
    let input_bits = a4_bits.concat(a8_bits);

    let samples = Testbench::new(m).run(vec![input_bits]).run()?;
    assert_eq!(samples.len(), 1);

    // Output layout: A8 (8 bits) ++ A4 (4 bits).  Split and decode.
    let out = samples[0].value();
    assert_eq!(out.len(), 12);
    let a8_out: hdl_cat_kind::BitSeq = out.as_slice().iter().take(8).copied().collect();
    let a4_out: hdl_cat_kind::BitSeq = out.as_slice().iter().skip(8).take(4).copied().collect();
    let a8_val = Bits::<8>::from_bits_seq(&a8_out)?;
    let a4_val = Bits::<4>::from_bits_seq(&a4_out)?;
    assert_eq!(a8_val.to_u128(), 0x42);
    assert_eq!(a4_val.to_u128(), 0xa);
    Ok(())
}

#[test]
fn par_sync_runs_two_independent_inverters() -> Result<(), hdl_cat_error::Error> {
    use hdl_cat_sync::par_sync;
    let inv_a = gates::not_bits::<4>()?;
    let inv_b = gates::not_bits::<4>()?;
    let ma: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_a);
    let mb: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv_b);
    let paired = par_sync(ma, mb);

    // Each cycle feeds two 4-bit inputs (concat'd) and gets two 4-bit outputs.
    // Cycle 0: (a=0x0, b=0xf) -> expect (a'=0xf, b'=0x0) concat'd.
    let bits_a = Bits::<4>::try_new(0x0)?.to_bits_seq();
    let bits_b = Bits::<4>::try_new(0xf)?.to_bits_seq();
    let combined_input = bits_a.concat(bits_b);
    let samples = Testbench::new(paired).run(vec![combined_input]).run()?;
    assert_eq!(samples.len(), 1);
    // Output is 8 bits: first 4 = !0x0 = 0xf, second 4 = !0xf = 0x0.
    let out = samples[0].value();
    assert_eq!(out.len(), 8);
    // Low 4 bits should be 0xf (inverted 0x0).
    let low: Vec<bool> = out.as_slice().iter().take(4).copied().collect();
    assert_eq!(low, vec![true, true, true, true]);
    // High 4 bits should be 0x0 (inverted 0xf).
    let high: Vec<bool> = out.as_slice().iter().skip(4).take(4).copied().collect();
    assert_eq!(high, vec![false, false, false, false]);
    Ok(())
}
