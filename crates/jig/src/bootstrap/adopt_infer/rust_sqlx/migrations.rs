//! Migration-path discovery, kept separate from SQLx identification.
//!
//! Numbered SQL files are shared by many migration tools, so they never enable
//! SQLx. Once SQLx is established, a candidate is usable only when it belongs
//! to a Rust owner and carries no other tool's markers; anything else is either
//! attributed elsewhere or reported as ambiguous.

use std::collections::BTreeSet;
use std::path::Path;

use super::super::scan::{RepoScan, read_limited_text, relative_path_string};

const MAX_MIGRATION_SQL_DEPTH: usize = 3;
const SQLX_DEFAULT_MIGRATION_DIR: &str = "migrations";

#[derive(Clone, Debug, Eq, PartialEq)]
enum MigrationOwner {
    Rust(String),
    Go(String),
    RustAndGo(String),
    Unowned,
}

#[derive(Clone, Debug)]
pub(in crate::bootstrap::adopt_infer) struct MigrationCandidate {
    pub(in crate::bootstrap::adopt_infer) dir: String,
    pub(in crate::bootstrap::adopt_infer) source: String,
    owner: MigrationOwner,
    goose: bool,
    owner_declares_sqlx: bool,
}

/// Where SQLx migrations live once SQLx is established, independent of
/// whether the repository scan found SQLx evidence.
#[derive(Clone, Debug, Default)]
pub(in crate::bootstrap::adopt_infer) enum MigrationChoice {
    #[default]
    NoCandidates,
    Selected(MigrationCandidate),
    Ambiguous(Vec<MigrationCandidate>),
}

pub(super) struct MigrationSurvey {
    pub(super) choice: MigrationChoice,
    /// Candidates that could hold SQLx migrations, in path order.
    pub(super) sqlx_candidates: Vec<MigrationCandidate>,
    /// Candidates attributed to another owner or tool.
    pub(super) excluded: Vec<MigrationCandidate>,
}

enum Role {
    Eligible,
    Unresolved,
    Excluded,
}

impl MigrationCandidate {
    fn role(&self, repository_has_go: bool) -> Role {
        if self.goose {
            return Role::Excluded;
        }
        match self.owner {
            MigrationOwner::Rust(_) => Role::Eligible,
            MigrationOwner::Go(_) => Role::Excluded,
            MigrationOwner::RustAndGo(_) => Role::Unresolved,
            MigrationOwner::Unowned if repository_has_go => Role::Unresolved,
            MigrationOwner::Unowned => Role::Eligible,
        }
    }

    pub(in crate::bootstrap::adopt_infer) fn describe(&self) -> String {
        let owner = if self.goose {
            "contains Goose migration annotations".to_string()
        } else {
            match &self.owner {
                MigrationOwner::Rust(root) if self.owner_declares_sqlx => {
                    format!("Cargo manifest at {root} declares sqlx")
                }
                MigrationOwner::Rust(root) => format!("inside Cargo manifest at {root}"),
                MigrationOwner::Go(root) => format!("inside Go module {root}"),
                MigrationOwner::RustAndGo(root) => {
                    format!("{root} has both Cargo.toml and go.mod")
                }
                MigrationOwner::Unowned => "no owning Cargo or Go manifest".to_string(),
            }
        };
        format!("{} ({owner})", self.dir)
    }
}

impl MigrationChoice {
    pub(in crate::bootstrap::adopt_infer) fn selected_dir(&self) -> Option<&str> {
        match self {
            Self::Selected(candidate) => Some(candidate.dir.as_str()),
            Self::NoCandidates | Self::Ambiguous(_) => None,
        }
    }

