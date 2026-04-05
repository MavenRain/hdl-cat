//! [`Signal<D, T>`] and [`Reset<D>`].

use core::marker::PhantomData;
use std::sync::Arc;

use comp_cat_rs::effect::{io::Io, stream::Stream};
use hdl_cat_error::Error;
use hdl_cat_kind::Hw;

use crate::domain::ClockDomain;

/// A time-varying value of hardware type `T` in clock domain `D`.
///
/// Internally wraps a `Stream<Error, T>`.  Pulled one sample at a
/// time by downstream consumers (simulator, codegen).
///
/// `Signal` is move-only: fan-out is modeled by explicit Io-level
/// collection followed by re-broadcast.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), hdl_cat_error::Error> {
/// use hdl_cat_signal::{Signal, Red};
/// use hdl_cat_bits::Bits;
///
/// let b0 = Bits::<4>::try_new(0)?;
/// let b1 = Bits::<4>::try_new(1)?;
/// let b2 = Bits::<4>::try_new(2)?;
/// let s: Signal<Red, Bits<4>> = Signal::from_vec(vec![b0, b1, b2]);
/// let doubled = s.map(|b| b + b);
/// let samples: Vec<Bits<4>> = doubled.collect().run()?;
/// assert_eq!(samples[0].to_u128(), 0);
/// assert_eq!(samples[1].to_u128(), 2);
/// assert_eq!(samples[2].to_u128(), 4);
/// # Ok(()) }
/// ```
#[must_use]
pub struct Signal<D, T>
where
    D: ClockDomain,
    T: Hw + Send + 'static,
{
    inner: Stream<Error, T>,
    _domain: PhantomData<D>,
}

impl<D, T> Signal<D, T>
where
    D: ClockDomain,
    T: Hw + Send + 'static,
{
    /// Wrap an existing `Stream` as a signal in domain `D`.
    pub fn from_stream(stream: Stream<Error, T>) -> Self {
        Self { inner: stream, _domain: PhantomData }
    }

    /// Unwrap to the underlying `Stream`.
    #[must_use]
    pub fn into_stream(self) -> Stream<Error, T> {
        self.inner
    }

    /// A signal from a finite vector of samples.
    pub fn from_vec(items: Vec<T>) -> Self {
        Self::from_stream(Stream::from_vec(items))
    }

    /// The empty signal (no samples).
    pub fn empty() -> Self {
        Self::from_stream(Stream::empty())
    }

    /// A signal that emits `value` exactly `n` times.
    pub fn constant_n(value: &T, n: usize) -> Self
    where
        T: Clone,
    {
        let v: Vec<T> = (0..n).map(|_| value.clone()).collect();
        Self::from_vec(v)
    }

    /// Map a pure function pointwise over this signal's samples.
    ///
    /// The closure is wrapped in an `Arc` at the comp-cat-rs
    /// boundary — see the crate-level docs.
    pub fn map<U, F>(self, f: F) -> Signal<D, U>
    where
        U: Hw + Send + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        Signal::from_stream(self.inner.map(Arc::new(f)))
    }

    /// Truncate this signal to the first `n` samples.
    pub fn take(self, n: usize) -> Self {
        Self::from_stream(self.inner.take(n))
    }

    /// Concatenate two signals in the same domain.
    pub fn concat(self, other: Self) -> Self {
        Self::from_stream(self.inner.concat(other.inner))
    }

    /// Shift this signal by one cycle, prepending an initial sample.
    ///
    /// Models a one-cycle delay register: the output at cycle 0 is
    /// `init`; the output at cycle `k+1` is the original signal's
    /// sample at cycle `k`.
    pub fn delay(self, init: T) -> Self {
        let emit_init: Stream<Error, T> = Stream::from_vec(vec![init]);
        Self::from_stream(emit_init.concat(self.inner))
    }

    /// Collect all samples into an `Io<Error, Vec<T>>`.
    ///
    /// This is the escape hatch from `Signal` into `Io` land —
    /// call `.run()` on the returned `Io` at the simulation
    /// boundary to actually execute.
    #[must_use]
    pub fn collect(self) -> Io<Error, Vec<T>> {
        self.inner.collect()
    }
}

