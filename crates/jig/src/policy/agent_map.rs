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
    let map_path = normalize_map_path(map_path)?;
    let depth = map_path
        .parent()
        .map_or(0, |parent| parent.components().count());
    let root_prefix = if depth == 0 {
        "./".to_owned()
    } else {
        "../".repeat(depth)
    };
    let guides = list_guides(root)?;
    let mut body = String::new();
    body.push_str("# Agent Map\n\n");
    body.push_str("Use this index when you need help locating the guide for an area.\n");
    body.push_str("If the owning area is already clear, read its nearest guide directly.\n\n");
    body.push_str("## Root guide\n\n");
    let _ = writeln!(body, "- [Repository AGENTS.md]({root_prefix}AGENTS.md)\n");
    body.push_str("## Nested guides\n\n");
    let nested = guides.iter().filter(|path| path.as_str() != "AGENTS.md");
    let mut nested_count = 0usize;
    for guide in nested {
        nested_count += 1;
        let label = escape_link_label(guide.trim_end_matches("/AGENTS.md"));
        let destination = encode_link_path(guide);
        let _ = writeln!(body, "- [{label}]({root_prefix}{destination})");
    }
    if nested_count == 0 {
        body.push_str("_None yet_\n");
    }
    body.push_str("\n## Suggested usage pattern\n\n");
    let _ = writeln!(
        body,
        "1. Start with the root [AGENTS.md]({root_prefix}AGENTS.md)."
    );
    body.push_str("2. Open the nearest guide for the area you will change.\n");
    body.push_str("3. Follow that guide's entrypoint map before editing.\n");
    Ok(body.into_bytes())
}

fn escape_link_label(label: &str) -> String {
    let mut escaped = String::new();
    for character in label.chars() {
        if matches!(character, '\\' | '[' | ']' | '`' | '*' | '_' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn encode_link_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

pub(crate) fn write_rendered(root: &Path, map_path: &Path, body: &[u8]) -> Result<()> {
    let map_path = normalize_map_path(map_path)?;
    let expected_leaf = validate_repository_regular_file_leaf(root, &map_path)?;
    write_repository_file_atomic(root, &map_path, body, expected_leaf).map(|_| ())
}

pub(super) fn check_guides(ctx: &RepoContext) -> Result<Value> {
    super::guide_check::check(ctx)
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
    use crate::agent_guides::references::{
        Destination, GuideFiles, markdown_references, resolve_reference,
    };
    let map_path = normalize_map_path(map_path)?;
    let files = GuideFiles::new(root)?;
    let text = files.read(&map_path.to_string_lossy())?;
    let mut linked_set = HashSet::new();
    let mut broken_links = Vec::new();
    for reference in markdown_references(&text) {
        if let Some(problem) = reference.problem {
            broken_links.push(format!(
                "{}:{}: {} ({problem})",
                map_path.display(),
                reference.line,
                reference.target
            ));
            continue;
        }
        let problem = match resolve_reference(&map_path, &reference.target) {
            Ok(Destination::Local(path)) => {
                linked_set.insert(path.clone());
                files
                    .check_target(&path, false)
                    .err()
                    .map(|error| format!("{path} ({error})"))
            }
            Ok(Destination::External | Destination::Fragment) => None,
            Err(error) => Some(error.to_string()),
        };
        if let Some(problem) = problem {
            broken_links.push(format!(
                "{}:{}: {} -> {problem}",
                map_path.display(),
                reference.line,
                reference.target
            ));
        }
    }
    let guides = list_guides(root)?;
    let missing_agents = guides
        .iter()
        .filter(|path| !linked_set.contains(*path))
        .cloned()
        .collect();
    Ok(CheckResult {
        agent_count: guides.len(),
        missing_agents,
        broken_links,
    })
}

fn normalize_map_path(map_path: &Path) -> Result<PathBuf> {
    crate::repository_path::normalize_repo_relative_path(map_path, "agent map path")
}

pub(super) fn list_guides(root: &Path) -> Result<Vec<String>> {
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
                if (path == "AGENTS.md" || path.ends_with("/AGENTS.md"))
                    && !Path::new(&path)
                        .components()
                        .any(is_ignored_guide_component)
                {
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

pub(super) fn relative_string(root: &Path, path: &Path) -> Result<String> {
    Ok(normalize_relative_path(path.strip_prefix(root)?))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

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

    #[test]
    fn generated_map_links_round_trip_literal_paths_from_each_map_location() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("AGENTS.md"), "# ExampleProject\n").unwrap();
        let names = [
            "100%",
            "example%20guide",
            "example guide",
            "example#guide",
            "example(guide)",
            "example[guide",
            "example]guide",
            "example`guide",
            "example_guide",
            "café",
        ];
        for name in names {
            let directory = root.path().join(name);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("AGENTS.md"), "# Owner\n").unwrap();
        }
        for map in [
            "agent-map.md",
            "docs/agent-map.md",
            "docs/nested/agent-map.md",
        ] {
            let path = Path::new(map);
            fs::create_dir_all(root.path().join(path.parent().unwrap())).unwrap();
            write(root.path(), path).unwrap();
            let result = validate(root.path(), path).unwrap();
            assert_eq!(result.agent_count, names.len() + 1);
            assert!(
                result.missing_agents.is_empty(),
                "{map}: {:?}",
                result.missing_agents
            );
            assert!(
                result.broken_links.is_empty(),
                "{map}: {:?}",
                result.broken_links
            );
        }
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
            .config("rust_crate_roots = [\"crates\"]")
            .write();
        fs::write(
            temp.path().join("crates/api/AGENTS.md"),
            "## Purpose\n## Key entrypoints\n`src/lib.rs`\n## Edit here for X\n## Invariants\n## Common commands\n",
        )
        .unwrap();
        let output = check_guides(&RepoContext::load_from(temp.path()).unwrap()).unwrap();
        assert_eq!(output["ok"], true);
        assert_eq!(output["guide_count"], 1);
        assert_eq!(output["missing_guides"], serde_json::json!([]));
        assert_eq!(output["missing_sections"], serde_json::json!([]));
        assert_eq!(output["missing_entry_ref"], serde_json::json!([]));
    }

    #[test]
    fn check_guides_preserves_legacy_go_entrypoint_requirements() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("cmd/api")).unwrap();
        fs::create_dir_all(temp.path().join("internal/core")).unwrap();
        TestRepoBuilder::new(temp.path())
            .config("backend_language = \"go\"\ngo_database = \"none\"\nrust_crate_roots = []")
            .write();
        fs::write(
            temp.path().join("cmd/api/AGENTS.md"),
            "## Purpose\n## Key entrypoints\n`main.go`\n## Edit here for X\n## Invariants\n## Common commands\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("internal/core/AGENTS.md"),
            "## Purpose\n## Key entrypoints\n`core.go`\n## Edit here for X\n## Invariants\n## Common commands\n",
        )
        .unwrap();
        let output = check_guides(&RepoContext::load_from(temp.path()).unwrap()).unwrap();
        assert_eq!(output["ok"], true);
        assert_eq!(output["guide_count"], 2);
        assert_eq!(output["missing_entry_ref"], serde_json::json!([]));
    }

    #[test]
    fn check_guides_combines_v6_component_languages_with_rust_root_fallbacks() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("services/api")).unwrap();
        fs::create_dir_all(temp.path().join("services/worker")).unwrap();
        fs::create_dir_all(temp.path().join("crates/shared")).unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
