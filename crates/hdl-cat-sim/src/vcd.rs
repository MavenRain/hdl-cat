//! VCD (Value Change Dump) trace emission.
//!
//! Writes a simulation run to a string in VCD format, viewable
//! in `GTKWave`, Surfer, and other waveform tools.
//!
//! Emits every wire in the machine's graph at every cycle (no
//! change-only compression).  Each wire is identified by
//! `v{index}` in the VCD output.

use hdl_cat_error::{Error, SignalName};
use hdl_cat_ir::{HdlGraph, WireId, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::Sync;

use crate::interp::interpret;

/// Run `machine` for `per_cycle_inputs.len()` cycles and return
/// a VCD trace as a string.
///
/// # Errors
///
/// Propagates any error raised by the IR interpreter (undefined
/// signals, width mismatches, etc.).
pub fn trace_to_string<S, I, O>(
    machine: &Sync<S, I, O>,
    per_cycle_inputs: Vec<BitSeq>,
) -> Result<String, Error> {
    let state_count = machine.state_wire_count();
    let widths: Vec<usize> = machine
        .input_wires()
        .iter()
        .map(|w| graph_wire_width(machine.graph(), *w))
        .collect();
    let (state_widths, input_widths) = widths.split_at(state_count);

    let initial_state = machine.initial_state().clone();
    let total_state_bits: usize = state_widths.iter().sum();
    (initial_state.len() == total_state_bits)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(total_state_bits).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(initial_state.len()).unwrap_or(u32::MAX),
            ),
        })?;

    // Collect one env per cycle plus the final state.
    let (envs, _final_state) = per_cycle_inputs.into_iter().try_fold(
        (Vec::<Vec<Option<BitSeq>>>::new(), initial_state),
        |(acc, state_bits), cycle_input| {
            let state_values = split_by_widths(&state_bits, state_widths)?;
            let input_values = split_by_widths(&cycle_input, input_widths)?;
            let all_inputs: Vec<BitSeq> =
                state_values.into_iter().chain(input_values).collect();

            let env = interpret(machine.graph(), machine.input_wires(), &all_inputs)?;

            // Extract next-state wires.
            let next_state_bits = machine
                .output_wires()
                .iter()
                .take(state_count)
                .map(|w| read_env(&env, *w))
                .collect::<Result<Vec<BitSeq>, Error>>()?
                .into_iter()
                .fold(BitSeq::new(), BitSeq::concat);

            let new_acc = acc
                .into_iter()
                .chain(core::iter::once(env))
                .collect();
            Ok::<(Vec<Vec<Option<BitSeq>>>, BitSeq), Error>((new_acc, next_state_bits))
        },
    )?;

    Ok(render_vcd(machine.graph(), &envs))
}

fn render_vcd(graph: &HdlGraph, envs: &[Vec<Option<BitSeq>>]) -> String {
    let header = render_header(graph);
    let body = envs
        .iter()
        .enumerate()
        .map(|(cycle, env)| render_cycle(cycle, env))
        .collect::<Vec<_>>()
        .concat();
    format!("{header}{body}")
}

fn render_header(graph: &HdlGraph) -> String {
    let prelude = "$version hdl-cat $end\n$timescale 1ns $end\n$scope module top $end\n";
    let vars: String = graph
        .wires()
        .iter()
        .enumerate()
        .map(|(idx, ty)| render_var_decl(idx, ty))
        .collect();
    let closing = "$upscope $end\n$enddefinitions $end\n";
    format!("{prelude}{vars}{closing}")
}

fn render_var_decl(idx: usize, ty: &WireTy) -> String {
    let id = vcd_id(idx);
    let width = ty.width();
    format!("$var wire {width} {id} w{idx} $end\n")
}

fn render_cycle(cycle: usize, env: &[Option<BitSeq>]) -> String {
    let time = cycle * 10;
    let values: String = env
        .iter()
        .enumerate()
        .map(|(idx, maybe)| render_wire_value(idx, maybe.as_ref()))
        .collect();
    if cycle == 0 {
        format!("#{time}\n$dumpvars\n{values}$end\n")
    } else {
        format!("#{time}\n{values}")
    }
}

fn render_wire_value(idx: usize, value: Option<&BitSeq>) -> String {
    let id = vcd_id(idx);
    match value {
        None => format!("bx {id}\n"),
        Some(bits) if bits.len() <= 1 => {
            let bit = bits.as_slice().first().copied().unwrap_or(false);
            let digit = if bit { "1" } else { "0" };
            format!("{digit}{id}\n")
        }
        Some(bits) => {
            // VCD binary: MSB first, so reverse the LSB-first BitSeq.
            let binary: String = bits
                .as_slice()
                .iter()
                .rev()
                .map(|b| if *b { '1' } else { '0' })
                .collect();
            format!("b{binary} {id}\n")
        }
    }
}

