//! Pretty-printing for the Circom AST.

use crate::ast::{Expr, Signal, SignalDir, Stmt, Template};

/// Render a [`Template`] to a self-contained Circom file.
///
/// The output contains the `pragma` line, each `include`, the
/// template body, and a `component main = name();` instantiation.
#[must_use]
pub fn render_template(t: &Template) -> String {
    let pragma = "pragma circom 2.0.0;\n".to_string();
    let include_lines =
        t.includes().iter().fold(String::new(), |acc, inc| {
            acc + &format!("include \"{inc}\";\n")
        });
    let separator = if t.includes().is_empty() {
        String::new()
    } else {
        "\n".to_string()
    };
    let header = format!("template {}() {{\n", t.name());
    let port_lines = t.ports().iter().fold(String::new(), |acc, s| {
        acc + "    " + &render_signal_decl(s) + "\n"
    });
    let mid_sep = if t.intermediates().is_empty() {
        String::new()
    } else {
        "\n".to_string()
    };
    let intermediate_lines =
        t.intermediates().iter().fold(String::new(), |acc, s| {
            acc + "    " + &render_signal_decl(s) + "\n"
        });
    let body_sep = if t.body().is_empty() {
        String::new()
    } else {
        "\n".to_string()
    };
    let body_lines = t.body().iter().fold(String::new(), |acc, s| {
        acc + "    " + &render_stmt(s) + "\n"
    });
    let footer = "}\n\n".to_string();
    let main_line = format!("component main = {}();\n", t.name());
    format!(
        "{pragma}{include_lines}{separator}{header}{port_lines}{mid_sep}{intermediate_lines}{body_sep}{body_lines}{footer}{main_line}"
    )
}

fn render_signal_decl(s: &Signal) -> String {
    let prefix = match s.dir() {
        SignalDir::Input => "signal input",
        SignalDir::Output => "signal output",
        SignalDir::Intermediate => "signal",
    };
    format!("{prefix} {}[{}];", s.name(), s.width())
}

fn render_stmt(s: &Stmt) -> String {
    match s {
        Stmt::SignalDecl(sig) => render_signal_decl(sig),
        Stmt::Component { name, template, args } => {
            let args_rendered = args
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("component {name} = {template}({args_rendered});")
        }
        Stmt::ComponentDrive { comp, port, index, rhs } => {
            index.map_or_else(
                || format!("{comp}.{port} <== {};", render_expr(rhs)),
                |i| format!("{comp}.{port}[{i}] <== {};", render_expr(rhs)),
            )
        }
        Stmt::Assign { lhs, index, rhs } => index.map_or_else(
            || format!("{lhs} <== {};", render_expr(rhs)),
            |i| format!("{lhs}[{i}] <== {};", render_expr(rhs)),
        ),
        Stmt::Constraint(e) => format!("{} === 0;", render_expr(e)),
    }
}

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Bit { sig, index } => format!("{sig}[{index}]"),
        Expr::BitLiteral(b) => if *b { "1" } else { "0" }.to_string(),
        Expr::FieldLiteral(v) => v.to_string(),
        Expr::Bits2Num(bits) => render_bits_to_num(bits),
        Expr::Neg(inner) => format!("-{}", render_expr(inner)),
        Expr::Add(lhs, rhs) => {
            format!("({} + {})", render_expr(lhs), render_expr(rhs))
        }
        Expr::Sub(lhs, rhs) => {
            format!("({} - {})", render_expr(lhs), render_expr(rhs))
        }
        Expr::Mul(lhs, rhs) => {
            format!("({} * {})", render_expr(lhs), render_expr(rhs))
        }
        Expr::CompOutBit { comp, port, index } => {
            format!("{comp}.{port}[{index}]")
        }
        Expr::CompOutField { comp, port } => format!("{comp}.{port}"),
    }
}

fn render_bits_to_num(bits: &[Expr]) -> String {
    let terms: Vec<String> = bits
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let rendered = render_expr(b);
            match i {
                0 => rendered,
                1 => format!("2*{rendered}"),
                n => {
                    let shift = u32::try_from(n).unwrap_or(u32::MAX);
                    let coef = 1u128.checked_shl(shift).unwrap_or(0);
                    format!("{coef}*{rendered}")
                }
            }
        })
        .collect();
    format!("({})", terms.join(" + "))
}

#[cfg(test)]
mod tests {
    use super::{render_expr, render_template};
    use crate::ast::{Expr, Field, Signal, SignalDir, Stmt, Template};

