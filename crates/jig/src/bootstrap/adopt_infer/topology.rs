use std::fs;
use std::path::Path;

use serde_json::{Value as JsonValue, json};

use super::scan::{RepoScan, push_scan_warning, read_toml_for_inference, relative_path_string};
use crate::bootstrap::crate_classification::non_production_crate_reason;

#[derive(Clone, Debug, Default)]
pub(super) struct RepoTopology {
    rust_crates: Vec<RustCrateTopology>,
}

#[derive(Clone, Debug)]
struct RustCrateTopology {
    name: String,
    dir: String,
    kind: RustCrateKind,
    role: RustCrateRole,
    targets: Vec<String>,
    owner_guide: Option<String>,
    guide_action: CrateGuideAction,
    source: String,
}

#[derive(Clone, Debug)]
enum RustCrateKind {
    Binary,
    Library,
    Mixed,
    Unknown,
}

#[derive(Clone, Debug)]
enum RustCrateRole {
    AppService,
    Support,
    ExampleFixtureTest,
}

#[derive(Clone, Debug)]
enum CrateGuideAction {
    Existing,
    MissingProjectOwned,
    SkipNonProduction(String),
    NotDirectConfiguredCrate,
}

pub(super) fn infer_repo_topology(
    root: &Path,
    scan: &RepoScan,
    rust_crate_roots: &[String],
    warnings: &mut Vec<String>,
) -> RepoTopology {
    let mut rust_crates = Vec::new();
    for cargo_path in scan.named_files("Cargo.toml") {
        let Some(parsed) = read_toml_for_inference(cargo_path, warnings) else {
            continue;
        };
        let Some(package) = parsed.get("package").and_then(toml::Value::as_table) else {
            continue;
        };
        let crate_dir = cargo_path.parent().unwrap_or(root);
        let relative_dir = relative_path_string(crate_dir.strip_prefix(root).unwrap_or(crate_dir));
        let dir = if relative_dir.is_empty() {
            ".".into()
        } else {
            relative_dir
        };
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                crate_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("crate")
                    .to_string()
            });
        let targets = crate_targets(crate_dir, &parsed, warnings);
        let kind = crate_kind(&targets);
        let relative_path = Path::new(&dir);
        let skip_reason = non_production_crate_reason(relative_path, Some(&name));
        let role = if skip_reason.is_some() {
            RustCrateRole::ExampleFixtureTest
        } else if matches!(kind, RustCrateKind::Binary | RustCrateKind::Mixed) {
            RustCrateRole::AppService
        } else {
            RustCrateRole::Support
        };
        let guide_action = guide_action(root, crate_dir, rust_crate_roots, skip_reason);
        rust_crates.push(RustCrateTopology {
            name,
            dir,
            kind,
            role,
            targets,
            owner_guide: nearest_agent_guide(root, crate_dir),
            guide_action,
            source: format!(
                "{} [package]",
                relative_path_string(cargo_path.strip_prefix(root).unwrap_or(cargo_path))
            ),
        });
    }
    rust_crates.sort_by(|left, right| left.dir.cmp(&right.dir).then(left.name.cmp(&right.name)));
    RepoTopology { rust_crates }
}

impl RepoTopology {
    pub(super) fn report(&self) -> JsonValue {
        json!({
            "rust_crates": self
                .rust_crates
                .iter()
                .map(RustCrateTopology::report)
                .collect::<Vec<_>>(),
        })
    }
}

impl RustCrateTopology {
    fn report(&self) -> JsonValue {
        json!({
            "name": self.name,
            "dir": self.dir,
            "kind": self.kind.as_str(),
            "role": self.role.as_str(),
            "targets": self.targets,
            "owner_guide": self.owner_guide,
            "guide_action": self.guide_action.as_str(),
            "guide_action_reason": self.guide_action.reason(),
            "source": self.source,
        })
    }
}

