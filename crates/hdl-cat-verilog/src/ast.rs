//! The Verilog AST emitted by this crate.

use comp_cat_rs::effect::io::Io;
use hdl_cat_error::Error;

/// The direction of a module port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortDirection {
    /// `input` port.
    Input,
    /// `output` port — a `wire` type, driven by a continuous
    /// `assign` statement.
    Output,
    /// `output reg` port — a `reg` type, assigned inside an
    /// `always_ff` block.  Used for state wires that are also
    /// exposed on the module interface.
    OutputReg,
}

/// A named, directed, sized port declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    name: String,
    direction: PortDirection,
    width: u32,
}

impl Port {
    /// Construct a new port.
    #[must_use]
    pub fn new(name: impl Into<String>, direction: PortDirection, width: u32) -> Self {
        Self { name: name.into(), direction, width }
    }

    /// The port's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The port's direction.
    #[must_use]
    pub fn direction(&self) -> PortDirection {
        self.direction
    }

    /// The port's bit width.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }
}

/// A continuous-assignment expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A wire reference.
    Wire(String),
    /// A literal integer in decimal form, with width.
    Literal {
        /// The literal's bit width.
        width: u32,
        /// The literal value as an unsigned integer.
        value: u128,
    },
    /// `~x`: bitwise NOT.
    Not(Box<Expr>),
    /// Infix binary operator (`&`, `|`, `^`, `+`, `-`, `*`, `==`, `<`).
    Binary {
        /// Verilog operator token.
        op: &'static str,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// `sel ? hi : lo`: ternary.
    Mux {
        /// Selector expression.
        selector: Box<Expr>,
        /// False-arm (when selector is zero).
        false_arm: Box<Expr>,
        /// True-arm (when selector is non-zero).
        true_arm: Box<Expr>,
    },
    /// `{high, low}`: concatenation.
    Concat {
        /// High-bits operand (appears first in Verilog concat).
        high: Box<Expr>,
        /// Low-bits operand.
        low: Box<Expr>,
    },
    /// `x[hi-1:lo]`: slice.
    Slice {
        /// Source expression.
        source: Box<Expr>,
        /// Low bit (inclusive).
        lo: u32,
        /// High bit (exclusive).
        hi: u32,
    },
    /// `name[index]`: array element access (for register arrays).
    ArrayIndex {
        /// Array name.
        array: String,
        /// Element index (constant).
        index: usize,
    },
    /// `name[expr]`: array element access with a dynamic index.
    ///
    /// Used by circular-buffer delay lines where the read address
    /// is a register (the write pointer).
    ArrayDynIndex {
        /// Array name.
        array: String,
        /// Index expression (typically a wire or register reference).
        index: Box<Expr>,
    },
}

