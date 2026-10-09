use super::*;

#[cfg(unix)]
fn assert_dependency_scope_case(
    root: &Path,
    scripts: &GeneratedWebCheckScripts,
    case_name: &str,
    package_manager: &str,
    package_json: &str,
    pnpm_workspace: &str,
    root_scope: bool,
) {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let repo = root.join(case_name);
    scripts.install(&repo);
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), package_json).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), pnpm_workspace).unwrap();
    if package_manager == "yarn" {
        fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
    }

    let lockfile = match package_manager {
        "bun" => "bun.lock",
        "npm" => "package-lock.json",
        "pnpm" => "pnpm-lock.yaml",
        "yarn" => "yarn.lock",
        _ => unreachable!(),
    };
    fs::write(
        repo.join(lockfile),
        if package_manager == "yarn" {
            "__metadata:\n  version: 8\n"
        } else {
            "unrelated root lock\n"
        },
    )
    .unwrap();
    if package_manager == "pnpm" && !root_scope {
        fs::write(
            repo.join("apps/web/pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
    }
    if package_manager == "npm" && !root_scope {
        fs::write(
            repo.join("apps/web/package-lock.json"),
            "standalone app lock\n",
        )
        .unwrap();
    }
    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_manager = fake_bin.join(package_manager);
    fs::write(
        &fake_manager,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
case "$(basename "$0")" in
  pnpm) printf '%s\n' '10.12.1' ;;
  yarn) printf '%s\n' '4.17.1' ;;
  *) exit 2 ;;
esac
;;
  ci|install)
pwd > "$INSTALL_CWD"
if [ "$(basename "$0")" = yarn ]; then
  [ -f "$LOCK_NAME" ] || printf '%s\n' '__metadata:' '  version: 8' > "$LOCK_NAME"
else
  [ -f "$LOCK_NAME" ] || printf '%s\n' lock > "$LOCK_NAME"
fi
mkdir -p node_modules/test-package
printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
;;
  config)
if [ "$(basename "$0")" = pnpm ]; then
  [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
  printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
  exit 0
fi
[ "$(basename "$0")" = yarn ] && [ "${2:-}" = --json ] || exit 2
scope="$(pwd -P)"
printf '%s\n' '{"key":"nodeLinker","effective":"node-modules"}'
printf '{"key":"cacheFolder","effective":"%s/.yarn/cache"}\n' "$scope"
printf '{"key":"installStatePath","effective":"%s/.yarn/install-state.gz"}\n' "$scope"
printf '{"key":"pnpUnpluggedFolder","effective":"%s/.yarn/unplugged"}\n' "$scope"
printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
;;
  pkg)
[ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
printf '%s\n' '{}'
;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();
    let install_cwd = repo.join("install-cwd");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_CWD", &install_cwd)
            .env("LOCK_NAME", lockfile)
            .output()
            .unwrap()
    };

    let install = run("dependencies-install");
    assert_output_succeeded(case_name, &install);
    let expected_cwd = if root_scope {
        fs::canonicalize(&repo).unwrap()
    } else {
        fs::canonicalize(repo.join("apps/web")).unwrap()
    };
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        expected_cwd.display().to_string(),
        "{case_name} chose the wrong dependency scope"
    );
    assert_output_succeeded("initial dependency readiness", &run("dependencies-ready"));

    if case_name == "npm-package-wins" {
        fs::create_dir_all(repo.join("apps/worker")).unwrap();
        fs::write(
            repo.join("apps/worker/package.json"),
            r#"{"name":"worker","version":"1"}"#,
        )
        .unwrap();
        assert_output_failed(
            "new authoritative workspace manifest",
            &run("dependencies-ready"),
        );
        assert_output_succeeded("workspace manifest reinstall", &run("dependencies-install"));
        fs::write(
            repo.join("apps/worker/package.json"),
            r#"{"name":"worker","version":"2"}"#,
        )
        .unwrap();
        assert_output_failed("changed workspace manifest", &run("dependencies-ready"));
        assert_output_succeeded("changed workspace reinstall", &run("dependencies-install"));
    }

    if matches!(
        case_name,
        "bun-character-class-workspace" | "pnpm-workspace-wins"
    ) {
        fs::create_dir_all(repo.join("patches")).unwrap();
        fs::write(repo.join("patches/dependency.patch"), "patch-v1\n").unwrap();
        assert_output_failed("root patch input", &run("dependencies-ready"));
        assert_output_succeeded("root patch reinstall", &run("dependencies-install"));
    }

    let irrelevant = match package_manager {
        "npm" | "yarn" => "bunfig.toml",
        "pnpm" => ".yarnrc",
        "bun" => ".pnpmfile.cjs",
        _ => unreachable!(),
    };
    fs::write(repo.join(irrelevant), "irrelevant manager config\n").unwrap();
    assert_output_succeeded("irrelevant manager config", &run("dependencies-ready"));

    if case_name == "pnpm-workspace-wins" {
        let workspace = repo.join("pnpm-workspace.yaml");
        let original = fs::read_to_string(&workspace).unwrap();
        fs::write(&workspace, "packages: invalid-scalar\n").unwrap();
        let malformed = run("dependencies-ready");
        assert_output_failed("malformed pnpm workspace", &malformed);
        assert_text_contains_all(
            &String::from_utf8_lossy(&malformed.stderr),
            &["packages must be a block or flow sequence"],
        );
        fs::write(&workspace, original).unwrap();
        assert_output_succeeded("restored pnpm workspace", &run("dependencies-ready"));
    }

    let relevant = match package_manager {
        "npm" => ".npmrc",
        "pnpm" => ".pnpmfile.cjs",
        "bun" => "bunfig.toml",
        "yarn" => ".yarnrc",
        _ => unreachable!(),
    };
    let relevant_path = repo.join(relevant);
    let original_relevant = fs::read(&relevant_path).ok();
    if relevant_path.exists() {
        fs::remove_file(&relevant_path).unwrap();
    }
    let relevant_target = repo.join("selected-manager-config-target");
    fs::write(&relevant_target, "selected manager config\n").unwrap();
    symlink(&relevant_target, &relevant_path).unwrap();
    assert_output_failed("symlinked manager config", &run("dependencies-ready"));
    fs::remove_file(&relevant_path).unwrap();
    fs::remove_file(&relevant_target).unwrap();
    if let Some(original) = original_relevant {
        fs::write(&relevant_path, original).unwrap();
    }
    assert_output_succeeded("restored manager config", &run("dependencies-ready"));

    if case_name == "npm-package-wins" {
        let manifest = repo.join("package.json");
        let manifest_target = repo.join("package-target.json");
        fs::rename(&manifest, &manifest_target).unwrap();
        symlink(&manifest_target, &manifest).unwrap();
        assert_output_failed("symlinked package manifest", &run("dependencies-ready"));
        fs::remove_file(&manifest).unwrap();
        fs::rename(&manifest_target, &manifest).unwrap();
        assert_output_succeeded("restored package manifest", &run("dependencies-ready"));
    }

    fs::write(repo.join(relevant), "selected manager config changed\n").unwrap();
    assert_output_failed(
        "changed selected manager config",
        &run("dependencies-ready"),
    );
}

