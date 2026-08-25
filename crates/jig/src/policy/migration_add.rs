use super::*;

pub(crate) fn migration_add(ctx: &RepoContext, name: &str) -> Result<NativeToolOutput> {
    let backend = ctx.migration_backend()?.ok_or_else(|| {
        if ctx.contract_version() >= 6 {
            anyhow::anyhow!(
                "migration add requires one declared migration authoring target with a configured SQLx or Go/PostgreSQL migration backend"
            )
        } else {
            anyhow::anyhow!(
                "migration add requires a configured SQLx or Go/PostgreSQL migration backend"
            )
        }
    })?;
    if backend == crate::context::MigrationBackend::Sqlx && !ctx.migration_add_enabled() {
        bail!(
            "sqlx migration add requires rust_migration_layout = \"flat_migrations\"; this repository has rust_migration_layout = \"{}\"",
            ctx.rust_migration_layout().as_str()
        );
    }
    let migration_dir = ctx
        .migration_relative_dir()
        .context("migration_dir is empty, unsafe, or has no legacy rust_migration_dir fallback")?;
    let slug = slugify(name);
    if slug.is_empty() {
        bail!("Migration name {name:?} must contain at least one alphanumeric character.");
    }
    let timestamp = utc_timestamp();
    validate_repository_directory_path(ctx.root(), &migration_dir)?;
    let base = ctx
        .root()
        .join(&migration_dir)
        .join(format!("{timestamp}_{slug}"));
    match backend {
        crate::context::MigrationBackend::Goose => goose_migration_add(&base, &slug),
        crate::context::MigrationBackend::Sqlx => sqlx_migration_add(&base, &slug),
    }
}

fn goose_migration_add(base: &Path, slug: &str) -> Result<NativeToolOutput> {
    let migration = base.with_extension("sql");
    if migration.exists() {
        bail!("Migration file already exists: {}.", migration.display());
    }
    if let Some(parent) = migration.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(
        &migration,
        format!(
            "-- +goose Up\n-- forward migration: {slug}\n\n-- +goose Down\n-- rollback migration: {slug}\n"
        ),
    )
    .with_context(|| format!("Failed to write {}", migration.display()))?;
    Ok(NativeToolOutput {
        exit_status: 0,
        stdout: format!("Created:\n  - {}\n", migration.display()),
        stderr: String::new(),
    })
}

fn sqlx_migration_add(base: &Path, slug: &str) -> Result<NativeToolOutput> {
    let up = base.with_extension("up.sql");
    let down = base.with_extension("down.sql");
    if up.exists() || down.exists() {
        bail!("Migration files already exist for {}.", base.display());
    }
    if let Some(parent) = up.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&up, format!("-- forward migration: {slug}\n"))
        .with_context(|| format!("Failed to write {}", up.display()))?;
    fs::write(&down, format!("-- rollback migration: {slug}\n"))
        .with_context(|| format!("Failed to write {}", down.display()))?;
    Ok(NativeToolOutput {
        exit_status: 0,
        stdout: format!("Created:\n  - {}\n  - {}\n", up.display(), down.display()),
        stderr: String::new(),
    })
}
