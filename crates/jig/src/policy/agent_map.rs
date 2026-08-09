use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::agent_guides::is_ignored_guide_component;
use crate::bootstrap::path::{validate_repository_regular_file_leaf, write_repository_file_atomic};
use crate::context::RepoContext;
use crate::policy::AgentMapInput;

pub(super) fn generate(ctx: &RepoContext, opts: &AgentMapInput) -> Result<Value> {
    let map_path = normalize_map_path(&opts.map_path)?;
    write(ctx.root(), &map_path)?;
    Ok(json!({ "ok": true, "path": map_path }))
}

pub(super) fn check(ctx: &RepoContext, opts: &AgentMapInput) -> Result<Value> {
    let map_path = normalize_map_path(&opts.map_path)?;
    let result = validate(ctx.root(), &map_path)?;
    Ok(json!({
        "ok": result.ok(),
        "agents": result.agent_count,
        "missing_agents": result.missing_agents,
        "broken_links": result.broken_links,
    }))
}

pub(crate) fn write(root: &Path, map_path: &Path) -> Result<()> {
    let body = render(root, map_path)?;
    write_rendered(root, map_path, &body)
}

pub(crate) fn render(root: &Path, map_path: &Path) -> Result<Vec<u8>> {
    // Normalize here as the boundary guard for both CLI generation and
    // renderer post-processing callers.
    let _map_path = normalize_map_path(map_path)?;
    let guides = list_guides(root)?;
    let mut body = String::new();
    body.push_str("# Agent Map\n\n");
    body.push_str("Fast jump index for agent-facing guidance in this repository.\n\n");
    body.push_str("## Root guide\n\n");
    body.push_str("- [Repository AGENTS.md](./AGENTS.md)\n\n");
    body.push_str("## Nested guides\n\n");
    let nested = guides.iter().filter(|path| path.as_str() != "AGENTS.md");
    let mut nested_count = 0usize;
    for guide in nested {
        nested_count += 1;
        let label = guide.trim_end_matches("/AGENTS.md");
        let _ = writeln!(body, "- [{label}](./{guide})");
    }
    if nested_count == 0 {
        body.push_str("_None yet_\n");
    }
    body.push_str("\n## Suggested usage pattern\n\n");
    body.push_str("1. Start with the root [AGENTS.md](./AGENTS.md).\n");
    body.push_str("2. Open the nearest guide for the area you will change.\n");
    body.push_str("3. Follow that guide's entrypoint map before editing.\n");
    Ok(body.into_bytes())
}

pub(crate) fn write_rendered(root: &Path, map_path: &Path, body: &[u8]) -> Result<()> {
    let map_path = normalize_map_path(map_path)?;
    let expected_leaf = validate_repository_regular_file_leaf(root, &map_path)?;
    write_repository_file_atomic(root, &map_path, body, expected_leaf).map(|_| ())
}

pub(super) fn check_guides(ctx: &RepoContext) -> Result<Value> {
    // Crate guides intentionally use this exact repo-wide heading contract so
    // agents can scan every crate guide without learning local synonyms.
    let required = [
        "## Purpose",
        "## Key entrypoints",
        "## Edit here for X",
        "## Invariants",
        "## Common commands",
    ];
    let mut missing_sections = Vec::new();
    let mut missing_entry_ref = Vec::new();
    let mut guide_count = 0usize;
    for root in ctx.rust_crate_roots() {
        let crate_root = ctx.root().join(root);
        if !crate_root.is_dir() {
            continue;
        }
        // Crate roots contain first-level crates; deeper AGENTS.md files are
        // covered by agent-map link validation rather than crate-guide policy.
        for entry in sorted_dirs(&crate_root)? {
            let guide = entry.join("AGENTS.md");
            let rel = relative_string(ctx.root(), &guide)?;
            if !guide.exists() {
                continue;
            }
            guide_count += 1;
            let text = fs::read_to_string(&guide)?;
            for section in required {
                if !text.lines().any(|line| line.trim_end() == section) {
                    missing_sections.push(format!("{rel}: missing section '{section}'"));
                }
            }
            if !text.contains("`src/lib.rs`") && !text.contains("`src/main.rs`") {
                missing_entry_ref.push(format!(
                    "{rel}: missing src/lib.rs or src/main.rs entrypoint reference"
                ));
            }
        }
    }
    Ok(json!({
        "ok": missing_sections.is_empty() && missing_entry_ref.is_empty(),
        "guide_count": guide_count,
        "missing_guides": [],
        "missing_guides_note": "placeholder crate-level AGENTS.md files are no longer required; existing guides are validated when present",
        "missing_sections": missing_sections,
        "missing_entry_ref": missing_entry_ref,
    }))
}

struct CheckResult {
    agent_count: usize,
    missing_agents: Vec<String>,
    broken_links: Vec<String>,
}