    #[test]
    fn renders_pragma_and_main_for_empty_template() {
        let t = Template::new(
            Field::Bn254,
            "empty",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let text = render_template(&t);
        assert!(text.starts_with("pragma circom 2.0.0;\n"));
        assert!(text.contains("template empty()"));
        assert!(text.contains("component main = empty();"));
    }

    #[test]
    fn renders_input_port_with_width() {
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            vec![Signal::new("w0", SignalDir::Input, 4)],
            Vec::new(),
            Vec::new(),
        );
        let text = render_template(&t);
        assert!(text.contains("signal input w0[4];"));
    }

    #[test]
    fn renders_output_port_with_width() {
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            vec![Signal::new("w1", SignalDir::Output, 1)],
            Vec::new(),
            Vec::new(),
        );
        let text = render_template(&t);
        assert!(text.contains("signal output w1[1];"));
    }

    #[test]
    fn renders_intermediate_declaration() {
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            Vec::new(),
            vec![Signal::new("tmp", SignalDir::Intermediate, 8)],
            Vec::new(),
        );
        let text = render_template(&t);
        assert!(text.contains("signal tmp[8];"));
    }

    #[test]
    fn renders_include_lines() {
        let t = Template::new(
            Field::Bn254,
            "t",
            vec!["circomlib/circuits/bitify.circom".to_string()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let text = render_template(&t);
        assert!(text.contains("include \"circomlib/circuits/bitify.circom\";"));
    }

    #[test]
    fn renders_bit_expression() {
        let e = Expr::Bit {
            sig: "w0".into(),
            index: 3,
        };
        assert_eq!(render_expr(&e), "w0[3]");
    }

    #[test]
    fn renders_bit_literal_as_zero_or_one() {
        assert_eq!(render_expr(&Expr::BitLiteral(true)), "1");
        assert_eq!(render_expr(&Expr::BitLiteral(false)), "0");
    }

    #[test]
    fn renders_sub_with_parens() {
        let e = Expr::Sub(
            Box::new(Expr::BitLiteral(true)),
            Box::new(Expr::Bit {
                sig: "w0".into(),
                index: 0,
            }),
        );
        assert_eq!(render_expr(&e), "(1 - w0[0])");
    }

    #[test]
    fn renders_mul_with_parens() {
        let e = Expr::Mul(
            Box::new(Expr::Bit {
                sig: "a".into(),
                index: 0,
            }),
            Box::new(Expr::Bit {
                sig: "b".into(),
                index: 0,
            }),
        );
        assert_eq!(render_expr(&e), "(a[0] * b[0])");
    }

    #[test]
    fn renders_assign_with_index() {
        let s = Stmt::Assign {
            lhs: "w1".into(),
            index: Some(0),
            rhs: Expr::Sub(
                Box::new(Expr::BitLiteral(true)),
                Box::new(Expr::Bit {
                    sig: "w0".into(),
                    index: 0,
                }),
            ),
        };
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![s],
        );
        let text = render_template(&t);
        assert!(text.contains("w1[0] <== (1 - w0[0]);"));
    }

    #[test]
    fn renders_boolean_constraint() {
        let bit = Expr::Bit {
            sig: "w0".into(),
            index: 0,
        };
        let s = Stmt::Constraint(Expr::Mul(
            Box::new(bit.clone()),
            Box::new(Expr::Sub(
                Box::new(bit),
                Box::new(Expr::BitLiteral(true)),
            )),
        ));
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![s],
        );
        let text = render_template(&t);
        assert!(text.contains("(w0[0] * (w0[0] - 1)) === 0;"));
    }

    #[test]
    fn renders_component_declaration() {
        let s = Stmt::Component {
            name: "add_0".into(),
            template: "Num2Bits".into(),
            args: vec![9],
        };
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![s],
        );
        let text = render_template(&t);
        assert!(text.contains("component add_0 = Num2Bits(9);"));
    }

    #[test]
    fn renders_component_drive_with_index() {
        let s = Stmt::ComponentDrive {
            comp: "add_0".into(),
            port: "in".into(),
            index: Some(0),
            rhs: Expr::Bit {
                sig: "w0".into(),
                index: 0,
            },
        };
        let t = Template::new(
            Field::Bn254,
            "t",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![s],
        );
        let text = render_template(&t);
        assert!(text.contains("add_0.in[0] <== w0[0];"));
    }

    #[test]
    fn renders_bits_to_num_linear_combination() {
        let bits = vec![
            Expr::Bit {
                sig: "w0".into(),
                index: 0,
            },
            Expr::Bit {
                sig: "w0".into(),
                index: 1,
            },
            Expr::Bit {
                sig: "w0".into(),
                index: 2,
            },
        ];
        let e = Expr::Bits2Num(bits);
        assert_eq!(render_expr(&e), "(w0[0] + 2*w0[1] + 4*w0[2])");
    }
}
