//! Tests for the `#[kernel]` proc-macro.

use hdl_cat::prelude::*;
use hdl_cat::kernel;

#[kernel]
fn xor_then_add(a: Bits<8>, b: Bits<8>) -> Bits<8> {
    let x = a ^ b;
    x + a
}

#[kernel]
fn single_and(a: Bits<4>, b: Bits<4>) -> Bits<4> {
    a & b
}

#[kernel]
fn triple(a: Bits<4>, b: Bits<4>, c: Bits<4>) -> Bits<4> {
    a + b + c
}

#[test]
fn xor_then_add_builds_correct_arrow() -> Result<(), hdl_cat_error::Error> {
    let arrow = xor_then_add()?;
    // Expected: one Xor + one Add = two instructions, three input wires (a, b) + two tmps = 4 wires.
    assert_eq!(arrow.graph().instructions().len(), 2);
    assert_eq!(arrow.inputs().len(), 2);
    assert_eq!(arrow.outputs().len(), 1);
    Ok(())
}

#[test]
fn single_and_has_one_instruction() -> Result<(), hdl_cat_error::Error> {
    let arrow = single_and()?;
    assert_eq!(arrow.graph().instructions().len(), 1);
    assert_eq!(arrow.inputs().len(), 2);
    Ok(())
}

#[test]
fn triple_add_builds() -> Result<(), hdl_cat_error::Error> {
    let arrow = triple()?;
    assert_eq!(arrow.graph().instructions().len(), 2); // two additions
    assert_eq!(arrow.inputs().len(), 3);
    assert_eq!(arrow.outputs().len(), 1);
    Ok(())
}

#[test]
fn xor_then_add_simulates_correctly() -> Result<(), hdl_cat_error::Error> {
    // Lift the kernel arrow to a Sync and drive it.
    let arrow = xor_then_add()?;
    let m: Sync<CircuitUnit, _, Obj<Bits<8>>> = Sync::lift_comb(arrow);

    // Inputs: a=0x0f, b=0x33 → a^b=0x3c → 0x3c + 0x0f = 0x4b.
    let a_bits = Bits::<8>::try_new(0x0f)?.to_bits_seq();
    let b_bits = Bits::<8>::try_new(0x33)?.to_bits_seq();
    let combined = a_bits.concat(b_bits);
    let samples = Testbench::new(m).run(vec![combined]).run()?;
    assert_eq!(samples.len(), 1);
    let result = Bits::<8>::from_bits_seq(samples[0].value())?;
    assert_eq!(result.to_u128(), 0x4b);
    Ok(())
}

#[test]
fn triple_simulates_correctly() -> Result<(), hdl_cat_error::Error> {
    let arrow = triple()?;
    let m: Sync<CircuitUnit, _, Obj<Bits<4>>> = Sync::lift_comb(arrow);
    // a=1, b=2, c=3 → sum = 6.
    let a = Bits::<4>::try_new(1)?.to_bits_seq();
    let b = Bits::<4>::try_new(2)?.to_bits_seq();
    let c = Bits::<4>::try_new(3)?.to_bits_seq();
    let combined = a.concat(b).concat(c);
    let samples = Testbench::new(m).run(vec![combined]).run()?;
    let result = Bits::<4>::from_bits_seq(samples[0].value())?;
    assert_eq!(result.to_u128(), 6);
    Ok(())
}
