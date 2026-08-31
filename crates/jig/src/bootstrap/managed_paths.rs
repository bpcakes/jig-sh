use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::answers::{HarnessFootprint, RenderAnswers};
use super::path::{
    validate_no_reserved_git_metadata_components, validate_repository_relative_ancestors,
};
pub(super) const ROOT_AGENTS_PATH: &str = "AGENTS.md";
pub(super) const ROOT_AGENTS_BLOCK_BEGIN: &str = "<!-- BEGIN JIG MANAGED BLOCK -->";
pub(super) const ROOT_AGENTS_BLOCK_END: &str = "<!-- END JIG MANAGED BLOCK -->";
pub(super) const ROOT_GITATTRIBUTES_PATH: &str = ".gitattributes";
pub(super) const ROOT_GITATTRIBUTES_BLOCK_BEGIN: &str = "# BEGIN JIG MANAGED BLOCK";
pub(super) const ROOT_GITATTRIBUTES_BLOCK_END: &str = "# END JIG MANAGED BLOCK";
pub(super) const ROOT_GITIGNORE_PATH: &str = ".gitignore";
pub(super) const ROOT_GITIGNORE_BLOCK_BEGIN: &str = "# BEGIN JIG MANAGED BLOCK";
pub(super) const ROOT_GITIGNORE_BLOCK_END: &str = "# END JIG MANAGED BLOCK";
pub(super) const AGENT_MAP_PATH: &str = "agent-map.md";
pub(super) const MANIFEST_PATH: &str = ".agent/jig-managed-paths.json";
const MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub(super) struct ManagedBlockSpec {
    pub(super) path: &'static str,
    pub(super) begin: &'static str,
    pub(super) end: &'static str,
    pub(super) progress_label: &'static str,
}

const MANAGED_BLOCK_SPECS: &[ManagedBlockSpec] = &[
    ManagedBlockSpec {
        path: ROOT_AGENTS_PATH,
        begin: ROOT_AGENTS_BLOCK_BEGIN,
        end: ROOT_AGENTS_BLOCK_END,
        progress_label: "root guide",
    },
    ManagedBlockSpec {
        path: ROOT_GITATTRIBUTES_PATH,
        begin: ROOT_GITATTRIBUTES_BLOCK_BEGIN,
        end: ROOT_GITATTRIBUTES_BLOCK_END,
        progress_label: "git attributes",
    },
    ManagedBlockSpec {
        path: ROOT_GITIGNORE_PATH,
        begin: ROOT_GITIGNORE_BLOCK_BEGIN,
        end: ROOT_GITIGNORE_BLOCK_END,
        progress_label: "git ignore",
    },
];

const WEB_MANAGED_PATHS: &[&str] = &[
    ".github/workflows/webapp-checks.yml",
    "scripts/check-webapp-scripts.mjs",
    "scripts/check-webapps.sh",
    "scripts/enforce-coverage.cjs",
    "scripts/web-node.cjs",
];

/// Paths rendered for `harness_footprint = "minimal"` (plus block-managed gitignore/gitattributes).
const MINIMAL_MANAGED_PATHS: &[&str] = &[
    ".jig.toml",
    ".agent/jig-contract.json",
    ".agent/PLANS.md",
    ".agent/plans/.gitkeep",
    ".agent/state/.gitkeep",
    ".agent/.cache/.gitignore",
    MANIFEST_PATH,
    ROOT_GITATTRIBUTES_PATH,
    ROOT_GITIGNORE_PATH,
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManagedPathsManifest {
    version: u32,
    paths: Vec<String>,
}

pub(super) fn load_manifest(root: &Path) -> Result<Option<BTreeSet<PathBuf>>> {
    validate_repository_relative_ancestors(root, Path::new(MANIFEST_PATH))?;
    let path = root.join(MANIFEST_PATH);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to stat {}", path.display()));
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!(
            "Invalid Jig managed-path manifest {}: expected a regular file, not a symlink or directory",
            path.display()
        );
    }
    let contents = fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest: ManagedPathsManifest = serde_json::from_slice(&contents)
        .with_context(|| format!("Invalid Jig managed-path manifest {}", path.display()))?;
    validate_manifest(&manifest, &path)?;
    Ok(Some(
        manifest.paths.into_iter().map(PathBuf::from).collect(),
    ))
}

