//! Resource boundary for recursive Rust parsing, AST traversal, and destruction.

use anyhow::{Context as _, Result, anyhow, bail};
use proc_macro2::{TokenStream, TokenTree};

// Bound flat unary, binary, type, and method chains as well as delimiter depth.
// Independent semicolon-separated declarations/statements share no AST path.
const MAX_TOKEN_PATH_COST: usize = 2_048;
const PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

pub fn with_bounded_syntax<T: Send>(
    text: &str,
    purpose: &str,
    parse: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    // All tokens and ASTs must be created and dropped in this worker. The
    // conservative token budget prevents exhaustion; catching panics cannot
    // recover from a stack overflow. Results must contain no borrowed ASTs.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("rust-syntax-parser".into())
            .stack_size(PARSER_STACK_BYTES)
            .spawn_scoped(scope, || {
                check_token_complexity(text, purpose)?;
                parse()
            })
            .with_context(|| format!("cannot start {purpose} parser"))?
            .join()
            .map_err(|_| anyhow!("{purpose} parser panicked"))?
    })
}

fn check_token_complexity(text: &str, purpose: &str) -> Result<()> {
    // proc_macro2's fallback lexer and token destructor use explicit stacks,
    // so even rejected deeply nested input can be tokenized and dropped.
    let tokens = text.parse::<TokenStream>().map_err(|error| {
        let start = error.span().start();
        anyhow!(
            "{purpose}:{}:{}: Rust tokenization: {error}",
            start.line,
            start.column + 1
        )
    })?;
    let mut pending = vec![(tokens, 0)];
    while let Some((tokens, ancestors)) = pending.pop() {
        let mut cost = ancestors;
        let mut children = Vec::new();
        for token in tokens {
            cost += 1;
            if cost > MAX_TOKEN_PATH_COST {
                bail!(
                    "{purpose}: Rust source exceeds token complexity limit ({MAX_TOKEN_PATH_COST} tokens along enclosing semicolon-separated regions); simplify nested syntax or split long expressions/declarations"
                );
            }
            match token {
                TokenTree::Group(group) => children.push(group.stream()),
                TokenTree::Punct(punct) if punct.as_char() == ';' => {
                    pending.extend(children.drain(..).map(|child| (child, cost)));
                    cost = ancestors;
                }
                _ => {}
            }
        }
        // Charge the complete enclosing region, including tokens after a group:
        // a trailing method/binary chain still contributes to the AST depth.
        // Do not reset at commas: nested generic types can contain commas at
        // the same token-tree level and still form one recursive AST path.
        pending.extend(children.into_iter().map(|child| (child, cost)));
    }
    Ok(())
}
