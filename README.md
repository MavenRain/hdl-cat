# hdl-cat

A Rust hardware description library re-architected around
[`comp-cat-rs`](https://github.com/MavenRain/comp-cat-rs) — every abstraction
(composition, state, effects, simulation, codegen) is a morphism, a Kleisli
arrow, or a catamorphism over a comp-cat-rs effect type.

Conceptually parallel to [RHDL](https://github.com/samitbasu/rhdl): bit-precise
integer types, a typed hardware description IR, a cycle-accurate simulator,
and a Verilog backend. Implementation is independent and categorical.

## Status

Work in progress. Early phases.

## Architecture

```text
hdl-cat-error/    Shared Error enum (hand-rolled Display/From)
hdl-cat-bits/     Bits<N>, SignedBits<N> via const generics
hdl-cat-kind/     Hw trait + TypeDesc (runtime type witness)
hdl-cat-signal/   Signal<D, T> over comp_cat_rs::Stream
hdl-cat-ir/       Free-category IR (comp_cat_rs::Graph instance)
hdl-cat-circuit/  Circuit: Category + MonoidalCategory + Symmetric
hdl-cat-sync/     Mealy/Moore machines as Kleisli arrows over Io
hdl-cat-sim/      Stream-based testbench / simulator
hdl-cat-verilog/  Io-based Verilog AST emitter
hdl-cat-std/      Component library (adder, counter, FIFO, ...)
hdl-cat/          Umbrella crate + examples
```

## Core design

- **Signals are Streams.** `Signal<D, T>` wraps `Stream<Error, T>`; clock
  domains are zero-sized phantom types; domain-mixing is a type error.
- **Circuits form a symmetric monoidal category.** Sequential composition is
  `Category::comp`; parallel composition is `MonoidalCategory::tensor_map`.
- **IR is a free category.** `HdlGraph` implements
  `comp_cat_rs::collapse::free_category::Graph`. Compiled circuits are
  `Path`s. Simulation and codegen are `GraphMorphism` interpretations.
- **State is Kleisli.** `Sync<S, I, O>` is a state-threading arrow over
  `Io<Error, _>`. No `&mut self`, no interior mutability.
- **`run` at the boundary.** Simulation and codegen build `Io`/`Stream`
  pipelines internally, calling `.run()` only at public entry points.

## Conventions

Every crate follows the same Rust discipline (functional, type-driven,
domain-driven). See the per-crate `CLAUDE.md` files and the rustdoc for the
enforced rules: newtypes for domain primitives, hand-rolled error handling,
combinators over pattern matching, static dispatch only.

## License

Dual-licensed under MIT OR Apache-2.0. See [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE).