impl CheckResult {
    fn ok(&self) -> bool {
        self.missing_agents.is_empty() && self.broken_links.is_empty()
    }
}

fn validate(root: &Path, map_path: &Path) -> Result<CheckResult> {
    let map_path = normalize_map_path(map_path)?;
    let full_map_path = root.join(&map_path);
    let text = fs::read_to_string(&full_map_path)
        .with_context(|| format!("Failed to read {}", full_map_path.display()))?;
    let map_dir = map_path.parent().unwrap_or_else(|| Path::new(""));
    let linked = markdown_local_links(&text, map_dir);
    let linked_set = linked
        .iter()
        .map(|(_, _, path)| path.clone())
        .collect::<HashSet<_>>();
    let mut broken_links = Vec::new();
    for (line, raw, path) in linked {
        if path.starts_with("../") || path == ".." {
            broken_links.push(format!(
                "{}:{line}: {raw} -> {path} (outside repository)",
                map_path.display()
            ));
        } else if !root.join(&path).exists() {
            broken_links.push(format!("{}:{line}: {raw} -> {path}", map_path.display()));
        }
    }
    let guides = list_guides(root)?;
    let missing_agents = guides
        .iter()
        .filter(|path| !linked_set.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    Ok(CheckResult {
        agent_count: guides.len(),
        missing_agents,
        broken_links,
    })
}

fn normalize_map_path(map_path: &Path) -> Result<PathBuf> {
    super::normalize_repo_relative_path(map_path, "agent map path")
}

fn list_guides(root: &Path) -> Result<Vec<String>> {
    let mut guides = BTreeSet::new();
    if super::git_success(root, &["rev-parse", "--is-inside-work-tree"])? {
        for args in [
            vec!["ls-files", "-z", "--", "*AGENTS.md"],
            vec![
                "ls-files",
                "-z",
                "--others",
                "--exclude-standard",
                "--",
                "*AGENTS.md",
            ],
        ] {
            for path in super::split_nul(&super::git_output(root, &args)?) {
                if path == "AGENTS.md" || path.ends_with("/AGENTS.md") {
                    guides.insert(path);
                }
            }
        }
    } else {
        collect_guides(root, root, &mut guides)?;
    }
    Ok(guides.into_iter().collect())
}

fn collect_guides(root: &Path, current: &Path, guides: &mut BTreeSet<String>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize {}", path.display()))?;
        if relative.components().any(is_ignored_guide_component) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            collect_guides(root, &path, guides)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("AGENTS.md") {
            guides.insert(relative_string(root, &path)?);
        }
    }
    Ok(())
}

fn markdown_local_links(text: &str, map_dir: &Path) -> Vec<(usize, String, String)> {
    let mut links = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(open) = rest.find("](") {
            let after_open = &rest[open + 2..];
            let Some(close) = after_open.find(')') else {
                break;
            };
            let raw = &after_open[..close];
            if let Some(normalized) = normalize_link(raw, map_dir) {
                links.push((line_index + 1, raw.to_string(), normalized));
            }
            rest = &after_open[close + 1..];
        }
    }
    links
}

fn normalize_link(raw: &str, map_dir: &Path) -> Option<String> {
    let mut target = raw.trim();
    if target.is_empty() {
        return None;
    }
    if let Some(stripped) = target.strip_prefix('<') {
        target = stripped.split('>').next().unwrap_or(stripped);
    } else {
        target = target.split_whitespace().next().unwrap_or(target);
    }
    target = target.split('#').next().unwrap_or(target);
    target = target.split('?').next().unwrap_or(target);
    if target.is_empty()
        || target.starts_with('#')
        || target.starts_with("http:")
        || target.starts_with("https:")
        || target.starts_with("mailto:")
    {
        return None;
    }
    let combined = if let Some(stripped) = target.strip_prefix('/') {
        PathBuf::from(stripped)
    } else {
        map_dir.join(target)
    };
    Some(normalize_relative_path(&combined))
}

// Keep the explicit current-directory case distinct from unsupported/root
// components even though both intentionally leave the relative stack unchanged.
#[allow(clippy::match_same_arms)]
fn normalize_relative_path(path: &Path) -> String {
    let mut stack = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => stack.push(part.to_string_lossy().to_string()),
            Component::ParentDir => {
                if stack.last().is_some_and(|last| last != "..") {
                    stack.pop();
                } else {
                    stack.push("..".into());
                }
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        ".".into()
    } else {
        stack.join("/")
    }
}

