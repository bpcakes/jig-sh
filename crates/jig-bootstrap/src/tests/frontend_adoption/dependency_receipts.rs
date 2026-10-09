use super::*;

mod scopes;

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

#[cfg(unix)]
#[test]
fn generated_web_dependency_receipts_accept_only_genuinely_dependency_free_installs() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for (package_manager, lockfile, creates_empty_directory) in [
        ("npm", "package-lock.json", false),
        ("bun", "bun.lock", true),
    ] {
        let repo = temp.path().join(format!("empty-{package_manager}"));
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
                repo_name: Some(format!("empty-{package_manager}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
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
            r#"{"name":"web","scripts":{"lint":"true"}}"#,
        )
        .unwrap();

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
  config)
    [ "$(basename "$0")" = pnpm ] && [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
    ;;
  pkg)
    [ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
    printf '%s\n' '{}'
    ;;
  ci|install)
    printf '%s\n' lock > "$LOCK_NAME"
    if [ "$CREATE_EMPTY_DIRECTORY" = "1" ]; then mkdir -p node_modules; fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let command = |mode: &str| {
            let mut command = std::process::Command::new("bash");
            command.args(["scripts/check-webapps.sh", mode]);
            if matches!(mode, "dependencies-ready" | "dependencies-install") {
                command.arg("apps/web");
            }
            command
                .current_dir(&repo)
                .env("PATH", &path)
                .env("LOCK_NAME", lockfile)
                .env(
                    "CREATE_EMPTY_DIRECTORY",
                    if creates_empty_directory { "1" } else { "0" },
                )
                .output()
                .unwrap()
        };

        let bootstrap = command("bootstrap");
        assert!(
            bootstrap.status.success(),
            "dependency-free {package_manager} bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&bootstrap.stdout),
            String::from_utf8_lossy(&bootstrap.stderr)
        );
        assert!(
            repo.join(lockfile).is_file(),
            "{package_manager} bootstrap did not create the root workspace lockfile"
        );
        assert_eq!(repo.join("node_modules").is_dir(), creates_empty_directory);
        assert!(command("dependencies-ready").status.success());
        assert!(
            command("dependencies-install").status.success(),
            "{package_manager} frozen dependency path rejected the bootstrapped lock and receipt"
        );

        fs::write(
            repo.join("apps/web/package.json"),
            r#"{"name":"web","scripts":{"lint":"true"},"dependencies":{"dep":"1.0.0"}}"#,
        )
        .unwrap();
        assert!(!command("dependencies-ready").status.success());
        assert!(
            !command("bootstrap").status.success(),
            "{package_manager} accepted an empty artifact after dependencies were declared"
        );
    }
}
