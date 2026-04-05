//! `#[kernel]` attribute macro for `hdl-cat`.
//!
//! Lifts a Rust function with a restricted body into an IR
//! builder returning a combinational
//! `hdl_cat_circuit::CircuitArrow`.
//!
//! # Supported subset
//!
//! - Parameter types: `bool`, `Bits<N>`, `SignedBits<N>` (with
//!   a literal `N`).
//! - Return type: a single scalar (same constraints as parameters).
//! - Body: zero or more `let` bindings followed by a tail
//!   expression.
//! - Expressions: identifiers, binary operators (`+`, `-`, `*`,
//!   `&`, `|`, `^`), unary `!`.
//!
//! Function bodies outside this subset raise a compile error.
//!
//! # Example
//!
//! The macro cannot be doctested from within its own crate
//! because expanded code references `hdl_cat_ir`, `hdl_cat_bits`,
//! `hdl_cat_circuit`, and `hdl_cat_error`, which aren't in this
//! crate's dependency graph.  Real doctests live in the umbrella
//! `hdl-cat` crate's `tests/kernel_macro.rs`.
//!
//! Conceptually:
//!
//! ```ignore
//! use hdl_cat_macros::kernel;
//! use hdl_cat_bits::Bits;
//!
//! #[kernel]
//! fn xor_plus_a(a: Bits<8>, b: Bits<8>) -> Bits<8> {
//!     let x = a ^ b;
//!     x + a
//! }
//! ```
//!
//! After expansion, `xor_plus_a` becomes a nullary function
//! returning
//! `Result<CircuitArrow<CircuitTensor<Obj<Bits<8>>, Obj<Bits<8>>>, Obj<Bits<8>>>, Error>`
//! that builds the IR for the given expression.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, BinOp, Expr, FnArg, Ident, ItemFn, Lit, PatType, Stmt, Type, UnOp};

/// Lift a Rust function into an `hdl-cat` IR builder.
#[proc_macro_attribute]
pub fn kernel(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    expand_kernel(&input).map_or_else(|e| e.to_compile_error().into(), Into::into)
}

/// An abstract hardware type extracted from the Rust type syntax.
#[derive(Clone)]
enum ScalarTy {
    Bool,
    Bits(u32),
    Signed(u32),
}

impl ScalarTy {
    fn wire_ty_tokens(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Bool => quote! { ::hdl_cat_ir::WireTy::Bit },
            Self::Bits(n) => quote! { ::hdl_cat_ir::WireTy::Bits(#n) },
            Self::Signed(n) => quote! { ::hdl_cat_ir::WireTy::Signed(#n) },
        }
    }

    fn obj_ty_tokens(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Bool => quote! { ::hdl_cat_circuit::Obj<bool> },
            Self::Bits(n) => {
                let n_literal = *n as usize;
                quote! { ::hdl_cat_circuit::Obj<::hdl_cat_bits::Bits<#n_literal>> }
            }
            Self::Signed(n) => {
                let n_literal = *n as usize;
                quote! { ::hdl_cat_circuit::Obj<::hdl_cat_bits::SignedBits<#n_literal>> }
            }
        }
    }
}

fn parse_scalar_ty(ty: &Type) -> Result<ScalarTy, syn::Error> {
    let Type::Path(p) = ty else {
        return Err(syn::Error::new_spanned(ty, "unsupported type"));
    };
    let segment = p
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new_spanned(p, "empty path"))?;
    let name = segment.ident.to_string();
    match name.as_str() {
        "bool" => Ok(ScalarTy::Bool),
        "Bits" | "SignedBits" => {
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return Err(syn::Error::new_spanned(
                    segment,
                    "Bits/SignedBits requires a const generic width",
                ));
            };
            let arg = args.args.first().ok_or_else(|| {
                syn::Error::new_spanned(args, "expected single const generic arg")
            })?;
            let width = const_width_from_generic_arg(arg)?;
            if name == "Bits" {
                Ok(ScalarTy::Bits(width))
            } else {
                Ok(ScalarTy::Signed(width))
            }
        }
        other => Err(syn::Error::new_spanned(
            segment,
            format!("unsupported type `{other}`"),
        )),
    }
}

fn const_width_from_generic_arg(arg: &syn::GenericArgument) -> Result<u32, syn::Error> {
    let expr = match arg {
        syn::GenericArgument::Const(e) => Ok(e),
        syn::GenericArgument::Type(Type::Path(p)) => Err(syn::Error::new_spanned(
            p,
            "expected a literal width, not a type path",
        )),
        other => Err(syn::Error::new_spanned(other, "expected a const literal width")),
    }?;
    let Expr::Lit(lit) = expr else {
        return Err(syn::Error::new_spanned(expr, "expected a const literal"));
    };
    let Lit::Int(n) = &lit.lit else {
        return Err(syn::Error::new_spanned(&lit.lit, "expected an integer literal"));
    };
    n.base10_parse::<u32>()
}

/// Immutable state threaded through body compilation.
///
/// Every mutation returns a new `BodyCtx` — no `&mut`.
#[derive(Clone)]
struct BodyCtx {
    stmts: Vec<proc_macro2::TokenStream>,
    env: Vec<(String, Ident, ScalarTy)>,
    fresh_counter: usize,
}

impl BodyCtx {
    fn new() -> Self {
        Self {
            stmts: Vec::new(),
            env: Vec::new(),
            fresh_counter: 0,
        }
    }

    fn fresh_wire_ident(self) -> (Self, Ident) {
        let id = Ident::new(
            &format!("__k_tmp_{}", self.fresh_counter),
            Span::call_site(),
        );
        (
            Self {
                fresh_counter: self.fresh_counter + 1,
                ..self
            },
            id,
        )
    }

    fn bind(self, source_name: String, wire_ident: Ident, ty: ScalarTy) -> Self {
        let new_env = self
            .env
            .into_iter()
            .chain(core::iter::once((source_name, wire_ident, ty)))
            .collect();
        Self {
            env: new_env,
            ..self
        }
    }

    fn lookup(&self, name: &str) -> Option<(Ident, ScalarTy)> {
        self.env
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, id, ty)| (id.clone(), ty.clone()))
    }

