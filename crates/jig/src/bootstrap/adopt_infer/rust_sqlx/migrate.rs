use anyhow::{Context as _, Result, anyhow, bail};
use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::visit::Visit as _;

// A conservative upper bound for recursive AST work, including flat unary,
// binary, type, and method chains that delimiter depth alone cannot bound.
const MAX_TOKEN_PATH_COST: usize = 2_048;
const PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

// This is a syntax signal for adoption, not name resolution or macro expansion.
// Ignore opaque macro inputs/definitions and aliases; inspect complete files or
// expression fragments (for example, include! inputs) with the same visitor.
pub(super) fn has_migrate_macro(text: &str) -> Result<bool> {
    // Negative-only filter: both migrate and r#migrate contain these bytes.
    // A positive substring match is never itself a migration signal.
    if !text.contains("migrate") {
        return Ok(false);
    }
    // Use a fixed stack independent of the caller (including small test
    // threads). Tokenization, AST visitation, and all drops stay in this worker.
    // The token guard, not panic catching, prevents recursive stack exhaustion.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("adoption-rust-parser".into())
            .stack_size(PARSER_STACK_BYTES)
            .spawn_scoped(scope, || parse_migrate_macro(text))
            .context("cannot start Rust adoption parser")?
            .join()
            .map_err(|_| anyhow!("Rust adoption parser panicked"))?
    })
}

fn parse_migrate_macro(text: &str) -> Result<bool> {
    check_token_complexity(text)?;
    let mut visitor = MigrateVisitor::default();
    match syn::parse_file(text) {
        Ok(file) => visitor.visit_file(&file),
        Err(file_error) => {
            let expression = syn::parse_str::<syn::Expr>(text).map_err(|expression_error| {
                anyhow!("Rust file parse: {file_error}; expression parse: {expression_error}")
            })?;
            visitor.visit_expr(&expression);
        }
    }
    Ok(visitor.found)
}

fn check_token_complexity(text: &str) -> Result<()> {
    // proc_macro2's fallback lexer and TokenStream destructor use explicit
    // stacks, so even rejected deeply nested input is safe to tokenize/drop.
    let tokens = text
        .parse::<TokenStream>()
        .map_err(|error| anyhow!("Rust tokenization: {error}"))?;
    let mut pending = vec![(tokens, 0)];
    while let Some((tokens, ancestors)) = pending.pop() {
        let mut cost = ancestors;
        let mut children = Vec::new();
        for token in tokens {
            cost += 1;
            if cost > MAX_TOKEN_PATH_COST {
                bail!(
                    "Rust source exceeds adoption token complexity limit ({MAX_TOKEN_PATH_COST})"
                );
            }
            if let TokenTree::Group(group) = token {
                children.push(group.stream());
            }
        }
        // Charge all siblings at each enclosing level, not just delimiter
        // nesting. This bounds recursive syntax within and across groups.
        pending.extend(children.into_iter().map(|child| (child, cost)));
    }
    Ok(())
}

#[derive(Default)]
struct MigrateVisitor {
    found: bool,
}

impl<'ast> syn::visit::Visit<'ast> for MigrateVisitor {
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let mut segments = node.path.segments.iter();
        self.found |= segments.len() == 2
            && segments
                .next()
                .is_some_and(|part| part.ident.unraw() == "sqlx")
            && segments
                .next()
                .is_some_and(|part| part.ident.unraw() == "migrate");
    }
}