    pub(in crate::bootstrap::adopt_infer) fn ambiguity(&self) -> Option<String> {
        let Self::Ambiguous(candidates) = self else {
            return None;
        };
        Some(format!(
            "cannot infer the SQLx migration directory from {}; pass --rust-migration-dir <dir> (migration_dir in an answers file) to choose it, or --sqlx-enabled false if SQLx does not own these migrations",
            candidates
                .iter()
                .map(MigrationCandidate::describe)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// `sqlx_manifest_dirs` are repository-relative directories (`.` for the
/// root) whose Cargo.toml declares a sqlx dependency.
pub(super) fn survey_migration_dirs(
    root: &Path,
    scan: &RepoScan,
    sqlx_manifest_dirs: &BTreeSet<String>,
) -> MigrationSurvey {
    let rust_dirs = manifest_dirs(root, scan, "Cargo.toml");
    let go_dirs = manifest_dirs(root, scan, "go.mod");
    let mut sqlx_candidates = Vec::new();
    let mut excluded = Vec::new();
    let mut unresolved = false;
    for path in scan.dirs_named("migrations") {
        let Some(source_path) = migration_dir_sql_source(path, scan) else {
            continue;
        };
        let dir = relative_dir(root, path);
        let owner = owner_of(&dir, &rust_dirs, &go_dirs);
        let owner_declares_sqlx = match &owner {
            MigrationOwner::Rust(owner) | MigrationOwner::RustAndGo(owner) => {
                sqlx_manifest_dirs.contains(owner)
            }
            MigrationOwner::Go(_) | MigrationOwner::Unowned => false,
        };
        let candidate = MigrationCandidate {
            dir,
            source: relative_path_string(source_path.strip_prefix(root).unwrap_or(source_path)),
            owner,
            goose: sql_declares_goose(source_path),
            owner_declares_sqlx,
        };
        match candidate.role(!go_dirs.is_empty()) {
            Role::Eligible => sqlx_candidates.push(candidate),
            Role::Unresolved => {
                unresolved = true;
                sqlx_candidates.push(candidate);
            }
            Role::Excluded => excluded.push(candidate),
        }
    }
    sqlx_candidates.sort_by(|left, right| left.dir.cmp(&right.dir));
    excluded.sort_by(|left, right| left.dir.cmp(&right.dir));
    let choice = choose(&sqlx_candidates, &excluded, unresolved);
    MigrationSurvey {
        choice,
        sqlx_candidates,
        excluded,
    }
}

fn choose(
    sqlx_candidates: &[MigrationCandidate],
    excluded: &[MigrationCandidate],
    unresolved: bool,
) -> MigrationChoice {
    if unresolved {
        return MigrationChoice::Ambiguous(sqlx_candidates.to_vec());
    }
    match sqlx_candidates {
        [] => {
            // The synthesized SQLx default must not silently adopt another
            // tool's migrations that happen to live at the default path.
            let default_owner = excluded
                .iter()
                .filter(|candidate| candidate.dir == SQLX_DEFAULT_MIGRATION_DIR)
                .cloned()
                .collect::<Vec<_>>();
            if default_owner.is_empty() {
                MigrationChoice::NoCandidates
            } else {
                MigrationChoice::Ambiguous(default_owner)
            }
        }
        [candidate] => MigrationChoice::Selected(candidate.clone()),
        candidates => {
            let justified = candidates
                .iter()
                .filter(|candidate| candidate.owner_declares_sqlx)
                .collect::<Vec<_>>();
            match justified.as_slice() {
                [candidate] => MigrationChoice::Selected((*candidate).clone()),
                _ => MigrationChoice::Ambiguous(candidates.to_vec()),
            }
        }
    }
}

fn manifest_dirs(root: &Path, scan: &RepoScan, name: &str) -> BTreeSet<String> {
    scan.named_files(name)
        .map(|path| relative_dir(root, path.parent().unwrap_or(root)))
        .collect()
}

fn owner_of(dir: &str, rust_dirs: &BTreeSet<String>, go_dirs: &BTreeSet<String>) -> MigrationOwner {
    for ancestor in Path::new(dir).ancestors() {
        let key = if ancestor.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative_path_string(ancestor)
        };
        match (rust_dirs.contains(&key), go_dirs.contains(&key)) {
            (true, true) => return MigrationOwner::RustAndGo(key),
            (true, false) => return MigrationOwner::Rust(key),
            (false, true) => return MigrationOwner::Go(key),
            (false, false) => {}
        }
    }
    MigrationOwner::Unowned
}

pub(super) fn relative_dir(root: &Path, path: &Path) -> String {
    let relative = relative_path_string(path.strip_prefix(root).unwrap_or(path));
    if relative.is_empty() {
        ".".into()
    } else {
        relative
    }
}

fn sql_declares_goose(path: &Path) -> bool {
    // Goose requires `-- +goose Up` style annotations in every SQL migration.
    read_limited_text(path).is_ok_and(|text| {
        text.lines().any(|line| {
            line.trim_start()
                .strip_prefix("--")
                .is_some_and(|rest| rest.trim_start().starts_with("+goose"))
        })
    })
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
