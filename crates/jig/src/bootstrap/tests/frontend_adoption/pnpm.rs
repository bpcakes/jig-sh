use super::*;

#[cfg(unix)]
#[test]
fn generated_web_dependency_scope_requires_workspace_membership_and_honors_app_locks() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let generated_scripts = generated_web_check_scripts();

    for (case_name, package_manager, lockfile) in [
        ("bun", "bun", "bun.lock"),
        ("npm-package-lock", "npm", "package-lock.json"),
        ("npm-shrinkwrap", "npm", "npm-shrinkwrap.json"),
        ("pnpm", "pnpm", "pnpm-lock.yaml"),
        ("yarn", "yarn", "yarn.lock"),
    ] {
        for workspace_member in [false, true] {
            let case = if workspace_member {
                "workspace"
            } else {
                "standalone"
            };
            let repo = temp.path().join(format!("{case_name}-{case}"));
            generated_scripts[package_manager].install(&repo);

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
                fs::write(
                    repo.join(lockfile),
                    if package_manager == "yarn" {
                        "__metadata:\n  version: 8\n"
                    } else {
                        "unrelated-root-lock\n"
                    },
                )
                .unwrap();
                fs::create_dir_all(repo.join("node_modules")).unwrap();
            }
            if package_manager == "pnpm" {
                fs::write(
                    repo.join("pnpm-workspace.yaml"),
                    if workspace_member {
                        "packages:\n  - 'apps/**'\n"
                    } else {
                        "packages: ['apps/**', '!apps/web']\n"
                    },
                )
                .unwrap();
            }
            if package_manager == "yarn" {
                fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
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
            assert!(
                !before.status.success(),
                "artifact-only {package_manager} {case} state was accepted"
            );
            let bootstrap = std::process::Command::new("/bin/bash")
                .args(["scripts/check-webapps.sh", "bootstrap"])
                .current_dir(&repo)
                .env("PATH", &path)
                .env("INSTALL_COUNT", &install_count)
                .env("INSTALL_CWD", &install_cwd)
                .env("LOCK_NAME", lockfile)
                .output()
                .unwrap();
            assert!(
                bootstrap.status.success(),
                "{package_manager} {case} bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&bootstrap.stdout),
                String::from_utf8_lossy(&bootstrap.stderr)
            );
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
            assert!(run("dependencies-ready").status.success());
            assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

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
                    "fake {package_manager} root install unexpectedly populated its workspace member"
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
            let cached_ready = run("dependencies-ready");
            assert!(
                cached_ready.status.success(),
                "top-level runtime caches invalidated {package_manager} {case} readiness:\n{}",
                String::from_utf8_lossy(&cached_ready.stderr)
            );
            assert_eq!(
                fs::read(&dependency_marker).unwrap(),
                marker_before_runtime_caches,
                "runtime caches rewrote the {package_manager} {case} dependency marker"
            );
            assert_eq!(
                fs::read(&dependency_stamp).unwrap(),
                stamp_before_runtime_caches,
                "runtime caches rewrote the {package_manager} {case} dependency stamp"
            );

            if workspace_member {
                for cache_name in [".astro", ".cache", ".vite", ".vite-temp", ".tmp"] {
                    fs::remove_dir_all(runtime_node_modules.join(cache_name)).unwrap();
                }
                fs::remove_file(runtime_node_modules.join(".DS_Store")).unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "an empty runtime-created member node_modules invalidated {package_manager} readiness"
                );

                if case_name == "npm-package-lock" {
                    let unknown_empty = runtime_node_modules.join("unknown-empty-directory");
                    fs::create_dir(&unknown_empty).unwrap();
                    assert!(
                        !run("dependencies-ready").status.success(),
                        "an unknown empty workspace-member directory was normalized away"
                    );
                    fs::remove_dir(&unknown_empty).unwrap();
                    assert!(run("dependencies-ready").status.success());
                }
            }

            if !workspace_member && lockfile == "npm-shrinkwrap.json" {
                let inactive_lock = repo.join("apps/web/package-lock.json");
                fs::write(&inactive_lock, "inactive-app-lock-v1\n").unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "an app package-lock replaced its authoritative npm shrinkwrap"
                );
                fs::write(&inactive_lock, "inactive-app-lock-v2\n").unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "app receipt fingerprint included inactive package-lock content"
                );
            }

            fs::write(
                repo.join("apps/web/package.json"),
                r#"{"name":"web","version":"2","scripts":{"lint":"true"}}"#,
            )
            .unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "stale {package_manager} {case} stamp was accepted"
            );
            let install = run("dependencies-install");
            assert!(
                install.status.success(),
                "{package_manager} {case} frozen install failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&install.stdout),
                String::from_utf8_lossy(&install.stderr)
            );
            assert!(run("dependencies-ready").status.success());
            assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

            if workspace_member {
                let app_lock = repo.join("apps/web").join(lockfile);
                if package_manager == "yarn" {
                    fs::write(&app_lock, "# yarn lockfile v1\n").unwrap();
                    assert!(
                        run("dependencies-ready").status.success(),
                        "a Yarn Classic member lock incorrectly replaced its root workspace"
                    );

                    fs::write(&app_lock, "__metadata:\n  version: 8\n").unwrap();
                    assert!(
                        !run("dependencies-ready").status.success(),
                        "a nested Yarn Berry project reused root artifacts"
                    );
                    let app_bootstrap = std::process::Command::new("/bin/bash")
                        .args(["scripts/check-webapps.sh", "bootstrap"])
                        .current_dir(&repo)
                        .env("PATH", &path)
                        .env("INSTALL_COUNT", &install_count)
                        .env("INSTALL_CWD", &install_cwd)
                        .env("LOCK_NAME", lockfile)
                        .output()
                        .unwrap();
                    assert!(
                        app_bootstrap.status.success(),
                        "Yarn Berry nested-project bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&app_bootstrap.stdout),
                        String::from_utf8_lossy(&app_bootstrap.stderr)
                    );
                    assert_eq!(
                        fs::read_to_string(&install_cwd).unwrap().trim(),
                        fs::canonicalize(repo.join("apps/web"))
                            .unwrap()
                            .display()
                            .to_string()
                    );
                    assert!(run("dependencies-ready").status.success());
                    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "3");
                } else {
                    fs::write(&app_lock, "ignored-member-lock\n").unwrap();
                    assert!(
                        run("dependencies-ready").status.success(),
                        "{package_manager} let a nested member lock replace the root workspace"
                    );
                    let bootstrap = run("bootstrap");
                    assert!(bootstrap.status.success());
                    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
                    assert_eq!(
                        fs::read_to_string(&install_cwd).unwrap().trim(),
                        fs::canonicalize(&repo).unwrap().display().to_string()
                    );
                }
            }
        }
    }
}