fn vcd_id(idx: usize) -> String {
    format!("v{idx}")
}

fn graph_wire_width(graph: &HdlGraph, w: WireId) -> usize {
    graph
        .wire_ty(w.index())
        .map_or(0, |ty: &WireTy| usize::try_from(ty.width()).unwrap_or(0))
}

fn read_env(env: &[Option<BitSeq>], w: WireId) -> Result<BitSeq, Error> {
    env.get(w.index())
        .and_then(Clone::clone)
        .ok_or_else(|| Error::UndefinedSignal {
            name: SignalName::new(format!("{w}")),
        })
}

fn split_by_widths(assembled: &BitSeq, widths: &[usize]) -> Result<Vec<BitSeq>, Error> {
    let total: usize = widths.iter().sum();
    (assembled.len() == total)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(u32::try_from(total).unwrap_or(u32::MAX)),
            actual: hdl_cat_error::Width::new(
                u32::try_from(assembled.len()).unwrap_or(u32::MAX),
            ),
        })?;
    let (chunks, _) = widths.iter().fold(
        (Vec::<BitSeq>::new(), 0usize),
        |(acc, offset), &w| {
            let chunk: BitSeq = assembled
                .as_slice()
                .iter()
                .skip(offset)
                .take(w)
                .copied()
                .collect();
            let new_acc = acc
                .into_iter()
                .chain(core::iter::once(chunk))
                .collect();
            (new_acc, offset + w)
        },
    );
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::trace_to_string;
    use hdl_cat_bits::Bits;
    use hdl_cat_circuit::{gates, CircuitUnit, Obj};
    use hdl_cat_kind::Hw;
    use hdl_cat_sync::Sync;

    #[test]
    fn vcd_header_is_well_formed() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let inputs = vec![Bits::<4>::try_new(0x5)?.to_bits_seq()];
        let vcd = trace_to_string(&m, inputs)?;
        assert!(vcd.contains("$version"));
        assert!(vcd.contains("$timescale"));
        assert!(vcd.contains("$scope module top"));
        assert!(vcd.contains("$var wire 4"));
        assert!(vcd.contains("$enddefinitions"));
        assert!(vcd.contains("$dumpvars"));
        Ok(())
    }

    #[test]
    fn vcd_has_one_dumpvars_plus_timestamps_per_cycle() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let inputs = vec![
            Bits::<4>::try_new(0x0)?.to_bits_seq(),
            Bits::<4>::try_new(0x1)?.to_bits_seq(),
            Bits::<4>::try_new(0x2)?.to_bits_seq(),
        ];
        let vcd = trace_to_string(&m, inputs)?;
        // Three cycles → timestamps at #0, #10, #20.
        assert!(vcd.contains("#0"));
        assert!(vcd.contains("#10"));
        assert!(vcd.contains("#20"));
        Ok(())
    }

    #[test]
    fn vcd_encodes_output_binary_msb_first() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        // Input 0xa = 1010 LSB-first -> binary "1010" MSB-first.
        // Inverted = 0101 MSB-first (value 0x5).
        let inputs = vec![Bits::<4>::try_new(0xa)?.to_bits_seq()];
        let vcd = trace_to_string(&m, inputs)?;
        // Input wire w0 holds 0xa at cycle 0; MSB-first is "1010".
        assert!(vcd.contains("b1010 v0"));
        // Output wire w1 holds 0x5 = ~0xa & 0xf; MSB-first "0101".
        assert!(vcd.contains("b0101 v1"));
        Ok(())
    }

    #[test]
    fn vcd_emits_one_var_per_graph_wire() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let inputs = vec![Bits::<4>::try_new(0x0)?.to_bits_seq()];
        let vcd = trace_to_string(&m, inputs)?;
        // Graph has 2 wires (input, output); expect two $var declarations.
        let var_count = vcd.matches("$var wire").count();
        assert_eq!(var_count, 2);
        Ok(())
    }

    #[test]
    fn empty_inputs_produces_header_only() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let vcd = trace_to_string(&m, Vec::new())?;
        assert!(vcd.contains("$enddefinitions"));
        // No cycle markers.
        assert!(!vcd.contains("#0\n"));
        Ok(())
    }
}
