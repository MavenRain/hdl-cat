//! Shared error type for the `hdl-cat` workspace.
//!
//! This crate defines a single [`Error`] enum used throughout every other
//! `hdl-cat-*` crate.  Each variant wraps an underlying concrete error
//! from `std`, from `comp-cat-rs`, or from a domain context in hdl-cat.
//!
//! # Design
//!
//! Error handling is explicit and hand-rolled — no `thiserror`, no
//! `anyhow`, no silent panics.  Every fallible operation in the
//! workspace returns `Result<T, Error>`.
//!
//! `From` impls are provided for every underlying error type so that
//! `?` propagates cleanly at every call site.

use comp_cat_rs::collapse::free_category::FreeCategoryError;

/// A bit width, measured in bits.
///
/// Used in [`Error::WidthMismatch`] to communicate expected/actual
/// operand widths when a hardware operation's width constraint is
/// violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Width(u32);

impl Width {
    /// Construct a `Width` from a raw bit count.
    #[must_use]
    pub fn new(bits: u32) -> Self {
        Self(bits)
    }

    /// The underlying bit count.
    #[must_use]
    pub fn bits(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for Width {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} bit(s)", self.0)
    }
}

/// A cycle index in a simulation.
///
/// Used by [`Error::ImmatureSim`] and by `hdl-cat-sim`'s
/// `TimedSample` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cycle(u64);

impl Cycle {
    /// Construct a `Cycle` from a raw index.
    #[must_use]
    pub fn new(index: u64) -> Self {
        Self(index)
    }

    /// The underlying cycle index.
    #[must_use]
    pub fn index(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for Cycle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "cycle {}", self.0)
    }
}

/// A human-readable type name used in mismatch diagnostics.
///
/// Opaque newtype: construct via [`TypeName::new`], display via
/// [`core::fmt::Display`].  Exists so that `Error::TypeMismatch`
/// cannot be confused with raw `String` domain primitives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeName(String);

impl TypeName {
    /// Construct a `TypeName` from any displayable value.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for TypeName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A signal name used in trace / codegen diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignalName(String);

impl SignalName {
    /// Construct a `SignalName` from any displayable value.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for SignalName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The workspace-wide error enum.
///
/// Every fallible operation in `hdl-cat-*` returns `Result<T, Error>`.
/// Variants either wrap an underlying error type or encode a
/// domain-specific failure.
///
/// # Examples
///
/// ```
/// use hdl_cat_error::{Error, Width};
///
/// let e = Error::WidthMismatch {
///     expected: Width::new(8),
///     actual: Width::new(4),
/// };
/// assert_eq!(
///     e.to_string(),
///     "width mismatch: expected 8 bit(s), got 4 bit(s)",
/// );
/// ```
#[derive(Debug)]
pub enum Error {
    /// Underlying I/O failure.
    Io(std::io::Error),

    /// Underlying `core::fmt` failure (e.g. from a writer).
    Fmt(core::fmt::Error),

    /// Underlying integer parse failure.
    ParseInt(core::num::ParseIntError),

    /// A free-category IR construction error from `comp-cat-rs`.
    FreeCategory(FreeCategoryError),

    /// A value did not fit its declared hardware width.
    WidthMismatch {
        /// The width declared by the target type.
        expected: Width,
        /// The width actually supplied.
        actual: Width,
    },

    /// A hardware value's runtime type does not match the expected type.
    TypeMismatch {
        /// The expected type's display name.
        expected: TypeName,
        /// The actual type's display name.
        actual: TypeName,
    },

    /// Two signals from different clock domains were composed.
    ClockDomainMismatch,

    /// A signal referenced in an IR or codegen pass was never defined.
    UndefinedSignal {
        /// The name that could not be resolved.
        name: SignalName,
    },

    /// A simulation attempted to read a register before the first
    /// clock edge had advanced state beyond its initial value.
    ImmatureSim {
        /// The cycle at which the read was attempted.
        cycle: Cycle,
    },

