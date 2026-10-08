use anyhow::{Result, anyhow};
use syn::ext::IdentExt as _;
use syn::visit::Visit as _;

// This is a syntax signal for adoption, not name resolution or macro expansion.
// Ignore opaque macro inputs/definitions and aliases; inspect complete files or
// expression fragments (for example, include! inputs) with the same visitor.
pub(super) fn has_migrate_macro(text: &str) -> Result<bool> {
    // Negative-only filter: both migrate and r#migrate contain these bytes.
    // A positive substring match is never itself a migration signal.
    if !text.contains("migrate") {
        return Ok(false);
    }
    jig_policy::rust_syntax::with_bounded_syntax(text, "Rust adoption", || {
        parse_migrate_macro(text)
    })
}

fn parse_migrate_macro(text: &str) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_boundary_preserves_migration_detection_and_bounds_recursive_syntax() {
        let near_limit = format!(
            "fn example() {{ sqlx::migrate!(); let _ = {}0{}; }}",
            "(".repeat(2_000),
            ")".repeat(2_000)
        );
        assert!(has_migrate_macro(&near_limit).unwrap());
        let excessive = format!(
            "fn example() {{ sqlx::migrate!(); let _ = {}0; }}",
            "!".repeat(5_000)
        );
        let error = has_migrate_macro(&excessive).unwrap_err();
        assert!(error.to_string().contains("token complexity limit"));
        assert!(!has_migrate_macro("fn example() { sqlx::query(\"SELECT 1\"); }").unwrap());
    }
}
