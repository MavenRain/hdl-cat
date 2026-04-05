//! Clock-domain-indexed signals.
//!
//! A [`Signal<D, T>`] is a time-varying value of hardware type `T`,
//! flowing through clock domain `D`.  Internally it wraps
//! [`comp_cat_rs::effect::stream::Stream<Error, T>`], and its
//! combinators desugar directly to that crate's combinators.
//!
//! Clock domains are zero-sized marker types that implement
//! [`ClockDomain`].  Mixing signals from different domains is a
//! type error.
//!
//! # Domains
//!
//! Two default domains are provided — [`domain::Red`] and
//! [`domain::Blue`].  Downstream crates may define their own
//! domain markers.
//!
//! # Combinators
//!
//! - [`Signal::from_vec`] / [`Signal::constant_n`] — construct
//! - [`Signal::map`] — pointwise transform
//! - [`Signal::take`] — truncate to the first `n` samples
//! - [`Signal::concat`] — concatenate domain-matched signals
//! - [`Signal::delay`] — insert one-cycle register (shift by one,
//!   prepending a supplied initial value)
//! - [`Signal::collect`] — escape into `Io<Error, Vec<T>>` at
//!   the simulation boundary
//!
//! For `zip`, cross `Signal` with `Io`: collect both signals to
//! `Vec`s inside an `Io` and zip in Io-land.  The underlying
//! `Stream` type does not expose a native `zip`, and we do not
//! work around that here — the zip should happen as part of the
//! test-bench construction, not as a pure signal combinator.
//!
//! # Arc at the boundary
//!
//! `Stream::map` requires `Arc<dyn Fn + Send + Sync>`.  The
//! `Signal::map` wrapper accepts an ordinary closure and boxes
//! it into an `Arc` internally.  `Arc` is therefore present at
//! this API boundary by necessity — it is a comp-cat-rs
//! requirement, not a design choice.

pub mod domain;
pub mod signal;

pub use domain::{ClockDomain, Red, Blue};
pub use signal::{Signal, Reset};
