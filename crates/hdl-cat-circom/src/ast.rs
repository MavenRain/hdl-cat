//! The Circom AST emitted by this crate.
//!
//! A minimal four-type AST: [`Field`] selects the target scalar
//! field, [`Signal`] declares a bit-array port or intermediate,
//! [`Expr`] is a quadratic expression over bits and subcomponent
//! outputs, and [`Stmt`] is one line of a template body.  [`Template`]
//! wraps all of the above plus the `include` set and the `main`
//! instantiation so that a `Template` renders to a self-contained
//! Circom file.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::Error;

/// The prime field targeted by a generated template.
///
/// Affects diagnostics and the maximum safe bit width of arithmetic
/// intermediates; the emitted `pragma` line is fixed at
/// `circom 2.0.0` and the caller's Circom toolchain decides the
/// actual curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// BN254 scalar field.  Circom's default.
    Bn254,
    /// BLS12-381 scalar field.
    Bls12_381,
    /// Goldilocks scalar field (requires a non-default toolchain).
    Goldilocks,
}

impl Field {
    /// The maximum safe bit width for a single field signal.
    ///
    /// Used by the emitter to reject arithmetic intermediates whose
    /// worst-case witness would exceed the field's bit budget.
    #[must_use]
    pub fn max_bits(self) -> u32 {
        match self {
            Self::Bn254 | Self::Bls12_381 => 252,
            Self::Goldilocks => 63,
        }
    }
}

impl core::fmt::Display for Field {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bn254 => f.write_str("bn254"),
            Self::Bls12_381 => f.write_str("bls12_381"),
            Self::Goldilocks => f.write_str("goldilocks"),
        }
    }
}

/// The direction of a template signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDir {
    /// `signal input` — driven by the caller.
    Input,
    /// `signal output` — driven inside the template.
    Output,
    /// `signal` — a local intermediate not exposed on the interface.
    Intermediate,
}

/// A bit-array signal of width `N`.
///
/// In the bit-level encoding, every hdl-cat wire of width `N`
/// renders as `signal ... name[N]`, with per-bit boolean constraints
/// emitted on entry for input ports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    name: String,
    dir: SignalDir,
    width: u32,
}

impl Signal {
    /// Construct a new signal.
    #[must_use]
    pub fn new(name: impl Into<String>, dir: SignalDir, width: u32) -> Self {
        Self { name: name.into(), dir, width }
    }

    /// The signal's identifier.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The signal's declaration direction.
    #[must_use]
    pub fn dir(&self) -> SignalDir {
        self.dir
    }

    /// The signal's bit width.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }
}

/// A Circom expression.
///
/// Must remain quadratic per emitted constraint; the invariant is
/// enforced by the emitter's op lowerings, not by this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// `sig[i]` — a single bit of a named signal.
    Bit {
        /// Signal name.
        sig: String,
        /// Bit index (LSB-first, matching [`hdl_cat_kind::BitSeq`]).
        index: u32,
    },
    /// `0` or `1`.
    BitLiteral(bool),
    /// A field-element literal.
    FieldLiteral(u128),
    /// `(bits[0] + 2*bits[1] + 4*bits[2] + ...)`, flattened at render
    /// time into a linear combination.
    Bits2Num(Vec<Expr>),
    /// `-e`.
    Neg(Box<Expr>),
    /// `(a + b)`.
    Add(Box<Expr>, Box<Expr>),
    /// `(a - b)`.
    Sub(Box<Expr>, Box<Expr>),
    /// `(a * b)` — at most one such factor per rendered constraint
    /// once the emitter's own invariants are honored.
    Mul(Box<Expr>, Box<Expr>),
    /// `comp.port[i]` — one bit of a subcomponent output port.
    CompOutBit {
        /// Subcomponent name.
        comp: String,
        /// Output port name.
        port: String,
        /// Bit index.
        index: u32,
    },
    /// `comp.port` — a field-valued output of a subcomponent.
    CompOutField {
        /// Subcomponent name.
        comp: String,
        /// Output port name.
        port: String,
    },
}

/// A statement in a Circom template body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// `signal [input|output]? name[width];`.
    SignalDecl(Signal),
    /// `component name = Template(args);`.
    Component {
        /// Local component name.
        name: String,
        /// Name of the template being instantiated.
        template: String,
        /// Numeric template parameters.
        args: Vec<u32>,
    },
    /// Drive one of a subcomponent's input ports.
    ///
    /// Rendered as `comp.port <== rhs;` for a scalar port or
    /// `comp.port[i] <== rhs;` for an array port.
    ComponentDrive {
        /// Subcomponent name.
        comp: String,
        /// Input port on the subcomponent.
        port: String,
        /// Array index into the port, or `None` for a scalar port.
        index: Option<u32>,
        /// Right-hand expression.
        rhs: Expr,
    },
    /// `lhs[?index] <== rhs;` — assign-and-constrain.
    Assign {
        /// Target signal name.
        lhs: String,
        /// Bit index, or `None` for a scalar signal.
        index: Option<u32>,
        /// Right-hand expression.
        rhs: Expr,
    },
    /// `expr === 0;` — pure constraint, used for per-bit boolean
    /// constraints on input ports.
    Constraint(Expr),
}

