use anyhow::{Result, anyhow};
use syn::visit::{self, Visit};

use super::SqlxCall;

pub(super) fn scan_sqlx_calls(path: &str, text: &str) -> Result<Vec<SqlxCall>> {
    let file = syn::parse_file(text).map_err(|error| {
        let start = error.span().start();
        anyhow!(
            "cannot parse SQLx inventory source {path}:{}:{}: {error}",
            start.line,
            start.column + 1
        )
    })?;
    let mut scanner = SqlxScanner {
        path,
        is_test: is_test_path(path) || has_cfg_test(&file.attrs),
        calls: Vec::new(),
    };
    scanner.visit_file(&file);
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
        let name = path.segments[1].ident.to_string();
        if namespace.ident != "sqlx"
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
        // Macro input has macro-specific grammar. Do not guess that its tokens
        // are Rust expressions or count calls in unexpanded macro definitions.
    }
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| matches!(meta, syn::Meta::Path(path) if path.is_ident("test")))
    })
}

fn is_test_path(path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("tests/")
        || path.contains("/tests/")
        || matches!(basename, "tests.rs" | "test_support.rs")
        || basename.starts_with("tests_")
        || basename.ends_with("_tests.rs")
}
