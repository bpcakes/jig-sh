use super::*;

#[cfg(unix)]
pub(super) fn write_dependency_scope_manifests(
    repo: &Path,
    package_manager: &str,
    lockfile: &str,
    workspace_member: bool,
) {
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    if workspace_member {
        fs::write(
            repo.join("package.json"),
            r#"{"private":true,"workspaces":["apps/*"]}"#,
        )
        .unwrap();
    } else {
        fs::write(
            repo.join("package.json"),
            r#"{"private":true,"workspaces":["tools/*","!apps/web"]}"#,
        )
        .unwrap();
        let unrelated_lock = if package_manager == "yarn" {
            "__metadata:\n  version: 8\n"
        } else {
            "unrelated-root-lock\n"
        };
        fs::write(repo.join(lockfile), unrelated_lock).unwrap();
        fs::create_dir_all(repo.join("node_modules")).unwrap();
    }
    if package_manager == "pnpm" {
        let workspace = if workspace_member {
            "packages:\n  - 'apps/**'\n"
        } else {
            "packages: ['apps/**', '!apps/web']\n"
        };
        fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();
    }
    if package_manager == "yarn" {
        fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
    }
}

#[cfg(unix)]
pub(super) fn assert_runtime_caches_do_not_invalidate_scope(
    repo: &Path,
    scope_label: &str,
    case_name: &str,
    workspace_member: bool,
    run: &impl Fn(&str) -> std::process::Output,
) {
    let installed_node_modules = if workspace_member {
        repo.join("node_modules")
    } else {
        repo.join("apps/web/node_modules")
    };
    let dependency_marker = installed_node_modules.join(".jig-web-dependencies-v3");
    let dependency_stamp = if workspace_member {
        repo.join(".agent/tmp/web-dependencies/root.sha256")
    } else {
        repo.join(".agent/tmp/web-dependencies/apps/apps/web.sha256")
    };
    let marker_before_runtime_caches = fs::read(&dependency_marker).unwrap();
    let stamp_before_runtime_caches = fs::read(&dependency_stamp).unwrap();
    assert!(marker_before_runtime_caches.starts_with(b"v2 "));
    assert!(stamp_before_runtime_caches.starts_with(b"v5 "));

    let runtime_node_modules = repo.join("apps/web/node_modules");
    if workspace_member {
        assert!(
            !runtime_node_modules.exists(),
            "root install populated its workspace member"
        );
    }
    for cache_name in [".astro", ".cache", ".vite", ".vite-temp", ".tmp"] {
        let cache = runtime_node_modules.join(cache_name);
        fs::create_dir_all(cache.join("nested")).unwrap();
        fs::write(
            cache.join("nested/runtime-state"),
            "generated runtime state\n",
        )
        .unwrap();
    }
    fs::write(runtime_node_modules.join(".DS_Store"), "finder state\n").unwrap();
    assert_output_succeeded(
        "readiness with top-level runtime caches",
        &run("dependencies-ready"),
    );
    assert_eq!(
        fs::read(&dependency_marker).unwrap(),
        marker_before_runtime_caches,
        "runtime caches rewrote the {scope_label} dependency marker"
    );
    assert_eq!(
        fs::read(&dependency_stamp).unwrap(),
        stamp_before_runtime_caches,
        "runtime caches rewrote the {scope_label} dependency stamp"
    );

    if workspace_member {
        remove_runtime_cache_entries(&runtime_node_modules);
        assert_output_succeeded(
            "empty runtime-created member node_modules",
            &run("dependencies-ready"),
        );
        if case_name == "npm-package-lock" {
            assert_unknown_member_directory_is_rejected(&runtime_node_modules, run);
        }
    }
}

#[cfg(unix)]
pub(super) fn remove_runtime_cache_entries(runtime_node_modules: &Path) {
    for cache_name in [".astro", ".cache", ".vite", ".vite-temp", ".tmp"] {
        fs::remove_dir_all(runtime_node_modules.join(cache_name)).unwrap();
    }
    fs::remove_file(runtime_node_modules.join(".DS_Store")).unwrap();
}

#[cfg(unix)]
pub(super) fn assert_unknown_member_directory_is_rejected(
    runtime_node_modules: &Path,
    run: &impl Fn(&str) -> std::process::Output,
) {
    let unknown_empty = runtime_node_modules.join("unknown-empty-directory");
    fs::create_dir(&unknown_empty).unwrap();
    assert_output_failed(
        "unknown empty workspace-member directory",
        &run("dependencies-ready"),
    );
    fs::remove_dir(&unknown_empty).unwrap();
    assert_output_succeeded(
        "removed unknown workspace-member directory",
        &run("dependencies-ready"),
    );
}

