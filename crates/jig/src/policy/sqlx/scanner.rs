use anyhow::{Result, anyhow};
use proc_macro2::TokenStream;
use syn::Token;
use syn::ext::IdentExt as _;
use syn::parse::{ParseStream, Parser as _};
use syn::visit::{self, Visit};

use super::SqlxCall;

/// Macros whose input is a list of Rust expressions. Traversing these keeps
/// call sites visible inside ordinary wrappers such as `vec![sqlx::query(..)]`,
/// while macros that define a grammar of their own stay opaque.
const EXPRESSION_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "dbg",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "eprint",
    "eprintln",
    "format",
    "format_args",
    "join",
    "matches",
    "panic",
    "print",
    "println",
    "todo",
    "try_join",
    "unimplemented",
    "unreachable",
    "vec",
    "write",
    "writeln",
];

/// Namespaces an expression macro may be spelled through, as in `std::vec!`.
const EXPRESSION_MACRO_NAMESPACES: &[&str] = &["alloc", "core", "futures", "std", "tokio"];

pub(super) fn scan_sqlx_calls(path: &str, text: &str) -> Result<Vec<SqlxCall>> {
    crate::rust_syntax::with_bounded_syntax(
        text,
        &format!("cannot parse SQLx inventory source {path}"),
        || scan_bounded_sqlx_calls(path, text),
    )
}

fn scan_bounded_sqlx_calls(path: &str, text: &str) -> Result<Vec<SqlxCall>> {
    let mut scanner = SqlxScanner {
        path,
        is_test: is_test_path(path),
        calls: Vec::new(),
    };
    match syn::parse_file(text) {
        Ok(file) => {
            scanner.is_test |= has_cfg_test(&file.attrs);
            scanner.visit_file(&file);
        }
        Err(error) => {
            // `include!` accepts expression fragments as well as items. Parse
            // the complete expression without wrapping it so spans stay at
            // their original lines and trailing invalid input still fails.
            let expr = syn::parse_str::<syn::Expr>(text).map_err(|expr_error| {
                let start = error.span().start();
                let expr_start = expr_error.span().start();
                anyhow!(
                    "cannot parse SQLx inventory source {path}:{}:{}: {error} (file parse); \
                     expression parse at {path}:{}:{}: {expr_error}",
                    start.line,
                    start.column + 1,
                    expr_start.line,
                    expr_start.column + 1
                )
            })?;
            scanner.visit_expr(&expr);
        }
    }
    Ok(scanner.calls)
}

struct SqlxScanner<'a> {
    path: &'a str,
    is_test: bool,
    calls: Vec<SqlxCall>,
}

impl SqlxScanner<'_> {
    fn record_call(&mut self, path: &syn::Path, checked: bool) {
        if path.segments.len() != 2 {
            return;
        }
        let namespace = &path.segments[0];
        // A raw identifier names the same item as its bare spelling, so compare
        // and report the canonical name rather than the `r#` form.
        let name = unraw(&path.segments[1].ident);
        if namespace.ident.unraw() != "sqlx"
            || !matches!(
                name.as_str(),
                "query"
                    | "query_as"
                    | "query_scalar"
                    | "query_file"
                    | "query_file_as"
                    | "query_file_scalar"
            )
        {
            return;
        }
        self.calls.push(SqlxCall {
            path: self.path.into(),
            line: namespace.ident.span().start().line,
            function: format!("sqlx::{name}"),
            checked,
            is_test: self.is_test,
        });
    }
}

impl<'ast> Visit<'ast> for SqlxScanner<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let parent_is_test = self.is_test;
        self.is_test |= has_cfg_test(&module.attrs);
        visit::visit_item_mod(self, module);
        self.is_test = parent_is_test;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let mut callee = call.func.as_ref();
        loop {
            match callee {
                syn::Expr::Paren(expr) => callee = &expr.expr,
                syn::Expr::Group(expr) => callee = &expr.expr,
                syn::Expr::Path(expr) if expr.qself.is_none() => {
                    self.record_call(&expr.path, false);
                    break;
                }
                _ => break,
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.record_call(&invocation.path, true);
        // Macro input has macro-specific grammar, so only macros known to take
        // Rust expressions are traversed. Opaque grammars and unexpanded macro
        // definitions are left alone rather than guessed at.
        if !is_expression_macro(&invocation.path) {
            return;
        }
        for expr in macro_input_exprs(invocation.tokens.clone()) {
            self.visit_expr(&expr);
        }
    }
}

fn is_expression_macro(path: &syn::Path) -> bool {
    let Some(name) = path.segments.last() else {
        return false;
    };
    EXPRESSION_MACROS.contains(&unraw(&name.ident).as_str())
        && path
            .segments
            .iter()
            .rev()
            .skip(1)
            .all(|segment| EXPRESSION_MACRO_NAMESPACES.contains(&unraw(&segment.ident).as_str()))
}

/// Reads the expressions an expression macro was handed. The tokens carry their
/// original spans, so recorded call sites still point at the source line.
fn macro_input_exprs(tokens: TokenStream) -> Vec<syn::Expr> {
    let mut exprs = Vec::new();
    // A failed nested parse can make syn reject the enclosing parse2 result
    // even after leading_exprs returns Ok. Keep the successfully parsed prefix
    // outside that result so an opaque suffix cannot erase earlier calls.
    let _ = (|input: ParseStream| leading_exprs(input, &mut exprs)).parse2(tokens);
    exprs
}

fn leading_exprs(input: ParseStream, exprs: &mut Vec<syn::Expr>) -> syn::Result<()> {
    while !input.is_empty() {
        let Ok(expr) = input.parse() else {
            break;
        };
        exprs.push(expr);
        // `vec![value; count]` separates with `;` and other macros use `,`.
        // Anything else means the macro has grammar of its own from here on,
        // as `matches!` does with its pattern, so stop rather than guess.
        if input.peek(Token![;]) {
            input.parse::<Token![;]>()?;
        } else if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        } else {
            break;
        }
    }
    input.parse::<TokenStream>()?;
    Ok(())
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        is_ident(attr.path(), "cfg")
            && attr
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| matches!(meta, syn::Meta::Path(path) if is_ident(&path, "test")))
    })
}

fn is_ident(path: &syn::Path, name: &str) -> bool {
    path.get_ident().is_some_and(|ident| ident.unraw() == name)
}

fn unraw(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

fn is_test_path(path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("tests/")
        || path.contains("/tests/")
        || matches!(basename, "tests.rs" | "test_support.rs")
        || basename.starts_with("tests_")
        || basename.ends_with("_tests.rs")
}