#[cfg(unix)]
fn init_pnpm_dependency_checker_fixture(
    repo: &Path,
    template: &Path,
) -> (std::ffi::OsString, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    run_init(InitOpts {
        path: repo.to_path_buf(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("pnpm-checker-fixture".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("pnpm".into()),
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

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_pnpm = fake_bin.join("pnpm");
    fs::write(
        &fake_pnpm,
        r#"#!/bin/sh
set -eu
if [ -n "${PNPM_QUERY_MARKER:-}" ]; then printf '%s\n' "${1:-}" >> "$PNPM_QUERY_MARKER"; fi
case "${1:-}" in
  --version)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_VERSION_FILE:-}" ] && [ -f "$PNPM_VERSION_FILE" ]; then
      cat "$PNPM_VERSION_FILE"
    else
      printf '%s\n' "${PNPM_VERSION:-10.12.1}"
    fi
    ;;
  config)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    if [ -n "${PNPM_CONFIG_JSON:-}" ]; then printf '%s\n' "$PNPM_CONFIG_JSON"; exit 0; fi
    shared="${PNPM_SHARED_WORKSPACE_LOCKFILE:-true}"
    global="${PNPM_ENABLE_GLOBAL_VIRTUAL_STORE:-false}"
    case "$shared:$global" in
      true:true|true:false|false:true|false:false)
        printf '{"sharedWorkspaceLockfile":%s,"enableGlobalVirtualStore":%s}\n' "$shared" "$global"
        ;;
      true:undefined|false:undefined)
        printf '{"sharedWorkspaceLockfile":%s}\n' "$shared"
        ;;
      undefined:true|undefined:false)
        printf '{"enableGlobalVirtualStore":%s}\n' "$global"
        ;;
      undefined:undefined) printf '%s\n' '{}' ;;
      *) exit 2 ;;
    esac
    ;;
  pkg)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_PKG_JSON:-}" ]; then printf '%s\n' "$PNPM_PKG_JSON"; else printf '%s\n' '{}'; fi
    ;;
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    pwd -P > "$INSTALL_CWD"
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    if [ -n "${PNPM_DRIFT_TO:-}" ] && [ -n "${PNPM_VERSION_FILE:-}" ]; then
      printf '%s\n' "$PNPM_DRIFT_TO" > "$PNPM_VERSION_FILE"
    fi
    if [ "${PNPM_DRIFT_LOCK:-0}" = 1 ]; then
      printf '%s\n' drift >> pnpm-lock.yaml
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_pnpm, fs::Permissions::from_mode(0o755)).unwrap();

    let fake_corepack = fake_bin.join("corepack");
    fs::write(
        &fake_corepack,
        r#"#!/bin/sh
set -eu
[ "${1:-}" = pnpm@11.22.0 ] || exit 21
[ "${COREPACK_ENABLE_PROJECT_SPEC:-}" = 0 ] || exit 22
[ "${COREPACK_ENABLE_AUTO_PIN:-}" = 0 ] || exit 23
[ "${COREPACK_ENABLE_DOWNLOAD_PROMPT:-}" = 0 ] || exit 24
[ "${COREPACK_ENV_FILE:-}" = 0 ] || exit 25
[ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] || exit 26
[ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] || exit 27
[ "${NPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS:-}" = false ] || exit 28
[ "${PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS:-}" = false ] || exit 29
[ "${NPM_CONFIG_PM_ON_FAIL:-}" = ignore ] || exit 30
[ "${PNPM_CONFIG_PM_ON_FAIL:-}" = ignore ] || exit 31
[ "${NPM_CONFIG_RUNTIME_ON_FAIL:-}" = ignore ] || exit 32
[ "${PNPM_CONFIG_RUNTIME_ON_FAIL:-}" = ignore ] || exit 33
if [ -n "${COREPACK_QUERY_MARKER:-}" ]; then printf '%s\n' "$*" >> "$COREPACK_QUERY_MARKER"; fi
shift
exec "$(dirname "$0")/pnpm" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_corepack, fs::Permissions::from_mode(0o755)).unwrap();

    let mut path = std::ffi::OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    (path, repo.join("install-count"), repo.join("install-cwd"))
}

