use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{
    RepoScan, push_scan_warning, read_limited_text, read_toml_for_inference, relative_path_string,
};
use crate::bootstrap::crate_classification::non_production_crate_reason;

const MAX_MIGRATION_SQL_DEPTH: usize = 3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum RustCrateRootSourceKind {
    #[default]
    None,
    SinglePackage,
    WorkspaceMembers,
    WorkspaceFallback,
    ScannedPackages,
}

#[derive(Debug, Default)]
pub(super) struct RustCrateRootsInference {
    pub(super) roots: Vec<String>,
    pub(super) sources: Vec<String>,
    pub(super) source_kind: RustCrateRootSourceKind,
    pub(super) scanned_manifest_paths: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct SqlxInference {
    pub(super) enabled: InferredSqlxValue<bool>,
    pub(super) migration_dir: Option<InferredSqlxValue<String>>,
    pub(super) migration_dirs: InferredSqlxValue<Vec<String>>,
    pub(super) metadata_dir: Option<InferredSqlxValue<String>>,
    pub(super) check_command: Option<InferredSqlxValue<String>>,
    pub(super) signals: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct InferredSqlxValue<T> {
    pub(super) value: T,
    pub(super) sources: Vec<String>,
    pub(super) warnings: Vec<String>,
}

impl<T> InferredSqlxValue<T> {
    fn with_source(value: T, source: String) -> Self {
        Self {
            value,
            sources: vec![source],
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
pub(super) fn infer_rust_crate_roots(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    infer_rust_crate_roots_with_metadata(root, warnings).roots
}

pub(super) fn infer_rust_crate_roots_with_metadata(
    root: &Path,
    warnings: &mut Vec<String>,
) -> RustCrateRootsInference {
    let cargo_path = root.join("Cargo.toml");
    if !cargo_path.is_file() {
        return RustCrateRootsInference::default();
    }
    let Some(parsed) = read_toml_for_inference(&cargo_path, warnings) else {
        return RustCrateRootsInference::default();
    };
    let Some(workspace) = parsed.get("workspace").and_then(toml::Value::as_table) else {
        if parsed
            .get("package")
            .and_then(toml::Value::as_table)
            .is_some()
        {
            return RustCrateRootsInference {
                roots: vec![".".into()],
                sources: vec!["Cargo.toml [package]".into()],
                source_kind: RustCrateRootSourceKind::SinglePackage,
                ..RustCrateRootsInference::default()
            };
        }
        push_scan_warning(
            warnings,
            &cargo_path,
            "Cargo.toml has neither [workspace] nor [package]; Rust crate roots were not inferred",
        );
        return RustCrateRootsInference::default();
    };
    let mut roots = BTreeSet::new();
    if let Some(members) = workspace.get("members").and_then(toml::Value::as_array) {
        for member in members.iter().filter_map(toml::Value::as_str) {
            if member.starts_with('!') {
                continue;
            }
            if let Some(root) = crate_root_from_workspace_member(member) {
                roots.insert(root);
            }
        }
    }
    let used_workspace_fallback = roots.is_empty();
    let source = if used_workspace_fallback {
        roots.insert(".".into());
        "Cargo.toml [workspace] (no usable workspace members)".into()
    } else {
        "Cargo.toml [workspace.members]".into()
    };
    let source_kind = if used_workspace_fallback {
        RustCrateRootSourceKind::WorkspaceFallback
    } else {
        RustCrateRootSourceKind::WorkspaceMembers
    };
    RustCrateRootsInference {
        roots: roots.into_iter().collect(),
        sources: vec![source],
        source_kind,
        ..RustCrateRootsInference::default()
    }
}

pub(super) fn infer_rust_crate_roots_from_scan(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> RustCrateRootsInference {
    let mut roots = BTreeSet::new();
    let mut manifest_paths = BTreeSet::new();
    let mut package_count = 0usize;
    for cargo_path in scan.named_files("Cargo.toml") {
        let relative_cargo_path = cargo_path.strip_prefix(root).unwrap_or(cargo_path);
        if relative_cargo_path == Path::new("Cargo.toml") {
            continue;
        }
        let Some(parsed) = read_toml_for_inference(cargo_path, warnings) else {
            continue;
        };
        let package = parsed.get("package").and_then(toml::Value::as_table);
        let workspace = parsed.get("workspace").and_then(toml::Value::as_table);
        // Workspace-only manifests are runnable Cargo roots with --manifest-path,
        // so the fallback keeps them even without a local [package].
        if package.is_none() && workspace.is_none() {
            continue;
        }
        let package_name = package
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str);
        let crate_dir = cargo_path.parent().unwrap_or(root);
        let relative_crate_dir =
            relative_path_string(crate_dir.strip_prefix(root).unwrap_or(crate_dir));
        if let Some(reason) =
            non_production_crate_reason(Path::new(&relative_crate_dir), package_name)
        {
            if reason.starts_with("package name ") {
                push_scan_warning(
                    warnings,
                    cargo_path,
                    &format!("nested Cargo.toml skipped during Rust inference: {reason}"),
                );
            }
            continue;
        }
        let crate_root = crate_dir.parent().unwrap_or(root);
        let relative_crate_root =
            relative_path_string(crate_root.strip_prefix(root).unwrap_or(crate_root));
        roots.insert(if relative_crate_root.is_empty() {
            ".".into()
        } else {
            relative_crate_root
        });
        manifest_paths.insert(relative_path_string(relative_cargo_path));
        package_count += 1;
    }
    if roots.is_empty() {
        return RustCrateRootsInference::default();
    }
    let mut roots: Vec<String> = roots.into_iter().collect();
    if roots.iter().any(|root| root == ".") {
        roots = vec![".".into()];
    }

    RustCrateRootsInference {
        roots,
        sources: vec![format!("scanned {package_count} nested Cargo.toml file(s)")],
        source_kind: RustCrateRootSourceKind::ScannedPackages,
        scanned_manifest_paths: manifest_paths.into_iter().collect(),
    }
}

// Jig crate roots are parent directories whose direct children are crates.
// Keep the optional inference boundary so callers can treat this like the
// neighboring best-effort detectors without manufacturing a sentinel root.
#[allow(clippy::unnecessary_wraps)]
pub(super) fn crate_root_from_workspace_member(member: &str) -> Option<String> {
    let path = member.trim().trim_end_matches('/');
    if path.is_empty() || path == "." {
        return Some(".".into());
    }
    let first_glob = path.find(['*', '[', '?']);
    if let Some(index) = first_glob {
        let prefix = path[..index].trim_end_matches('/');
        if prefix.is_empty() {
            return Some(".".into());
        }
        return Some(relative_path_string(Path::new(prefix)));
    }
    let parent = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let root = relative_path_string(parent);
    Some(if root.is_empty() { ".".into() } else { root })
}

pub(super) fn infer_sqlx(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> SqlxInference {
    let mut out = SqlxInference::default();
    for path in scan.named_files("Cargo.toml") {
        if let Some(source) = cargo_toml_sqlx_source(root, path, warnings) {
            out.enabled.value = true;
            out.signals.push(format!("SQLx dependency in {source}"));
            out.enabled.sources.push(source);
        }
    }
    if scan.has_dir_named_at_root(root, ".sqlx") {
        out.enabled.value = true;
        out.metadata_dir = Some(InferredSqlxValue::with_source(
            ".sqlx".into(),
            ".sqlx/".into(),
        ));
        out.signals.push("SQLx metadata directory .sqlx".into());
        out.enabled.sources.push(".sqlx/".into());
    }
    let migration_candidates = find_migration_dirs(root, scan);
    out.migration_dirs.value = migration_candidates
        .iter()
        .map(|candidate| candidate.dir.clone())
        .collect();
    out.migration_dirs.sources = migration_candidates
        .iter()
        .map(|candidate| candidate.source.clone())
        .collect();
    if let Some(candidate) = migration_candidates.first() {
        let dir = &candidate.dir;
        out.enabled.value = true;
        out.migration_dir = Some(InferredSqlxValue::with_source(
            dir.clone(),
            candidate.source.clone(),
        ));
        out.enabled.sources.push(candidate.source.clone());
        if out.migration_dirs.value.len() == 1 {
            out.signals.push(format!("migration directory {dir}"));
        } else {
            let warning = format!(
                "multiple migration directories detected; using alphabetically first {dir} unless overridden"
            );
            push_scan_warning(warnings, root, &warning);
            if let Some(migration_dir) = &mut out.migration_dir {
                migration_dir.warnings.push(warning.clone());
            }
            out.migration_dirs.warnings.push(warning);
            out.signals.push(format!(
                "migration directories detected: {}",
                out.migration_dirs.value.join(", ")
            ));
        }
    }
    if let Some(source) = first_text_file_matching(root, scan, &["rs"], warnings, |text| {
        text.lines().any(rust_line_invokes_sqlx_migrate)
    }) {
        out.enabled.value = true;
        out.signals.push("sqlx::migrate! macro".into());
        out.enabled
            .sources
            .push(format!("sqlx::migrate! macro in {source}"));
    }
    if let Some(source) = first_text_file_matching(root, scan, &["sh"], warnings, |text| {
        text.lines().any(shell_line_invokes_cargo_sqlx)
    }) {
        out.enabled.value = true;
        out.signals.push("cargo sqlx command".into());
        out.enabled
            .sources
            .push(format!("cargo sqlx command in {source}"));
    } else if let Some(source) =
        first_text_file_matching(root, scan, &["yml", "yaml"], warnings, |text| {
            text.lines().any(yaml_run_invokes_cargo_sqlx)
        })
    {
        out.enabled.value = true;
        out.signals.push("cargo sqlx command".into());
        out.enabled
            .sources
            .push(format!("cargo sqlx command in {source}"));
    }
    if out.enabled.value {
        let synthesized_migration_dir = out.migration_dir.is_none();
        let synthesized_metadata_dir = out.metadata_dir.is_none();
        if synthesized_migration_dir {
            out.migration_dir = Some(InferredSqlxValue::with_source(
                "migrations".into(),
                "SQLx default migrations/".into(),
            ));
        }
        if synthesized_metadata_dir {
            out.metadata_dir = Some(InferredSqlxValue::with_source(
                ".sqlx".into(),
                "SQLx default .sqlx/".into(),
            ));
        }
        let warning = match (synthesized_migration_dir, synthesized_metadata_dir) {
            (true, true) => Some(
                "SQLx was detected but migration and metadata directories were not; using default SQLx paths unless overridden",
            ),
            (true, false) => Some(
                "SQLx was detected but no migration directory was found; using default migrations/ unless overridden",
            ),
            (false, true) => Some(
                "SQLx metadata directory was not detected; using default .sqlx/ unless overridden",
            ),
            (false, false) => None,
        };
        if let Some(warning) = warning {
            push_scan_warning(warnings, root, warning);
            if synthesized_migration_dir {
                if let Some(migration_dir) = &mut out.migration_dir {
                    migration_dir.warnings.push(warning.into());
                }
            }
            if synthesized_metadata_dir {
                if let Some(metadata_dir) = &mut out.metadata_dir {
                    metadata_dir.warnings.push(warning.into());
                }
            }
        }
        let metadata_dir = out
            .metadata_dir
            .as_ref()
            .map(|metadata_dir| metadata_dir.value.as_str())
            .unwrap_or(".sqlx");
        let mut check_sources = Vec::new();
        let workspace_arg = if cargo_workspace_declared(root, warnings) {
            check_sources.push("Cargo.toml [workspace]".into());
            " --workspace"
        } else {
            ""
        };
        if let Some(metadata_dir) = &out.metadata_dir {
            check_sources.extend(metadata_dir.sources.iter().cloned());
        }
        // `prepare --check` intentionally connects to the database while
        // comparing against the configured metadata directory. Adopt renders
        // this command for POSIX-like local and CI environments.
        // Windows SQLx checks should be supplied explicitly with
        // `--sqlx-check-command`.
        out.check_command = Some(InferredSqlxValue {
            value: format!(
                "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='{}' cargo sqlx prepare --check{} -- --all-targets",
                metadata_dir.replace('\'', "'\\''"),
                workspace_arg
            ),
            sources: check_sources,
            warnings: Vec::new(),
        });
        out.signals
            .push("SQLx check command assumes online cargo sqlx prepare".into());
    } else {
        out.signals.push("no SQLx signals detected".into());
        out.enabled
            .sources
            .push("repository scan found no SQLx signals".into());
    }
    out
}

fn cargo_toml_sqlx_source(root: &Path, path: &Path, warnings: &mut Vec<String>) -> Option<String> {
    let parsed = read_toml_for_inference(path, warnings)?;
    let relative = relative_source_path(root, path);
    for section in [
        "dependencies",
        "dev-dependencies",
        "build-dependencies",
        "workspace.dependencies",
    ]
    .iter()
    {
        if toml_section(&parsed, section).is_some_and(|table| table.contains_key("sqlx")) {
            return Some(format!("{relative} [{section}].sqlx"));
        }
    }
    None
}

fn cargo_workspace_declared(root: &Path, warnings: &mut Vec<String>) -> bool {
    let path = root.join("Cargo.toml");
    path.is_file()
        && read_toml_for_inference(&path, warnings)
            .is_some_and(|parsed| parsed.get("workspace").is_some())
}

fn toml_section<'a>(
    value: &'a toml::Value,
    dotted: &str,
) -> Option<&'a toml::map::Map<String, toml::Value>> {
    let mut cursor = value;
    for key in dotted.split('.') {
        cursor = cursor.get(key)?;
    }
    cursor.as_table()
}

#[derive(Debug)]
struct MigrationDirCandidate {
    dir: String,
    source: String,
}

fn find_migration_dirs(root: &Path, scan: &RepoScan) -> Vec<MigrationDirCandidate> {
    let mut candidates = Vec::new();
    for path in scan.dirs_named("migrations") {
        if let Some(source_path) = migration_dir_sql_source(path, scan) {
            candidates.push(MigrationDirCandidate {
                dir: relative_path_string(path.strip_prefix(root).unwrap_or(path)),
                source: relative_source_path(root, source_path),
            });
        }
    }
    candidates.sort_by(|left, right| left.dir.cmp(&right.dir));
    candidates
}

fn migration_dir_sql_source<'a>(path: &'a Path, scan: &'a RepoScan) -> Option<&'a Path> {
    scan.files_under(path)
        .find(|entry_path| {
            let Ok(relative) = entry_path.strip_prefix(path) else {
                return false;
            };
            relative.components().count() <= MAX_MIGRATION_SQL_DEPTH + 1
                && migration_sql_file_is_supported(entry_path)
        })
        .map(std::path::PathBuf::as_path)
}

fn migration_sql_file_is_supported(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(starts_with_ascii_digit)
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(starts_with_ascii_digit)
}

fn starts_with_ascii_digit(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_digit)
}

fn first_text_file_matching<F>(
    root: &Path,
    scan: &RepoScan,
    extensions: &[&str],
    warnings: &mut Vec<String>,
    mut predicate: F,
) -> Option<String>
where
    F: FnMut(&str) -> bool,
{
    for path in scan.files_with_extensions(extensions) {
        match read_limited_text(path) {
            Ok(text) if predicate(&text) => return Some(relative_source_path(root, path)),
            Ok(_) => {}
            Err(error) => push_scan_warning(
                warnings,
                path,
                &format!("could not read text for inference: {error:#}"),
            ),
        }
    }
    None
}

fn relative_source_path(root: &Path, path: &Path) -> String {
    relative_path_string(path.strip_prefix(root).unwrap_or(path))
}

fn yaml_run_invokes_cargo_sqlx(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let Some(command) = trimmed.strip_prefix("run:") else {
        return false;
    };
    command_invokes_cargo_sqlx(strip_yaml_inline_comment(command).trim())
}

fn strip_yaml_inline_comment(value: &str) -> &str {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (index, ch) in value.char_indices() {
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '#' if !in_single_quote && !in_double_quote => return &value[..index],
            _ => {}
        }
    }
    value
}

fn rust_line_invokes_sqlx_migrate(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with("//")
        && !trimmed.starts_with("/*")
        && !trimmed.starts_with('*')
        && trimmed.contains("sqlx::migrate!")
}

fn shell_line_invokes_cargo_sqlx(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with('#') && command_invokes_cargo_sqlx(strip_shell_inline_comment(trimmed))
}

fn strip_shell_inline_comment(value: &str) -> &str {
    strip_yaml_inline_comment(value).trim()
}

fn command_invokes_cargo_sqlx(command: &str) -> bool {
    // Keep this conservative: detect direct invocations and skip comments or prose.
    let mut tokens = command
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')' | '"' | '\'')
        })
        .filter(|token| !token.is_empty())
        .skip_while(|token| token.contains('='));
    matches!(
        (tokens.next(), tokens.next()),
        (Some("cargo"), Some("sqlx"))
    )
}