fn assert_selected_manager_metadata(selected_manager: &str) {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let generated_scripts = generated_web_check_scripts(selected_manager);
    let cases = [
        (
            "npm-package-wins",
            "npm",
            r#"{"private":true,"workspaces":["apps/*"]}"#,
            "packages:\n  - 'tools/*'\n  - '!apps/web'\n",
            true,
        ),
        (
            "npm-brace-workspace",
            "npm",
            r#"{"private":true,"workspaces":["apps/{web,admin}"]}"#,
            "packages:\n  - 'tools/*'\n",
            true,
        ),
        (
            "npm-ignores-yarn-object",
            "npm",
            r#"{"private":true,"workspaces":{"packages":["apps/*"]}}"#,
            "packages:\n  - 'tools/*'\n",
            false,
        ),
        (
            "bun-ignores-pnpm",
            "bun",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages:\n  - 'apps/*'\n",
            false,
        ),
        (
            "bun-character-class-workspace",
            "bun",
            r#"{"private":true,"workspaces":["apps/[w]eb"]}"#,
            "packages:\n  - 'tools/*'\n",
            true,
        ),
        (
            "yarn-object-wins",
            "yarn",
            r#"{"private":true,"workspaces":{"packages":["apps/*"]}}"#,
            "packages:\n  - '!apps/web'\n",
            true,
        ),
        (
            "pnpm-ignores-package",
            "pnpm",
            r#"{"private":true,"workspaces":["apps/*"]}"#,
            "packages: ['tools/*', 'tools/hash#workspace'] # app excluded\n",
            false,
        ),
        (
            "pnpm-workspace-wins",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages:\n  - 'apps/*' # web application\n  - tools/hash#workspace\n",
            true,
        ),
        (
            "pnpm-flow-comment-wins",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages: ['apps/*', 'tools/hash#workspace'] # web application\n",
            true,
        ),
        (
            "pnpm-brace-workspace",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages: ['apps/{web,admin}']\n",
            true,
        ),
    ];

    for (case_name, package_manager, package_json, pnpm_workspace, root_scope) in cases {
        if package_manager != selected_manager {
            continue;
        }
        assert_dependency_scope_case(
            temp.path(),
            &generated_scripts,
            case_name,
            package_manager,
            package_json,
            pnpm_workspace,
            root_scope,
        );
    }
}

macro_rules! selected_manager_metadata_test {
    ($name:ident, $manager:literal) => {
        #[test]
        fn $name() {
            assert_selected_manager_metadata($manager);
        }
    };
}

selected_manager_metadata_test!(generated_web_dependency_metadata_bun, "bun");

selected_manager_metadata_test!(generated_web_dependency_metadata_npm, "npm");

selected_manager_metadata_test!(generated_web_dependency_metadata_pnpm, "pnpm");

selected_manager_metadata_test!(generated_web_dependency_metadata_yarn, "yarn");

#[cfg(unix)]
#[test]
fn generated_web_dependency_fingerprints_isolate_mixed_root_and_app_scopes() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("mixed-web-scopes");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("mixed-web-scopes".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "root-web".into(),
                    dir: "apps/root-web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "legacy-web".into(),
                    dir: "legacy-web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/root-web")).unwrap();
    fs::create_dir_all(repo.join("legacy-web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root-lock\n").unwrap();
    fs::write(
        repo.join("apps/root-web/package.json"),
        r#"{"name":"root-web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("legacy-web/package.json"),
        r#"{"name":"legacy-web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("legacy-web/package-lock.json"), "app-lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  install|ci)
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let output = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "bootstrap"])
        .current_dir(&repo)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "mixed-scope bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let ready = |app_dir: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", app_dir])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    };
    assert!(ready("apps/root-web"));
    assert!(ready("legacy-web"));

    fs::write(
        repo.join("legacy-web/package.json"),
        r#"{"name":"legacy-web","version":"2","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    assert!(
        ready("apps/root-web"),
        "app-local package changes must not stale the root workspace receipt"
    );
    assert!(!ready("legacy-web"));
}
