use super::*;

/// Only the configured flat SQLx source is a numeric migration namespace.
/// Versioned artifacts and a canonical Goose source have different rules.
pub fn violations(ctx: &RepoContext) -> Result<Vec<String>> {
    if !ctx.sqlx_enabled()
        || !ctx.rust_migration_layout().allows_migration_add()
        || !ctx.sqlx_owns_migration_authoring()
    {
        return Ok(Vec::new());
    }
    let directory = ctx.migration_relative_dir()?;
    let path = ctx.root().join(&directory);
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()));
        }
    };
    let mut filenames = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read {}", path.display()))?;
        // SQLx follows file symlinks but does not recurse into child directories.
        if entry
            .path()
            .metadata()
            .with_context(|| format!("Failed to inspect {:?}", entry.path()))?
            .is_file()
        {
            filenames.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(jig_sqlx::migration_version_conflicts(filenames.iter().map(String::as_str))
        .into_iter()
        .map(|conflict| {
            let files = conflict.filenames.iter()
                .map(|name| format!("{:?}", directory.join(name)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Duplicate SQLx migration version {}: {files}. Use a distinct numeric version for each migration; only one .up.sql/.down.sql pair may share a version.",
                conflict.version
            )
        })
        .collect())
}

pub fn check(ctx: &RepoContext) -> Result<()> {
    let violations = violations(ctx)?;
    if !violations.is_empty() {
        bail!("{}", violations.join("\n"));
    }
    Ok(())
}
