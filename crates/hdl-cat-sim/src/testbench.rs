//! The [`Testbench`] driver.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::{Cycle, Error, SignalName};
use hdl_cat_ir::{HdlGraph, WireId, WireTy};
use hdl_cat_kind::BitSeq;
use hdl_cat_sync::Sync;

use crate::interp::interpret;
use crate::sample::TimedSample;

/// Drive a [`Sync`] machine for a fixed number of cycles and
/// collect each cycle's output.
#[must_use]
pub struct Testbench<S, I, O> {
    machine: Sync<S, I, O>,
}

impl<S, I, O> Testbench<S, I, O>
where
    S: 'static,
    I: 'static,
    O: 'static,
{
    /// Create a testbench wrapping a machine.
    pub fn new(machine: Sync<S, I, O>) -> Self {
        Self { machine }
    }

    /// Run the machine over a supplied sequence of inputs.
    ///
    /// Each element of `inputs` is a `BitSeq` containing the
    /// input wires' values for one cycle (state is not included —
    /// state is threaded automatically from the machine's
    /// `initial_state`).  Each cycle produces one
    /// [`TimedSample<BitSeq>`] whose `value` is the concatenation
    /// of the machine's non-state output wires.
    #[must_use]
    pub fn run(self, inputs: Vec<BitSeq>) -> Io<Error, Vec<TimedSample<BitSeq>>> {
        let Self { machine } = self;
        Io::suspend(move || run_cycles(&machine, inputs))
    }
}

fn run_cycles<S, I, O>(
    machine: &Sync<S, I, O>,
    per_cycle_inputs: Vec<BitSeq>,
) -> Result<Vec<TimedSample<BitSeq>>, Error> {
    let state_count = machine.state_wire_count();

    // Split wire lists into state vs data portions.
    let (state_in_wires, _data_in_wires) =
        machine.input_wires().split_at(state_count);

    // Compact (element-width) widths for initial state validation.
    let compact_widths =
        all_wire_widths(machine.graph(), machine.input_wires());
    let (compact_state_widths, input_widths) =
        compact_widths.split_at(state_count);

    // Storage widths for state wires (element_width * depth for
    // arrays, same as width for scalars).  Used for cycle-to-cycle
    // state threading.
    let storage_state_widths =
        all_wire_storage_widths(machine.graph(), state_in_wires);

    // Validate the compact initial state.
    let initial_state = machine.initial_state().clone();
    let total_compact_bits: usize = compact_state_widths.iter().sum();
    (initial_state.len() == total_compact_bits)
        .then_some(())
        .ok_or_else(|| Error::WidthMismatch {
            expected: hdl_cat_error::Width::new(
                u32::try_from(total_compact_bits).unwrap_or(u32::MAX),
            ),
            actual: hdl_cat_error::Width::new(
                u32::try_from(initial_state.len()).unwrap_or(u32::MAX),
            ),
        })?;

    // Expand compact initial state to full storage.
    let compact_values =
        split_by_widths(&initial_state, compact_state_widths)?;
    let full_state_bits = expand_state_for_interpreter(
        machine.graph(),
        state_in_wires,
        compact_values,
    )
    .into_iter()
    .fold(BitSeq::new(), BitSeq::concat);

    let (samples, _final_state) = per_cycle_inputs
        .into_iter()
        .enumerate()
        .try_fold(
            (Vec::<TimedSample<BitSeq>>::new(), full_state_bits),
            |(acc, state_bits), (cycle_idx, cycle_input_bits)| {
                // Split state by storage widths (full storage
                // for arrays).
                let state_values = split_by_widths(
                    &state_bits,
                    &storage_state_widths,
                )?;
                // Split the cycle's input bits by element widths.
                let input_values =
                    split_by_widths(&cycle_input_bits, input_widths)?;
                // Concatenate: state wires first, input wires
                // second.
                let all_inputs: Vec<BitSeq> = state_values
                    .into_iter()
                    .chain(input_values)
                    .collect();

                // Interpret the graph.
                let env = interpret(
                    machine.graph(),
                    machine.input_wires(),
                    &all_inputs,
                )?;

                // Read outputs.
                let output_values: Vec<BitSeq> = machine
                    .output_wires()
                    .iter()
                    .map(|w| read_env(&env, *w))
                    .collect::<Result<Vec<_>, Error>>()?;

                let (next_state_values, data_out_values) =
                    output_values.split_at(state_count);

                // Pack next_state into a flat BitSeq (full
                // storage preserved between cycles).
                let next_state_bits = next_state_values
                    .iter()
                    .cloned()
                    .fold(BitSeq::new(), BitSeq::concat);

                // Pack data outputs into a flat BitSeq.
                let sample_bits = data_out_values
                    .iter()
                    .cloned()
                    .fold(BitSeq::new(), BitSeq::concat);

                let new_acc = acc
                    .into_iter()
                    .chain(core::iter::once(TimedSample::new(
                        Cycle::new(cycle_idx_as_u64(cycle_idx)),
                        sample_bits,
                    )))
                    .collect();
                Ok::<(Vec<TimedSample<BitSeq>>, BitSeq), Error>(
                    (new_acc, next_state_bits),
                )
            },
        )?;
    Ok(samples)
}

