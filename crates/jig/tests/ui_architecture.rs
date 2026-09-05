use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jig_ui::dashboard::{
    DEFAULT_TIMELINE_ROWS, LIMIT_SPECS, PLAN_ROOT_FIELDS, RECORDER_ROOT_FIELDS,
    SNAPSHOT_ERROR_CODES, SNAPSHOT_ERROR_SCOPES, STATUS_ROOT_FIELDS, STATUS_SCHEMA_VERSION,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn workspace_and_release_exclude_the_obsolete_status_package() {
    let root = repo_root();
    let workspace = manifest(&root.join("Cargo.toml"));
    let members = workspace["workspace"]["members"].as_array().unwrap();
    assert!(
        members
            .iter()
            .all(|member| member.as_str() != Some("crates/jig-status-tui"))
    );
    assert!(
        workspace["workspace"]["dependencies"]
            .get("jig-status-tui")
            .is_none()
    );

    let cli = manifest(&root.join("crates/jig/Cargo.toml"));
    assert!(cli["dependencies"].get("jig-status-tui").is_none());
    assert!(!root.join("crates/jig-status-tui").exists());

    let release = fs::read_to_string(root.join("scripts/release.sh")).unwrap();
    assert!(!release.contains("jig-status-tui"));
}

#[test]
fn dashboard_manifest_has_no_web_only_direct_dependencies() {
    let root = repo_root();
    let dashboard = manifest(&root.join("crates/jig-ui/Cargo.toml"));
    let mut dependencies = BTreeSet::new();
    collect_dependency_names(&dashboard, &mut dependencies);

    // These are the template, capability-randomness, and constant-time
    // comparison crates used by the retired dashboard server.
    for dependency in ["askama", "getrandom", "subtle"] {
        assert!(
            !dependencies.contains(dependency),
            "jig-ui must not directly depend on web-only crate {dependency}"
        );
    }
}

#[test]
fn production_tree_contains_no_http_surface() {
    let root = repo_root();
    for relative in [
        "crates/jig-ui/src/server.rs",
        "crates/jig-ui/src/html.rs",
        "crates/jig-ui/src/html",
        "crates/jig-ui/src/model.rs",
        "crates/jig/src/ui/snapshot.rs",
        "crates/jig/src/status/tui.rs",
    ] {
        assert!(
            !root.join(relative).exists(),
            "retired dashboard source remains at {relative}"
        );
    }

    let dashboard_source = read_rust_tree(&root.join("crates/jig-ui/src"));
    let mut cli_source = fs::read_to_string(root.join("crates/jig/src/ui.rs")).unwrap();
    cli_source.push_str(&read_rust_tree(&root.join("crates/jig/src/ui")));
    cli_source.push_str(&fs::read_to_string(root.join("crates/jig/src/status.rs")).unwrap());
    cli_source.push_str(&read_rust_tree(&root.join("crates/jig/src/status")));
    for forbidden in [
        "TcpListener",
        "UiServer",
        "SnapshotProvider",
        "DEFAULT_UI_PORT",
        "HttpResponse",
        "SESSION_COOKIE",
    ] {
        assert!(
            !dashboard_source.contains(forbidden) && !cli_source.contains(forbidden),
            "retired dashboard server symbol remains: {forbidden}"
        );
    }
}

#[test]
fn external_status_subsystem_is_absent() {
    let root = repo_root();
    for relative in [
        "crates/jig-contract/src/status_provider.rs",
        "crates/jig-contract/contracts/status-provider",
        "crates/jig/src/context/status_config.rs",
        "crates/jig/src/status/summary.rs",
        "crates/jig-ui/src/terminal/model/package_detail.rs",
        "crates/jig-ui/src/terminal/model/typed.rs",
        "crates/jig-ui/src/terminal/model/wire.rs",
        "crates/jig-ui/src/terminal/render/package_detail.rs",
        "docs/status-provider.md",
    ] {
        assert!(
            !root.join(relative).exists(),
            "retired external-status artifact remains at {relative}"
        );
    }

    let dashboard_source = read_rust_tree(&root.join("crates/jig-ui/src"));
    let mut status_source = fs::read_to_string(root.join("crates/jig/src/status.rs")).unwrap();
    status_source.push_str(&read_rust_tree(&root.join("crates/jig/src/status")));
    for forbidden in [
        "StatusRefresh",
        "StatusRequest",
        "StatusPhase",
        "AcceptedProviderReport",
        "ProviderSummary",
        "PackageDetail",
        "View::Packages",
        "View::Blockers",
    ] {
        assert!(
            !dashboard_source.contains(forbidden) && !status_source.contains(forbidden),
            "retired external-status symbol remains: {forbidden}"
        );
    }
}

#[test]
fn maintained_guides_describe_the_terminal_only_dashboard_contract() {
    let root = repo_root();
    let documents = read_maintained_dashboard_documents(&root);
    assert_no_retired_dashboard_guidance(&documents);
    assert_human_dashboard_guidance(&documents);
    assert_machine_dashboard_contract(document(&documents, "docs/public-contract.md"));
    assert_dashboard_release_notes(document(&documents, "CHANGELOG.md"));
}

fn read_maintained_dashboard_documents(root: &Path) -> Vec<(&'static str, String)> {
    [
        "README.md",
        "docs/developer-ux.md",
        "docs/repo-intent.md",
        "docs/public-contract.md",
        "CHANGELOG.md",
        "agent-map.md",
        "crates/jig/AGENTS.md",
        "crates/jig-ui/AGENTS.md",
        "crates/jig-tui/AGENTS.md",
    ]
    .into_iter()
    .map(|path| (path, fs::read_to_string(root.join(path)).unwrap()))
    .collect()
}

fn document<'a>(documents: &'a [(&str, String)], expected: &str) -> &'a str {
    documents
        .iter()
        .find_map(|(path, document)| (*path == expected).then_some(document.as_str()))
        .unwrap()
}

