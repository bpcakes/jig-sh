use super::*;

#[cfg(unix)]
#[test]
fn generated_web_checks_track_npm_package_lock_install_state() {
    generated_web_checks_track_lockfile_install_state(
        "npm",
        "npm",
        "package-lock.json",
        "node_modules",
        "lock-v1\n",
        false,
        None,
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_track_npm_shrinkwrap_install_state() {
    generated_web_checks_track_lockfile_install_state(
        "npm-shrinkwrap",
        "npm",
        "npm-shrinkwrap.json",
        "node_modules",
        "shrinkwrap-v1\n",
        false,
        None,
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_track_modern_yarn_pnp_install_state() {
    generated_web_checks_track_lockfile_install_state(
        "yarn-modern",
        "yarn",
        "yarn.lock",
        ".pnp.cjs",
        "__metadata:\n  version: 8\n",
        false,
        None,
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_track_classic_yarn_pnp_install_state() {
    generated_web_checks_track_lockfile_install_state(
        "yarn-classic",
        "yarn",
        "yarn.lock",
        ".pnp.js",
        "# yarn lockfile v1\n",
        true,
        None,
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_track_yarn_node_modules_install_state() {
    generated_web_checks_track_lockfile_install_state(
        "yarn-node-modules",
        "yarn",
        "yarn.lock",
        "node_modules",
        "__metadata:\n  version: 8\n",
        false,
        Some("nodeLinker: node-modules\n"),
    );
}

#[cfg(unix)]
fn generated_web_checks_track_lockfile_install_state(
    case_name: &str,
    package_manager: &str,
    lockfile: &str,
    artifact: &str,
    initial_lock: &str,
    classic_pnp: bool,
    yarn_config: Option<&str>,
) {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let repo = temp.path().join(case_name);
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
            repo_name: Some(format!("sentinel-{case_name}")),
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
    let package_json = r#"{"private":true,"workspaces":["apps/web"]}"#;
    fs::write(repo.join("package.json"), package_json).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join(".node-version"), "22.22.2\n").unwrap();
    fs::write(repo.join(lockfile), initial_lock).unwrap();
    if package_manager == "yarn" {
        fs::write(
            repo.join(".yarnrc"),
            if classic_pnp {
                "--install.pure-lockfile false\n--pnp true\n"
            } else {
                "--install.pure-lockfile false\n"
            },
        )
        .unwrap();
        if let Some(config) = yarn_config {
            fs::write(repo.join(".yarnrc.yml"), config).unwrap();
        }
        for runtime_file in [
            ".yarn/patches/dependency.patch",
            ".yarn/plugins/plugin.cjs",
            ".yarn/releases/yarn.cjs",
        ] {
            let path = repo.join(runtime_file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "runtime-v1\n").unwrap();
        }
    }

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_node = fake_bin.join("node");
    fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-managed-npm" ]; then
  launcher_operation="$3"
  app_dir="$4"
  operation_argument="$5"
  cd "$app_dir"
  case "$launcher_operation" in
    install)
      case "$operation_argument" in
        frozen) operation=ci ;;
        bootstrap) operation=install ;;
        *) exit 2 ;;
      esac
      unset NODE_ENV NPM_CONFIG_OMIT NPM_CONFIG_INCLUDE NPM_CONFIG_PRODUCTION NPM_CONFIG_OPTIONAL
      unset NPM_CONFIG_ONLY NPM_CONFIG_DEV NPM_CONFIG_ALSO
      unset Npm_Config_Bin_Links npm_CONFIG_dry_run NPM_CONFIG_PACKAGE_LOCK_ONLY
      unset NPM_CONFIG_PACKAGE_LOCK NPM_CONFIG_GLOBAL NPM_CONFIG_WORKSPACE NPM_CONFIG_WORKSPACES
      unset NPM_CONFIG_INCLUDE_WORKSPACE_ROOT NPM_CONFIG_PREFIX NPM_CONFIG_LOCATION NPM_CONFIG_IF_PRESENT
      unset NPM_CONFIG_CPU NPM_CONFIG_OS NPM_CONFIG_LIBC
      set -- npm "$operation" \
        --include=dev --include=optional --include=peer \
        --bin-links=true --dry-run=false --package-lock-only=false \
        --package-lock=true --global=false --location=project \
        "--prefix=$(pwd -P)" --cpu=test-cpu --os=test-platform
      if [ "$app_dir" = "." ]; then
        set -- "$@" --workspaces=true --include-workspace-root=true
      else
        set -- "$@" --workspaces=false
      fi
      exec "$@"
      ;;
    run-script)
      unset NPM_CONFIG_OMIT NPM_CONFIG_INCLUDE NPM_CONFIG_PRODUCTION NPM_CONFIG_OPTIONAL
      unset NPM_CONFIG_ONLY NPM_CONFIG_DEV NPM_CONFIG_ALSO
      unset NPM_CONFIG_GLOBAL NPM_CONFIG_WORKSPACE NPM_CONFIG_WORKSPACES
      unset NPM_CONFIG_INCLUDE_WORKSPACE_ROOT NPM_CONFIG_PREFIX NPM_CONFIG_LOCATION NPM_CONFIG_IF_PRESENT
      exec npm --prefix=. --workspace=. --workspaces=true --include-workspace-root=true \
        --global=false --location=project --if-present=false \
        --include=dev --include=optional --include=peer run "$operation_argument"
      ;;
    *) exit 2 ;;
  esac
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-lockfile-kind" ]; then
  lockfile="${3:-}"
  [ -n "$lockfile" ] && [ -f "$lockfile" ] && [ ! -L "$lockfile" ] || exit 1
  if tr -d '\r' < "$lockfile" | grep -Eq '^# yarn lockfile v1$'; then
    printf '%s\n' classic
  else
    printf '%s\n' berry
  fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-authority-preflight" ]; then
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-config" ]; then
  printf '%s\n' 'classic:dGVzdA=='
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-pnp-proof" ]; then
  [ -s "$4" ] || exit 1
  cksum "$4" | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-manifest" ]; then
  manifest="$3"
  if tr '\n' ' ' < "$manifest" | grep -Eq '"installConfig"[[:space:]]*:[[:space:]]*\{[^}]*"pnp"[[:space:]]*:[[:space:]]*true'; then
    printf '%s\n' true
    exit 0
  fi
  exit 1
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-berry-config" ]; then
  linker=pnp
  if [ -f .yarnrc.yml ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*(node-modules|pnpm)' .yarnrc.yml; then
    linker=node-modules
  fi
  config="{\"nodeLinker\":\"$linker\",\"cacheFolder\":\"$(pwd)/.yarn/cache\",\"installStatePath\":\"$(pwd)/.yarn/install-state.gz\",\"pnpUnpluggedFolder\":\"$(pwd)/.yarn/unplugged\",\"pnpEnableInlining\":false,\"pnpEnableEsmLoader\":false}"
  printf '%s' "$config" | base64 | tr -d '\n'
  printf '\n'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-config-value" ]; then
  config="$(printf '%s' "$3" | base64 --decode 2>/dev/null || printf '%s' "$3" | base64 -D)"
  case "$4" in
    nodeLinker) printf '%s\n' "$config" | sed -n 's/.*"nodeLinker":"\([^"]*\)".*/\1/p' ;;
    *) exit 1 ;;
  esac
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-pnp-proof" ]; then
  scope="$3"
  for required in "$4" "$scope/.pnp.data.json" "$scope/.yarn/install-state.gz" "$scope/.yarn/cache/dependency.zip"; do
    [ -s "$required" ] && [ ! -L "$required" ] || exit 1
    cksum "$required"
  done | cksum | awk '{print $1}'
  exit 0
fi

if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-node-modules-proof" ]; then
  root="$3/node_modules"
  entries="$(find "$root" -mindepth 1 \( -type f -o -type l \) ! -name '.jig-web-dependencies-v3' ! -name '.jig-web-dependencies-v3.tmp.*' ! -path "$root/.cache/*" ! -path "$root/.vite/*" ! -path "$root/.tmp/*" ! -name '.DS_Store' -print | LC_ALL=C sort)"
  [ -n "$entries" ] || exit 1
  printf '%s\n' "$entries" | while IFS= read -r entry; do
    relative="${entry#"$root"/}"
    if [ -L "$entry" ]; then
      printf 'link %s %s\n' "$relative" "$(readlink "$entry")"
    else
      printf 'file %s %s\n' "$relative" "$(wc -c < "$entry" | tr -d ' ')"
      [ "${entry##*/}" != "package.json" ] || cksum "$entry"
    fi
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ]; then
  shift
  for file in "$@"; do
    if [ -f "$file" ]; then cksum "$file"; fi
    if [ -d "$file" ]; then
      find "$file" -type f -print | LC_ALL=C sort | while IFS= read -r nested; do
        printf '%s\n' "$nested"
        cksum "$nested"
      done
    fi
  done | cksum | awk '{print $1}'
