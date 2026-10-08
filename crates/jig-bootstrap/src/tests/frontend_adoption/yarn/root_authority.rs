use super::*;

pub(super) struct RootYarnHarness {
    pub(super) repo: PathBuf,
    pub(super) app_dir: &'static str,
    pub(super) path: std::ffi::OsString,
    pub(super) install_count: PathBuf,
    pub(super) yarn_execution_marker: PathBuf,
    pub(super) symlinked_yarn_authority: PathBuf,
    pub(super) yarn_path_probe: PathBuf,
}
impl RootYarnHarness {
    fn command(&self, mode: &str) -> std::process::Output {
        self.command_with_environment(mode, None)
    }

    fn command_with_environment(
        &self,
        mode: &str,
        environment: Option<(&str, &Path)>,
    ) -> std::process::Output {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, self.app_dir])
            .current_dir(&self.repo)
            .env("PATH", &self.path)
            .env("INSTALL_COUNT", &self.install_count)
            .env("YARN_EXECUTION_MARKER", &self.yarn_execution_marker)
            .env("SYMLINKED_YARN_AUTHORITY", &self.symlinked_yarn_authority)
            .env("YARN_PATH_PROBE", &self.yarn_path_probe);
        if let Some((name, value)) = environment {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn resolve_spec(&self) -> String {
        let output = self.command("package-manager-spec");
        assert_output_succeeded("nested root-workspace Yarn spec", &output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn assert_install(&self, label: &str) {
        assert_output_succeeded(label, &self.command("dependencies-install"));
    }

    fn assert_ready(&self, expected: bool, label: &str) {
        let output = self.command("dependencies-ready");
        assert_dependency_readiness(label, output.status.success(), expected);
    }

    fn clear_execution_marker(&self) {
        let _ = fs::remove_file(&self.yarn_execution_marker);
    }
}

#[cfg(unix)]
pub(super) fn assert_nested_yarn_authority_changes(harness: &RootYarnHarness) {
    assert_eq!(harness.resolve_spec(), "yarn@3.8.7");
    harness.assert_install("initial nested-authority install");
    harness.assert_ready(true, "initial nested authority");
    assert_eq!(
        fs::read_to_string(&harness.install_count).unwrap().trim(),
        "1"
    );

    fs::write(
        harness.repo.join("apps/group/package.json"),
        r#"{"private":true,"packageManager":"yarn@3.8.8"}"#,
    )
    .unwrap();
    assert_eq!(harness.resolve_spec(), "yarn@3.8.8");
    harness.assert_ready(false, "changed intermediate package authority");
    harness.assert_install("changed intermediate package authority install");
    harness.assert_ready(true, "refreshed intermediate package authority");

    fs::write(
        harness.repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    harness.assert_ready(false, "changed intermediate Yarn config");
    harness.assert_install("changed intermediate Yarn config install");

    fs::create_dir_all(harness.repo.join("apps/group/.yarn/releases")).unwrap();
    let runtime = harness.repo.join("apps/group/.yarn/releases/yarn.cjs");
    for (contents, label) in [
        ("runtime-v1\n", "added intermediate Yarn runtime install"),
        ("runtime-v2\n", "changed intermediate Yarn runtime install"),
    ] {
        fs::write(&runtime, contents).unwrap();
        harness.assert_ready(false, label);
        harness.assert_install(label);
    }
    harness.assert_ready(true, "refreshed intermediate Yarn runtime");
}

#[cfg(unix)]
pub(super) fn assert_yaml_runtime_authority_rejected(
    harness: &RootYarnHarness,
    label: &str,
    config: &str,
    diagnostic: &str,
) {
    fs::write(harness.repo.join("apps/group/.yarnrc.yml"), config).unwrap();
    harness.clear_execution_marker();
    let output = harness.command("dependencies-ready");
    assert_output_failed(label, &output);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
        "{label} did not fail at the authority boundary: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !harness.yarn_execution_marker.exists(),
        "Yarn executed before {label} was rejected"
    );
}

#[cfg(unix)]
pub(super) fn assert_classic_runtime_authority_rejected(
    harness: &RootYarnHarness,
    label: &str,
    config: &str,
) {
    fs::write(harness.repo.join("apps/group/.yarnrc"), config).unwrap();
    harness.clear_execution_marker();
    let output = harness.command("dependencies-ready");
    assert_output_failed(label, &output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("yarn-path") || stderr.contains("symbolic link"),
        "{label} Classic yarn-path rejection was not diagnostic: {stderr}"
    );
    assert!(
        !harness.yarn_execution_marker.exists(),
        "Yarn executed before {label} Classic yarn-path was rejected"
    );
}

#[cfg(unix)]
pub(super) fn assert_ambient_runtime_authority_rejected(harness: &RootYarnHarness, name: &str) {
    harness.clear_execution_marker();
    let output = harness
        .command_with_environment("dependencies-ready", Some((name, &harness.yarn_path_probe)));
    assert_output_failed(name, &output);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(name),
        "ambient {name} rejection was not diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !harness.yarn_execution_marker.exists(),
        "Yarn executed before ambient {name} was rejected"
    );
}

#[cfg(unix)]
pub(super) fn assert_external_yarn_runtime_authorities_are_rejected(
    harness: &RootYarnHarness,
    temp_root: &Path,
) {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let outside_runtime = temp_root.join("outside-yarn-config-runtime");
    fs::create_dir_all(&outside_runtime).unwrap();
    fs::write(
        outside_runtime.join("yarn.cjs"),
        "#!/bin/sh\nset -eu\n: > \"$YARN_EXECUTION_MARKER\"\n",
    )
    .unwrap();
    fs::set_permissions(
        outside_runtime.join("yarn.cjs"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink(&outside_runtime, harness.repo.join("tools")).unwrap();

    for (label, config) in [
        ("external yarnPath", "yarnPath: ../../tools/yarn.cjs\n"),
        (
            "external plugin path",
            "plugins:\n  - path: ../../tools/yarn.cjs\n    spec: external-probe\n",
        ),
    ] {
        assert_yaml_runtime_authority_rejected(harness, label, config, "symbolic link");
    }
    for (label, config) in [
        (
            "flow-style Yarn runtime authority",
            "{ yarnPath: ../../tools/yarn.cjs }\n",
        ),
        (
            "escaped Yarn runtime key",
            "\"yarn\\u0050ath\": ../../tools/yarn.cjs\n",
        ),
    ] {
        assert_yaml_runtime_authority_rejected(
            harness,
            label,
            config,
            "unsupported top-level YAML",
        );
    }

    fs::write(
        harness.repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    for (label, config) in [
        ("canonical", "yarn-path ../../tools/yarn.cjs\n"),
        ("quoted", "\"yarn-path\" \"../../tools/yarn.cjs\"\n"),
    ] {
        assert_classic_runtime_authority_rejected(harness, label, config);
    }
    fs::remove_file(harness.repo.join("apps/group/.yarnrc")).unwrap();

    for name in [
        "YARN_RC_FILENAME",
        "YARN_YARN_PATH",
        "YARN_PLUGINS",
        "NPM_CONFIG_YARN_PATH",
    ] {
        assert_ambient_runtime_authority_rejected(harness, name);
    }
    fs::remove_file(harness.repo.join("tools")).unwrap();
    fs::write(
        harness.repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    harness.assert_ready(true, "restored in-repository Yarn authority");
}

#[cfg(unix)]
pub(super) fn assert_yarn_authority_producer_failures(harness: &RootYarnHarness) {
    let failure_environment = harness.repo.join("yarn-authority-producer-failure");
    fs::write(
        &failure_environment,
        r#"pwd() {
  if [ "${FUNCNAME[1]:-}" = "yarn_scope_authority_paths" ]; then
    return 41
  fi
  builtin pwd "$@"
}
"#,
    )
    .unwrap();
    let invalid_readiness = harness.command_with_environment(
        "dependencies-ready",
        Some(("BASH_ENV", &failure_environment)),
    );
    assert!(
        invalid_readiness
            .status
            .code()
            .is_some_and(|status| status >= 2),
        "Yarn authority producer failure was downgraded to stale readiness: {:?}\n{}",
        invalid_readiness.status.code(),
        String::from_utf8_lossy(&invalid_readiness.stderr)
    );

    let stamp = harness.repo.join(".agent/tmp/web-dependencies/root.sha256");
    fs::remove_file(&stamp).unwrap();
    let install_count_before_failure = fs::read_to_string(&harness.install_count)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let invalid_install = harness.command_with_environment(
        "dependencies-install",
        Some(("BASH_ENV", &failure_environment)),
    );
    assert!(
        invalid_install
            .status
            .code()
            .is_some_and(|status| status >= 2),
        "Yarn install published an incomplete authority fingerprint: {:?}\n{}",
        invalid_install.status.code(),
        String::from_utf8_lossy(&invalid_install.stderr)
    );
    assert!(!stamp.exists());
    assert!(
        !harness
            .repo
            .join("node_modules/.jig-web-dependencies-v3")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(&harness.install_count)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap(),
        install_count_before_failure + 1
    );
    harness.assert_install("restore after Yarn authority producer failure");
    harness.assert_ready(true, "restored Yarn authority producer");
}

#[cfg(unix)]
pub(super) fn assert_symlinked_yarn_authorities_are_rejected(
    harness: &RootYarnHarness,
    temp_root: &Path,
) {
    use std::os::unix::fs::symlink;

    let outside_yarn = temp_root.join("outside-yarn-authority");
    fs::create_dir_all(outside_yarn.join("releases")).unwrap();
    fs::write(outside_yarn.join("releases/yarn.cjs"), "runtime-v2\n").unwrap();
    fs::remove_dir_all(harness.repo.join("apps/group/.yarn")).unwrap();
    symlink(&outside_yarn, harness.repo.join("apps/group/.yarn")).unwrap();
    let symlinked_authority = harness.command("dependencies-ready");
    assert_output_failed("symlinked Yarn authority", &symlinked_authority);
    assert!(
        String::from_utf8_lossy(&symlinked_authority.stderr).contains("symbolic link"),
        "Yarn fingerprint followed an out-of-repository authority symlink: {}",
        String::from_utf8_lossy(&symlinked_authority.stderr)
    );
    assert!(
        !harness.yarn_execution_marker.exists(),
        "Yarn executed before inherited authority symlinks were rejected"
    );

    fs::remove_file(harness.repo.join("apps/group/.yarn")).unwrap();
    fs::write(
        harness.repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/group/*"]}"#,
    )
    .unwrap();
    fs::write(
        harness.repo.join("apps/group/package.json"),
        r#"{"private":true}"#,
    )
    .unwrap();
    let outside_lock = temp_root.join("outside-yarn.lock");
    fs::write(&outside_lock, "# yarn lockfile v1\n").unwrap();
    fs::remove_file(harness.repo.join("yarn.lock")).unwrap();
    symlink(&outside_lock, harness.repo.join("yarn.lock")).unwrap();
    let symlinked_lock = harness.command("package-manager-spec");
    assert_eq!(symlinked_lock.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&symlinked_lock.stderr).contains("symbolic link"),
        "Yarn package-manager resolution followed an out-of-repository lock symlink: {}",
        String::from_utf8_lossy(&symlinked_lock.stderr)
    );
    let _ = fs::remove_dir_all(harness.repo.join(".agent/tmp/web-dependencies"));
    assert_eq!(
        harness.command("dependencies-ready").status.code(),
        Some(2),
        "an absent receipt must not downgrade malformed Yarn lock authority"
    );
    let install_count_before_hard_failure = fs::read_to_string(&harness.install_count).unwrap();
    assert_eq!(
        harness.command("dependencies-install").status.code(),
        Some(2)
    );
    assert_eq!(
        fs::read_to_string(&harness.install_count).unwrap(),
        install_count_before_hard_failure,
        "a hard readiness failure must not invoke Yarn install"
    );
    assert_eq!(
        harness.command("node-version-file").status.code(),
        Some(2),
        "root Node-version fallback must not mask malformed dependency scope authority"
    );
}