/// A reset line in clock domain `D`.
///
/// Newtype wrapper over `Signal<D, bool>`.  Convention: `true`
/// means "assert reset"; `false` means "normal operation."
#[must_use]
pub struct Reset<D>(Signal<D, bool>)
where
    D: ClockDomain;

impl<D: ClockDomain> Reset<D> {
    /// Wrap a `bool` signal as a reset line.
    pub fn from_signal(s: Signal<D, bool>) -> Self {
        Self(s)
    }

    /// Unwrap to the underlying boolean signal.
    pub fn into_signal(self) -> Signal<D, bool> {
        self.0
    }

    /// A reset line held low (deasserted) for `n` cycles.
    pub fn deasserted_for(n: usize) -> Self {
        Self(Signal::<D, bool>::constant_n(&false, n))
    }

    /// A reset line held high (asserted) for `n` cycles.
    pub fn asserted_for(n: usize) -> Self {
        Self(Signal::<D, bool>::constant_n(&true, n))
    }
}

#[cfg(test)]
mod tests {
    use super::{Reset, Signal};
    use crate::domain::Red;
    use hdl_cat_bits::Bits;

    fn bits(v: u128) -> Result<Bits<8>, hdl_cat_error::Error> {
        Bits::<8>::try_new(v)
    }

    #[test]
    fn from_vec_then_collect_round_trips() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> =
            Signal::from_vec(vec![bits(1)?, bits(2)?, bits(3)?]);
        let v = s.collect().run()?;
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].to_u128(), 1);
        assert_eq!(v[2].to_u128(), 3);
        Ok(())
    }

    #[test]
    fn empty_signal_collects_to_empty() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> = Signal::empty();
        let v = s.collect().run()?;
        assert!(v.is_empty());
        Ok(())
    }

    #[test]
    fn constant_n_produces_n_copies() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> = Signal::constant_n(&bits(42)?, 4);
        let v = s.collect().run()?;
        assert_eq!(v.len(), 4);
        assert!(v.iter().all(|b| b.to_u128() == 42));
        Ok(())
    }

    #[test]
    fn map_doubles_each_sample() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> = Signal::from_vec(vec![bits(1)?, bits(2)?, bits(3)?]);
        let doubled = s.map(|b| b + b);
        let v = doubled.collect().run()?;
        assert_eq!(v[0].to_u128(), 2);
        assert_eq!(v[1].to_u128(), 4);
        assert_eq!(v[2].to_u128(), 6);
        Ok(())
    }

    #[test]
    fn take_truncates() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> =
            Signal::from_vec(vec![bits(1)?, bits(2)?, bits(3)?, bits(4)?]);
        let v = s.take(2).collect().run()?;
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].to_u128(), 2);
        Ok(())
    }

    #[test]
    fn concat_joins_signals() -> Result<(), hdl_cat_error::Error> {
        let a: Signal<Red, Bits<8>> = Signal::from_vec(vec![bits(1)?, bits(2)?]);
        let b: Signal<Red, Bits<8>> = Signal::from_vec(vec![bits(3)?, bits(4)?]);
        let v = a.concat(b).collect().run()?;
        assert_eq!(v.len(), 4);
        assert_eq!(v[2].to_u128(), 3);
        Ok(())
    }

    #[test]
    fn delay_prepends_initial_sample() -> Result<(), hdl_cat_error::Error> {
        let s: Signal<Red, Bits<8>> =
            Signal::from_vec(vec![bits(10)?, bits(20)?, bits(30)?]);
        let d = s.delay(bits(0)?);
        let v = d.collect().run()?;
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].to_u128(), 0);
        assert_eq!(v[1].to_u128(), 10);
        assert_eq!(v[3].to_u128(), 30);
        Ok(())
    }

    #[test]
    fn reset_deasserted_for_n_is_all_false() -> Result<(), hdl_cat_error::Error> {
        let r: Reset<Red> = Reset::deasserted_for(3);
        let v = r.into_signal().collect().run()?;
        assert_eq!(v, vec![false, false, false]);
        Ok(())
    }

    #[test]
    fn reset_asserted_for_n_is_all_true() -> Result<(), hdl_cat_error::Error> {
        let r: Reset<Red> = Reset::asserted_for(2);
        let v = r.into_signal().collect().run()?;
        assert_eq!(v, vec![true, true]);
        Ok(())
    }
}