rust_crate_roots = [".", "crates", "services"]

[commands]
api_test_command = "go test ./..."
worker_test_command = "cargo test"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "services/api"
adapters = ["go"]

[[repository.components]]
id = "worker"
root = "services/worker"
adapters = []

[[repository.actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }

[[repository.actions]]
target = { component = "worker", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "worker_test_command" }

[[repository.profiles]]
id = "verify"
targets = [
  { component = "api", action = "test" },
  { component = "worker", action = "test" },
]
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "contract_version": 6,
                "tool_namespace": "jig",
                "required_commands": ["api_test_command", "worker_test_command"],
                "tools": [],
                "components": [
                    {"id": "api", "root": "services/api", "adapters": ["go"]},
                    {"id": "worker", "root": "services/worker"}
                ],
                "actions": [
                    {
                        "target": {"component": "api", "action": "test"},
                        "intent": "check",
                        "effects": ["read_only", "process"],
                        "runner": {"kind": "command", "command": "api_test_command"}
                    },
                    {
                        "target": {"component": "worker", "action": "test"},
                        "intent": "check",
                        "effects": ["read_only", "process"],
                        "runner": {"kind": "command", "command": "worker_test_command"}
                    }
                ],
                "profiles": [{
                    "id": "verify",
                    "targets": [
                        {"component": "api", "action": "test"},
                        {"component": "worker", "action": "test"}
                    ]
                }],
                "default_check_profile": "verify"
            }))
            .unwrap(),
        )
        .unwrap();
        let required_sections = "## Purpose\nExample component.\n\n## Key entrypoints\nENTRYPOINT\n\n## Edit here for X\nExample edits.\n\n## Invariants\nExample invariant.\n\n## Common commands\nExample command.\n";
        fs::write(
            temp.path().join("services/api/AGENTS.md"),
            required_sections.replace("ENTRYPOINT", "- `main.go`"),
        )
        .unwrap();
        fs::write(
            temp.path().join("services/worker/AGENTS.md"),
            required_sections.replace("ENTRYPOINT", "- `src/lib.rs`"),
        )
        .unwrap();
        fs::write(
            temp.path().join("crates/shared/AGENTS.md"),
            required_sections.replace("ENTRYPOINT", "- `src/lib.rs`"),
        )
        .unwrap();

        fs::create_dir_all(temp.path().join("docs")).unwrap();
        fs::write(
            temp.path().join("docs/AGENTS.md"),
            "Documentation editing guidance.\n",
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let output = check_guides(&ctx).unwrap();

        assert_eq!(output["ok"], true);
        assert_eq!(output["guide_count"], 3);
    }
}