#[cfg(unix)]
pub(super) fn assert_workspace_member_lock_authority(
    repo: &Path,
    package_manager: &str,
    lockfile: &str,
    path: &std::ffi::OsStr,
    install_count: &Path,
    install_cwd: &Path,
    run: &impl Fn(&str) -> std::process::Output,
) {
    let app_lock = repo.join("apps/web").join(lockfile);
    if package_manager == "yarn" {
        fs::write(&app_lock, "# yarn lockfile v1\n").unwrap();
        assert_output_succeeded("Yarn Classic member lock", &run("dependencies-ready"));

        fs::write(&app_lock, "__metadata:\n  version: 8\n").unwrap();
        assert_output_failed("nested Yarn Berry project", &run("dependencies-ready"));
        let app_bootstrap = std::process::Command::new("/bin/bash")
            .args(["scripts/check-webapps.sh", "bootstrap"])
            .current_dir(repo)
            .env("PATH", path)
            .env("INSTALL_COUNT", install_count)
            .env("INSTALL_CWD", install_cwd)
            .env("LOCK_NAME", lockfile)
            .output()
            .unwrap();
        assert_output_succeeded("Yarn Berry nested-project bootstrap", &app_bootstrap);
        assert_eq!(
            fs::read_to_string(install_cwd).unwrap().trim(),
            fs::canonicalize(repo.join("apps/web"))
                .unwrap()
                .display()
                .to_string()
        );
        assert_output_succeeded("nested Yarn Berry readiness", &run("dependencies-ready"));
        assert_eq!(fs::read_to_string(install_count).unwrap().trim(), "3");
    } else {
        fs::write(&app_lock, "ignored-member-lock\n").unwrap();
        assert_output_succeeded("ignored nested member lock", &run("dependencies-ready"));
        assert_output_succeeded("member-lock bootstrap", &run("bootstrap"));
        assert_eq!(fs::read_to_string(install_count).unwrap().trim(), "2");
        assert_eq!(
            fs::read_to_string(install_cwd).unwrap().trim(),
            fs::canonicalize(repo).unwrap().display().to_string()
        );
    }
}

#[cfg(unix)]
pub(super) fn assert_dependency_scope_case(
    root: &Path,
    scripts: &GeneratedWebCheckScripts,
    case_name: &str,
    package_manager: &str,
    lockfile: &str,
    workspace_member: bool,
) {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let case = if workspace_member {
        "workspace"
    } else {
        "standalone"
    };
    let repo = root.join(format!("{case_name}-{case}"));
    scripts.install(&repo);

    write_dependency_scope_manifests(&repo, package_manager, lockfile, workspace_member);

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
  install|ci)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    pwd > "$INSTALL_CWD"
    if [ "$(basename "$0")" = yarn ]; then
      [ -f "$LOCK_NAME" ] || printf '%s\n' '__metadata:' '  version: 8' > "$LOCK_NAME"
    else
      printf '%s\n' 'installed-lock' > "$LOCK_NAME"
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
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_cwd = repo.join("install-cwd");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run = |mode: &str| {
        std::process::Command::new("/bin/bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("LOCK_NAME", lockfile)
            .output()
            .unwrap()
    };

    let before = run("dependencies-ready");
    assert_output_failed("artifact-only dependency state", &before);
    let bootstrap = std::process::Command::new("/bin/bash")
        .args(["scripts/check-webapps.sh", "bootstrap"])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("INSTALL_COUNT", &install_count)
        .env("INSTALL_CWD", &install_cwd)
        .env("LOCK_NAME", lockfile)
        .output()
        .unwrap();
    assert_output_succeeded("dependency bootstrap", &bootstrap);
    let expected_cwd = if workspace_member {
        fs::canonicalize(&repo).unwrap()
    } else {
        fs::canonicalize(repo.join("apps/web")).unwrap()
    };
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        expected_cwd.display().to_string(),
        "wrong {package_manager} {case} install scope"
    );
    assert_output_succeeded("post-bootstrap readiness", &run("dependencies-ready"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let scope_label = format!("{package_manager} {case}");
    assert_runtime_caches_do_not_invalidate_scope(
        &repo,
        &scope_label,
        case_name,
        workspace_member,
        &run,
    );

    if !workspace_member && lockfile == "npm-shrinkwrap.json" {
        let inactive_lock = repo.join("apps/web/package-lock.json");
        fs::write(&inactive_lock, "inactive-app-lock-v1\n").unwrap();
        assert_output_succeeded("inactive app package-lock", &run("dependencies-ready"));
        fs::write(&inactive_lock, "inactive-app-lock-v2\n").unwrap();
        assert_output_succeeded("changed inactive package-lock", &run("dependencies-ready"));
    }

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","version":"2","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    assert_output_failed("stale dependency stamp", &run("dependencies-ready"));
    let install = run("dependencies-install");
    assert_output_succeeded("frozen dependency install", &install);
    assert_output_succeeded("post-install readiness", &run("dependencies-ready"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    if workspace_member {
        assert_workspace_member_lock_authority(
            &repo,
            package_manager,
            lockfile,
            &path,
            &install_count,
            &install_cwd,
            &run,
        );
    }
}
