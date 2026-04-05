//! Timed-sample type for simulator output.

use hdl_cat_error::Cycle;

/// A timestamped simulation sample.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct TimedSample<T> {
    cycle: Cycle,
    value: T,
}

impl<T> TimedSample<T> {
    /// Construct a timed sample.
    pub fn new(cycle: Cycle, value: T) -> Self {
        Self { cycle, value }
    }

    /// The cycle at which this sample was observed.
    #[must_use]
    pub fn cycle(&self) -> Cycle {
        self.cycle
    }

    /// The sampled value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consume the sample and yield its value.
    pub fn into_value(self) -> T {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::TimedSample;
    use hdl_cat_error::Cycle;

    #[test]
    fn sample_holds_cycle_and_value() {
        let s = TimedSample::new(Cycle::new(3), 42u32);
        assert_eq!(s.cycle().index(), 3);
        assert_eq!(*s.value(), 42);
    }

    #[test]
    fn into_value_consumes() {
        let s = TimedSample::new(Cycle::new(0), "hello".to_string());
        assert_eq!(s.into_value(), "hello");
    }
}
