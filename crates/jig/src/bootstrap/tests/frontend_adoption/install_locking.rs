use super::*;

#[cfg(unix)]
#[test]
fn generated_web_checks_use_ignore_workspace_and_track_parent_config_for_standalone_pnpm() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("app-local-pnpm");

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
            repo_name: Some("app-local-pnpm".into()),
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

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.11.0"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages:\n  - tools/*\n").unwrap();
    fs::write(
        repo.join(".npmrc"),
        "shared-workspace-lockfile=false\nregistry=https://registry.npmjs.org/\n",
    )
    .unwrap();
    fs::write(repo.join(".pnpmfile.cjs"), "module.exports = {}\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_node = fake_bin.join("fake-node");
    fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-workspace-metadata" ]; then
  [ "${3:-}" != "contains" ]
  exit $?
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

    let fake_pnpm = fake_bin.join("pnpm");
    fs::write(
        &fake_pnpm,
        r#"#!/bin/sh
set -eu
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
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALL_COUNT"
    pwd > "$INSTALL_CWD"
    if [ "${FAIL_INSTALL:-0}" = "1" ]; then exit 9; fi
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    printf '%s\n' 'layout-v1' > node_modules/.modules.yaml
    if [ -n "${PNPM_DRIFT_TO:-}" ] && [ -n "${PNPM_VERSION_FILE:-}" ]; then
      printf '%s\n' "$PNPM_DRIFT_TO" > "$PNPM_VERSION_FILE"
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_pnpm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_cwd = repo.join("install-cwd");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let resolved_spec = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "package-manager-spec",
            "apps/web",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_output_succeeded("resolve standalone pnpm spec", &resolved_spec);
    assert_eq!(
        String::from_utf8(resolved_spec.stdout).unwrap().trim(),
        "pnpm@10.11.0"
    );
    let run_mode = |mode: &str, fail_install: bool| {
        let output = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("FAIL_INSTALL", if fail_install { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_install,
            "app-local pnpm web {mode} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_mode("bootstrap", false);
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(repo.join("apps/web"))
            .unwrap()
            .display()
            .to_string()
    );
    assert!(!repo.join("pnpm-lock.yaml").exists());
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert!(web_check.contains("pnpm install --ignore-workspace"));
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let dependencies_ready = || {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .status()
            .unwrap()
            .success()
    };
    let node_modules = repo.join("apps/web/node_modules");
    for workspace_state_name in [
        ".pnpm-workspace-state.json",
        ".pnpm-workspace-state-v1.json",
    ] {
        let workspace_state = node_modules.join(workspace_state_name);
        fs::write(
            &workspace_state,
            r#"{"lastValidatedTimestamp":1,"settings":{"dedupeInjectedDeps":true}}"#,
        )
        .unwrap();
        assert_dependency_readiness(workspace_state_name, dependencies_ready(), true);
        fs::write(
            &workspace_state,
            r#"{"lastValidatedTimestamp":1784300000000,"settings":{"dedupeInjectedDeps":true}}"#,
        )
        .unwrap();
        assert_dependency_readiness("rewritten pnpm workspace state", dependencies_ready(), true);

        let nested_workspace_state = node_modules.join("test-package").join(workspace_state_name);
        fs::write(&nested_workspace_state, "nested package-owned state\n").unwrap();
        assert_dependency_readiness("nested pnpm workspace state", dependencies_ready(), false);
        fs::remove_file(&nested_workspace_state).unwrap();
        assert_dependency_readiness("removed nested workspace state", dependencies_ready(), true);

        fs::remove_file(&workspace_state).unwrap();
        assert_dependency_readiness("deleted volatile root cache", dependencies_ready(), true);
        fs::create_dir(&workspace_state).unwrap();
        assert_dependency_readiness(
            "directory replacing workspace state",
            dependencies_ready(),
            false,
        );
        fs::remove_dir(&workspace_state).unwrap();
        assert_dependency_readiness("removed replacement directory", dependencies_ready(), true);

        symlink("test-package/package.json", &workspace_state).unwrap();
        assert_dependency_readiness(
            "symlink replacing workspace state",
            dependencies_ready(),
            false,
        );
        fs::remove_file(&workspace_state).unwrap();
        assert_dependency_readiness("removed replacement symlink", dependencies_ready(), true);
    }

    let bin_dir = node_modules.join(".bin");
    fs::create_dir(&bin_dir).unwrap();
    fs::write(bin_dir.join("test-package"), "shim\n").unwrap();
    assert_dependency_readiness("pnpm .bin layout", dependencies_ready(), false);
    fs::remove_dir_all(&bin_dir).unwrap();
    assert_dependency_readiness("removed pnpm .bin", dependencies_ready(), true);

    let modules_metadata = node_modules.join(".modules.yaml");
    fs::write(&modules_metadata, "layout-v2\n").unwrap();
    assert_dependency_readiness("pnpm metadata mutation", dependencies_ready(), false);
    fs::write(&modules_metadata, "layout-v1\n").unwrap();
    assert_dependency_readiness("restored pnpm metadata", dependencies_ready(), true);

    fs::write(repo.join("apps/web/local.patch"), "patch contents\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-workspace.yaml"),
        "patchedDependencies:\n  dependency@1: local.patch\n",
    )
    .unwrap();
    for version in ["10.33.4", "11.13.1"] {
        let rejected = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "lint"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("FAIL_INSTALL", "0")
            .env("PNPM_VERSION", version)
            .output()
            .unwrap();
        assert_output_failed("scope-local pnpm YAML patch", &rejected);
        let stderr = String::from_utf8_lossy(&rejected.stderr);
        assert_text_contains_all(
            &stderr,
            &["apps/web/pnpm-workspace.yaml", "--ignore-workspace"],
        );
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "1",
            "pnpm {version} reached install before rejecting inactive local patches"
        );
    }
    fs::remove_file(repo.join("apps/web/pnpm-workspace.yaml")).unwrap();
    fs::remove_file(repo.join("apps/web/local.patch")).unwrap();
    run_mode("lint", false);

    for (path, contents) in [
        (
            ".npmrc",
            "shared-workspace-lockfile=false\nregistry=https://registry.example/\n",
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - tools/*\n  - packages/*\n",
        ),
        (
            ".pnpmfile.cjs",
            "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n",
        ),
    ] {
        fs::write(repo.join(path), contents).unwrap();
        run_mode("lint", true);
        let failed_count = fs::read_to_string(&install_count)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        run_mode("lint", false);
        assert_eq!(
            fs::read_to_string(&install_count)
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap(),
            failed_count + 1
        );
    }

    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_eq!(workflow.matches(r#"- ".pnpmfile.cjs""#).count(), 2);
    assert_eq!(workflow.matches(r#"- "pnpmfile.cjs""#).count(), 2);
}

#[cfg(unix)]
#[test]
fn generated_web_checks_recover_interrupted_and_contended_stale_install_locks() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("stale-lock-contention");

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
            repo_name: Some("stale-lock-contention".into()),
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
        r#"{"private":true,"workspaces":["apps/web"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "{\"lockfileVersion\":3}\n").unwrap();
    fs::write(repo.join(".node-version"), "24.19.0\n").unwrap();

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
  case "$launcher_operation:$operation_argument" in
    install:frozen) exec npm ci ;;
    install:bootstrap) exec npm install ;;
    run-script:*) exec npm --prefix=. --workspace=. --workspaces=true --include-workspace-root=true --global=false --location=project --if-present=false --include=dev --include=optional --include=peer run "$operation_argument" ;;
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

    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    if ! mkdir "$INSTALL_ACTIVE" 2>/dev/null; then
      : > "$INSTALL_OVERLAP"
      exit 11
    fi
    trap 'rmdir "$INSTALL_ACTIVE"' EXIT
    printf '%s\n' "$$" >> "$INSTALL_LOG"
    sleep 0.2
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

    let recovery_barrier = repo.join("recovery-barrier");
    let recovery_moves = repo.join("recovery-moves");
    let bash_env = repo.join("bash-env");
    let paused_bash_env = repo.join("paused-bash-env");
    let pid_reuse_bash_env = repo.join("pid-reuse-bash-env");
    let claim_ready = repo.join("claim-ready");
    fs::write(&recovery_barrier, "").unwrap();
    fs::write(
        &bash_env,
        r#"kill() {
  if [ "${1:-}" = "-0" ] && [ "${2:-}" = "999999999" ]; then
    printf '%s\n' "$$" >> "$RECOVERY_BARRIER"
    attempt=0
    while [ "$(wc -l < "$RECOVERY_BARRIER")" -lt 2 ]; do
      attempt=$((attempt + 1))
      if [ "$attempt" -ge 1000 ]; then return 1; fi
      sleep 0.01
    done
    return 1
  fi
  builtin kill "$@"
}

mv() {
  if [ "${1:-}" = ".agent/tmp/web-dependencies.lock" ]; then
    printf '%s\n' "$$" >> "$RECOVERY_MOVES"
  fi
  command mv "$@"
}
"#,
    )
    .unwrap();
    fs::write(
        &paused_bash_env,
        r#"ln() {
  command ln "$@"
  status=$?
  if [ "$status" -eq 0 ]; then
    destination=
    for argument in "$@"; do destination="$argument"; done
    case "${PAUSE_AFTER_LINK:-}:$destination" in
      candidate:.agent/tmp/web-dependencies.lock|claim:*.recover.*)
        printf '%s\n' "$$" > "$CLAIM_READY"
        while :; do sleep 1; done
        ;;
    esac
  fi
  return "$status"
}
"#,
    )
    .unwrap();
    fs::write(
        &pid_reuse_bash_env,
        r#"ps() {
  for argument in "$@"; do
    if [ "$argument" = "$REUSED_PID" ]; then
      printf '%s\n' 'Thu Jan  1 00:00:00 2099'
      return 0
    fi
  done
  command ps "$@"
}
"#,
    )
    .unwrap();

    let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
    fs::create_dir_all(install_lock.parent().unwrap()).unwrap();

    let install_active = repo.join("install-active");
    let install_overlap = repo.join("install-overlap");
    let install_log = repo.join("install-log");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = |bash_env: Option<&std::path::Path>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", "bootstrap"])
            .current_dir(&repo)
            .env("NODE", &fake_node)
            .env("PATH", &path)
            .env("RECOVERY_BARRIER", &recovery_barrier)
            .env("RECOVERY_MOVES", &recovery_moves)
            .env("CLAIM_READY", &claim_ready)
            .env("REUSED_PID", std::process::id().to_string())
            .env("INSTALL_ACTIVE", &install_active)
            .env("INSTALL_OVERLAP", &install_overlap)
            .env("INSTALL_LOG", &install_log)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(bash_env) = bash_env {
            command.env("BASH_ENV", bash_env);
        }
        command
    };

    let interrupt_after_link = |kind: &str| {
        if claim_ready.exists() {
            fs::remove_file(&claim_ready).unwrap();
        }
        let mut interrupted = command(Some(&paused_bash_env));
        interrupted.env("PAUSE_AFTER_LINK", kind);
        let mut interrupted = interrupted.spawn().unwrap();
        let interrupted_pid = interrupted.id();
        let observed_pid =
            wait_for_positive_pid_file(&claim_ready, std::time::Duration::from_secs(5));
        let kill_result = interrupted.kill();
        let output_result = interrupted.wait_with_output();
        let observed_pid = observed_pid.unwrap_or_else(|error| {
            panic!("interrupted {kind} transition never reached its pause point: {error}")
        });
        assert_eq!(
            observed_pid, interrupted_pid,
            "interrupted {kind} transition recorded the wrong PID"
        );
        kill_result.unwrap();
        let output = output_result.unwrap();
        assert!(!output.status.success());
    };
    let assert_no_lock_sidecars = || {
        assert!(
            fs::read_dir(install_lock.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("web-dependencies.lock.")),
            "stale-lock recovery left a sidecar behind"
        );
    };
    let reset_dependency_state = || {
        for directory in [
            repo.join("node_modules"),
            repo.join(".agent/tmp/web-dependencies"),
        ] {
            if directory.exists() {
                fs::remove_dir_all(directory).unwrap();
            }
        }
        for file in [
            &install_log,
            &install_overlap,
            &recovery_moves,
            &claim_ready,
        ] {
            if file.exists() {
                fs::remove_file(file).unwrap();
            }
        }
        fs::write(&recovery_barrier, "").unwrap();
    };

    interrupt_after_link("candidate");
    assert!(install_lock.exists());
    let lock_metadata = fs::read_to_string(&install_lock).unwrap();
    let lock_fields = lock_metadata.split_whitespace().collect::<Vec<_>>();
    assert_eq!(lock_fields.len(), 3);
    assert_ne!(lock_fields[2], "unknown");
    assert!(
        fs::read_dir(install_lock.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("web-dependencies.lock.candidate.")),
        "candidate hardlink was not retained at the simulated kill point"
    );
    let recovered = command(None).output().unwrap();
    assert_output_succeeded("reclaim interrupted lock creation", &recovered);
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(&install_lock, "999999999\n").unwrap();
    interrupt_after_link("claim");
    assert!(install_lock.exists());
    let recovered = command(None).output().unwrap();
    assert_output_succeeded("reclaim interrupted recovery claim", &recovered);
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(
        &install_lock,
        format!("{} reused-token DefinitelyOldStart\n", std::process::id()),
    )
    .unwrap();
    let recovered = command(Some(&pid_reuse_bash_env)).output().unwrap();
    assert_output_succeeded("detect reused lock-owner PID", &recovered);
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(&install_lock, "999999999\n").unwrap();

    let first = command(Some(&bash_env)).spawn().unwrap();
    let second = command(Some(&bash_env)).spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for (name, output) in [("first", first), ("second", second)] {
        assert_output_succeeded(name, &output);
    }

    assert_eq!(
        fs::read_to_string(&recovery_moves).unwrap().lines().count(),
        1,
        "more than one process moved the stale lock generation"
    );
    assert_eq!(
        fs::read_to_string(&install_log).unwrap().lines().count(),
        1,
        "dependency installation ran more than once"
    );
    assert!(
        !install_overlap.exists(),
        "dependency installation overlapped under stale-lock contention"
    );
    assert!(!install_lock.exists());
    assert!(repo.join("node_modules").is_dir());
    assert_no_lock_sidecars();
}

#[cfg(unix)]
fn assert_unverified_live_lock_owners_are_preserved(
    repo: &Path,
    install_lock: &Path,
    command: &impl Fn() -> std::process::Command,
) {
    use std::os::unix::fs::PermissionsExt;

    let kill_failure_env = repo.join("kill-failure-env");
    fs::write(
        &kill_failure_env,
        "kill() { if [ \"${1:-}\" = \"-0\" ]; then return 1; fi; builtin kill \"$@\"; }\n",
    )
    .unwrap();
    let probe_node = repo.join("probe-node");
    fs::write(
        &probe_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  case "${PROBE_BEHAVIOR:-}" in
    eperm) printf '%s\n' unverified; exit 0 ;;
    tool-failure) exit 19 ;;
  esac
fi
exec node "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&probe_node, fs::Permissions::from_mode(0o755)).unwrap();
    let simulated_owner = format!(
        "{} simulated-permission-owner RecordedStart\n",
        std::process::id()
    );
    for behavior in ["eperm", "tool-failure"] {
        fs::write(install_lock, &simulated_owner).unwrap();
        let mut simulated = command();
        simulated
            .env("BASH_ENV", &kill_failure_env)
            .env("NODE", &probe_node)
            .env("PROBE_BEHAVIOR", behavior);
        let simulated = simulated.output().unwrap();
        assert!(
            !simulated.status.success(),
            "{behavior} process probe was treated as a stale owner"
        );
        assert_eq!(fs::read_to_string(install_lock).unwrap(), simulated_owner);
    }

    let unknown_live = format!("{} legacy-token unknown\n", std::process::id());
    fs::write(install_lock, &unknown_live).unwrap();
    let unverified = command().output().unwrap();
    assert_output_failed("unknown live lock owner", &unverified);
    assert_eq!(fs::read_to_string(install_lock).unwrap(), unknown_live);
    assert!(
        String::from_utf8_lossy(&unverified.stderr).contains("could not be validated or recovered")
    );

    #[cfg(target_os = "macos")]
    {
        let ps_failure_env = repo.join("ps-failure-env");
        fs::write(&ps_failure_env, "ps() { return 1; }\n").unwrap();
        fs::write(
            install_lock,
            format!("{} known-token RecordedStart\n", std::process::id()),
        )
        .unwrap();
        let mut unreadable_identity = command();
        unreadable_identity.env("BASH_ENV", &ps_failure_env);
        let unreadable_identity = unreadable_identity.output().unwrap();
        assert_output_failed("unreadable live lock identity", &unreadable_identity);
        assert!(install_lock.exists(), "unverified live lock was removed");
    }
}

#[cfg(unix)]
#[test]
fn generated_web_checks_wait_for_live_installs_and_never_suggest_removing_their_lock() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("live-install-lock");
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
            repo_name: Some("live-install-lock".into()),
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
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root-lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    : > "$INSTALL_ACTIVE"
    sleep 0.25
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    rm -f "$INSTALL_ACTIVE"
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_active = repo.join("install-active");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = || {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", "bootstrap"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_ACTIVE", &install_active)
            .env("JIG_WEB_INSTALL_LOCK_UNRESOLVED_ATTEMPTS", "1")
            .env("JIG_WEB_INSTALL_LOCK_POLL_SECONDS", "0.01")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
    };

    let mut first_command = command();
    first_command.env("TZ", "Europe/Prague");
    let first = first_command.spawn().unwrap();
    for _ in 0..200 {
        if install_active.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(install_active.exists(), "first install never became active");
    let mut second_command = command();
    second_command.env("TZ", "America/Los_Angeles");
    let second = second_command.spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for (name, output) in [("first", first), ("second", second)] {
        assert!(
            output.status.success(),
            "{name} live-lock bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    assert!(
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web",])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();
    let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
    fs::write(&install_lock, "malformed lock metadata\n").unwrap();
    let malformed = command().output().unwrap();
    assert!(!malformed.status.success());
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(stderr.contains("could not be validated or recovered"));
    assert!(!stderr.to_ascii_lowercase().contains("remove"));

    fs::write(&install_lock, "999999999 absent-token RecordedStart\n").unwrap();
    let absent = command().output().unwrap();
    assert!(
        absent.status.success(),
        "an ESRCH owner was not recovered:\n{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    assert!(!install_lock.exists());
    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();

    assert_unverified_live_lock_owners_are_preserved(&repo, &install_lock, &command);
}

#[cfg(unix)]
fn wait_for_path(path: &Path) -> bool {
    for _ in 0..500 {
        if path.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[cfg(unix)]
fn wait_for_child(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    for _ in 0..500 {
        let status = child.try_wait().unwrap();
        if status.is_some() {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[cfg(unix)]
fn assert_install_worker_script_contract(checker: &str) {
    let start_worker = checker
        .split_once("start_install_worker() {")
        .unwrap()
        .1
        .split_once("run_dependency_install() {")
        .unwrap()
        .0;
    assert!(
        start_worker
            .find("trap 'forward_install_worker_signal HUP' HUP")
            .unwrap()
            < start_worker.find("\"$bash_bin\" \"$0\"").unwrap()
    );
    assert_text_contains_all(
        checker,
        &["trap 'preserve_install_lock_for_group_recovery' EXIT"],
    );
    assert_text_contains_none(
        checker,
        &["trap 'release_install_lock' EXIT\n          break"],
    );
    let handoff = checker
        .split_once("dependency_install_worker() {")
        .unwrap()
        .1
        .split_once("scope=\"$(dependency_scope")
        .unwrap()
        .0;
    assert_text_contains_all(
        handoff,
        &[
            "while :; do",
            "unresolved_handoff_attempts=0",
            "max_unresolved_handoff_attempts=600",
        ],
    );
    assert_text_contains_none(handoff, &["attempt -lt 500"]);
}

#[cfg(unix)]
fn kill_parent_after_worker_starts(child: &std::process::Child, install_started: &Path) -> bool {
    let started = wait_for_path(install_started);
    if started {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGKILL);
        }
    }
    started
}

#[cfg(unix)]
fn reset_install_worker_fixture(repo: &Path, install_active: &Path, fixture_files: &[&Path]) {
    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();
    for file in fixture_files {
        if file.exists() {
            fs::remove_file(file).unwrap();
        }
    }
    if install_active.exists() {
        fs::remove_dir_all(install_active).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn generated_web_install_worker_survives_parent_sigkill_without_overlapping_install() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("sigkill-install-worker");
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
            repo_name: Some("sigkill-install-worker".into()),
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
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert_install_worker_script_contract(&checker);
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
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
trap '' TERM
case "${1:-}" in
  ci|install)
    if ! mkdir "$INSTALL_ACTIVE" 2>/dev/null; then
      : > "$INSTALL_OVERLAP"
      exit 11
    fi
    trap 'rmdir "$INSTALL_ACTIVE" 2>/dev/null || true' EXIT
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    : > "$INSTALL_STARTED"
    while [ ! -f "$INSTALL_RELEASE" ]; do sleep 0.01; done
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_active = repo.join("install-active");
    let install_overlap = repo.join("install-overlap");
    let install_count = repo.join("install-count");
    let install_started = repo.join("install-started");
    let install_release = repo.join("install-release");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = || {
        let mut command = std::process::Command::new("bash");
        command
            .args([
                "scripts/check-webapps.sh",
                "dependencies-install",
                "apps/web",
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_ACTIVE", &install_active)
            .env("INSTALL_OVERLAP", &install_overlap)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_STARTED", &install_started)
            .env("INSTALL_RELEASE", &install_release)
            .env("JIG_WEB_INSTALL_LOCK_POLL_SECONDS", "0.01")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };

    let mut first = command().spawn().unwrap();
    let started = kill_parent_after_worker_starts(&first, &install_started);
    let _ = first.wait();
    let mut second = command().spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let overlap_before_release = install_overlap.exists();
    fs::write(&install_release, "release\n").unwrap();

    let second_status = wait_for_child(&mut second);

    assert!(started, "installer worker never started");
    assert!(
        !overlap_before_release,
        "second install overlapped the orphaned worker"
    );
    assert!(
        second_status.unwrap().success(),
        "waiting install wrapper failed"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
    assert!(
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web",])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    reset_install_worker_fixture(
        &repo,
        &install_active,
        &[
            &install_overlap,
            &install_count,
            &install_started,
            &install_release,
        ],
    );

    let mut interrupted = command().spawn().unwrap();
    let interrupted_started = wait_for_path(&install_started);
    assert!(
        interrupted_started,
        "signal-interrupted installer worker never started"
    );
    assert_eq!(
        unsafe { libc::kill(interrupted.id() as i32, libc::SIGTERM) },
        0,
        "could not signal dependency-install coordinator"
    );
    let interrupted_status = wait_for_child(&mut interrupted);
    assert!(
        interrupted_status.is_some_and(|status| !status.success()),
        "signal-interrupted coordinator did not exit through its forwarding path"
    );

    let mut waiting = command().spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "a waiter stopped honoring the interrupted worker generation"
    );
    assert!(
        !install_overlap.exists(),
        "a second install overlapped a signal-surviving package-manager descendant"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    fs::write(&install_release, "release\n").unwrap();

    let waiting_status = wait_for_child(&mut waiting);
    assert!(
        waiting_status.unwrap().success(),
        "waiter failed after the interrupted install group exited"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
    assert!(!install_overlap.exists());
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
}

#[cfg(unix)]
#[test]
fn generated_web_install_worker_preserves_status_after_wait_without_pid_probe() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("install-worker-status");
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
            repo_name: Some("install-worker-status".into()),
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
        r#"{"private":true,"workspaces":["apps/web"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    : > "$INSTALL_FINISHED"
    exit 42
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let bash_env = repo.join("simulate-post-wait-pid-reuse");
    fs::write(
        &bash_env,
        r#"kill() {
  if [ "${1:-}" = "-0" ] && [ -f "$INSTALL_FINISHED" ] && [ ! -f "$PID_REUSE_PROBED" ]; then
    : > "$PID_REUSE_PROBED"
    return 0
  fi
  builtin kill "$@"
}
"#,
    )
    .unwrap();
    let install_finished = repo.join("install-finished");
    let pid_reuse_probed = repo.join("pid-reuse-probed");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let output = std::process::Command::new("/bin/bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-install",
            "apps/web",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("BASH_ENV", &bash_env)
        .env("INSTALL_FINISHED", &install_finished)
        .env("PID_REUSE_PROBED", &pid_reuse_probed)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(42),
        "worker status was clobbered:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(install_finished.exists());
    assert!(
        !pid_reuse_probed.exists(),
        "coordinator probed a reaped worker PID"
    );
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
}