fn sorted_dirs(path: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn relative_string(root: &Path, path: &Path) -> Result<String> {
    Ok(normalize_relative_path(path.strip_prefix(root)?))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    #[cfg(unix)]
    use super::write;
    use super::{check_guides, normalize_map_path, validate};
    use crate::context::RepoContext;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn normalize_map_path_accepts_repo_relative_paths() {
        assert_eq!(
            normalize_map_path(Path::new("./docs/agent-map.md")).unwrap(),
            Path::new("docs/agent-map.md")
        );
    }

    #[test]
    fn normalize_map_path_rejects_parent_traversal() {
        let error = normalize_map_path(Path::new("../agent-map.md")).unwrap_err();

        assert!(error.to_string().contains("inside the repository"));
    }

    #[test]
    fn normalize_map_path_rejects_absolute_paths() {
        let error = normalize_map_path(Path::new("/tmp/agent-map.md")).unwrap_err();

        assert!(error.to_string().contains("repository-relative"));
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_symlink_leaf_and_ancestor_without_writing_outside_root() {
        use std::os::unix::fs::symlink;

        let leaf_root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_map = outside.path().join("outside-map.md");
        fs::write(&outside_map, "outside sentinel\n").unwrap();
        symlink(&outside_map, leaf_root.path().join("agent-map.md")).unwrap();

        let error = write(leaf_root.path(), Path::new("agent-map.md"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("is a symlink"), "{error}");
        assert_eq!(
            fs::read_to_string(&outside_map).unwrap(),
            "outside sentinel\n"
        );

        let ancestor_root = tempdir().unwrap();
        let outside_docs = outside.path().join("docs");
        fs::create_dir(&outside_docs).unwrap();
        symlink(&outside_docs, ancestor_root.path().join("docs")).unwrap();

        let error = write(ancestor_root.path(), Path::new("docs/agent-map.md"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("ancestor"), "{error}");
        assert!(error.contains("is a symlink"), "{error}");
        assert!(!outside_docs.join("agent-map.md").exists());
    }

    #[test]
    fn validate_reports_missing_guides_and_broken_links() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("crates/api")).unwrap();
        fs::create_dir_all(temp.path().join("target/package/demo")).unwrap();
        fs::write(temp.path().join("AGENTS.md"), "root").unwrap();
        fs::write(temp.path().join("crates/api/AGENTS.md"), "api").unwrap();
        fs::write(
            temp.path().join("target/package/demo/AGENTS.md"),
            "artifact",
        )
        .unwrap();
        fs::write(
            temp.path().join("agent-map.md"),
            "- [root](./AGENTS.md)\n- [missing](./missing.md)\n- [escape](../outside.md)\n",
        )
        .unwrap();

        let result = validate(temp.path(), Path::new("agent-map.md")).unwrap();

        assert_eq!(result.agent_count, 2);
        assert_eq!(result.missing_agents, vec!["crates/api/AGENTS.md"]);
        assert_eq!(result.broken_links.len(), 2);
        assert!(
            result
                .broken_links
                .iter()
                .any(|link| link.contains("missing.md"))
        );
        assert!(
            result
                .broken_links
                .iter()
                .any(|link| link.contains("outside repository"))
        );
    }

    #[test]
    fn validate_ignores_target_relative_to_repo_root_only() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("target/checkout");
        fs::create_dir_all(root.join("crates/api")).unwrap();
        fs::create_dir_all(root.join("target/package/demo")).unwrap();
        fs::write(root.join("AGENTS.md"), "root").unwrap();
        fs::write(root.join("crates/api/AGENTS.md"), "api").unwrap();
        fs::write(root.join("target/package/demo/AGENTS.md"), "artifact").unwrap();
        fs::write(
            root.join("agent-map.md"),
            "- [root](./AGENTS.md)\n- [api](./crates/api/AGENTS.md)\n",
        )
        .unwrap();

        let result = validate(&root, Path::new("agent-map.md")).unwrap();

        assert_eq!(result.agent_count, 2);
        assert!(result.missing_agents.is_empty());
        assert!(result.broken_links.is_empty());
    }

    #[test]
    fn check_guides_validates_existing_guides_only() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("crates/api")).unwrap();
        fs::create_dir_all(temp.path().join("crates/worker")).unwrap();
        TestRepoBuilder::new(temp.path())
            .config(
                r#"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
            )
            .contract_version(2)
            .required_commands(["rust_test_command"])
            .write();
        fs::write(
            temp.path().join("crates/api/AGENTS.md"),
            "## Purpose\nNo entrypoint reference yet.\n",
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let output = check_guides(&ctx).unwrap();

        assert_eq!(output["ok"], false);
        assert_eq!(output["guide_count"], 1);
        assert!(output["missing_guides"].as_array().unwrap().is_empty());
        assert!(
            output["missing_guides_note"]
                .as_str()
                .unwrap()
                .contains("no longer required")
        );
        assert!(
            output["missing_sections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|section| section.as_str().unwrap().contains("## Key entrypoints"))
        );
        assert!(
            output["missing_entry_ref"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry.as_str().unwrap().contains("crates/api/AGENTS.md"))
        );
    }
}