pub(super) fn write_manifest(root: &Path, active_paths: &BTreeSet<PathBuf>) -> Result<()> {
    let manifest_path = PathBuf::from(MANIFEST_PATH);
    if !active_paths.contains(&manifest_path) {
        bail!("Internal error: managed-path manifest does not list itself");
    }
    let paths = active_paths
        .iter()
        .map(|path| manifest_path_string(path))
        .collect::<Result<Vec<_>>>()?;
    let manifest = ManagedPathsManifest {
        version: MANIFEST_VERSION,
        paths,
    };
    let mut contents = serde_json::to_vec_pretty(&manifest)?;
    contents.push(b'\n');
    let path = root.join(MANIFEST_PATH);
    validate_repository_relative_ancestors(root, Path::new(MANIFEST_PATH))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Managed-path manifest has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    validate_repository_relative_ancestors(root, Path::new(MANIFEST_PATH))?;
    fs::write(&path, contents).with_context(|| format!("Failed to write {}", path.display()))
}

fn validate_manifest(manifest: &ManagedPathsManifest, path: &Path) -> Result<()> {
    if manifest.version != MANIFEST_VERSION {
        bail!(
            "Invalid Jig managed-path manifest {}: unsupported version {} (expected {})",
            path.display(),
            manifest.version,
            MANIFEST_VERSION
        );
    }
    let mut previous: Option<&str> = None;
    for entry in &manifest.paths {
        validate_manifest_path(entry, path)?;
        if let Some(previous) = previous {
            if previous == entry {
                bail!(
                    "Invalid Jig managed-path manifest {}: duplicate path {entry:?}",
                    path.display()
                );
            }
            if previous > entry.as_str() {
                bail!(
                    "Invalid Jig managed-path manifest {}: paths must be sorted",
                    path.display()
                );
            }
        }
        previous = Some(entry);
    }
    if manifest
        .paths
        .binary_search_by(|entry| entry.as_str().cmp(MANIFEST_PATH))
        .is_err()
    {
        bail!(
            "Invalid Jig managed-path manifest {}: manifest must list {MANIFEST_PATH:?}",
            path.display()
        );
    }
    Ok(())
}

fn validate_manifest_path(entry: &str, manifest_path: &Path) -> Result<()> {
    if entry.is_empty() {
        bail!(
            "Invalid Jig managed-path manifest {}: unsafe path {entry:?}",
            manifest_path.display()
        );
    }
    let path = Path::new(entry);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Invalid Jig managed-path manifest {}: unsafe path {entry:?}",
            manifest_path.display()
        );
    }
    if let Err(error) = validate_no_reserved_git_metadata_components(path) {
        bail!(
            "Invalid Jig managed-path manifest {}: path {entry:?} is unsafe: {error}",
            manifest_path.display()
        );
    }
    if entry.contains('\\') {
        bail!(
            "Invalid Jig managed-path manifest {}: unsafe path {entry:?}",
            manifest_path.display()
        );
    }
    let canonical = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().unwrap_or_default(),
            _ => "",
        })
        .collect::<Vec<_>>()
        .join("/");
    if canonical != entry {
        bail!(
            "Invalid Jig managed-path manifest {}: non-canonical path {entry:?}",
            manifest_path.display()
        );
    }
    Ok(())
}

fn manifest_path_string(path: &Path) -> Result<String> {
    let value = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("Managed path {} is not UTF-8", path.display())),
            _ => Err(anyhow::anyhow!(
                "Managed path {} is not repository-relative",
                path.display()
            )),
        })
        .collect::<Result<Vec<_>>>()?
        .join("/");
    validate_manifest_path(&value, Path::new(MANIFEST_PATH))?;
    Ok(value)
}

