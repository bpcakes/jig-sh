use super::*;
use crate::test_env::CurrentDirGuard;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
#[cfg(unix)]
use std::process::Command;

mod adoption_modes;
mod adoption_ownership;
mod adoption_receipts;
mod inference;
mod init_safety;
mod node_versions;
mod path_and_git;
mod rendering;
mod scaffold_generation;
mod scaffold_runtime;

const WEB_HARNESS_PATHS: &[&str] = &[
    ".github/workflows/webapp-checks.yml",
    "scripts/check-webapp-scripts.mjs",
    "scripts/check-webapps.sh",
    "scripts/enforce-coverage.cjs",
    "scripts/web-node.cjs",
];

fn regular_file_tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                visit(root, &path, snapshot);
            } else if file_type.is_file() {
                snapshot.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[cfg(unix)]
fn write_executable_test_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn rollback_test_init_opts(path: PathBuf, force: bool) -> InitOpts {
    InitOpts {
        path,
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("rollback-demo".into()),
            ..AnswerOpts::default()
        },
    }
}

fn rendered_vault_scope_id(repo: &std::path::Path) -> String {
    let text = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    let value = toml::from_str::<toml::Value>(&text).unwrap();
    value["vault"]["scope_id"].as_str().unwrap().to_string()
}

fn managed_manifest_paths(repo: &Path) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
    )
    .unwrap()["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_string())
        .collect()
}

fn add_managed_manifest_path(repo: &Path, relative: &str) {
    let path = repo.join(managed_paths::MANIFEST_PATH);
    let mut manifest =
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    let paths = manifest["paths"].as_array_mut().unwrap();
    paths.push(serde_json::Value::String(relative.to_string()));
    paths.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}

fn footprint_adopt_opts(repo: &Path, template: &Path, minimal: bool, force: bool) -> AdoptOpts {
    AdoptOpts {
        // macOS exposes temporary directories through `/var`, while
        // canonical paths use `/private/var`. Normalize the fixture root so
        // the adoption helper recognizes it as the destination it may safely
        // initialize as a Git worktree.
        path: fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf()),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force,
        write: true,
        minimal,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    }
}

fn authored_mixed_repository_config() -> toml::Value {
    toml::from_str(
        r#"[commands]
repo_bootstrap_command = "true"
api_verify_command = "go test ./..."
worker_verify_command = "cargo test -p worker"
release_command = "just release"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."
adapters = ["jig"]

[[repository.components]]
id = "api"
root = "services/api"
adapters = ["go"]

[[repository.components]]
id = "worker"
root = "services/worker"
adapters = ["rust"]

[[repository.actions]]
target = { component = "repo", action = "contract" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "native", operation = "jig.contract_check" }
legacy_aliases = ["jig.contract_check"]

[[repository.actions]]
target = { component = "repo", action = "bootstrap" }
intent = "operate"
effects = ["worktree", "process", "external"]
runner = { kind = "command", command = "repo_bootstrap_command" }
legacy_aliases = ["jig.bootstrap"]

[[repository.actions]]
target = { component = "api", action = "verify-custom" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_verify_command" }

[[repository.actions]]
target = { component = "worker", action = "verify-custom" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "worker_verify_command" }

[[repository.profiles]]
id = "verify"
targets = [
  { component = "repo", action = "contract" },
  { component = "api", action = "verify-custom" },
  { component = "worker", action = "verify-custom" },
]
"#,
    )
    .unwrap()
}

fn add_project_runtime_tables(repo: &Path) {
    let path = repo.join(".jig.toml");
    let mut config = toml::from_str::<toml::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    let root = config.as_table_mut().unwrap();

    root.entry("commands")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .unwrap()
        .insert(
            "release_command".into(),
            toml::Value::String("just release".into()),
        );

    root.get_mut("work")
        .unwrap()
        .as_table_mut()
        .unwrap()
        .insert(
            "checks".into(),
            toml::Value::Array(vec![toml::Value::String("jig.fmt_check".into())]),
        );

    let mut workflow = toml::Table::new();
    workflow.insert("id".into(), toml::Value::String("project-status".into()));
    workflow.insert("kind".into(), toml::Value::String("noop_status".into()));
    let mut loop_config = toml::Table::new();
    loop_config.insert(
        "workflows".into(),
        toml::Value::Array(vec![toml::Value::Table(workflow)]),
    );
    root.insert("loop".into(), toml::Value::Table(loop_config));

    fs::write(path, toml::to_string_pretty(&config).unwrap()).unwrap();
}

fn assert_project_runtime_tables(config: &toml::Value) {
    assert_eq!(
        config["commands"]["release_command"].as_str(),
        Some("just release")
    );
    assert_eq!(config["work"]["checks"][0].as_str(), Some("jig.fmt_check"));
    assert_eq!(
        config["loop"]["workflows"][0]["id"].as_str(),
        Some("project-status")
    );
    assert_eq!(
        config["loop"]["workflows"][0]["kind"].as_str(),
        Some("noop_status")
    );
}

fn configure_frontend_fixture(repo: &Path) {
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();
}

fn frontend_app() -> FrontendApp {
    FrontendApp {
        name: "web".into(),
        dir: "apps/web".into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    }
}

fn write_project_sentinels(repo: &Path, paths: &[&str]) {
    for relative in paths {
        let path = repo.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("project-owned {relative}\n")).unwrap();
    }
}

fn assert_project_sentinels(repo: &Path, paths: &[&str]) {
    for relative in paths {
        assert_eq!(
            fs::read_to_string(repo.join(relative)).unwrap(),
            format!("project-owned {relative}\n")
        );
    }
}

fn update_opts(repo: &Path, template: &Path, force: bool) -> UpdateOpts {
    UpdateOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    }
}
