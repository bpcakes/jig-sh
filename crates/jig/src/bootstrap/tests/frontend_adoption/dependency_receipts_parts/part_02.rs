
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
