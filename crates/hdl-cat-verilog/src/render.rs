//! Pretty-printing for the Verilog AST.

use crate::ast::{Expr, Module, Port, PortDirection, Stmt};

/// Render a [`Module`] to Verilog text.
///
/// The output targets SystemVerilog-lite: `module`, `wire`,
/// `assign`, and `always_ff` constructs.  Keeps formatting
/// conservative for tool compatibility.
#[must_use]
pub fn render_module(m: &Module) -> String {
    let header = render_header(m);
    let body = m
        .body()
        .iter()
        .map(render_stmt)
        .fold(String::new(), |acc, line| acc + "    " + &line + "\n");
    let footer = "endmodule\n";
    format!("{header}{body}{footer}")
}

fn render_header(m: &Module) -> String {
    let port_list = m
        .ports()
        .iter()
        .map(render_port)
        .collect::<Vec<_>>()
        .join(",\n    ");
    let port_list_rendered = if m.ports().is_empty() {
        String::new()
    } else {
        format!("    {port_list}\n")
    };
    format!("module {} (\n{port_list_rendered});\n", m.name())
}

fn render_port(p: &Port) -> String {
    let dir = match p.direction() {
        PortDirection::Input => "input",
        PortDirection::Output => "output",
        PortDirection::OutputReg => "output reg",
    };
    render_decl_head(dir, p.width(), p.name())
}

fn render_decl_head(prefix: &str, width: u32, name: &str) -> String {
    if width <= 1 {
        format!("{prefix} {name}")
    } else {
        format!("{prefix} [{}:0] {name}", width - 1)
    }
}

fn render_stmt(s: &Stmt) -> String {
    match s {
        Stmt::WireDecl { name, width } => {
            format!("{};", render_decl_head("wire", *width, name))
        }
        Stmt::RegDecl { name, width } => {
            format!("{};", render_decl_head("reg", *width, name))
        }
        Stmt::Assign { lhs, rhs } => {
            format!("assign {lhs} = {};", render_expr(rhs))
        }
        Stmt::AlwaysFf { clock, reset, reg, reset_value, next } => render_always_ff(
            clock, reset.as_deref(), reg, reset_value, next,
        ),
    }
}

fn render_always_ff(
    clock: &str,
    reset: Option<&str>,
    reg: &str,
    reset_value: &Expr,
    next: &Expr,
) -> String {
    match reset {
        None => format!(
            "always_ff @(posedge {clock}) {reg} <= {};",
            render_expr(next)
        ),
        Some(rst) => format!(
            "always_ff @(posedge {clock}) if ({rst}) {reg} <= {}; else {reg} <= {};",
            render_expr(reset_value),
            render_expr(next)
        ),
    }
}

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Wire(n) => n.clone(),
        Expr::Literal { width, value } => format!("{width}'d{value}"),
        Expr::Not(inner) => format!("~{}", render_expr(inner)),
        Expr::Binary { op, lhs, rhs } => {
            format!("({} {op} {})", render_expr(lhs), render_expr(rhs))
        }
        Expr::Mux { selector, false_arm, true_arm } => format!(
            "({} ? {} : {})",
            render_expr(selector),
            render_expr(true_arm),
            render_expr(false_arm)
        ),
        Expr::Concat { high, low } => {
            format!("{{{}, {}}}", render_expr(high), render_expr(low))
        }
        Expr::Slice { source, lo, hi } => {
            let h = hi.saturating_sub(1);
            format!("{}[{}:{}]", render_expr(source), h, lo)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render_expr, render_module};
    use crate::ast::{Expr, Module, Port, PortDirection, Stmt};

    #[test]
    fn renders_empty_module() {
        let m = Module::new("m", Vec::new(), Vec::new());
        let text = render_module(&m);
        assert!(text.starts_with("module m"));
        assert!(text.contains("endmodule"));
    }

    #[test]
    fn renders_wire_expr() {
        assert_eq!(render_expr(&Expr::Wire("a".into())), "a");
    }

    #[test]
    fn renders_binary_expr() {
        let e = Expr::Binary {
            op: "+",
            lhs: Box::new(Expr::Wire("a".into())),
            rhs: Box::new(Expr::Wire("b".into())),
        };
        assert_eq!(render_expr(&e), "(a + b)");
    }

    #[test]
    fn renders_literal() {
        let e = Expr::Literal { width: 8, value: 42 };
        assert_eq!(render_expr(&e), "8'd42");
    }

    #[test]
    fn renders_slice_with_correct_range() {
        let e = Expr::Slice {
            source: Box::new(Expr::Wire("x".into())),
            lo: 0,
            hi: 4,
        };
        assert_eq!(render_expr(&e), "x[3:0]");
    }

    #[test]
    fn renders_module_with_input_and_assign() {
        let m = Module::new(
            "inv",
            vec![
                Port::new("a", PortDirection::Input, 8),
                Port::new("y", PortDirection::Output, 8),
            ],
            vec![Stmt::Assign {
                lhs: "y".into(),
                rhs: Expr::Not(Box::new(Expr::Wire("a".into()))),
            }],
        );
        let text = render_module(&m);
        assert!(text.contains("module inv"));
        assert!(text.contains("input [7:0] a"));
        assert!(text.contains("output [7:0] y"));
        assert!(text.contains("assign y = ~a"));
    }

    #[test]
    fn renders_single_bit_port_without_range() {
        let m = Module::new(
            "t",
            vec![Port::new("clk", PortDirection::Input, 1)],
            Vec::new(),
        );
        let text = render_module(&m);
        assert!(text.contains("input clk"));
    }
}
