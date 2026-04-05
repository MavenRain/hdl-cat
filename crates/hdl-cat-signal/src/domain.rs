//! Clock domain marker types.
//!
//! A clock domain is a zero-sized type implementing the
//! [`ClockDomain`] marker trait.  Each [`crate::Signal`] carries
//! its domain as a phantom type parameter; mixing signals across
//! domains without an explicit crossing primitive is a compile-
//! time error.

/// Marker trait for clock domains.
///
/// Implementors must be `Copy`, `'static`, and carry no data.
/// Implement this trait on a unit struct for each distinct
/// clock in your design.
///
/// # Examples
///
/// ```
/// use hdl_cat_signal::ClockDomain;
///
/// #[derive(Copy, Clone)]
/// struct AudioClk;
/// impl ClockDomain for AudioClk {
///     const NAME: &'static str = "audio_clk";
/// }
/// ```
pub trait ClockDomain: Copy + Send + Sync + 'static {
    /// Human-readable name of the domain.
    const NAME: &'static str;
}

/// The "red" clock domain.
#[derive(Copy, Clone, Debug, Default)]
pub struct Red;

impl ClockDomain for Red {
    const NAME: &'static str = "red";
}

/// The "blue" clock domain.
#[derive(Copy, Clone, Debug, Default)]
pub struct Blue;

impl ClockDomain for Blue {
    const NAME: &'static str = "blue";
}

#[cfg(test)]
mod tests {
    use super::{Blue, ClockDomain, Red};

    #[test]
    fn red_name() {
        assert_eq!(Red::NAME, "red");
    }

    #[test]
    fn blue_name() {
        assert_eq!(Blue::NAME, "blue");
    }

    #[test]
    fn domain_types_are_zero_sized() {
        assert_eq!(core::mem::size_of::<Red>(), 0);
        assert_eq!(core::mem::size_of::<Blue>(), 0);
    }
}