#[cfg(unix)]
fn run_pnpm_dependency_checker(
    repo: &Path,
    path: &std::ffi::OsStr,
    install_count: &Path,
    install_cwd: &Path,
    mode: &str,
) -> std::process::Output {
    std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", mode, "apps/web"])
        .current_dir(repo)
        .env("PATH", path)
        .env("INSTALL_COUNT", install_count)
        .env("INSTALL_CWD", install_cwd)
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn generated_pnpm_receipts_bind_the_runtime_and_reject_install_drift() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-runtime-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1+sha224.deadbeef"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/*']\n").unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let version_file = repo.join("fake-pnpm-version");
    fs::write(&version_file, "10.12.1\n").unwrap();

    let resolved_spec = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "package-manager-spec",
            "apps/web",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(resolved_spec.status.success());
    assert_eq!(
        String::from_utf8(resolved_spec.stdout).unwrap().trim(),
        "pnpm@10.12.1+sha224.deadbeef"
    );

    let run = |mode: &str, drift_to: Option<&str>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION_FILE", &version_file);
        if let Some(version) = drift_to {
            command.env("PNPM_DRIFT_TO", version);
        }
        command.output().unwrap()
    };

    let install = run("dependencies-install", None);
    assert!(
        install.status.success(),
        "runtime-bound install failed:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(run("dependencies-ready", None).status.success());

    let fake_pnpm = repo.join("fake-bin/pnpm");
    let original_fake_pnpm = fs::read(&fake_pnpm).unwrap();
    let mut changed_fake_pnpm = original_fake_pnpm.clone();
    changed_fake_pnpm.extend_from_slice(b"\n# executable identity changed\n");
    fs::write(&fake_pnpm, changed_fake_pnpm).unwrap();
    assert!(
        !run("dependencies-ready", None).status.success(),
        "changing pnpm executable content with the same version reused the receipt"
    );
    fs::write(&fake_pnpm, &original_fake_pnpm).unwrap();
    assert!(run("dependencies-ready", None).status.success());

    fs::write(&version_file, "11.13.1\n").unwrap();
    assert!(
        !run("dependencies-ready", None).status.success(),
        "changing only the effective pnpm runtime left the receipt ready"
    );
    fs::write(&version_file, "10.12.1\n").unwrap();
    assert!(run("dependencies-ready", None).status.success());

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","version":"2","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    let drifted = run("dependencies-install", Some("11.13.1"));
    assert!(
        !drifted.status.success(),
        "runtime drift during install was accepted"
    );
    assert!(String::from_utf8_lossy(&drifted.stderr).contains("changed during installation"));
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
    assert!(!repo.join("node_modules/.jig-web-dependencies-v3").exists());

    fs::write(&version_file, "10.12.1\n").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","version":"3","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    let lock_drift = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-install",
            "apps/web",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("INSTALL_COUNT", &install_count)
        .env("INSTALL_CWD", &install_cwd)
        .env("PNPM_VERSION_FILE", &version_file)
        .env("PNPM_DRIFT_LOCK", "1")
        .output()
        .unwrap();
    assert!(
        !lock_drift.status.success(),
        "frozen lockfile drift was accepted"
    );
    assert!(String::from_utf8_lossy(&lock_drift.stderr).contains("frozen install changed"));
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_rejects_global_virtual_store_layouts_and_environment_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-global-virtual-store-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\nenableGlobalVirtualStore: false\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let run = |version: &str,
               global_virtual_store: &str,
               override_key: Option<&str>,
               query_marker: Option<&Path>| {
        let mut command = std::process::Command::new("bash");
        command
            .args([
                "scripts/check-webapps.sh",
                "dependencies-install",
                "apps/web",
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", version)
            .env("PNPM_ENABLE_GLOBAL_VIRTUAL_STORE", global_virtual_store);
        if let Some(key) = override_key {
            command.env(key, "false");
        }
        if let Some(marker) = query_marker {
            command.env("PNPM_QUERY_MARKER", marker);
        }
        command.output().unwrap()
    };

    for (version, setting) in [
        ("10.12.1", "false"),
        ("10.12.1", "undefined"),
        ("11.13.1", "false"),
    ] {
        let output = run(version, setting, None, None);
        assert!(
            output.status.success(),
            "pnpm {version} with enable-global-virtual-store={setting} was rejected:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        "2",
        "pnpm 10 undefined did not normalize to the same false contract"
    );

    for (version, setting) in [
        ("10.12.1", "true"),
        ("11.13.1", "true"),
        ("11.13.1", "undefined"),
    ] {
        let output = run(version, setting, None, None);
        assert!(
            !output.status.success(),
            "pnpm {version} with enable-global-virtual-store={setting} was accepted"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("enableGlobalVirtualStore: false"),
            "{stderr}"
        );
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "2",
            "rejected pnpm layout reached install"
        );
    }

    for override_key in [
        "NpM_cOnFiG_EnAbLe_GlObAl_ViRtUaL_StOrE",
        "pNpM-cOnFiG-eNaBlE-gLoBaL-vIrTuAl-sToRe",
    ] {
        let query_marker = repo.join("pnpm-query-marker");
        if query_marker.exists() {
            fs::remove_file(&query_marker).unwrap();
        }
        let output = run("10.12.1", "false", Some(override_key), Some(&query_marker));
        assert!(
            !output.status.success(),
            "inherited {override_key} was accepted"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("environment override"), "{stderr}");
        assert!(
            stderr.contains("enableGlobalVirtualStore: false"),
            "{stderr}"
        );
        assert!(
            !query_marker.exists(),
            "pnpm metadata was queried before rejecting {override_key}"
        );
        assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
    }
}

#[cfg(unix)]
#[test]
fn generated_pnpm_binds_supported_layout_and_proves_repository_local_virtual_store() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-layout-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@11.13.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\nenableGlobalVirtualStore: false\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let base_config = r#"{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false,"allowBuilds":{"esbuild":true},"supportedArchitectures":{"cpu":["current","x64"]},"dedupeInjectedDeps":true,"nodeLinker":"isolated"}"#;
    let run = |mode: &str, config: &str, environment: Option<(&str, &str)>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", "11.13.1")
            .env("PNPM_CONFIG_JSON", config);
        if let Some((key, value)) = environment {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    assert!(
        run("dependencies-install", base_config, None)
            .status
            .success()
    );
    assert!(
        run("dependencies-ready", base_config, None)
            .status
            .success()
    );
    let changed_build_policy = base_config.replace("\"esbuild\":true", "\"esbuild\":false");
    assert!(
        !run("dependencies-ready", &changed_build_policy, None)
            .status
            .success(),
        "a bounded object-valued build/layout setting was omitted from the contract"
    );
    let changed_architectures = base_config.replace("\"x64\"", "\"arm64\"");
    assert!(
        !run("dependencies-ready", &changed_architectures, None)
            .status
            .success(),
        "supported pnpm architecture selection was omitted from the contract"
    );
    let changed_injected_dedupe = base_config.replace(
        "\"dedupeInjectedDeps\":true",
        "\"dedupeInjectedDeps\":false",
    );
    assert!(
        !run("dependencies-ready", &changed_injected_dedupe, None)
            .status
            .success(),
        "pnpm injected-dependency layout selection was omitted from the contract"
    );

    for config in [
        r#"{"enableGlobalVirtualStore":false,"nodeLinker":"pnp"}"#,
        r#"{"enableGlobalVirtualStore":false,"symlink":false}"#,
        r#"{"enableGlobalVirtualStore":false,"modulesDir":".mods"}"#,
        r#"{"enableGlobalVirtualStore":false,"virtualStoreOnly":true}"#,
        r#"{"enableGlobalVirtualStore":false,"nodeExperimentalPackageMap":true}"#,
        r#"{"enableGlobalVirtualStore":false,"nodePackageMapType":"mount"}"#,
        r#"{"enableGlobalVirtualStore":false,"virtualStoreDir":"../../outside"}"#,
    ] {
        assert!(
            !run("dependencies-install", config, None).status.success(),
            "unsupported pnpm layout was accepted: {config}"
        );
    }

    fs::create_dir_all(repo.join(".custom-store/pkg")).unwrap();
    fs::write(
        repo.join(".custom-store/pkg/package.json"),
        r#"{"name":"custom"}"#,
    )
    .unwrap();
    let custom_config = r#"{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false,"virtualStoreDir":".custom-store"}"#;
    assert!(
        run("dependencies-install", custom_config, None)
            .status
            .success()
    );
    assert!(
        run("dependencies-ready", custom_config, None)
            .status
            .success()
    );
    fs::write(
        repo.join(".custom-store/pkg/package.json"),
        r#"{"name":"changed"}"#,
    )
    .unwrap();
    assert!(
        !run("dependencies-ready", custom_config, None)
            .status
            .success(),
        "repository-local custom virtual store was not structurally proved"
    );

    let outside = temp.path().join("outside-virtual-store");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, repo.join("linked-store")).unwrap();
    let linked_config =
        r#"{"enableGlobalVirtualStore":false,"virtualStoreDir":"linked-store/content"}"#;
    assert!(
        !run("dependencies-install", linked_config, None)
            .status
            .success(),
        "symlink-mediated custom virtual store escaped repository ownership"
    );

    assert!(
        run(
            "dependencies-install",
            base_config,
            Some(("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", "false")),
        )
        .status
        .success()
    );
    assert!(
        run(
            "dependencies-ready",
            base_config,
            Some(("pnpm_config_verify_deps_before_run", "false")),
        )
        .status
        .success(),
        "case-insensitive raw layout override was not normalized consistently"
    );
    assert!(
        !run("dependencies-ready", base_config, None)
            .status
            .success(),
        "removing a bound ambient layout override left the receipt ready"
    );
    let ignored_hook = run(
        "dependencies-install",
        base_config,
        Some(("NpM_cOnFiG_iGnOrE_pNpMfIlE", "false")),
    );
    assert!(!ignored_hook.status.success());
    assert!(String::from_utf8_lossy(&ignored_hook.stderr).contains("metadata-hook"));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_patch_sources_follow_runtime_major_and_shared_lock_mode() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-patch-source-matrix");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("custom")).unwrap();
    fs::write(repo.join("custom/root.patch"), "root\n").unwrap();
    fs::write(repo.join("custom/yaml.patch"), "yaml\n").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"root@1":"custom/root.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"},"pnpm":{"patchedDependencies":{"member@1":"missing-member.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\npatchedDependencies:\n  yaml@1: custom/missing-yaml.patch\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let run = |mode: &str, version: &str, shared: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", version)
            .env("PNPM_SHARED_WORKSPACE_LOCKFILE", shared)
            .output()
            .unwrap()
    };

    assert!(
        run("dependencies-install", "10.12.1", "true")
            .status
            .success(),
        "pnpm 10 shared install did not prefer root legacy metadata"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let member_active = run("dependencies-install", "10.12.1", "false");
    assert!(!member_active.status.success());
    assert!(String::from_utf8_lossy(&member_active.stderr).contains("missing-member.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    assert!(
        run("dependencies-install", "10.12.1", "false")
            .status
            .success(),
        "pnpm 10 unshared project did not inherit the root legacy map"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    let yaml_active = run("dependencies-install", "11.13.1", "true");
    assert!(!yaml_active.status.success());
    assert!(String::from_utf8_lossy(&yaml_active.stderr).contains("missing-yaml.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"ignored@1":"missing-root.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\npatchedDependencies:\n  yaml@1: custom/yaml.patch\n",
    )
    .unwrap();
    assert!(
        run("dependencies-install", "11.13.1", "true")
            .status
            .success(),
        "pnpm 11 root install did not use only workspace YAML patches"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "3");

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"},"pnpm":{"patchedDependencies":{"app@1":"missing-app.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['tools/*']\npatchedDependencies:\n  parent@1: missing-parent.patch\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    assert!(
        run("dependencies-install", "11.13.1", "true")
            .status
            .success(),
        "pnpm 11 standalone install activated ignored legacy/YAML patches"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "4");
    let v10_standalone = run("dependencies-install", "10.12.1", "true");
    assert!(!v10_standalone.status.success());
    assert!(String::from_utf8_lossy(&v10_standalone.stderr).contains("missing-app.patch"));
    assert!(!String::from_utf8_lossy(&v10_standalone.stderr).contains("missing-parent.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "4");
}

#[cfg(unix)]
#[test]
fn generated_pnpm_reads_alternate_manifests_without_loading_pnpmfile_hooks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-alternate-manifests");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/yaml")).unwrap();
    fs::create_dir_all(repo.join("apps/json5")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\ndependencies:\n  dep: '1'\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/json5/package.json5"),
        "{ name: 'json5-app', dependencies: { dep: '1' } }\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/*']\n").unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let hook_marker = repo.join("pnpmfile-ran");
    fs::write(
        repo.join(".pnpmfile.cjs"),
        format!(
            "require('node:fs').writeFileSync({}, 'ran')\nmodule.exports = {{}}\n",
            serde_json::to_string(&hook_marker).unwrap()
        ),
    )
    .unwrap();

    let run = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/yaml"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env(
                "PNPM_PKG_JSON",
                r#"{"name":"alternate","dependencies":{"dep":"1"}}"#,
            )
            .output()
            .unwrap()
    };

    assert_eq!(run("dependencies-ready").status.code(), Some(1));
    let install = run("dependencies-install");
    assert!(
        install.status.success(),
        "alternate-manifest install failed:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(
        !hook_marker.exists(),
        "pnpmfile hook ran during manifest inspection"
    );
    assert!(run("dependencies-ready").status.success());

    for (relative, changed) in [
        (
            "apps/yaml/package.yaml",
            "name: yaml-app\nversion: '2'\ndependencies:\n  dep: '1'\n",
        ),
        (
            "apps/json5/package.json5",
            "{ name: 'json5-app', version: '2', dependencies: { dep: '1' } }\n",
        ),
    ] {
        let manifest = repo.join(relative);
        let original = fs::read(&manifest).unwrap();
        fs::write(&manifest, changed).unwrap();
        let stale = run("dependencies-ready");
        assert!(
            !stale.status.success(),
            "selected alternate manifest {relative} was omitted from the fingerprint"
        );
        assert_eq!(stale.status.code(), Some(1));
        fs::write(&manifest, original).unwrap();
        assert!(run("dependencies-ready").status.success());
    }
    assert!(!hook_marker.exists());

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['tools/*']\n").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@9.9.9"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\npackageManager: pnpm@10.99.0\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/yaml/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    let corepack_marker = repo.join("corepack-query");
    let resolve_alternate = |app: &str, payload: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "package-manager-spec", app])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("PNPM_PKG_JSON", payload)
            .env("COREPACK_QUERY_MARKER", &corepack_marker)
            .env("npm_config_ignore_pnpmfile", "hostile")
            .env("pnpm_config_ignore_pnpmfile", "hostile")
            .env("npm_config_pm_on_fail", "error")
            .env("pnpm_config_runtime_on_fail", "error")
            .output()
            .unwrap()
    };

    let top_level = resolve_alternate("apps/yaml", r#"{"packageManager":"pnpm@10.99.0"}"#);
    assert_eq!(top_level.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(top_level.stdout).unwrap().trim(),
        "pnpm@10.99.0"
    );
    assert_eq!(
        fs::read_to_string(&corepack_marker).unwrap().trim(),
        "pnpm@11.22.0 pkg get packageManager devEngines.packageManager --json --ignore-workspace"
    );

    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\ndevEngines:\n  packageManager:\n    name: pnpm\n    version: 10.98.0\n",
    )
    .unwrap();
    let dev_engine = resolve_alternate(
        "apps/yaml",
        r#"{"devEngines.packageManager":{"name":"pnpm","version":"10.98.0"}}"#,
    );
    assert_eq!(dev_engine.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(dev_engine.stdout).unwrap().trim(),
        "pnpm@10.98.0"
    );

    fs::write(
        repo.join("apps/json5/package.json5"),
        "{ name: 'json5-app', packageManager: 'pnpm@10.97.0' }\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/json5/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    let json5 = resolve_alternate("apps/json5", r#"{"packageManager":"pnpm@10.97.0"}"#);
    assert_eq!(json5.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(json5.stdout).unwrap().trim(),
        "pnpm@10.97.0"
    );

    fs::write(
        repo.join("apps/yaml/package.json"),
        r#"{"name":"json-wins"}"#,
    )
    .unwrap();
    let precedence = resolve_alternate("apps/yaml", r#"{"packageManager":"pnpm@10.97.0"}"#);
    assert_eq!(precedence.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(precedence.stdout).unwrap().trim(),
        "pnpm@9.9.9",
        "a lower-priority package.yaml must not contribute authority beside package.json"
    );
    fs::remove_file(repo.join("apps/yaml/package.json")).unwrap();

    let wrong_manager = resolve_alternate("apps/yaml", r#"{"packageManager":"yarn@4.18.0"}"#);
    assert_eq!(wrong_manager.status.code(), Some(2));
    let invalid_readiness = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-ready",
            "apps/yaml",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("PNPM_PKG_JSON", r#"{"packageManager":"yarn@4.18.0"}"#)
        .output()
        .unwrap();
    assert_eq!(
        invalid_readiness.status.code(),
        Some(2),
        "an absent receipt must not downgrade invalid pnpm manifest authority"
    );
    let malformed = resolve_alternate("apps/yaml", "{}\n{}");
    assert_eq!(malformed.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_globstars_include_zero_segment_members() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-zero-segment-globstar");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/package.json"),
        r#"{"name":"apps-root","version":"1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*/**']\n",
    )
    .unwrap();
    let immediate = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        immediate.status.success(),
        "apps/*/** did not include the immediate apps/web member:\n{}",
        String::from_utf8_lossy(&immediate.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(&repo).unwrap().display().to_string()
    );

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: [' apps/*']\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        )
        .status
        .success(),
        "quoted workspace-pattern whitespace was incorrectly trimmed"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/**']\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        )
        .status
        .success()
    );
    fs::write(
        repo.join("apps/package.json"),
        r#"{"name":"apps-root","version":"2"}"#,
    )
    .unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "apps/** omitted the zero-segment apps/package.json member"
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_walk_is_bounded_to_relevant_branches() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-bounded-workspace-walk");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web/public")).unwrap();
    fs::create_dir_all(repo.join("shared-assets")).unwrap();
    fs::create_dir_all(repo.join("vendor")).unwrap();
    fs::create_dir_all(repo.join("apps/excluded")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', '!apps/excluded']\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    for index in 0..10_050 {
        fs::write(repo.join(format!("vendor/unrelated-{index}")), b"").unwrap();
        fs::write(repo.join(format!("apps/excluded/unrelated-{index}")), b"").unwrap();
    }
    symlink(
        "../../../shared-assets",
        repo.join("apps/web/public/assets"),
    )
    .unwrap();

    let run =
        |mode: &str| run_pnpm_dependency_checker(&repo, &path, &install_count, &install_cwd, mode);
    let install = run("dependencies-install");
    assert!(
        install.status.success(),
        "unrelated/excluded trees or a terminal-member symlink broke discovery:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );

    fs::remove_file(repo.join("apps/web/public/assets")).unwrap();
    fs::create_dir_all(repo.join("bower_components")).unwrap();
    for index in 0..10_050 {
        fs::write(repo.join(format!("bower_components/ignored-{index}")), b"").unwrap();
    }
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['**', '!vendor/**', '!apps/excluded/**']\n",
    )
    .unwrap();
    let broad_install = run("dependencies-install");
    assert!(
        broad_install.status.success(),
        "bower_components affected a broad pnpm workspace walk:\n{}",
        String::from_utf8_lossy(&broad_install.stderr)
    );
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', '!apps/excluded']\n",
    )
    .unwrap();
    assert!(run("dependencies-install").status.success());

    symlink("../shared-assets", repo.join("apps/selected-link")).unwrap();
    let selected_link = run("dependencies-ready");
    assert!(!selected_link.status.success());
    assert!(String::from_utf8_lossy(&selected_link.stderr).contains("symbolic link"));
    fs::remove_file(repo.join("apps/selected-link")).unwrap();
    assert!(run("dependencies-ready").status.success());

    for index in 0..10_050 {
        fs::create_dir(repo.join(format!("apps/relevant-{index}"))).unwrap();
    }
    let capped = run("dependencies-ready");
    assert!(!capped.status.success());
    assert!(String::from_utf8_lossy(&capped.stderr).contains("relevant filesystem entries"));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_metadata_uses_root_keys_and_keeps_selected_build_names() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-workspace-metadata");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"inactive-parent@1":"missing-parent-legacy.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"},"pnpm":{"patchedDependencies":{"active@1":"patches/active.patch"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(repo.join("apps/web/patches")).unwrap();
    fs::write(repo.join("apps/web/patches/active.patch"), "active-v1\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-workspace.yaml"),
        "patchedDependencies:\n  inactive-app-workspace@1: missing-app-workspace.patch\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "  \"packages\":\n    - 'tools/*'\n  'patchedDependencies':\n    inactive@1: missing-parent.patch\n  catalogs:\n    default:\n      packages: '^1.0.0'\n",
    )
    .unwrap();

    let inactive_local_yaml = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        !inactive_local_yaml.status.success(),
        "scope-local YAML patches that --ignore-workspace cannot apply were accepted"
    );
    let stderr = String::from_utf8_lossy(&inactive_local_yaml.stderr);
    assert!(stderr.contains("apps/web/pnpm-workspace.yaml"), "{stderr}");
    assert!(stderr.contains("--ignore-workspace"), "{stderr}");
    assert!(!install_count.exists());

    fs::remove_file(repo.join("apps/web/pnpm-workspace.yaml")).unwrap();
    let standalone = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        standalone.status.success(),
        "standalone legacy patches failed after removing inactive local YAML:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&standalone.stdout),
        String::from_utf8_lossy(&standalone.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(repo.join("apps/web"))
            .unwrap()
            .display()
            .to_string()
    );

    fs::write(repo.join("apps/web/patches/active.patch"), "active-v2\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "the active app-local legacy patch was omitted from the fingerprint"
    );
    fs::write(repo.join("apps/web/patches/active.patch"), "active-v1\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring the app-local legacy patch did not restore readiness"
    );

    let app_scope = repo.join("apps/web");
    let real_app_scope = repo.join("apps/web-real");
    fs::rename(&app_scope, &real_app_scope).unwrap();
    symlink(&real_app_scope, &app_scope).unwrap();
    let linked_scope = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_scope.status.success());
    assert!(
        String::from_utf8_lossy(&linked_scope.stderr).contains("symbolic links"),
        "symlinked app-scope ancestry omitted its diagnostic:\n{}",
        String::from_utf8_lossy(&linked_scope.stderr)
    );
    fs::remove_file(&app_scope).unwrap();
    fs::rename(&real_app_scope, &app_scope).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring real app-scope ancestry did not restore readiness"
    );

    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();

    let mut selected_manifests = Vec::new();
    for relative in [
        "apps/dist/package.json",
        "apps/coverage/package.json",
        "apps/target/package.json",
        "target/worker/package.json",
    ] {
        let manifest = repo.join(relative);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, r#"{"name":"selected","version":"1"}"#).unwrap();
        selected_manifests.push(manifest);
    }
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "\u{feff}---\n  \"packages\":\n    - 'apps/*'\n    - 'target/*'\n    - \"!apps/excluded\"\n  catalogs:\n    default:\n      packages: '^1.0.0'\n      patchedDependencies: missing.patch\n",
    )
    .unwrap();

    let valid_root_workspace = fs::read_to_string(repo.join("pnpm-workspace.yaml")).unwrap();

    let root_install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        root_install.status.success(),
        "root pnpm install failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&root_install.stdout),
        String::from_utf8_lossy(&root_install.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    let apps = repo.join("apps");
    let real_apps = repo.join("apps-real");
    fs::rename(&apps, &real_apps).unwrap();
    symlink(&real_apps, &apps).unwrap();
    let linked_workspace_ancestor = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_workspace_ancestor.status.success());
    assert!(
        String::from_utf8_lossy(&linked_workspace_ancestor.stderr).contains("symbolic link"),
        "selected workspace-member ancestor symlink omitted its diagnostic:\n{}",
        String::from_utf8_lossy(&linked_workspace_ancestor.stderr)
    );
    fs::remove_file(&apps).unwrap();
    fs::rename(&real_apps, &apps).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring the real workspace-member ancestor did not restore readiness"
    );

    for manifest in selected_manifests {
        fs::write(&manifest, r#"{"name":"selected","version":"2"}"#).unwrap();
        assert!(
            !run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "selected build-name manifest {} was omitted",
            manifest.display()
        );
        fs::write(&manifest, r#"{"name":"selected","version":"1"}"#).unwrap();
        assert!(
            run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "restoring {} did not restore readiness",
            manifest.display()
        );
    }

    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', 'target/*',]\n",
    )
    .unwrap();
    let trailing_comma = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "package-manager-spec",
    );
    assert!(
        trailing_comma.status.success(),
        "valid YAML flow trailing comma was rejected:\n{}",
        String::from_utf8_lossy(&trailing_comma.stderr)
    );
    fs::write(repo.join("pnpm-workspace.yaml"), &valid_root_workspace).unwrap();

    let invalid_workspaces = [
        (
            "packages:\n  - 'apps/*'\n\"packages\":\n  - 'target/*'\n",
            "declares packages more than once",
        ),
        ("{packages: ['apps/*']}\n", "root flow mappings"),
        ("- packages: ['apps/*']\n", "root block sequences"),
        ("? packages\n: ['apps/*']\n", "explicit mapping keys"),
        ("packages: [{x: y}]\n", "unsupported YAML collection syntax"),
        (
            "packages:\n  - key: value\n",
            "unsupported YAML collection syntax",
        ),
        (
            "packages:\n  - - nested\n",
            "unsupported YAML collection syntax",
        ),
        ("packages:\n  -apps/*\n", "non-string sequence entry"),
        (
            "packages: [, 'apps/*']\n",
            "packages contains an empty glob",
        ),
        (
            "packages: ['apps/*',,]\n",
            "packages contains an empty glob",
        ),
        (
            "packages:\n  - &web 'apps/*'\n",
            "unsupported YAML node properties",
        ),
        ("packages:\n  - *web\n", "unsupported YAML node properties"),
        (
            "packages:\n  - !!str 'apps/*'\n",
            "unsupported YAML node properties",
        ),
        (
            "packages:\n  - |-\n      apps/*\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  dep@1: &patch custom.patch\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  &selector dep@1: custom.patch\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  dep@1: custom.patch\n  \"dep@1\": custom.patch\n",
            "declares patchedDependencies selector \"dep@1\" more than once",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies: {dep@1: custom.patch}\n",
            "patchedDependencies must be a block mapping",
        ),
        (
            "packages: ['apps/*']\n---\npatchedDependencies:\n  dep@1: custom.patch\n",
            "multiple YAML documents",
        ),
        ("packages: ['apps/*']\n...\n", "document end markers"),
    ];
    for (workspace, diagnostic) in invalid_workspaces {
        fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();
        let invalid = run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        );
        assert!(
            !invalid.status.success(),
            "invalid YAML was accepted: {workspace}"
        );
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains(diagnostic),
            "invalid YAML omitted {diagnostic:?}:\n{}",
            String::from_utf8_lossy(&invalid.stderr)
        );
    }
    fs::write(repo.join("pnpm-workspace.yaml"), valid_root_workspace).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring valid indented and quoted pnpm workspace YAML did not restore readiness"
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_custom_patch_fingerprints_are_required_and_repository_bounded() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-custom-patches");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("custom")).unwrap();
    fs::create_dir_all(repo.join("linked-parent")).unwrap();
    let root_manifest =
        r#"{"private":true,"pnpm":{"patchedDependencies":{"legacy@1":"custom/root.patch"}}}"#;
    let member_manifest = r#"{"name":"web","scripts":{"lint":"true"},"pnpm":{"patchedDependencies":{"member@1":"../../custom/member.patch"}}}"#;
    fs::write(repo.join("package.json"), root_manifest).unwrap();
    fs::write(repo.join("apps/web/package.json"), member_manifest).unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    for (relative, contents) in [
        ("custom/yaml.patch", "yaml-v1\n"),
        ("custom/root.patch", "root-v1\n"),
        ("custom/member.patch", "member-v1\n"),
        ("linked-parent/parent.patch", "parent-v1\n"),
        ("custom/unreferenced.patch", "unreferenced-v1\n"),
    ] {
        fs::write(repo.join(relative), contents).unwrap();
    }
    let workspace = "packages:\n  - 'apps/*'\npatchedDependencies:\n  yaml@1: custom/yaml.patch\n  parent@1: linked-parent/parent.patch\n";
    fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();

    let install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        install.status.success(),
        "custom-patch install failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let active_patch = "custom/root.patch";
    let patch = repo.join(active_patch);
    let original = fs::read(&patch).unwrap();
    fs::write(&patch, "changed patch\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "configured patch {active_patch} was omitted from the fingerprint"
    );
    fs::write(&patch, original).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring configured patch {active_patch} did not restore readiness"
    );

    for relative in [
        "custom/yaml.patch",
        "custom/member.patch",
        "linked-parent/parent.patch",
    ] {
        let patch = repo.join(relative);
        let original = fs::read(&patch).unwrap();
        fs::write(&patch, "inactive patch changed\n").unwrap();
        assert!(
            run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "inactive pnpm 10 patch source {relative} changed readiness"
        );
        fs::write(&patch, original).unwrap();
    }

    fs::write(repo.join("custom/unreferenced.patch"), "unreferenced-v2\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "an unreferenced custom patch changed the fingerprint"
    );

    let root_patch = repo.join("custom/root.patch");
    let root_contents = fs::read(&root_patch).unwrap();
    fs::remove_file(&root_patch).unwrap();
    let missing = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!missing.status.success());
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
    fs::write(&root_patch, &root_contents).unwrap();

    let symlink_target = repo.join("custom/root-real.patch");
    fs::rename(&root_patch, &symlink_target).unwrap();
    symlink(&symlink_target, &root_patch).unwrap();
    let linked_file = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_file.status.success());
    assert_eq!(linked_file.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&linked_file.stderr).contains("symbolic links"));
    fs::remove_file(&root_patch).unwrap();
    fs::rename(&symlink_target, &root_patch).unwrap();

    let linked_parent = repo.join("custom");
    let real_parent = repo.join("custom-real");
    fs::rename(&linked_parent, &real_parent).unwrap();
    symlink(&real_parent, &linked_parent).unwrap();
    let linked_directory = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_directory.status.success());
    assert_eq!(linked_directory.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&linked_directory.stderr).contains("symbolic links"));
    fs::remove_file(&linked_parent).unwrap();
    fs::rename(&real_parent, &linked_parent).unwrap();

    let outside_patch = temp.path().join("outside.patch");
    fs::write(&outside_patch, "outside\n").unwrap();
    // Without root legacy metadata, pnpm 10 falls back to the workspace YAML.
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    let invalid_paths = [
        ("../outside.patch".to_owned(), "escapes the repository"),
        (
            format!("'{}'", outside_patch.display()),
            "must be repository-relative",
        ),
        ("custom".to_owned(), "must be a file"),
        ("false".to_owned(), "must be a string"),
    ];
    for (configured_path, diagnostic) in invalid_paths {
        fs::write(
            repo.join("pnpm-workspace.yaml"),
            format!("packages:\n  - 'apps/*'\npatchedDependencies:\n  yaml@1: {configured_path}\n"),
        )
        .unwrap();
        let invalid = run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        );
        assert!(
            !invalid.status.success(),
            "invalid patch path {configured_path} was accepted"
        );
        assert_eq!(
            invalid.status.code(),
            Some(2),
            "invalid patch path {configured_path} was not a hard authority error"
        );
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains(diagnostic),
            "invalid patch path {configured_path} omitted {diagnostic:?}:\n{}",
            String::from_utf8_lossy(&invalid.stderr)
        );
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "1",
            "the installer ran for invalid patch path {configured_path}"
        );
    }

    fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":[]}}"#,
    )
    .unwrap();
    let malformed_manifest = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(!malformed_manifest.status.success());
    assert_eq!(malformed_manifest.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&malformed_manifest.stderr)
            .contains("pnpm.patchedDependencies must be an object")
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(repo.join("package.json"), root_manifest).unwrap();
    let restored_readiness = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert_eq!(
        restored_readiness.status.code(),
        Some(0),
        "restoring exact patch authority did not reattest the existing receipt:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&restored_readiness.stdout),
        String::from_utf8_lossy(&restored_readiness.stderr)
    );
    let restored_install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        restored_install.status.success(),
        "restoring valid patch metadata did not reuse the existing receipt:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&restored_install.stdout),
        String::from_utf8_lossy(&restored_install.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        "1",
        "restoring exact authority unnecessarily reran the package manager"
    );
}