fi
exit 0
"#,
        )
        .unwrap();
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).unwrap();

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
    printf '%s\n' "$@" > "$INSTALL_ARGV"
    if [ "$(basename "$0")" = npm ]; then env | LC_ALL=C sort > "$INSTALL_ENV"; fi
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALL_COUNT"
    if [ "${FAIL_INSTALL:-0}" = "1" ]; then exit 9; fi
    if [ ! -f "$TEST_LOCKFILE" ] && { [ "$1" != ci ] || [ "$(basename "$0")" != npm ]; }; then
      printf '%s\n' "lock-v1" > "$TEST_LOCKFILE"
    fi
    if [ "$TEST_ARTIFACT" = "node_modules" ]; then
      mkdir -p node_modules/test-package
      printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    elif [ "$TEST_PACKAGE_MANAGER" = "yarn" ]; then
      if [ "$TEST_ARTIFACT" = ".pnp.cjs" ]; then
        printf '%s\n' 'generated pnp loader using .pnp.data.json' > "$TEST_ARTIFACT"
        printf '%s\n' '{"dependencyTreeRoots":[]}' > .pnp.data.json
        mkdir -p .yarn/cache
        printf '%s\n' archive > .yarn/cache/dependency.zip
        printf '%s\n' state > .yarn/install-state.gz
      else
        printf '%s\n' 'generated pnp loader' > "$TEST_ARTIFACT"
      fi
    else
      exit 3
    fi
    ;;
  --prefix=.)
    [ "$#" -eq 12 ]
    [ "$2" = --workspace=. ]
    [ "$3" = --workspaces=true ]
    [ "$4" = --include-workspace-root=true ]
    [ "$5" = --global=false ]
    [ "$6" = --location=project ]
    [ "$7" = --if-present=false ]
    [ "$8" = --include=dev ]
    [ "$9" = --include=optional ]
    [ "${10}" = --include=peer ]
    [ "${11}" = run ]
    [ "${12}" = lint ]
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
    fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_argv = repo.join("install-argv");
    let install_env = repo.join("install-env");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run_mode = |mode: &str, fail_install: bool| {
        let output = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode])
            .current_dir(&repo)
            .env("NODE", &fake_node)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_ARGV", &install_argv)
            .env("INSTALL_ENV", &install_env)
            .env("TEST_LOCKFILE", lockfile)
            .env("TEST_PACKAGE_MANAGER", package_manager)
            .env("TEST_ARTIFACT", artifact)
            .env("FAIL_INSTALL", if fail_install { "1" } else { "0" })
            .env("NODE_ENV", "production")
            .env("NPM_CONFIG_OMIT", "dev optional peer")
            .env("NPM_CONFIG_INCLUDE", "prod")
            .env("NPM_CONFIG_PRODUCTION", "true")
            .env("NPM_CONFIG_OPTIONAL", "false")
            .env("NPM_CONFIG_ONLY", "production")
            .env("NPM_CONFIG_DEV", "false")
            .env("NPM_CONFIG_ALSO", "production")
            .env("Npm_Config_Bin_Links", "false")
            .env("npm_CONFIG_dry_run", "true")
            .env("NPM_CONFIG_PACKAGE_LOCK_ONLY", "true")
            .env("NPM_CONFIG_PACKAGE_LOCK", "false")
            .env("NPM_CONFIG_GLOBAL", "true")
            .env("NPM_CONFIG_WORKSPACE", "other")
            .env("NPM_CONFIG_WORKSPACES", "false")
            .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
            .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
            .env("NPM_CONFIG_LOCATION", "global")
            .env("NPM_CONFIG_IF_PRESENT", "true")
            .env("NPM_CONFIG_CPU", "hostile-cpu")
            .env("NPM_CONFIG_OS", "hostile-os")
            .env("NPM_CONFIG_LIBC", "hostile-libc")
            .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
            .env("npm_config_install_strategy", "nested")
            .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
            .env("NPM_CONFIG_IGNORE_SCRIPTS", "true")
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_install,
            "{package_manager} web {mode} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let dependencies_ready = || {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web"])
            .current_dir(&repo)
            .env("NODE", &fake_node)
            .env("PATH", &path)
            .status()
            .unwrap()
            .success()
    };

    let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
    fs::create_dir_all(install_lock.parent().unwrap()).unwrap();
    fs::write(&install_lock, "999999999\n").unwrap();
    run_mode("bootstrap", false);
    assert!(
        !install_lock.exists(),
        "stale dependency lock was not recovered"
    );
    assert_eq!(
        fs::read_to_string(repo.join(lockfile)).unwrap(),
        initial_lock
    );
    assert!(
        dependencies_ready(),
        "{package_manager} dependency receipt was not reusable immediately after publication"
    );
    let dependency_stamp = repo.join(".agent/tmp/web-dependencies/root.sha256");
    let current_stamp = fs::read_to_string(&dependency_stamp).unwrap();
    assert!(
        current_stamp.starts_with("v5 "),
        "{package_manager} did not publish a v5 dependency receipt"
    );
    fs::write(&dependency_stamp, current_stamp.replacen("v5 ", "v4 ", 1)).unwrap();
    assert!(
        !dependencies_ready(),
        "{package_manager} accepted a stale v4 dependency receipt"
    );
    fs::write(&dependency_stamp, current_stamp).unwrap();
    assert!(dependencies_ready());
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    assert!(dependencies_ready());

    let mut expected_install_count = 1;
    if package_manager == "npm" {
        let npm_install_argv = format!(
            "{{operation}}\n--include=dev\n--include=optional\n--include=peer\n--bin-links=true\n--dry-run=false\n--package-lock-only=false\n--package-lock=true\n--global=false\n--location=project\n--prefix={}\n--cpu=test-cpu\n--os=test-platform\n--workspaces=true\n--include-workspace-root=true\n",
            repo.canonicalize().unwrap().display()
        );
        assert_eq!(
            fs::read_to_string(&install_argv).unwrap(),
            npm_install_argv.replace("{operation}", "install"),
            "npm bootstrap did not freeze install-shaping inputs"
        );
        let environment = fs::read_to_string(&install_env).unwrap();
        for removed in [
            "NODE_ENV=",
            "NPM_CONFIG_OMIT=",
            "NPM_CONFIG_INCLUDE=",
            "NPM_CONFIG_PRODUCTION=",
            "NPM_CONFIG_OPTIONAL=",
            "NPM_CONFIG_ONLY=",
            "NPM_CONFIG_DEV=",
            "NPM_CONFIG_ALSO=",
            "Npm_Config_Bin_Links=",
            "npm_CONFIG_dry_run=",
            "NPM_CONFIG_PACKAGE_LOCK_ONLY=",
            "NPM_CONFIG_PACKAGE_LOCK=",
            "NPM_CONFIG_GLOBAL=",
            "NPM_CONFIG_WORKSPACE=",
            "NPM_CONFIG_WORKSPACES=",
            "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
            "NPM_CONFIG_PREFIX=",
            "NPM_CONFIG_LOCATION=",
            "NPM_CONFIG_IF_PRESENT=",
            "NPM_CONFIG_CPU=",
            "NPM_CONFIG_OS=",
            "NPM_CONFIG_LIBC=",
        ] {
            assert!(
                !environment.lines().any(|line| line.starts_with(removed)),
                "npm install inherited shaping input {removed}:\n{environment}"
            );
        }
        for preserved in [
            "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
            "npm_config_install_strategy=nested",
            "NPM_CONFIG_LEGACY_PEER_DEPS=true",
            "NPM_CONFIG_IGNORE_SCRIPTS=true",
        ] {
            assert!(
                environment.lines().any(|line| line == preserved),
                "npm install removed supported input {preserved}:\n{environment}"
            );
        }
        fs::write(
            repo.join("package.json"),
            r#"{"private":true,"version":"2","workspaces":["apps/web"]}"#,
        )
        .unwrap();
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_argv).unwrap(),
            npm_install_argv.replace("{operation}", "ci"),
            "npm frozen install did not freeze install-shaping inputs"
        );
        assert!(dependencies_ready());
    }
    if package_manager == "yarn" {
        let opposite = if artifact == "node_modules" {
            repo.join(".pnp.cjs")
        } else {
            repo.join("node_modules")
        };
        if artifact == "node_modules" {
            fs::write(&opposite, "unexpected pnp loader\n").unwrap();
        } else {
            fs::create_dir_all(&opposite).unwrap();
        }
        assert!(
            !dependencies_ready(),
            "Yarn accepted artifacts for two effective linkers in {case_name}"
        );
        if opposite.is_dir() {
            fs::remove_dir_all(&opposite).unwrap();
        } else {
            fs::remove_file(&opposite).unwrap();
        }
        assert!(dependencies_ready());
        if artifact == ".pnp.cjs" {
            fs::write(
                repo.join(".yarn/cache/unrelated.zip"),
                "unrelated archive\n",
            )
            .unwrap();
            assert!(
                dependencies_ready(),
                "an unrelated shared-cache addition invalidated Yarn Berry readiness"
            );
            for companion in [
                ".pnp.data.json",
                ".yarn/cache/dependency.zip",
                ".yarn/install-state.gz",
            ] {
                fs::write(repo.join(companion), "changed companion\n").unwrap();
                assert!(
                    !dependencies_ready(),
                    "Yarn Berry ignored changed PnP companion {companion}"
                );
                run_mode("lint", false);
                expected_install_count += 1;
                assert_eq!(
                    fs::read_to_string(&install_count).unwrap().trim(),
                    expected_install_count.to_string()
                );
            }
        }
        if classic_pnp {
            for flag in ["--install.pnp true", "--enable-pnp true"] {
                fs::write(
                    repo.join(".yarnrc"),
                    format!("--install.pure-lockfile false\n{flag}\n"),
                )
                .unwrap();
                assert!(!dependencies_ready());
                run_mode("lint", false);
                expected_install_count += 1;
                assert!(dependencies_ready(), "Yarn Classic ignored {flag}");
            }
        }
    }

    if lockfile == "npm-shrinkwrap.json" {
        fs::write(repo.join("package-lock.json"), "inactive-lock-v1\n").unwrap();
        assert!(
            dependencies_ready(),
            "adding an inactive package-lock invalidated the shrinkwrap receipt"
        );
        fs::write(repo.join("package-lock.json"), "inactive-lock-v2\n").unwrap();
        assert!(
            dependencies_ready(),
            "changing an inactive package-lock invalidated the shrinkwrap receipt"
        );
        fs::remove_file(repo.join("npm-shrinkwrap.json")).unwrap();
        assert!(
            !dependencies_ready(),
            "removing the authoritative shrinkwrap reused its receipt for package-lock"
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert!(dependencies_ready());
    }

    if package_manager == "yarn" {
        for runtime_file in [
            ".yarn/patches/dependency.patch",
            ".yarn/plugins/plugin.cjs",
            ".yarn/releases/yarn.cjs",
        ] {
            fs::write(repo.join(runtime_file), "runtime-v2\n").unwrap();
            run_mode("lint", true);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
            run_mode("lint", false);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
        }
        for config_file in [".yarnrc", ".yarnrc.yml"] {
            let config = match (classic_pnp, artifact, config_file) {
                (true, _, ".yarnrc") => "--pnp true\n--install.pure-lockfile false\n",
                (_, "node_modules", ".yarnrc.yml") => {
                    "nodeLinker: node-modules\nchecksumBehavior: reset\n"
                }
                _ => "config-v2\n",
            };
            fs::write(repo.join(config_file), config).unwrap();
            run_mode("lint", true);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
            run_mode("lint", false);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
        }
    }

    fs::write(repo.join(".node-version"), "22.22.3\n").unwrap();
    run_mode("lint", true);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );
    run_mode("lint", false);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );

    fs::write(repo.join(lockfile), "lock-v2\n").unwrap();
    run_mode("lint", true);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );
    run_mode("lint", false);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );

    let artifact_path = repo.join(artifact);
    if artifact_path.is_dir() {
        fs::remove_dir_all(&artifact_path).unwrap();
        fs::create_dir_all(&artifact_path).unwrap();
    } else {
        fs::write(&artifact_path, "").unwrap();
    }
    assert!(
        !dependencies_ready(),
        "replacement or truncation of {artifact} was accepted"
    );
    run_mode("lint", false);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );

    if artifact != "node_modules" {
        fs::write(&artifact_path, "different nonempty loader\n").unwrap();
        assert!(
            !dependencies_ready(),
            "a changed nonempty PnP loader was not detected"
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );
    }

    let symlink_target = repo.join(format!("{case_name}-replacement-artifact"));
    if artifact == "node_modules" {
        fs::remove_dir_all(&artifact_path).unwrap();
        fs::create_dir_all(&symlink_target).unwrap();
    } else {
        fs::remove_file(&artifact_path).unwrap();
        fs::write(&symlink_target, "linked loader\n").unwrap();
    }
    symlink(&symlink_target, &artifact_path).unwrap();
    assert!(
        !dependencies_ready(),
        "a symlinked dependency artifact was accepted"
    );
    fs::remove_file(&artifact_path).unwrap();
    if symlink_target.is_dir() {
        fs::remove_dir_all(&symlink_target).unwrap();
    } else {
        fs::remove_file(&symlink_target).unwrap();
    }
    run_mode("lint", false);
    expected_install_count += 1;
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        expected_install_count.to_string()
    );

    if artifact == "node_modules" {
        let installed_manifest = artifact_path.join("test-package/package.json");
        fs::write(&installed_manifest, "").unwrap();
        assert!(
            !dependencies_ready(),
            "corrupt installed entries were accepted with an unchanged receipt"
        );
        run_mode("lint", false);
        expected_install_count += 1;

        let receipt_path = artifact_path.join(".jig-web-dependencies-v3");
        let copied_receipt = fs::read_to_string(&receipt_path).unwrap();
        fs::remove_dir_all(&artifact_path).unwrap();
        fs::create_dir_all(&artifact_path).unwrap();
        fs::write(
            artifact_path.join(".jig-web-dependencies-v3"),
            copied_receipt,
        )
        .unwrap();
        assert!(
            !dependencies_ready(),
            "an empty node_modules tree with a copied receipt was accepted"
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );
    }
}