fn cycle_idx_as_u64(idx: usize) -> u64 {
    u64::try_from(idx).unwrap_or(u64::MAX)
}

fn all_wire_widths(graph: &HdlGraph, wires: &[WireId]) -> Vec<usize> {
    wires
        .iter()
        .map(|w| graph.wire_ty(w.index()).map_or(0, wire_width_usize))
        .collect()
}

fn all_wire_storage_widths(graph: &HdlGraph, wires: &[WireId]) -> Vec<usize> {
    wires
        .iter()
        .map(|w| graph.wire_ty(w.index()).map_or(0, WireTy::storage_bits))
        .collect()
}

fn wire_width_usize(ty: &WireTy) -> usize {
    usize::try_from(ty.width()).unwrap_or(0)
}

/// Expand a compact (per-element) state into full storage for the
/// interpreter.  Scalar wires pass through unchanged.  Array wires
/// replicate their element-width reset value `depth` times.
fn expand_state_for_interpreter(
    graph: &HdlGraph,
    state_wires: &[WireId],
    compact_values: Vec<BitSeq>,
) -> Vec<BitSeq> {
    state_wires
        .iter()
        .zip(compact_values)
        .map(|(w, val)| {
            graph
                .wire_ty(w.index())
                .and_then(WireTy::depth)
                .map_or_else(
                    || val.clone(),
                    |d| (0..d).fold(BitSeq::new(), |acc, _| acc.concat(val.clone())),
                )
        })
        .collect()
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
    use super::Testbench;
    use hdl_cat_bits::Bits;
    use hdl_cat_circuit::{gates, CircuitUnit, Obj};
    use hdl_cat_kind::Hw;
    use hdl_cat_sync::Sync;

    #[test]
    fn stateless_inverter_inverts_each_cycle() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let inputs = vec![
            Bits::<4>::try_new(0x0)?.to_bits_seq(),
            Bits::<4>::try_new(0xf)?.to_bits_seq(),
            Bits::<4>::try_new(0xa)?.to_bits_seq(),
        ];
        let samples = Testbench::new(m).run(inputs).run()?;
        assert_eq!(samples.len(), 3);

        let v0 = Bits::<4>::from_bits_seq(samples[0].value())?;
        let v1 = Bits::<4>::from_bits_seq(samples[1].value())?;
        let v2 = Bits::<4>::from_bits_seq(samples[2].value())?;
        assert_eq!(v0.to_u128(), 0xf);
        assert_eq!(v1.to_u128(), 0x0);
        assert_eq!(v2.to_u128(), 0x5);
        Ok(())
    }

    #[test]
    fn cycles_are_indexed_from_zero() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<2>()?;
        let m: Sync<CircuitUnit, Obj<Bits<2>>, Obj<Bits<2>>> = Sync::lift_comb(inv);
        let inputs = vec![
            Bits::<2>::try_new(0b00)?.to_bits_seq(),
            Bits::<2>::try_new(0b01)?.to_bits_seq(),
        ];
        let samples = Testbench::new(m).run(inputs).run()?;
        assert_eq!(samples[0].cycle().index(), 0);
        assert_eq!(samples[1].cycle().index(), 1);
        Ok(())
    }

    #[test]
    fn empty_inputs_produces_no_samples() -> Result<(), hdl_cat_error::Error> {
        let inv = gates::not_bits::<4>()?;
        let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
        let samples = Testbench::new(m).run(Vec::new()).run()?;
        assert!(samples.is_empty());
        Ok(())
    }

    #[test]
    fn stateful_machine_threads_state_across_cycles() -> Result<(), hdl_cat_error::Error> {
        // Build a hand-rolled "state passthrough" machine:
        //
        //   input wires:  [state (Bits<4>), data_in (Bits<4>)]
        //   output wires: [next_state (= state + 1), data_out (= state)]
        //
        // Each cycle, the state increments and the output is the
        // *current* state (before increment).
        use hdl_cat_ir::{BinOp, HdlGraphBuilder, Op, WireTy};
        let (bld, state) = HdlGraphBuilder::new().with_wire(WireTy::Bits(4));
        let (bld, data_in) = bld.with_wire(WireTy::Bits(4));
        let (bld, one) = bld.with_wire(WireTy::Bits(4));
        let (bld, next_state) = bld.with_wire(WireTy::Bits(4));
        let (bld, data_out) = bld.with_wire(WireTy::Bits(4));
        // const one = 1
        let bld = bld.with_instruction(
            Op::Const {
                bits: {
                    let mut v: Vec<bool> = (0..4).map(|_| false).collect();
                    v[0] = true;
                    hdl_cat_kind::BitSeq::from_vec(v)
                },
                ty: WireTy::Bits(4),
            },
            vec![],
            one,
        )?;
        // next_state = state + one
        let bld = bld.with_instruction(Op::Bin(BinOp::Add), vec![state, one], next_state)?;
        // data_out = state   (no op: we just use state wire directly — but we need an instruction writing data_out)
        // Trick: use XOR with zero — but we don't have a literal zero here.  Instead use AND state with all-ones.
        let (bld, mask) = bld.with_wire(WireTy::Bits(4));
        let bld = bld.with_instruction(
            Op::Const {
                bits: hdl_cat_kind::BitSeq::from_iter([true, true, true, true]),
                ty: WireTy::Bits(4),
            },
            vec![],
            mask,
        )?;
        let bld = bld.with_instruction(Op::Bin(BinOp::And), vec![state, mask], data_out)?;
        let graph = bld.build();

        let m: Sync<Obj<Bits<4>>, Obj<Bits<4>>, Obj<Bits<4>>> = hdl_cat_sync::machine::from_raw(
            graph,
            vec![state, data_in],
            vec![next_state, data_out],
            Bits::<4>::try_new(0)?.to_bits_seq(),
            1,
        );

        // Drive 4 cycles with arbitrary data_in; expect outputs [0, 1, 2, 3].
        let inputs: Vec<_> = (0..4)
            .map(|i| Bits::<4>::try_new(i).unwrap_or(Bits::<4>::ZERO).to_bits_seq())
            .collect();
        let samples = Testbench::new(m).run(inputs).run()?;
        assert_eq!(samples.len(), 4);
        let values: Vec<u128> = samples
            .iter()
            .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(values, vec![0, 1, 2, 3]);
        Ok(())
    }
}
