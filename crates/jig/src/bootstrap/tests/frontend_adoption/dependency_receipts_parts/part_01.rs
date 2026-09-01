#[cfg(unix)]
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

#[test]
fn generated_web_dependency_scope_and_fingerprints_use_only_selected_manager_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let generated_scripts = generated_web_check_scripts();
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
        assert_dependency_scope_case(
            temp.path(),
            &generated_scripts[package_manager],
            case_name,
            package_manager,
            package_json,
            pnpm_workspace,
            root_scope,
        );
    }
}

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

#[cfg(unix)]
fn assert_runtime_caches_do_not_stale_receipt(
    node_modules: &Path,
    run: &impl Fn(&str) -> std::process::Output,
) {
    for cache_name in [".astro", ".cache", ".vite", ".vite-temp", ".tmp"] {
        let cache = node_modules.join(cache_name);
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("runtime-state"), "first\n").unwrap();
        assert_output_succeeded("added top-level runtime cache", &run("dependencies-ready"));
        fs::write(cache.join("runtime-state"), "rewritten runtime state\n").unwrap();
        assert_output_succeeded(
            "rewritten top-level runtime cache",
            &run("dependencies-ready"),
        );
        fs::remove_dir_all(&cache).unwrap();
        assert_output_succeeded(
            "removed top-level runtime cache",
            &run("dependencies-ready"),
        );
    }
    let finder_metadata = node_modules.join(".DS_Store");
    fs::write(&finder_metadata, "first\n").unwrap();
    assert_output_succeeded("added Finder metadata", &run("dependencies-ready"));
    fs::write(&finder_metadata, "rewritten Finder state\n").unwrap();
    assert_output_succeeded("rewritten Finder metadata", &run("dependencies-ready"));
    fs::remove_file(&finder_metadata).unwrap();
    assert_output_succeeded("removed Finder metadata", &run("dependencies-ready"));
}

#[cfg(unix)]
#[test]
fn generated_root_receipt_attests_workspace_member_node_modules_and_launcher_bytes_and_mode() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("member-node-modules-receipt");
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
            repo_name: Some("member-node-modules-receipt".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"tool":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), r#"{"lockfileVersion":3}"#).unwrap();
    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    if [ "$1" = install ]; then
      : > .bootstrap-install-ran
    fi
    mkdir -p node_modules/tool apps/web/node_modules/.bin apps/web/node_modules/runtime-owner
    printf '%s\n' '{"name":"tool"}' > node_modules/tool/package.json
    printf '%s\n' '{"name":"runtime-owner","v":1}' > apps/web/node_modules/runtime-owner/package.json
    printf '%s\n' 'layout-v1' > apps/web/node_modules/.modules.yaml
    printf '%s\n' '#!/bin/sh' 'exit 0' > apps/web/node_modules/.bin/tool
    chmod 755 apps/web/node_modules/.bin/tool
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .output()
            .unwrap()
    };

    let install = run("dependencies-install");
    assert_output_succeeded("workspace install", &install);
    assert_output_succeeded("initial workspace readiness", &run("dependencies-ready"));

    let member_modules = repo.join("apps/web/node_modules");
    for node_modules in [repo.join("node_modules"), member_modules.clone()] {
        assert_runtime_caches_do_not_stale_receipt(&node_modules, &run);
    }

    let cache_type_replacement = member_modules.join(".vite");
    fs::write(&cache_type_replacement, "not a cache directory\n").unwrap();
    assert_output_failed(
        "file replacing runtime-cache directory",
        &run("dependencies-ready"),
    );
    fs::remove_file(&cache_type_replacement).unwrap();
    assert_output_succeeded(
        "removed runtime-cache replacement",
        &run("dependencies-ready"),
    );
    std::os::unix::fs::symlink("runtime-owner", &cache_type_replacement).unwrap();
    assert_output_failed(
        "symlink replacing runtime-cache directory",
        &run("dependencies-ready"),
    );
    fs::remove_file(&cache_type_replacement).unwrap();
    assert_output_succeeded("removed runtime-cache symlink", &run("dependencies-ready"));

    let nested_cache = member_modules.join("runtime-owner/.vite");
    fs::create_dir(&nested_cache).unwrap();
    fs::write(nested_cache.join("runtime-state"), "nested\n").unwrap();
    assert_output_failed("nested cache-like directory", &run("dependencies-ready"));
    fs::remove_dir_all(&nested_cache).unwrap();
    assert_output_succeeded("removed nested cache", &run("dependencies-ready"));

    let package_metadata = member_modules.join("runtime-owner/package.json");
    fs::write(&package_metadata, "{\"name\":\"runtime-owner\",\"v\":2}\n").unwrap();
    assert_output_failed(
        "member package metadata mutation",
        &run("dependencies-ready"),
    );
    fs::write(&package_metadata, "{\"name\":\"runtime-owner\",\"v\":1}\n").unwrap();
    assert_output_succeeded(
        "restored member package metadata",
        &run("dependencies-ready"),
    );

    let modules_metadata = member_modules.join(".modules.yaml");
    fs::write(&modules_metadata, "layout-v2\n").unwrap();
    assert_output_failed(
        "member modules metadata mutation",
        &run("dependencies-ready"),
    );
    fs::write(&modules_metadata, "layout-v1\n").unwrap();
    assert_output_succeeded(
        "restored member modules metadata",
        &run("dependencies-ready"),
    );

    for receipt_like_name in [
        ".jig-web-dependencies-v3",
        ".jig-web-dependencies-v3.tmp.untrusted",
    ] {
        let receipt_like = member_modules.join(receipt_like_name);
        fs::write(&receipt_like, "untrusted\n").unwrap();
        assert_output_failed("member receipt-like file", &run("dependencies-ready"));
        fs::remove_file(&receipt_like).unwrap();
        assert_output_succeeded(
            "removed member receipt-like file",
            &run("dependencies-ready"),
        );
    }

    let launcher = repo.join("apps/web/node_modules/.bin/tool");
    fs::write(&launcher, "#!/bin/sh\nexit 1\n").unwrap();
    assert_output_failed(
        "member launcher content mutation",
        &run("dependencies-ready"),
    );
    fs::write(&launcher, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    assert_output_succeeded("restored member launcher", &run("dependencies-ready"));

    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o644)).unwrap();
    assert_output_failed("member launcher mode mutation", &run("dependencies-ready"));
    let bootstrap = run("dependencies-bootstrap");
    assert_output_succeeded("non-frozen dependency bootstrap", &bootstrap);
    assert!(
        repo.join(".bootstrap-install-ran").is_file(),
        "dependency bootstrap did not use the package manager's non-frozen install mode"
    );
    assert_output_succeeded("post-bootstrap readiness", &run("dependencies-ready"));

    let saved_modules = repo.join("apps/web/node_modules.saved");
    fs::rename(&member_modules, &saved_modules).unwrap();
    assert_output_failed(
        "missing workspace-member node_modules",
        &run("dependencies-ready"),
    );
    fs::rename(&saved_modules, &member_modules).unwrap();
    assert_output_succeeded(
        "restored workspace-member node_modules",
        &run("dependencies-ready"),
    );
}