fn assert_no_retired_dashboard_guidance(documents: &[(&str, String)]) {
    for (path, document) in documents {
        for retired_claim in [
            "scripts/jig ui --port",
            "prints a one-time loopback sign-in URL",
            "session cookie established by the printed one-time URL",
            "crates/jig-status-tui",
            "#flight-recorder-ui",
        ] {
            assert!(
                !document.contains(retired_claim),
                "{path} retains retired dashboard guidance: {retired_claim}"
            );
        }
    }
}

fn assert_human_dashboard_guidance(documents: &[(&str, String)]) {
    let readme = document(documents, "README.md");
    assert!(readme.contains("Status, Work, Timeline, and Health"));
    assert!(readme.contains("docs/developer-ux.md#terminal-dashboard"));

    let developer_ux = document(documents, "docs/developer-ux.md");
    assert!(developer_ux.contains("## Terminal Dashboard"));
    assert!(developer_ux.contains("108 by 24"));
    assert!(developer_ux.contains("may stop parsing in 0.4.0"));
    assert!(developer_ux.contains("public-contract.md#dashboard-and-status-output"));
    assert!(developer_ux.contains("one cancellable worker"));
    assert!(developer_ux.contains("schema version 2"));

    let repo_intent = document(documents, "docs/repo-intent.md");
    assert!(repo_intent.contains("no replacement server or HTTP compatibility layer"));
}

fn assert_machine_dashboard_contract(public_contract: &str) {
    assert!(public_contract.contains("## Dashboard And Status Output"));
    let dashboard_contract = public_contract
        .split_once("## Dashboard And Status Output")
        .unwrap()
        .1
        .split_once("\n## ")
        .unwrap()
        .0;
    assert!(
        dashboard_contract.contains("`snapshot_kind` is the string `\"recorder\"` or `\"plan\"`")
    );
    assert!(public_contract.contains("contract version 7"));
    assert!(
        dashboard_contract.contains(&format!("defaults to {DEFAULT_TIMELINE_ROWS}")),
        "dashboard contract must match the default timeline size"
    );
    for fields in [RECORDER_ROOT_FIELDS, PLAN_ROOT_FIELDS, STATUS_ROOT_FIELDS] {
        let ordered_fields = fields
            .iter()
            .map(|field| format!("`{field}`"))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            dashboard_contract.contains(&ordered_fields),
            "dashboard contract omits or reorders a root field set"
        );
    }
    for scope in SNAPSHOT_ERROR_SCOPES {
        assert!(
            dashboard_contract.contains(&format!("`{scope}`")),
            "dashboard contract omits snapshot error scope {scope}"
        );
    }
    for code in SNAPSHOT_ERROR_CODES {
        assert!(
            dashboard_contract.contains(&format!("`{code}`")),
            "dashboard contract omits snapshot error code {code}"
        );
    }
    for spec in LIMIT_SPECS {
        assert!(
            dashboard_contract.contains(&format!("| `{}` | {} |", spec.id.as_str(), spec.ceiling)),
            "dashboard contract omits or misstates limit {}",
            spec.id.as_str(),
        );
    }
    assert!(dashboard_contract.contains(
        r#"{"scope": string, "code": string, "subject_id": string|null, "message": string}"#
    ));
    assert!(dashboard_contract.contains("retain `ok: true`"));
    assert!(dashboard_contract.contains("at 1048576 bytes"));
    assert!(dashboard_contract.contains(&format!("schema version {STATUS_SCHEMA_VERSION}")));
}

fn assert_dashboard_release_notes(changelog: &str) {
    assert!(changelog.contains("Breaking: replace the loopback browser dashboard"));
    assert!(changelog.contains("1,048,576 bytes"));
    assert!(changelog.contains("stop publishing the internal `jig-status-tui` crate"));
    assert!(changelog.contains("remove the external status-provider subsystem"));
}

fn collect_dependency_names(value: &toml::Value, names: &mut BTreeSet<String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (key, value) in table {
        if matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            names.extend(value.as_table().unwrap().keys().cloned());
        } else {
            collect_dependency_names(value, names);
        }
    }
}

fn read_rust_tree(root: &Path) -> String {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}