pub(super) fn should_omit_unmanaged_rendered_path(
    relative: &Path,
    answers: &RenderAnswers,
) -> bool {
    if (relative == Path::new(super::staged_render::FILE_BUDGET_POLICY_PATH)
        && answers.is_minimal_footprint())
        || relative == Path::new("Makefile")
        || (answers.frontend_apps().is_empty() && is_web_managed_path(relative))
        || (!answers.rust_ci_workflow_enabled()
            && relative == Path::new(".github/workflows/rust-tests.yml"))
        || (!answers.go_ci_workflow_enabled()
            && relative == Path::new(".github/workflows/go-tests.yml"))
        || (!(answers.go_ci_workflow_enabled()
            || answers.rust_backend_enabled()
            || answers.sqlx_enabled()
            || answers.go_postgres_enabled()
            || answers.file_budget_ci_enabled())
            && relative == Path::new(".github/workflows/repo-policy.yml"))
    {
        return true;
    }
    answers.harness_footprint() == HarnessFootprint::Minimal && !is_minimal_managed_path(relative)
}

pub(super) fn is_minimal_managed_path(relative: &Path) -> bool {
    MINIMAL_MANAGED_PATHS
        .iter()
        .any(|path| relative == Path::new(path))
}

fn is_web_managed_path(relative: &Path) -> bool {
    WEB_MANAGED_PATHS
        .iter()
        .any(|web_path| relative == Path::new(web_path))
}

pub(super) fn managed_block_spec(relative: &Path) -> Option<ManagedBlockSpec> {
    managed_block_specs()
        .iter()
        .find(|spec| relative == Path::new(spec.path))
        .copied()
}

pub(super) const fn managed_block_specs() -> &'static [ManagedBlockSpec] {
    MANAGED_BLOCK_SPECS
}