/// A complete Circom template together with the `include` set and
/// the `main` instantiation needed to render it as a stand-alone
/// file.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Template {
    field: Field,
    name: String,
    includes: Vec<String>,
    ports: Vec<Signal>,
    intermediates: Vec<Signal>,
    body: Vec<Stmt>,
    public_inputs: Vec<String>,
}

impl Template {
    /// Construct a new template.
    ///
    /// The `public_inputs` list names input signals that will be
    /// declared as public on the `component main` instantiation.
    /// An empty list emits a plain `component main = name();` line;
    /// a non-empty list emits
    /// `component main { public [a, b] } = name();`.
    pub fn new(
        field: Field,
        name: impl Into<String>,
        includes: Vec<String>,
        ports: Vec<Signal>,
        intermediates: Vec<Signal>,
        body: Vec<Stmt>,
        public_inputs: Vec<String>,
    ) -> Self {
        Self {
            field,
            name: name.into(),
            includes,
            ports,
            intermediates,
            body,
            public_inputs,
        }
    }

    /// The scalar field targeted by this template.
    #[must_use]
    pub fn field(&self) -> Field {
        self.field
    }

    /// The template's declared name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The circomlib or user paths this template includes.
    #[must_use]
    pub fn includes(&self) -> &[String] {
        &self.includes
    }

    /// The template's port signals (inputs followed by outputs).
    #[must_use]
    pub fn ports(&self) -> &[Signal] {
        &self.ports
    }

    /// The template's intermediate signals.
    #[must_use]
    pub fn intermediates(&self) -> &[Signal] {
        &self.intermediates
    }

    /// The template's body statements.
    #[must_use]
    pub fn body(&self) -> &[Stmt] {
        &self.body
    }

    /// The signal names declared as public on `component main`.
    ///
    /// An empty slice means the rendered template emits a plain
    /// `component main = name();` line with no `public [...]` clause.
    #[must_use]
    pub fn public_inputs(&self) -> &[String] {
        &self.public_inputs
    }

    /// Render this template to a self-contained Circom file
    /// (`pragma`, `include`s, template body, `component main`).
    #[must_use]
    pub fn render(&self) -> Io<Error, String> {
        let owned = self.clone();
        Io::suspend(move || Ok(crate::render::render_template(&owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::{Expr, Field, Signal, SignalDir, Stmt, Template};

    #[test]
    fn field_max_bits_known_curves() {
        assert_eq!(Field::Bn254.max_bits(), 252);
        assert_eq!(Field::Bls12_381.max_bits(), 252);
        assert_eq!(Field::Goldilocks.max_bits(), 63);
    }

    #[test]
    fn field_displays_canonical_name() {
        assert_eq!(Field::Bn254.to_string(), "bn254");
        assert_eq!(Field::Bls12_381.to_string(), "bls12_381");
        assert_eq!(Field::Goldilocks.to_string(), "goldilocks");
    }

    #[test]
    fn signal_accessors_round_trip() {
        let s = Signal::new("w0", SignalDir::Input, 8);
        assert_eq!(s.name(), "w0");
        assert_eq!(s.dir(), SignalDir::Input);
        assert_eq!(s.width(), 8);
    }

    #[test]
    fn expr_is_clone_and_debug() {
        let e = Expr::Bit {
            sig: "w0".into(),
            index: 3,
        };
        let _ = e.clone();
        let _ = format!("{e:?}");
    }

    #[test]
    fn stmt_is_clone_and_debug() {
        let s = Stmt::Constraint(Expr::BitLiteral(false));
        let _ = s.clone();
        let _ = format!("{s:?}");
    }

    #[test]
    fn template_holds_its_parts() {
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            vec![Signal::new("w0", SignalDir::Input, 4)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(t.name(), "t");
        assert_eq!(t.ports().len(), 1);
        assert_eq!(t.intermediates().len(), 0);
        assert_eq!(t.body().len(), 0);
        assert_eq!(t.field(), Field::Bn254);
        assert!(t.public_inputs().is_empty());
    }

    #[test]
    fn template_round_trips_public_inputs() {
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            vec![
                Signal::new("w0", SignalDir::Input, 4),
                Signal::new("w1", SignalDir::Input, 4),
            ],
            Vec::new(),
            Vec::new(),
            vec!["w0".to_string()],
        );
        assert_eq!(t.public_inputs(), &["w0".to_string()]);
    }
}
