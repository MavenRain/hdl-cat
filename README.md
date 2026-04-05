# hdl-cat

A Rust hardware description library re-architected around
[`comp-cat-rs`](https://github.com/MavenRain/comp-cat-rs) — every abstraction
(composition, state, effects, simulation, codegen) is a morphism, a Kleisli
arrow, or a catamorphism over a comp-cat-rs effect type.

Conceptually parallel to [RHDL](https://github.com/samitbasu/rhdl): bit-precise
integer types, a typed hardware description IR, a cycle-accurate simulator,
and a Verilog backend. Implementation is independent and categorical.

## Table of contents

1. [Quick start](#quick-start)
2. [Architecture](#architecture)
3. [Core design](#core-design)
4. [Building circuits](#building-circuits)
5. [Stateful machines](#stateful-machines)
6. [Simulation](#simulation)
7. [Verilog emission](#verilog-emission)
8. [VCD traces](#vcd-traces)
9. [The `#[kernel]` macro](#the-kernel-macro)
10. [Per-crate tour](#per-crate-tour)
11. [Conventions](#conventions)
12. [License](#license)

## Quick start

Build, simulate, and emit Verilog for a 4-bit counter:

```rust
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

    // 4. Emit synthesizable SystemVerilog.
    let counter_for_verilog = std_lib::counter::<8>()?;
    let module = verilog::emit_sync_graph(
        counter_for_verilog.graph(),
        "counter8",
        counter_for_verilog.state_wire_count(),
        counter_for_verilog.input_wires(),
        counter_for_verilog.output_wires(),
        counter_for_verilog.initial_state(),
    ).run()?;
    let verilog_text = module.render().run()?;
    assert!(verilog_text.contains("always_ff @(posedge clk)"));
    Ok(())
}
```

## Architecture

```text
hdl-cat-error/    Shared Error enum (hand-rolled Display/From)
hdl-cat-bits/     Bits<N>, SignedBits<N> via const generics
hdl-cat-kind/     Hw trait + TypeDesc (runtime type witness)
hdl-cat-signal/   Signal<D, T> over comp_cat_rs::Stream
hdl-cat-ir/       Free-category IR (comp_cat_rs::Graph instance)
hdl-cat-circuit/  Circuit: Category + MonoidalCategory + Symmetric
hdl-cat-sync/     Mealy machines as IR + initial state
hdl-cat-sim/      Stream-based testbench / simulator / VCD tracer
hdl-cat-verilog/  Verilog AST + stateful emitter
hdl-cat-std/      Component library (counter, accumulator, FIFO, ...)
hdl-cat-macros/   #[kernel] proc-macro frontend
hdl-cat/          Umbrella crate + examples
```

## Core design

- **Signals are Streams.** `Signal<D, T>` wraps `Stream<Error, T>`; clock
  domains are zero-sized phantom types; domain-mixing is a type error.
- **Circuits form a symmetric monoidal category.** Sequential composition is
  `Category::comp`; parallel composition is `MonoidalCategory::tensor_map`.
- **IR is a free category.** `HdlGraph` implements
  `comp_cat_rs::collapse::free_category::Graph`. Compiled circuits are
  sequences of typed `Instruction`s. Simulation and codegen walk the graph.
- **State is Kleisli.** `Sync<S, I, O>` holds an IR graph whose inputs and
  outputs pair `(state, data)`, threaded cycle to cycle. No closures, no
  interior mutability.
- **`run` at the boundary.** Simulation and codegen build `Io`/`Stream`
  pipelines internally, calling `.run()` only at public entry points.

## Building circuits

`hdl-cat-circuit` provides primitive gates and a categorical calculus for
composing them. Each gate is a `CircuitArrow<A, B>` where `A` and `B` are
typed objects (single `Obj<T>` or nested `CircuitTensor`).

```rust
use comp_cat_rs::foundation::category::Category;
use hdl_cat_circuit::{gates, Circuit};

# fn main() -> Result<(), hdl_cat_error::Error> {
// Two 4-bit inverters in series — the identity on 4-bit values.
let inv_a = gates::not_bits::<4>()?;
let inv_b = gates::not_bits::<4>()?;
let roundtrip = Circuit::comp(inv_a, inv_b);
assert_eq!(roundtrip.graph().instructions().len(), 2);
# Ok(()) }
```

Parallel composition places two arrows side-by-side:

```rust
use comp_cat_rs::foundation::monoidal::MonoidalCategory;
use hdl_cat_circuit::{gates, Circuit};

# fn main() -> Result<(), hdl_cat_error::Error> {
let a_inv = gates::not_bits::<4>()?;
let b_inv = gates::not_bits::<4>()?;
let paired = Circuit::tensor_map(a_inv, b_inv);
assert_eq!(paired.inputs().len(), 2);
assert_eq!(paired.outputs().len(), 2);
# Ok(()) }
```

Wire permutations (braid, associator, unitors) come from the monoidal
coherence arrows in `hdl_cat_circuit::coherence`.

## Stateful machines

A `Sync<S, I, O>` wraps an IR graph whose inputs are `state ⊗ input` and
outputs are `next_state ⊗ output`. The simulator threads `next_state` into
the following cycle's `state`.

```rust
use hdl_cat_bits::Bits;
use hdl_cat_circuit::{CircuitUnit, Obj};
use hdl_cat_sync::Sync;

# fn main() -> Result<(), hdl_cat_error::Error> {
// A stateless inverter lifted to a Sync machine (state = CircuitUnit).
let inv = hdl_cat_circuit::gates::not_bits::<4>()?;
let m: Sync<CircuitUnit, Obj<Bits<4>>, Obj<Bits<4>>> = Sync::lift_comb(inv);
assert_eq!(m.state_wire_count(), 0);
# Ok(()) }
```

Sync machines compose via `compose_sync` (state becomes `(S1, S2)`),
`par_sync` (parallel, state `(S1, S2)`), and `feedback_sync` (one-cycle
loop-back).

## Simulation

```rust
use hdl_cat::prelude::*;
use hdl_cat_kind::BitSeq;

# fn main() -> Result<(), hdl_cat_error::Error> {
let counter = std_lib::counter::<4>()?;
let empty = BitSeq::new();
let inputs = vec![empty.clone(), empty.clone(), empty.clone(), empty.clone()];
let samples = Testbench::new(counter).run(inputs).run()?;
let values: Vec<u128> = samples
    .iter()
    .map(|s| Bits::<4>::from_bits_seq(s.value()).map(Bits::to_u128))
    .collect::<Result<Vec<_>, _>>()?;
assert_eq!(values, vec![0, 1, 2, 3]);
# Ok(()) }
```

`Testbench::run` returns `Io<Error, Vec<TimedSample<BitSeq>>>`; `.run()`
collects the samples.

## Verilog emission

Two emitters:

- `emit_graph` — flat combinational view; treats every input wire as a module
  input port and every output wire as an output port. Useful for pure
  combinational circuits.
- `emit_sync_graph` — stateful view; promotes state wires to `reg`
  declarations driven by `always_ff @(posedge clk)` blocks with synchronous
  reset from the machine's initial state.

The counter above emits as:

```verilog
module counter4 (
    input clk,
    input rst,
    output reg [3:0] w0
);
    wire [3:0] w1;
    wire [3:0] w2;
    assign w1 = 4'd1;
    assign w2 = (w0 + w1);
    always_ff @(posedge clk) if (rst) w0 <= 4'd0; else w0 <= w2;
endmodule
```

## VCD traces

`hdl_cat_sim::trace_to_string` runs a simulation and emits a VCD (Value
Change Dump) string. Each graph wire becomes a `$var wire` declaration;
one timestamp per cycle with per-wire values.

```rust
use hdl_cat::prelude::*;
use hdl_cat_kind::BitSeq;
use hdl_cat_sim::trace_to_string;

# fn main() -> Result<(), hdl_cat_error::Error> {
let c = std_lib::counter::<4>()?;
let inputs: Vec<BitSeq> = (0..8).map(|_| BitSeq::new()).collect();
let vcd = trace_to_string(&c, inputs)?;
assert!(vcd.contains("$timescale"));
assert!(vcd.contains("#0"));
# Ok(()) }
```

Redirect the string to `counter.vcd` and open in GTKWave or Surfer.

## The `#[kernel]` macro

`#[kernel]` lifts a small Rust-subset function into a `CircuitArrow`
builder. v1 supports: `bool` / `Bits<N>` / `SignedBits<N>` parameters,
`let` bindings, binary arithmetic/bitwise ops, unary `!`.

```rust
use hdl_cat::prelude::*;
use hdl_cat::kernel;

#[kernel]
fn xor_then_add(a: Bits<8>, b: Bits<8>) -> Bits<8> {
    let x = a ^ b;
    x + a
}

# fn main() -> Result<(), hdl_cat_error::Error> {
let arrow = xor_then_add()?;
// Two instructions: Xor + Add.
assert_eq!(arrow.graph().instructions().len(), 2);
# Ok(()) }
```

After expansion, `xor_then_add` becomes a nullary function returning
`Result<CircuitArrow<CircuitTensor<Obj<Bits<8>>, Obj<Bits<8>>>, Obj<Bits<8>>>, Error>`.

## Per-crate tour

| Crate | Purpose |
|---|---|
| [`hdl-cat-error`](crates/hdl-cat-error) | The workspace-wide `Error` enum with hand-rolled `Display`/`From` |
| [`hdl-cat-bits`](crates/hdl-cat-bits) | `Bits<N>`/`SignedBits<N>` const-generic integers, wrap arithmetic |
| [`hdl-cat-kind`](crates/hdl-cat-kind) | `Hw` trait, `TypeDesc`, `BitSeq` buffer — the hardware-typable-values layer |
| [`hdl-cat-signal`](crates/hdl-cat-signal) | `Signal<D, T>` over `comp_cat_rs::Stream`, clock-domain phantoms |
| [`hdl-cat-ir`](crates/hdl-cat-ir) | `HdlGraph`, `Instruction`, `Op` — the IR implementing `comp_cat_rs::Graph` |
| [`hdl-cat-circuit`](crates/hdl-cat-circuit) | `CircuitArrow`, `Obj`, `CircuitTensor`, gates, `Circuit: Category + MonoidalCategory + Braided + Symmetric` |
| [`hdl-cat-sync`](crates/hdl-cat-sync) | `Sync<S, I, O>` Mealy machines, `compose_sync` / `par_sync` / `feedback_sync` |
| [`hdl-cat-sim`](crates/hdl-cat-sim) | `Testbench`, IR interpreter, `TimedSample`, VCD emission |
| [`hdl-cat-verilog`](crates/hdl-cat-verilog) | Verilog AST, `emit_graph`, `emit_sync_graph`, renderer |
| [`hdl-cat-std`](crates/hdl-cat-std) | Standard components: `counter`, `accumulator`, `down_counter`, `toggle_ff`, `shift_register_left`, `half_adder`, `full_adder` |
| [`hdl-cat-macros`](crates/hdl-cat-macros) | `#[kernel]` proc-macro |
| [`hdl-cat`](crates/hdl-cat) | Umbrella crate re-exporting everything; use `hdl_cat::prelude::*` |

## Conventions

Every crate follows the same Rust discipline (functional, type-driven,
domain-driven). See `CLAUDE.md` at the workspace root for the enforced
rules: newtypes for domain primitives, hand-rolled error handling,
combinators over pattern matching (on both `Option` and `Result`),
static dispatch only, no `mut`/`return`/`loop`/`for`/`unwrap`/`expect`.

Verification gate for every change:

```bash
RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features
cargo test --all
```

## License

Dual-licensed under MIT OR Apache-2.0. See [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE).