/// A statement in a Verilog module body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// A wire declaration: `wire [w-1:0] name;`.
    WireDecl {
        /// Wire name.
        name: String,
        /// Bit width.
        width: u32,
    },
    /// A reg declaration: `reg [w-1:0] name;`.
    RegDecl {
        /// Reg name.
        name: String,
        /// Bit width.
        width: u32,
    },
    /// A continuous assignment: `assign lhs = rhs;`.
    Assign {
        /// Left-hand wire name.
        lhs: String,
        /// Right-hand expression.
        rhs: Expr,
    },
    /// An `always_ff @(posedge clk)` block driving a single register
    /// from an expression.
    AlwaysFf {
        /// Clock name.
        clock: String,
        /// Reset name (or `None` for free-running).
        reset: Option<String>,
        /// Target register.
        reg: String,
        /// Reset value (used when `reset` is present).
        reset_value: Expr,
        /// Next-state expression.
        next: Expr,
    },
    /// A register array declaration: `reg [W-1:0] name [0:D-1];`.
    ///
    /// Used for BRAM-inferable delay lines and shift registers.
    RegArrayDecl {
        /// Array name.
        name: String,
        /// Element bit width.
        width: u32,
        /// Number of elements (depth).
        depth: usize,
    },
    /// An `always_ff @(posedge clk)` block implementing an array
    /// shift register with synchronous reset.
    ///
    /// On reset, every element is set to `reset_value`.  Otherwise,
    /// `arr[0] <= input; arr[i] <= arr[i-1]` for `i` in `1..depth`.
    AlwaysArrayShift {
        /// Clock name.
        clock: String,
        /// Reset name.
        reset: String,
        /// Array name (must match a [`RegArrayDecl`](Stmt::RegArrayDecl)).
        array: String,
        /// Number of elements in the array.
        depth: usize,
        /// Element bit width (for reset literal).
        width: u32,
        /// Reset value for every element.
        reset_value: Expr,
        /// Expression driving `arr[0]` each cycle.
        input: Expr,
    },
    /// An `always_ff @(posedge clk)` block implementing a circular-buffer
    /// delay line with synchronous reset.
    ///
    /// On reset, every element is set to `reset_value` and the write
    /// pointer is zeroed.  Otherwise, each cycle reads the oldest
    /// element at `arr[wr_ptr]`, writes `input` to `arr[wr_ptr]`,
    /// and advances `wr_ptr` modulo `depth`.
    ///
    /// Unlike [`AlwaysArrayShift`](Stmt::AlwaysArrayShift), this
    /// emits O(1) assignment lines regardless of depth, making it
    /// suitable for large BRAM-backed delay lines (2^20+ elements).
    AlwaysArrayCircBuf {
        /// Clock name.
        clock: String,
        /// Reset name.
        reset: String,
        /// Array name (must match a [`RegArrayDecl`](Stmt::RegArrayDecl)).
        array: String,
        /// Number of elements in the array.
        depth: usize,
        /// Element bit width (for reset literal).
        width: u32,
        /// Name of the write-pointer register.
        ptr_name: String,
        /// Bit width of the write pointer (`ceil(log2(depth))`).
        ptr_width: u32,
        /// Reset value for every element.
        reset_value: Expr,
        /// Expression driving the write each cycle.
        input: Expr,
    },
}

/// A complete Verilog module.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Module {
    name: String,
    ports: Vec<Port>,
    body: Vec<Stmt>,
}

impl Module {
    /// Construct a new module.
    pub fn new(name: impl Into<String>, ports: Vec<Port>, body: Vec<Stmt>) -> Self {
        Self { name: name.into(), ports, body }
    }

    /// The module's declared name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The module's port list.
    #[must_use]
    pub fn ports(&self) -> &[Port] {
        &self.ports
    }

    /// The module's body statements.
    #[must_use]
    pub fn body(&self) -> &[Stmt] {
        &self.body
    }

    /// Render this module to Verilog text.
    #[must_use]
    pub fn render(&self) -> Io<Error, String> {
        let owned = self.clone();
        Io::suspend(move || Ok(crate::render::render_module(&owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::{Expr, Module, Port, PortDirection, Stmt};

    #[test]
    fn port_accessors() {
        let p = Port::new("clk", PortDirection::Input, 1);
        assert_eq!(p.name(), "clk");
        assert_eq!(p.direction(), PortDirection::Input);
        assert_eq!(p.width(), 1);
    }

    #[test]
    fn expr_is_clone_and_debug() {
        let e = Expr::Wire("a".into());
        let _ = e.clone();
        let _ = format!("{e:?}");
    }

    #[test]
    fn module_holds_parts() {
        let m = Module::new(
            "m",
            vec![Port::new("a", PortDirection::Input, 4)],
            vec![Stmt::WireDecl {
                name: "w".into(),
                width: 4,
            }],
        );
        assert_eq!(m.name(), "m");
        assert_eq!(m.ports().len(), 1);
        assert_eq!(m.body().len(), 1);
    }
}
