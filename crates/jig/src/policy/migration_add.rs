use super::*;
use std::fs::{File, OpenOptions};

use fs4::fs_std::FileExt;

const MIGRATION_ADD_LOCK_PATH: &str = ".agent/.cache/migration-add.lock";

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
    validate_repository_directory_path(ctx.root(), &migration_dir)?;
    // A migration version is backend-wide identity, not part of the human
    // slug. Hold one repository lease across allocation and creation so two
    // Jig processes cannot publish different names with the same version.
    let _lock = acquire_migration_add_lock(ctx)?;
    let timestamp = next_migration_version(&ctx.root().join(&migration_dir))?;
    let base = ctx
        .root()
        .join(&migration_dir)
        .join(format!("{timestamp}_{slug}"));
    match backend {
        crate::context::MigrationBackend::Goose => goose_migration_add(&base, &slug),
        crate::context::MigrationBackend::Sqlx => sqlx_migration_add(&base, &slug),
    }
}

fn acquire_migration_add_lock(ctx: &RepoContext) -> Result<File> {
    let lock_path = ctx.root().join(MIGRATION_ADD_LOCK_PATH);
    let parent = lock_path
        .parent()
        .expect("the migration-add lock path has a parent");
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Failed to open {}", lock_path.display()))?;
    FileExt::lock_exclusive(&file)
        .with_context(|| format!("Failed to lock {}", lock_path.display()))?;
    Ok(file)
}

fn next_migration_version(migration_dir: &Path) -> Result<String> {
    next_migration_version_from(migration_dir, time::OffsetDateTime::now_utc())
}

fn next_migration_version_from(
    migration_dir: &Path,
    mut candidate: time::OffsetDateTime,
) -> Result<String> {
    let occupied = occupied_migration_versions(migration_dir)?;
    loop {
        let version = utc_timestamp_at(candidate);
        if !occupied.contains(&version) {
            return Ok(version);
        }
        candidate = candidate
            .checked_add(time::Duration::SECOND)
            .context("Migration timestamp overflowed while finding a unique version")?;
    }
}

fn occupied_migration_versions(migration_dir: &Path) -> Result<HashSet<String>> {
    let entries = match fs::read_dir(migration_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read {}", migration_dir.display()));
        }
    };
    let mut occupied = HashSet::new();
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Failed to read an entry in {}", migration_dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some((version, _)) = name.split_once('_')
            && version.len() == 14
            && version.bytes().all(|byte| byte.is_ascii_digit())
        {
            occupied.insert(version.to_owned());
        }
    }
    Ok(occupied)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_versions_advance_by_valid_utc_seconds_across_name_collisions() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("20261231235959_first.sql"), "").unwrap();
        fs::write(temp.path().join("20270101000000_second.up.sql"), "").unwrap();
        let start = time::OffsetDateTime::parse(
            "2026-12-31T23:59:59Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();

        let version = next_migration_version_from(temp.path(), start).unwrap();

        assert_eq!(version, "20270101000001");
    }
}