pub(super) fn is_executable_script(relative: &Path) -> bool {
    relative.starts_with("scripts")
        && (relative.extension().and_then(|ext| ext.to_str()) == Some("sh")
            || relative.file_name().and_then(|name| name.to_str()) == Some("jig"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_raw_manifest(root: &Path, contents: &str) {
        let path = root.join(MANIFEST_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn managed_path_manifest_requires_strict_sorted_safe_paths() {
        let cases = [
            (
                "unknown field",
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json"],"extra":true}"#,
            ),
            (
                "unsupported version",
                r#"{"version":2,"paths":[".agent/jig-managed-paths.json"]}"#,
            ),
            (
                "traversal",
                r#"{"version":1,"paths":["../outside",".agent/jig-managed-paths.json"]}"#,
            ),
            (
                "non-canonical",
                r#"{"version":1,"paths":[".agent//jig-managed-paths.json"]}"#,
            ),
            (
                "duplicate",
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json",".agent/jig-managed-paths.json"]}"#,
            ),
            (
                "unsorted",
                r#"{"version":1,"paths":["z",".agent/jig-managed-paths.json"]}"#,
            ),
            ("missing self", r#"{"version":1,"paths":[".jig.toml"]}"#),
        ];

        for (label, contents) in cases {
            let root = tempdir().unwrap();
            write_raw_manifest(root.path(), contents);
            let error = load_manifest(root.path()).unwrap_err().to_string();
            assert!(
                error.contains("Invalid Jig managed-path manifest"),
                "{label}: {error}"
            );
        }
    }

    #[test]
    fn managed_path_manifest_rejects_reserved_git_metadata_components() {
        let cases = [
            (
                "root",
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json",".git"]}"#,
            ),
            (
                "nested",
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json","vendor/.git/config"]}"#,
            ),
            (
                "mixed case",
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json","vendor/.GiT/config"]}"#,
            ),
            (
                "HFS ignored codepoint",
                "{\"version\":1,\"paths\":[\".agent/jig-managed-paths.json\",\".g\u{200c}it/config\"]}",
            ),
        ];

        for (label, contents) in cases {
            let root = tempdir().unwrap();
            write_raw_manifest(root.path(), contents);

            let error = load_manifest(root.path()).unwrap_err().to_string();

            assert!(
                error.contains("reserved Git metadata component"),
                "{label}: {error}"
            );
            assert!(
                error.contains("Invalid Jig managed-path manifest"),
                "{label}: {error}"
            );
            assert!(error.contains(".git"), "{label}: {error}");
            assert!(
                !error.to_ascii_lowercase().contains("--force"),
                "{label}: {error}"
            );
        }
    }

    #[test]
    fn managed_path_manifest_allows_git_near_misses() {
        let root = tempdir().unwrap();
        write_raw_manifest(
            root.path(),
            "{\"version\":1,\"paths\":[\".agent/jig-managed-paths.json\",\".git .config\",\".git..keep\",\".github/workflows/check.yml\",\".gitignore\",\".gitkeep\",\".gitx. \",\".git\u{a0}\",\"git/config\",\"vendor/git/config\"]}",
        );

        let paths = load_manifest(root.path()).unwrap().unwrap();

        assert!(paths.contains(Path::new(".github/workflows/check.yml")));
        assert!(paths.contains(Path::new(".gitignore")));
        assert!(paths.contains(Path::new(".gitkeep")));
        assert!(paths.contains(Path::new(".git .config")));
        assert!(paths.contains(Path::new(".git..keep")));
        assert!(paths.contains(Path::new(".gitx. ")));
        assert!(paths.contains(Path::new(".git\u{a0}")));
        assert!(paths.contains(Path::new("git/config")));
        assert!(paths.contains(Path::new("vendor/git/config")));

        for relative in [
            ".git\u{200b}",
            ".gi\u{200b}t",
            ".git\u{2029}",
            ".git\u{2060}",
            ".git\u{2069}",
        ] {
            let root = tempdir().unwrap();
            let expected = BTreeSet::from([PathBuf::from(MANIFEST_PATH), PathBuf::from(relative)]);
            write_manifest(root.path(), &expected).unwrap();
            assert_eq!(load_manifest(root.path()).unwrap(), Some(expected));
        }
    }

    #[test]
    fn managed_path_manifest_write_rejects_reserved_git_metadata_components() {
        for relative in [
            ".git",
            "vendor/.GIT/config",
            ".g\u{200c}it/config",
            "\u{feff}.G\u{202e}i\u{206a}T/config",
        ] {
            let root = tempdir().unwrap();
            write_raw_manifest(
                root.path(),
                r#"{"version":1,"paths":[".agent/jig-managed-paths.json"]}"#,
            );
            let manifest_path = root.path().join(MANIFEST_PATH);
            let before = fs::read(&manifest_path).unwrap();
            let paths = BTreeSet::from([PathBuf::from(MANIFEST_PATH), PathBuf::from(relative)]);

            let error = write_manifest(root.path(), &paths).unwrap_err().to_string();

            assert!(
                error.contains("Invalid Jig managed-path manifest"),
                "{relative}: {error}"
            );
            assert!(
                error.contains("reserved Git metadata component"),
                "{relative}: {error}"
            );
            assert!(error.contains(relative), "{relative}: {error}");
            assert_eq!(fs::read(&manifest_path).unwrap(), before, "{relative}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn managed_path_manifest_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let target = root.path().join("manifest-target.json");
        fs::write(
            &target,
            r#"{"version":1,"paths":[".agent/jig-managed-paths.json"]}"#,
        )
        .unwrap();
        let path = root.path().join(MANIFEST_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink(&target, &path).unwrap();

        let error = load_manifest(root.path()).unwrap_err().to_string();
        assert!(error.contains("expected a regular file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn managed_path_manifest_rejects_symlinked_agent_ancestor() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(
            outside.path().join("jig-managed-paths.json"),
            r#"{"version":1,"paths":[".agent/jig-managed-paths.json"]}"#,
        )
        .unwrap();
        symlink(outside.path(), root.path().join(".agent")).unwrap();

        let error = load_manifest(root.path()).unwrap_err().to_string();
        assert!(error.contains("ancestor"), "{error}");
        assert!(error.contains("is a symlink"), "{error}");
    }

    #[test]
    fn managed_path_manifest_round_trips_canonical_paths() {
        let root = tempdir().unwrap();
        let paths = BTreeSet::from([
            PathBuf::from(MANIFEST_PATH),
            PathBuf::from(".agent/jig-contract.json"),
            PathBuf::from("scripts/jig"),
        ]);

        write_manifest(root.path(), &paths).unwrap();

        assert_eq!(load_manifest(root.path()).unwrap(), Some(paths));
    }
}