impl RustCrateKind {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Library => "library",
            Self::Mixed => "mixed",
            Self::Unknown => "unknown",
        }
    }
}

impl RustCrateRole {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::AppService => "app/service",
            Self::Support => "support",
            Self::ExampleFixtureTest => "example/fixture/test",
        }
    }
}

impl CrateGuideAction {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::MissingProjectOwned => "missing_project_owned",
            Self::SkipNonProduction(_) => "skip_non_production",
            Self::NotDirectConfiguredCrate => "not_direct_configured_crate",
        }
    }

    fn reason(&self) -> Option<&str> {
        match self {
            Self::SkipNonProduction(reason) => Some(reason),
            _ => None,
        }
    }
}

fn crate_targets(
    crate_dir: &Path,
    cargo_toml: &toml::Value,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut targets = Vec::new();
    if cargo_toml.get("lib").is_some() || crate_dir.join("src/lib.rs").is_file() {
        targets.push("lib".into());
    }
    if cargo_toml.get("bin").is_some() || crate_dir.join("src/main.rs").is_file() {
        targets.push("bin".into());
    }
    let src_bin = crate_dir.join("src/bin");
    if src_bin.is_dir() && dir_contains_rs_file(&src_bin, warnings) {
        targets.push("bin".into());
    }
    targets.sort();
    targets.dedup();
    targets
}

fn crate_kind(targets: &[String]) -> RustCrateKind {
    let has_lib = targets.iter().any(|target| target == "lib");
    let has_bin = targets.iter().any(|target| target == "bin");
    match (has_lib, has_bin) {
        (true, true) => RustCrateKind::Mixed,
        (false, true) => RustCrateKind::Binary,
        (true, false) => RustCrateKind::Library,
        (false, false) => RustCrateKind::Unknown,
    }
}

fn guide_action(
    root: &Path,
    crate_dir: &Path,
    rust_crate_roots: &[String],
    skip_reason: Option<String>,
) -> CrateGuideAction {
    if let Some(reason) = skip_reason {
        return CrateGuideAction::SkipNonProduction(reason);
    }
    if crate_dir.join("AGENTS.md").is_file() {
        return CrateGuideAction::Existing;
    }
    if is_direct_child_of_configured_crate_root(root, crate_dir, rust_crate_roots) {
        return CrateGuideAction::MissingProjectOwned;
    }
    CrateGuideAction::NotDirectConfiguredCrate
}

fn is_direct_child_of_configured_crate_root(
    root: &Path,
    crate_dir: &Path,
    rust_crate_roots: &[String],
) -> bool {
    let Some(parent) = crate_dir.parent() else {
        return false;
    };
    rust_crate_roots
        .iter()
        .map(|crate_root| root.join(crate_root))
        .any(|crate_root| parent == crate_root)
}

fn nearest_agent_guide(root: &Path, crate_dir: &Path) -> Option<String> {
    let mut current = crate_dir;
    loop {
        let guide = current.join("AGENTS.md");
        if guide.is_file() {
            return Some(relative_path_string(
                guide.strip_prefix(root).unwrap_or(&guide),
            ));
        }
        if current == root {
            return None;
        }
        current = current.parent()?;
    }
}

fn dir_contains_rs_file(dir: &Path, warnings: &mut Vec<String>) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            push_scan_warning(
                warnings,
                dir,
                &format!("could not read src/bin for crate target inference: {error}"),
            );
            return false;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_scan_warning(
                    warnings,
                    dir,
                    &format!("could not read src/bin entry for crate target inference: {error}"),
                );
                continue;
            }
        };
        match entry.file_type() {
            Ok(file_type)
                if file_type.is_file()
                    && entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs") =>
            {
                return true;
            }
            Ok(_) => {}
            Err(error) => push_scan_warning(
                warnings,
                &entry.path(),
                &format!("could not inspect src/bin entry for crate target inference: {error}"),
            ),
        }
    }
    false
}