    /// A value overflowed its declared range during arithmetic.
    Overflow {
        /// The width of the operation that overflowed.
        width: Width,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Fmt(e) => write!(f, "formatter error: {e}"),
            Self::ParseInt(e) => write!(f, "parse-int error: {e}"),
            Self::FreeCategory(e) => write!(f, "free-category error: {e}"),
            Self::WidthMismatch { expected, actual } => {
                write!(f, "width mismatch: expected {expected}, got {actual}")
            }
            Self::TypeMismatch { expected, actual } => {
                write!(f, "type mismatch: expected {expected}, got {actual}")
            }
            Self::ClockDomainMismatch => f.write_str("clock domain mismatch"),
            Self::UndefinedSignal { name } => write!(f, "undefined signal: {name}"),
            Self::ImmatureSim { cycle } => {
                write!(f, "simulation read before first clock edge at {cycle}")
            }
            Self::Overflow { width } => write!(f, "arithmetic overflow at {width}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Fmt(e) => Some(e),
            Self::ParseInt(e) => Some(e),
            Self::FreeCategory(e) => Some(e),
            Self::WidthMismatch { .. }
            | Self::TypeMismatch { .. }
            | Self::ClockDomainMismatch
            | Self::UndefinedSignal { .. }
            | Self::ImmatureSim { .. }
            | Self::Overflow { .. } => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<core::fmt::Error> for Error {
    fn from(e: core::fmt::Error) -> Self {
        Self::Fmt(e)
    }
}

impl From<core::num::ParseIntError> for Error {
    fn from(e: core::num::ParseIntError) -> Self {
        Self::ParseInt(e)
    }
}

impl From<FreeCategoryError> for Error {
    fn from(e: FreeCategoryError) -> Self {
        Self::FreeCategory(e)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cycle, Error, SignalName, TypeName, Width};

    #[test]
    fn width_mismatch_displays_both_widths() {
        let e = Error::WidthMismatch {
            expected: Width::new(8),
            actual: Width::new(4),
        };
        assert_eq!(
            e.to_string(),
            "width mismatch: expected 8 bit(s), got 4 bit(s)",
        );
    }

    #[test]
    fn type_mismatch_displays_both_names() {
        let e = Error::TypeMismatch {
            expected: TypeName::new("Bits<8>"),
            actual: TypeName::new("Bits<4>"),
        };
        assert_eq!(e.to_string(), "type mismatch: expected Bits<8>, got Bits<4>");
    }

    #[test]
    fn clock_domain_mismatch_displays_message() {
        assert_eq!(Error::ClockDomainMismatch.to_string(), "clock domain mismatch");
    }

    #[test]
    fn undefined_signal_displays_name() {
        let e = Error::UndefinedSignal {
            name: SignalName::new("clk"),
        };
        assert_eq!(e.to_string(), "undefined signal: clk");
    }

    #[test]
    fn immature_sim_displays_cycle() {
        let e = Error::ImmatureSim { cycle: Cycle::new(0) };
        assert_eq!(
            e.to_string(),
            "simulation read before first clock edge at cycle 0",
        );
    }

    #[test]
    fn overflow_displays_width() {
        let e = Error::Overflow { width: Width::new(16) };
        assert_eq!(e.to_string(), "arithmetic overflow at 16 bit(s)");
    }

    #[test]
    fn from_io_error_wraps_without_loss() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        let e: Error = io.into();
        let msg = e.to_string();
        assert!(msg.starts_with("I/O error: "));
        assert!(msg.contains("nope"));
    }

    #[test]
    fn question_mark_propagates_parse_int_into_error() -> Result<(), Error> {
        let n: i32 = "42".parse()?;
        assert_eq!(n, 42);
        Ok(())
    }

    #[test]
    fn width_round_trips_through_accessor() {
        assert_eq!(Width::new(12).bits(), 12);
    }

    #[test]
    fn cycle_round_trips_through_accessor() {
        assert_eq!(Cycle::new(7).index(), 7);
    }

    #[test]
    fn type_name_round_trips_through_accessor() {
        assert_eq!(TypeName::new("Bool").as_str(), "Bool");
    }

    #[test]
    fn signal_name_round_trips_through_accessor() {
        assert_eq!(SignalName::new("rst").as_str(), "rst");
    }
}