    fn push_stmt(self, ts: proc_macro2::TokenStream) -> Self {
        let new_stmts = self
            .stmts
            .into_iter()
            .chain(core::iter::once(ts))
            .collect();
        Self {
            stmts: new_stmts,
            ..self
        }
    }
}

fn expand_kernel(func: &ItemFn) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = &func.sig.ident;
    let vis = &func.vis;

    // Extract args.
    let args: Vec<(String, ScalarTy, Ident)> = func
        .sig
        .inputs
        .iter()
        .map(parse_kernel_arg)
        .collect::<Result<Vec<_>, _>>()?;

    (!args.is_empty())
        .then_some(())
        .ok_or_else(|| syn::Error::new_spanned(&func.sig, "kernel needs at least one parameter"))?;

    // Return type.
    let out_ty = match &func.sig.output {
        syn::ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &func.sig,
                "kernel must return a scalar",
            ));
        }
        syn::ReturnType::Type(_, t) => parse_scalar_ty(t)?,
    };

    // Build input-side Obj<...> and CircuitTensor<...> types (left-nested).
    let input_ty_tokens = build_input_type_tokens(&args);
    let output_ty_tokens = out_ty.obj_ty_tokens();

    // Compile body.
    let ctx = compile_body(&args, &func.block, &out_ty)?;

    // Generate argument wire declarations.
    let arg_wire_decls: Vec<proc_macro2::TokenStream> = args
        .iter()
        .map(|(_, sty, ident)| {
            let ty_tok = sty.wire_ty_tokens();
            quote! {
                let (bld, #ident) = bld.with_wire(#ty_tok);
            }
        })
        .collect();

    let arg_wire_idents: Vec<&Ident> = args.iter().map(|(_, _, id)| id).collect();

    // The final output wire identifier is the last fresh wire
    // emitted by `compile_body`, stored in `ctx.final_output`.
    let final_output = ctx
        .final_output
        .ok_or_else(|| syn::Error::new_spanned(&func.block, "kernel body produced no value"))?;
    let body_stmts = ctx.ctx.stmts;

    Ok(quote! {
        #vis fn #name() -> ::core::result::Result<
            ::hdl_cat_circuit::CircuitArrow<#input_ty_tokens, #output_ty_tokens>,
            ::hdl_cat_error::Error,
        > {
            let bld = ::hdl_cat_ir::HdlGraphBuilder::new();
            #(#arg_wire_decls)*
            #(#body_stmts)*
            ::core::result::Result::Ok(
                ::hdl_cat_circuit::CircuitArrow::from_raw_parts(
                    bld.build(),
                    vec![#(#arg_wire_idents),*],
                    vec![#final_output],
                )
            )
        }
    })
}

fn parse_kernel_arg(arg: &FnArg) -> Result<(String, ScalarTy, Ident), syn::Error> {
    let FnArg::Typed(PatType { pat, ty, .. }) = arg else {
        return Err(syn::Error::new_spanned(
            arg,
            "self parameters not supported",
        ));
    };
    let syn::Pat::Ident(pat_ident) = pat.as_ref() else {
        return Err(syn::Error::new_spanned(pat, "expected a simple identifier"));
    };
    let source_name = pat_ident.ident.to_string();
    let wire_ident = Ident::new(
        &format!("__k_arg_{source_name}"),
        pat_ident.ident.span(),
    );
    let sty = parse_scalar_ty(ty)?;
    Ok((source_name, sty, wire_ident))
}

fn build_input_type_tokens(
    args: &[(String, ScalarTy, Ident)],
) -> proc_macro2::TokenStream {
    match args.len() {
        0 => quote! { ::hdl_cat_circuit::CircuitUnit },
        1 => args[0].1.obj_ty_tokens(),
        _ => {
            let (first_rest, last) = args.split_at(args.len() - 1);
            let head = build_input_type_tokens_owned(first_rest);
            let tail = last[0].1.obj_ty_tokens();
            quote! { ::hdl_cat_circuit::CircuitTensor<#head, #tail> }
        }
    }
}

fn build_input_type_tokens_owned(
    args: &[(String, ScalarTy, Ident)],
) -> proc_macro2::TokenStream {
    match args.len() {
        0 => quote! { ::hdl_cat_circuit::CircuitUnit },
        1 => args[0].1.obj_ty_tokens(),
        _ => {
            let (first_rest, last) = args.split_at(args.len() - 1);
            let head = build_input_type_tokens_owned(first_rest);
            let tail = last[0].1.obj_ty_tokens();
            quote! { ::hdl_cat_circuit::CircuitTensor<#head, #tail> }
        }
    }
}

/// Bundle of context and final-output wire identifier after
/// compiling a kernel body.
struct CompiledBody {
    ctx: BodyCtx,
    final_output: Option<Ident>,
}

fn compile_body(
    args: &[(String, ScalarTy, Ident)],
    block: &syn::Block,
    _out_ty: &ScalarTy,
) -> Result<CompiledBody, syn::Error> {
    let initial_ctx = args.iter().fold(BodyCtx::new(), |ctx, (name, sty, wire_ident)| {
        ctx.bind(name.clone(), wire_ident.clone(), sty.clone())
    });
    let (ctx, final_output, _ty) = compile_block(initial_ctx, block)?;
    Ok(CompiledBody {
        ctx,
        final_output: Some(final_output),
    })
}

fn compile_block(
    ctx: BodyCtx,
    block: &syn::Block,
) -> Result<(BodyCtx, Ident, ScalarTy), syn::Error> {
    let (head, tail) = block
        .stmts
        .split_last()
        .ok_or_else(|| syn::Error::new_spanned(block, "empty kernel body"))?;

    // Fold over leading `let` statements, threading the ctx.
    let ctx_after_lets = tail
        .iter()
        .try_fold(ctx, compile_let_stmt)?;

    // Tail expression produces the block's value.
    let tail_expr = match head {
        Stmt::Expr(e, _) => Ok(e),
        other => Err(syn::Error::new_spanned(
            other,
            "kernel body must end in an expression",
        )),
    }?;
    compile_expr(ctx_after_lets, tail_expr)
}

fn compile_let_stmt(ctx: BodyCtx, stmt: &Stmt) -> Result<BodyCtx, syn::Error> {
    let Stmt::Local(local) = stmt else {
        return Err(syn::Error::new_spanned(
            stmt,
            "only `let` bindings allowed before the tail expression",
        ));
    };
    let syn::Pat::Ident(pat_ident) = &local.pat else {
        return Err(syn::Error::new_spanned(
            &local.pat,
            "expected a simple identifier",
        ));
    };
    let name = pat_ident.ident.to_string();
    let init = local
        .init
        .as_ref()
        .ok_or_else(|| syn::Error::new_spanned(local, "`let` requires an initializer"))?;
    let (ctx_after_rhs, wire, ty) = compile_expr(ctx, &init.expr)?;
    Ok(ctx_after_rhs.bind(name, wire, ty))
}

fn compile_expr(
    ctx: BodyCtx,
    expr: &Expr,
) -> Result<(BodyCtx, Ident, ScalarTy), syn::Error> {
    match expr {
        Expr::Path(p) => {
            let ident = p
                .path
                .get_ident()
                .ok_or_else(|| syn::Error::new_spanned(p, "expected bare identifier"))?;
            let (id, ty) = ctx
                .lookup(&ident.to_string())
                .ok_or_else(|| syn::Error::new_spanned(ident, "unknown identifier"))?;
            Ok((ctx, id, ty))
        }
        Expr::Binary(b) => compile_binary(ctx, b),
        Expr::Unary(u) => compile_unary(ctx, u),
        Expr::Paren(p) => compile_expr(ctx, &p.expr),
        other => Err(syn::Error::new_spanned(
            other,
            "unsupported expression in kernel body",
        )),
    }
}

fn compile_binary(
    ctx: BodyCtx,
    b: &syn::ExprBinary,
) -> Result<(BodyCtx, Ident, ScalarTy), syn::Error> {
    let (ctx_l, lhs, lhs_ty) = compile_expr(ctx, &b.left)?;
    let (ctx_lr, rhs, _rhs_ty) = compile_expr(ctx_l, &b.right)?;
    let op_tok = bin_op_tokens(&b.op)?;
    let (ctx_fresh, output) = ctx_lr.fresh_wire_ident();
    // Output type = lhs type (binary ops preserve width in our IR
    // except for comparisons, which are out-of-scope for v1).
    let out_ty = lhs_ty;
    let out_ty_tok = out_ty.wire_ty_tokens();
    let stmt = quote! {
        let (bld, #output) = bld.with_wire(#out_ty_tok);
        let bld = bld.with_instruction(
            ::hdl_cat_ir::Op::Bin(#op_tok),
            vec![#lhs, #rhs],
            #output,
        )?;
    };
    let ctx_final = ctx_fresh.push_stmt(stmt);
    Ok((ctx_final, output, out_ty))
}

fn compile_unary(
    ctx: BodyCtx,
    u: &syn::ExprUnary,
) -> Result<(BodyCtx, Ident, ScalarTy), syn::Error> {
    match u.op {
        UnOp::Not(_) => {
            let (ctx_inner, operand, operand_ty) = compile_expr(ctx, &u.expr)?;
            let (ctx_fresh, output) = ctx_inner.fresh_wire_ident();
            let ty_tok = operand_ty.wire_ty_tokens();
            let stmt = quote! {
                let (bld, #output) = bld.with_wire(#ty_tok);
                let bld = bld.with_instruction(
                    ::hdl_cat_ir::Op::Not,
                    vec![#operand],
                    #output,
                )?;
            };
            let ctx_final = ctx_fresh.push_stmt(stmt);
            Ok((ctx_final, output, operand_ty))
        }
        other => Err(syn::Error::new_spanned(
            other.into_token_stream(),
            "only unary `!` is supported",
        )),
    }
}

fn bin_op_tokens(op: &BinOp) -> Result<proc_macro2::TokenStream, syn::Error> {
    Ok(match op {
        BinOp::Add(_) => quote! { ::hdl_cat_ir::BinOp::Add },
        BinOp::Sub(_) => quote! { ::hdl_cat_ir::BinOp::Sub },
        BinOp::Mul(_) => quote! { ::hdl_cat_ir::BinOp::Mul },
        BinOp::BitAnd(_) => quote! { ::hdl_cat_ir::BinOp::And },
        BinOp::BitOr(_) => quote! { ::hdl_cat_ir::BinOp::Or },
        BinOp::BitXor(_) => quote! { ::hdl_cat_ir::BinOp::Xor },
        BinOp::Eq(_) => quote! { ::hdl_cat_ir::BinOp::Eq },
        BinOp::Lt(_) => quote! { ::hdl_cat_ir::BinOp::Lt },
        other => {
            return Err(syn::Error::new_spanned(
                other.into_token_stream(),
                "unsupported binary operator",
            ));
        }
    })
}
