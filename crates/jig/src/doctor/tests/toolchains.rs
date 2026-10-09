use super::*;

use std::fs;
use std::path::Path;

#[cfg(unix)]
use jig_context::RepoContext;
use serde_json::{Value, json};
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
use super::support::{
    doctor_environment, write_doctor_fixture, write_test_executable,
    write_workspace_version_manifest,
};
use crate::cli::format_doctor_summary_for_test as format_summary;
#[cfg(unix)]
use crate::doctor::environment::DoctorProcessControl;
#[cfg(unix)]
use crate::doctor::toolchains::{go_runtime_check, node_runtime_check, rust_runtime_check};

#[cfg(unix)]
#[test]
fn rust_runtime_check_rejects_an_active_version_below_the_cargo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    write_workspace_version_manifest(&root, "1.94", None);
    write_test_executable(
        &bin.join("rustc"),
        "#!/bin/sh\nprintf 'rustc 1.85.1 (fixture 2025-03-18)\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = rust_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok, "{check:?}");
    assert_eq!(check.status, "incompatible", "{check:?}");
    assert_eq!(check.data["required"], "1.94.0");
    assert_eq!(check.data["actual"], "1.85.1");
    assert!(check.fix.as_deref().unwrap().contains("Rust 1.94.0"));
    let summary = format_summary(&output(None, vec![check]));
    assert!(summary.contains("Jig doctor: needs attention"));
    assert!(summary.contains("Rust runtime: needs setup"));
}
#[cfg(unix)]
#[test]
fn rust_runtime_check_accepts_an_active_version_at_or_above_the_cargo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    write_workspace_version_manifest(&root, "1.94", None);
    write_test_executable(
        &bin.join("rustc"),
        "#!/bin/sh\nprintf 'rustc 1.94.1 (fixture 2026-03-25)\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = rust_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok, "{check:?}");
    assert_eq!(check.status, "compatible");
    assert_eq!(check.data["actual"], "1.94.1");
    assert!(check.fix.is_none());
}
#[cfg(unix)]
#[test]
fn go_runtime_check_uses_the_go_module_authority() {
    for (actual, compatible) in [("1.27.2", false), ("1.27.4", true)] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_doctor_fixture(&root);
        configure_doctor_fixture_go_adapter(&root);
        fs::write(
            root.join("go.mod"),
            "module example.com/ExampleProject\n\ngo 1.27.3\n",
        )
        .unwrap();
        write_test_executable(
            &bin.join("go"),
            &format!("#!/bin/sh\nprintf 'go version go{actual} linux/amd64\\n'\n"),
        );
        let ctx = RepoContext::load_from_root(root.clone()).unwrap();

        let check = go_runtime_check(
            &ctx,
            &doctor_environment(&bin, None),
            DoctorProcessControl::allowed_without_signal_session(),
        )
        .unwrap();

        assert_eq!(check.ok, compatible, "{check:?}");
        assert_eq!(check.data["required"], "1.27.3");
        assert_eq!(check.data["actual"], actual);
        assert_eq!(
            check.data["authority"],
            root.join("go.mod").display().to_string()
        );
        assert_eq!(
            check.status,
            if compatible {
                "compatible"
            } else {
                "incompatible"
            }
        );
        if compatible {
            assert!(check.fix.is_none());
        } else {
            assert_eq!(
                check.fix.as_deref(),
                Some("Install or activate Go 1.27.3 or newer, then run `scripts/jig doctor`.")
            );
        }
    }
}
#[cfg(unix)]
#[test]
fn go_runtime_check_uses_a_nested_v6_component_module() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    configure_doctor_fixture_go_adapter_at(&root, "services/api");
    fs::create_dir_all(root.join("services/api")).unwrap();
    fs::write(
        root.join("services/api/go.mod"),
        "module example.com/ExampleProject/api\n\ngo 1.27.3\n",
    )
    .unwrap();
    write_test_executable(
        &bin.join("go"),
        "#!/bin/sh\nprintf 'go version go1.27.4 linux/amd64\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root.clone()).unwrap();
    jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();

    let check = go_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok, "{check:?}");
    assert_eq!(check.data["required"], "1.27.3");
    assert_eq!(
        check.data["authority"],
        root.join("services/api/go.mod").display().to_string()
    );
    assert_eq!(
        check.data["authorities"],
        json!([root.join("services/api/go.mod").display().to_string()])
    );
}
#[cfg(unix)]
#[test]
fn go_runtime_check_uses_the_nearest_parent_module_for_a_nested_component() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_doctor_fixture(&root);
    configure_doctor_fixture_go_adapter_at(&root, "cmd/api");
    fs::write(
        root.join("go.mod"),
        "module example.com/ExampleProject\n\ngo 1.27.3\n",
    )
    .unwrap();
    write_test_executable(
        &bin.join("go"),
        "#!/bin/sh\nprintf 'go version go1.27.4 linux/amd64\\n'\n",
    );
    let ctx = RepoContext::load_from_root(root.clone()).unwrap();
    jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();

    let check = go_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok, "{check:?}");
    assert_eq!(
        check.data["authority"],
        root.join("go.mod").display().to_string()
    );
}
#[cfg(unix)]
#[test]
fn go_runtime_check_rejects_a_symlinked_component_root_ancestor() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let outside = temp.path().join("outside");
    write_doctor_fixture(&root);
    configure_doctor_fixture_go_adapter_at(&root, "services/api");
    fs::create_dir_all(outside.join("api")).unwrap();
    fs::write(
        outside.join("api/go.mod"),
        "module example.com/Outside\n\ngo 9.99.0\n",
    )
    .unwrap();
    symlink(&outside, root.join("services")).unwrap();
    let ctx = RepoContext::load_from_root(root).unwrap();

    let error = ctx.go_module_authority_paths().unwrap_err().to_string();

    assert!(error.contains("Go component 'api' root"), "{error}");
    assert!(error.contains("is a symlink"), "{error}");
    assert!(!error.contains(&outside.display().to_string()), "{error}");
}
#[cfg(unix)]
#[test]
fn go_runtime_check_reports_a_missing_module_at_the_nested_component_root() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    write_doctor_fixture(&root);
    configure_doctor_fixture_go_adapter_at(&root, "services/api");
    let ctx = RepoContext::load_from_root(root.clone()).unwrap();
    jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();

    let check = go_runtime_check(
        &ctx,
        &doctor_environment(&temp.path().join("empty-bin"), None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    let authority = root.join("services/api/go.mod").display().to_string();
    assert!(!check.ok);
    assert_eq!(check.status, "invalid authority");
    assert_eq!(check.data["authority"], authority);
    assert!(check.fix.as_deref().unwrap().contains(&authority));
}
#[cfg(unix)]
#[test]
fn missing_go_runtime_fix_uses_the_go_module_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let empty_bin = temp.path().join("bin");
    fs::create_dir_all(&empty_bin).unwrap();
    write_doctor_fixture(&root);
    configure_doctor_fixture_go_adapter(&root);
    fs::write(
        root.join("go.mod"),
        "module example.com/ExampleProject\n\ngo 1.28.1\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = go_runtime_check(
        &ctx,
        &doctor_environment(&empty_bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert_eq!(
        check.fix.as_deref(),
        Some("Install or activate Go 1.28.1 or newer, then run `scripts/jig doctor`.")
    );
}
fn configure_doctor_fixture_go_adapter(root: &Path) {
    configure_doctor_fixture_go_adapter_at(root, ".");
}
fn configure_doctor_fixture_go_adapter_at(root: &Path, component_root: &str) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let mut go_config = config
        .replacen("root = \".\"", &format!("root = {component_root:?}"), 1)
        .replacen("adapters = [\"rust\"]", "adapters = [\"go\"]", 1);
    if component_root != "." {
        go_config = go_config
            .replacen("id = \"repo\"", "id = \"api\"", 1)
            .replace("component = \"repo\"", "component = \"api\"");
    }
    assert_ne!(config, go_config);
    fs::write(config_path, go_config).unwrap();

    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["components"][0]["root"] = json!(component_root);
    contract["components"][0]["adapters"] = json!(["go"]);
    if component_root != "." {
        contract["components"][0]["id"] = json!("api");
        for action in contract["actions"].as_array_mut().unwrap() {
            action["target"]["component"] = json!("api");
        }
        for profile in contract["profiles"].as_array_mut().unwrap() {
            for target in profile["targets"].as_array_mut().unwrap() {
                target["component"] = json!("api");
            }
        }
    }
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}
#[cfg(unix)]
#[test]
fn node_runtime_check_rejects_an_active_version_below_the_repo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::write(root.join(".node-version"), "24.19.0\n").unwrap();
    write_test_executable(&bin.join("node"), "#!/bin/sh\nprintf 'v22.23.2\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "incompatible");
    assert_eq!(check.data["required"], "24.19.0");
    assert_eq!(check.data["actual"], "22.23.2");
    assert!(
        check
            .fix
            .as_deref()
            .unwrap()
            .contains("Activate Node 24.19.0")
    );
    let summary = format_summary(&output(None, vec![check]));
    assert!(summary.contains("Jig doctor: needs attention"));
    assert!(summary.contains("Node runtime: needs setup"));
}
#[cfg(unix)]
#[test]
fn node_runtime_check_accepts_an_active_version_at_or_above_the_repo_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::write(root.join(".node-version"), "24.19.0\n").unwrap();
    write_test_executable(&bin.join("node"), "#!/bin/sh\nprintf 'v24.20.1\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(check.ok);
    assert_eq!(check.status, "compatible");
    assert_eq!(check.data["actual"], "24.20.1");
    assert!(check.fix.is_none());
}
#[cfg(unix)]
#[test]
fn node_runtime_check_rejects_a_non_regular_version_authority() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_frontend_doctor_fixture(&root);
    fs::create_dir(root.join(".node-version")).unwrap();
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = node_runtime_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "invalid authority");
    assert!(check.detail.contains("real regular file"));
}
#[cfg(unix)]
fn write_frontend_doctor_fixture(root: &Path) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let config = format!(
        "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\nkind = \"vite\"\nrole = \"spa\"\n\n[dev]\n\n[[dev.apps]]\nname = \"web\"\ndir = \"web\"\nkind = \"vite\"\nargv = [\"bun\", \"run\", \"dev\"]\n",
        fs::read_to_string(&config_path).unwrap()
    );
    fs::write(config_path, config).unwrap();
    fs::create_dir(root.join("web")).unwrap();
}
