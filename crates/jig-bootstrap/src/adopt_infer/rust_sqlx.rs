use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{
    RepoScan, push_scan_warning, read_limited_text, read_toml_for_inference, relative_path_string,
};
use crate::crate_classification::non_production_crate_reason;

mod migrate;
mod migrations;

use migrations::MigrationSurvey;
pub(super) use migrations::{MigrationChoice, repository_has_go_module};

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
    pub(super) migration_choice: MigrationChoice,
}

impl SqlxInference {
    fn enable(&mut self, signal: String, source: String) {
        self.enabled.value = true;
        self.signals.push(signal);
        self.enabled.sources.push(source);
    }
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
            roots.insert(crate_root_from_workspace_member(member));
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
pub(super) fn crate_root_from_workspace_member(member: &str) -> String {
    let path = member.trim().trim_end_matches('/');
    if path.is_empty() || path == "." {
        return ".".into();
    }
    let first_glob = path.find(['*', '[', '?']);
    if let Some(index) = first_glob {
        let prefix = path[..index].trim_end_matches('/');
        if prefix.is_empty() {
            return ".".into();
        }
        return relative_path_string(Path::new(prefix));
    }
    let parent = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let root = relative_path_string(parent);
    if root.is_empty() { ".".into() } else { root }
}

/// `repository_has_go` describes the whole repository even when `scan` is
/// restricted to selected components.
pub(super) fn infer_sqlx(
    root: &Path,
    scan: &RepoScan,
    repository_has_go: bool,
    warnings: &mut Vec<String>,
) -> SqlxInference {
    let mut out = SqlxInference::default();
    let sqlx_manifest_dirs = record_sqlx_evidence(root, scan, warnings, &mut out);
    let survey =
        migrations::survey_migration_dirs(root, scan, &sqlx_manifest_dirs, repository_has_go);
    if out.enabled.value {
        record_migration_survey(root, &survey, warnings, &mut out);
        let synthesize_migration_dir = matches!(survey.choice, MigrationChoice::NoCandidates);
        record_default_paths(root, warnings, &mut out, synthesize_migration_dir);
        record_check_command(root, warnings, &mut out);
    } else {
        out.signals.push("no SQLx signals detected".into());
        out.enabled
            .sources
            .push("repository scan found no SQLx signals".into());
    }
    out.migration_choice = survey.choice;
    out
}

/// Records SQLx-specific evidence and returns the directories whose
/// Cargo.toml declares sqlx. Numbered SQL files are deliberately not evidence:
/// many migration tools share that layout.
fn record_sqlx_evidence(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
    out: &mut SqlxInference,
) -> BTreeSet<String> {
    let mut sqlx_manifest_dirs = BTreeSet::new();
    for path in scan.named_files("Cargo.toml") {
        if let Some(source) = cargo_toml_sqlx_source(root, path, warnings) {
            sqlx_manifest_dirs.insert(migrations::relative_dir(
                root,
                path.parent().unwrap_or(root),
            ));
            out.enable(format!("SQLx dependency in {source}"), source);
        }
    }
    if scan.has_dir_named_at_root(root, ".sqlx") {
        out.metadata_dir = Some(InferredSqlxValue::with_source(
            ".sqlx".into(),
            ".sqlx/".into(),
        ));
        out.enable("SQLx metadata directory .sqlx".into(), ".sqlx/".into());
    }
    if let Some(source) =
        first_text_file_matching(root, scan, &["rs"], warnings, migrate::has_migrate_macro)
    {
        out.enable(
            "sqlx::migrate! macro".into(),
            format!("sqlx::migrate! macro in {source}"),
        );
    }
    if let Some(source) = cargo_sqlx_command_source(root, scan, warnings) {
        out.enable(
            "cargo sqlx command".into(),
            format!("cargo sqlx command in {source}"),
        );
    }
    sqlx_manifest_dirs
}

fn cargo_sqlx_command_source(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Option<String> {
    first_text_file_matching(root, scan, &["sh"], warnings, |text| {
        Ok(text.lines().any(shell_line_invokes_cargo_sqlx))
    })
    .or_else(|| {
        first_text_file_matching(root, scan, &["yml", "yaml"], warnings, |text| {
            Ok(text.lines().any(yaml_run_invokes_cargo_sqlx))
        })
    })
}

fn record_migration_survey(
    root: &Path,
    survey: &MigrationSurvey,
    warnings: &mut Vec<String>,
    out: &mut SqlxInference,
) {
    out.migration_dirs.value = survey
        .sqlx_candidates
        .iter()
        .map(|candidate| candidate.dir.clone())
        .collect();
    out.migration_dirs.sources = survey
        .sqlx_candidates
        .iter()
        .map(|candidate| candidate.source.clone())
        .collect();
    for candidate in &survey.excluded {
        out.signals.push(format!(
            "migration directory not used for SQLx: {}",
            candidate.describe()
        ));
    }
    if out.migration_dirs.value.len() > 1 {
        out.signals.push(format!(
            "migration directories detected: {}",
            out.migration_dirs.value.join(", ")
        ));
    }
    if let MigrationChoice::Selected(candidate) = &survey.choice {
        out.migration_dir = Some(InferredSqlxValue::with_source(
            candidate.dir.clone(),
            candidate.source.clone(),
        ));
        out.signals
            .push(format!("migration directory {}", candidate.describe()));
    } else if let Some(warning) = survey.choice.ambiguity() {
        push_scan_warning(warnings, root, &warning);
        out.migration_dirs.warnings.push(warning);
    }
}

fn record_default_paths(
    root: &Path,
    warnings: &mut Vec<String>,
    out: &mut SqlxInference,
    synthesize_migration_dir: bool,
) {
    let synthesize_metadata_dir = out.metadata_dir.is_none();
    if synthesize_migration_dir {
        out.migration_dir = Some(InferredSqlxValue::with_source(
            "migrations".into(),
            "SQLx default migrations/".into(),
        ));
    }
    if synthesize_metadata_dir {
        out.metadata_dir = Some(InferredSqlxValue::with_source(
            ".sqlx".into(),
            "SQLx default .sqlx/".into(),
        ));
    }
    let warning = match (synthesize_migration_dir, synthesize_metadata_dir) {
        (true, true) => Some(
            "SQLx was detected but migration and metadata directories were not; using default SQLx paths unless overridden",
        ),
        (true, false) => Some(
            "SQLx was detected but no migration directory was found; using default migrations/ unless overridden",
        ),
        (false, true) => {
            Some("SQLx metadata directory was not detected; using default .sqlx/ unless overridden")
        }
        (false, false) => None,
    };
    if let Some(warning) = warning {
        push_scan_warning(warnings, root, warning);
        if synthesize_migration_dir && let Some(migration_dir) = &mut out.migration_dir {
            migration_dir.warnings.push(warning.into());
        }
        if synthesize_metadata_dir && let Some(metadata_dir) = &mut out.metadata_dir {
            metadata_dir.warnings.push(warning.into());
        }
    }
}

fn record_check_command(root: &Path, warnings: &mut Vec<String>, out: &mut SqlxInference) {
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
    // this command for supported local and CI environments.
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

fn first_text_file_matching<F>(
    root: &Path,
    scan: &RepoScan,
    extensions: &[&str],
    warnings: &mut Vec<String>,
    mut predicate: F,
) -> Option<String>
where
    F: FnMut(&str) -> anyhow::Result<bool>,
{
    for path in scan.files_with_extensions(extensions) {
        match read_limited_text(path) {
            Ok(text) => match predicate(&text) {
                Ok(true) => return Some(relative_source_path(root, path)),
                Ok(false) => {}
                Err(error) => push_scan_warning(
                    warnings,
                    path,
                    &format!("could not parse source for inference: {error:#}"),
                ),
            },
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
