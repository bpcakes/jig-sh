#[test]
fn init_renders_web_commands_for_all_supported_package_managers() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let cases = [
        ("bun", "bun install --frozen-lockfile", "bun run"),
        (
            "npm",
            "run_managed_npm_command install",
            "run_npm_package_script",
        ),
        ("pnpm", "pnpm install --frozen-lockfile", "pnpm run"),
        ("yarn", "yarn install --frozen-lockfile", "yarn run"),
    ];

    for (package_manager, install_command, run_command) in cases {
        let repo = temp.path().join(package_manager);
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
                repo_name: Some(format!("demo-{package_manager}")),
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

        assert!(!repo.join("Makefile").exists());
        let agent_guidance = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(agent_guidance.contains(
            "Generated install steps select the package-manager project from workspace membership, not root-lock presence"
        ));
        assert!(agent_guidance.contains("It ignores only real top-level tool-cache directories"));
        assert!(
            !agent_guidance
                .contains("Generated install steps use a repo-root lockfile when one exists")
        );
        let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
        let web_node = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
        let web_sources = format!("{web_check}\n{web_node}");
        if package_manager == "npm" {
            assert!(web_check.contains("run_npm_dependency_install"));
            assert!(web_check.contains(install_command));
        } else {
            assert!(
                web_check.contains(install_command),
                "missing install command for {package_manager}"
            );
        }
        assert!(
            web_check.contains(run_command),
            "missing run command for {package_manager}"
        );
        assert!(web_check.contains("if dependencies_present \"$app_dir\""));
        assert!(web_check.contains("acquire_install_lock"));
        assert!(web_check.contains("dependency_stamp_path"));
        assert!(web_check.contains("dependency_fingerprint"));
        assert!(web_check.contains("jig-web-dependencies-v3"));
        assert!(web_check.contains("record_dependency_state"));
        assert!(web_check.contains("bootstrap_dependencies"));
        assert!(web_check.contains("dependencies-bootstrap"));
        assert!(web_check.contains("dependencies-ready"));
        assert!(web_check.contains("dependencies-install"));
        assert!(web_check.contains("node_version_file"));
        assert!(web_check.contains(r#"app_dir="apps/web""#));
        assert!(web_check.contains(r#""$app_dir/.node-version""#));
        assert!(web_check.contains("start_install_worker"));
        assert!(web_check.contains("transfer_install_lock_to_worker"));
        assert!(web_check.contains("recover_stale_install_lock"));
        assert!(web_node.contains("maximumEntries = 10_000"));
        assert!(!web_check.contains("root_lock_exists"));
        assert_eq!(
            web_node.contains("volatilePnpmWorkspaceState"),
            package_manager == "pnpm",
            "only pnpm checkers should exclude pnpm's volatile workspace-state cache"
        );
        #[cfg(unix)]
        {
            let syntax = std::process::Command::new("/bin/bash")
                .args(["-n", "scripts/check-webapps.sh"])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(
                syntax.success(),
                "invalid rendered Bash for {package_manager}"
            );
            let node_syntax = std::process::Command::new("node")
                .args(["--check", "scripts/web-node.cjs"])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(
                node_syntax.success(),
                "invalid rendered Node helper for {package_manager}"
            );
        }
        if package_manager == "yarn" {
            assert!(web_sources.contains("dependency_artifact_kind"));
            assert!(web_sources.contains("yarn_berry_config_payload"));
            assert!(web_sources.contains("yarn_berry_pnp_artifact_proof"));
            assert!(web_sources.contains("pnpEnableInlining"));
            assert!(web_sources.contains("pnpEnableEsmLoader"));
            assert!(web_sources.contains("installStatePath"));
            assert!(web_sources.contains("pnpUnpluggedFolder"));
            assert!(web_sources.contains("const RAW_RUNTIME_STATE"));
            assert!(web_sources.contains("setupStatePackageLocations"));
            assert!(web_sources.contains("maximumParsedInputBytes = 64 * 1024 * 1024"));
            assert!(web_sources.contains("hashFile(loader, \"loader\", maximumParsedInputBytes)"));
            assert!(web_sources.contains("hashFile(dataPath, \"data\", maximumParsedInputBytes)"));
            assert!(web_sources.contains("too many PnP package locations"));
            assert!(web_sources.contains("yarn_classic_actual_artifact_kind"));
            assert!(web_sources.contains("yarn_classic_pnp_artifact_proof"));
            assert!(web_sources.contains("YARN_PLUGNPLAY_OVERRIDE"));
            assert!(web_sources.contains(r#""$scope/.pnp.cjs""#));
            assert!(web_sources.contains(r#""$scope/.pnp.js""#));
            assert!(web_sources.contains(r#"yarn_scope_authority_paths "$scope" "$authority""#));
            assert!(web_sources.contains("yarn_runtime_identity"));
            assert!(web_sources.contains("yarn@1.22.22"));
            for runtime_dir in [".yarn/patches", ".yarn/plugins", ".yarn/releases"] {
                assert!(
                    web_sources.contains(runtime_dir),
                    "missing Yarn dependency input {runtime_dir}"
                );
            }
        }
        let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
        let expected_dev_argv = if package_manager == "npm" {
            "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]".to_string()
        } else {
            format!("argv = [\"{package_manager}\", \"run\", \"dev\"]")
        };
        assert!(
            answers.contains(&expected_dev_argv),
            "missing dev app argv for {package_manager}"
        );
        #[cfg(feature = "dev-proxy")]
        if package_manager == "npm" {
            let config = toml::from_str::<toml::Value>(&answers).unwrap();
            let generated_app = config["dev"]["apps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|app| app["name"].as_str() == Some("web"))
                .unwrap();
            let argv = generated_app["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|argument| argument.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert!(
                jig_dev_proxy::is_generated_npm_dev_argv(&argv),
                "rendered npm dev argv drifted from the runtime matcher: {argv:?}"
            );
        }

        let workflow =
            fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
        assert!(workflow.contains("Classic required status checks can remain pending"));
        assert!(workflow.contains("APP_DIR: ${{ matrix.app.dir }}"));
        assert!(workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
        for script in ["lint", "typecheck", "build:bundle", "test:coverage"] {
            assert!(
                workflow.contains(&format!(
                    r#"scripts/check-webapps.sh run-script "$APP_DIR" {script}"#
                )),
                "{package_manager} workflow bypassed the managed runner for {script}"
            );
        }
        assert_eq!(
            workflow
                .matches(r#"scripts/check-webapps.sh run-script "$APP_DIR""#)
                .count(),
            4
        );
        assert!(!workflow.contains(r#"cd "$APP_DIR" &&"#));
        assert!(!workflow.contains("if [ -f package.json ]"));
        assert!(workflow.contains("scripts/check-webapps.sh node-version-file \"$APP_DIR\""));
        assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
        assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
        assert!(workflow.contains("set -o noclobber"));
        assert!(workflow.contains("'24.19.0' > \"$node_version_file\""));
        assert!(!workflow.contains("> .node-version"));
        assert!(workflow.contains("status=$?"));
        assert!(workflow.contains("if [ \"$status\" -eq 1 ]"));
        assert!(workflow.contains("exit \"$status\""));
        assert!(!workflow.contains("if ! node_version_file="));
        assert!(workflow.contains("$node_version_file\" >> \"$GITHUB_OUTPUT"));
        assert!(!workflow.contains("node-version: 22.12.0"));
        let expected_node_setup_count = if matches!(package_manager, "pnpm" | "yarn") {
            2
        } else {
            1
        };
        assert_eq!(
            workflow
                .matches("node-version-file: ${{ steps.node-version.outputs.path }}")
                .count(),
            expected_node_setup_count
        );
        assert!(!workflow.contains("node-version-file: .node-version"));
        for trigger in [
            r#"- "**/package.json""#,
            r#"- "**/package.json5""#,
            r#"- "**/package.yaml""#,
            r#"- "**/.node-version""#,
            r#"- "**/.npmrc""#,
            r#"- "**/*.patch""#,
            r#"- "**/*.diff""#,
        ] {
            assert_eq!(
                workflow.matches(trigger).count(),
                2,
                "missing recursive web workflow trigger {trigger}"
            );
        }
        let bootstrap_node = workflow
            .find("Bootstrap Node for dependency metadata")
            .unwrap();
        let resolve_node = workflow.find("Resolve Node version file").unwrap();
        assert!(bootstrap_node < resolve_node);
        if package_manager == "bun" {
            assert!(workflow.contains("oven-sh/setup-bun@v2"));
            assert!(workflow.contains("bun-version: \"1.3.14\""));
            assert_eq!(workflow.matches(r#"- "bun.lockb""#).count(), 2);
        } else {
            assert!(workflow.contains(&format!("cache: {package_manager}")));
        }
        if package_manager == "npm" {
            assert!(web_sources.contains("run_managed_npm_command install \"$1\" \"$2\""));
            assert!(web_sources.contains("run_managed_npm_command run-script \"$1\" \"$2\""));
            assert!(web_sources.contains("const result = spawnSync(\"npm\", args, {"));
            assert!(web_sources.contains("stdio: \"inherit\""));
            assert!(web_sources.contains("shell: false"));
            assert!(web_sources.contains("const match = /^npm_config_(.*)$/i.exec(key);"));
            assert!(web_sources.contains("match[1].replaceAll(\"_\", \"-\").toLowerCase()"));
            for setting in [
                "omit",
                "include",
                "production",
                "optional",
                "only",
                "dev",
                "also",
                "bin-links",
                "dry-run",
                "package-lock-only",
                "package-lock",
                "global",
                "location",
                "if-present",
                "workspace",
                "workspaces",
                "include-workspace-root",
                "prefix",
                "cpu",
                "os",
                "libc",
            ] {
                assert!(web_sources.contains(&format!("  \"{setting}\",")));
            }
            assert!(!web_sources.contains("  \"ignore-scripts\","));
            assert!(!web_sources.contains("  \"install-strategy\","));
            for argument in [
                "--include=dev",
                "--include=optional",
                "--include=peer",
                "--bin-links=true",
                "--dry-run=false",
                "--package-lock-only=false",
                "--package-lock=true",
                "--global=false",
                "--location=project",
                "--libc=glibc",
                "--libc=musl",
                "--workspaces=true",
                "--include-workspace-root=true",
                "--workspaces=false",
                "--workspace=.",
                "--if-present=false",
            ] {
                assert!(
                    web_sources.contains(argument),
                    "missing npm argument {argument}"
                );
            }
            #[cfg(unix)]
            {
                use std::ffi::OsString;
                use std::os::unix::fs::PermissionsExt;
                use std::process::Command;

                let node = Command::new("node")
                    .args([
                        "-p",
                        "JSON.stringify({execPath:process.execPath,arch:process.arch,platform:process.platform,libc:process.platform==='linux'?(process.report.getReport().header.glibcVersionRuntime?'glibc':'musl'):null})",
                    ])
                    .output();
                if node.as_ref().is_ok_and(|output| output.status.success()) {
                    let node = node.unwrap();
                    let identity: serde_json::Value = serde_json::from_slice(&node.stdout).unwrap();
                    let node_executable = identity["execPath"].as_str().unwrap();
                    let fake_bin = repo.join("npm-launcher-bin");
                    fs::create_dir_all(&fake_bin).unwrap();
                    let fake_npm = fake_bin.join("npm");
                    fs::write(
                        &fake_npm,
                        r#"#!/bin/sh
set -eu
printf '%s\n' "$@" > "$INSTALL_ARGV"
env | LC_ALL=C sort > "$INSTALL_ENV"
"#,
                    )
                    .unwrap();
                    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
                    let install_argv = repo.join("npm-launcher-argv");
                    let install_env = repo.join("npm-launcher-env");
                    let mut path = OsString::from(fake_bin.as_os_str());
                    path.push(":");
                    path.push(std::env::var_os("PATH").unwrap_or_default());
                    let mut command = Command::new(node_executable);
                    command
                        .args(["-", "--jig-managed-npm", "install", ".", "bootstrap"])
                        .current_dir(&repo)
                        .stdin(fs::File::open(repo.join("scripts/web-node.cjs")).unwrap())
                        .env("JIG_WEB_NODE_HELPER", "run_managed_npm_command")
                        .env("PATH", &path)
                        .env("INSTALL_ARGV", &install_argv)
                        .env("INSTALL_ENV", &install_env)
                        .env("NODE_ENV", "production")
                        .env("NPM_CONFIG_OMIT", "dev optional peer")
                        .env("NPM_CONFIG_ONLY", "production")
                        .env("NPM_CONFIG_DEV", "false")
                        .env("NPM_CONFIG_ALSO", "production")
                        .env("Npm_Config_Bin_Links", "false")
                        .env("npm_CONFIG_dry_run", "true")
                        .env("NPM_CONFIG_PACKAGE_LOCK_ONLY", "true")
                        .env("NPM_CONFIG_WORKSPACE", "other")
                        .env("NPM_CONFIG_WORKSPACES", "false")
                        .env("Npm_Config_Location", "global")
                        .env("npm_CONFIG_if_present", "true")
                        .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
                        .env("npm_config_install_strategy", "nested")
                        .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
                        .env("NPM_CONFIG_IGNORE_SCRIPTS", "true");
                    assert!(command.status().unwrap().success());

                    let mut expected = vec![
                        "install".to_string(),
                        "--include=dev".into(),
                        "--include=optional".into(),
                        "--include=peer".into(),
                        "--bin-links=true".into(),
                        "--dry-run=false".into(),
                        "--package-lock-only=false".into(),
                        "--package-lock=true".into(),
                        "--global=false".into(),
                        "--location=project".into(),
                        format!("--prefix={}", repo.canonicalize().unwrap().display()),
                        format!("--cpu={}", identity["arch"].as_str().unwrap()),
                        format!("--os={}", identity["platform"].as_str().unwrap()),
                    ];
                    if let Some(libc) = identity["libc"].as_str() {
                        expected.push(format!("--libc={libc}"));
                    }
                    expected.extend([
                        "--workspaces=true".into(),
                        "--include-workspace-root=true".into(),
                    ]);
                    assert_eq!(
                        fs::read_to_string(&install_argv).unwrap(),
                        format!("{}\n", expected.join("\n"))
                    );
                    let environment = fs::read_to_string(&install_env).unwrap();
                    for removed in [
                        "NODE_ENV=",
                        "NPM_CONFIG_OMIT=",
                        "NPM_CONFIG_ONLY=",
                        "NPM_CONFIG_DEV=",
                        "NPM_CONFIG_ALSO=",
                        "Npm_Config_Bin_Links=",
                        "npm_CONFIG_dry_run=",
                        "NPM_CONFIG_PACKAGE_LOCK_ONLY=",
                        "NPM_CONFIG_WORKSPACE=",
                        "NPM_CONFIG_WORKSPACES=",
                        "Npm_Config_Location=",
                        "npm_CONFIG_if_present=",
                    ] {
                        assert!(!environment.lines().any(|line| line.starts_with(removed)));
                    }
                    for preserved in [
                        "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                        "npm_config_install_strategy=nested",
                        "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                        "NPM_CONFIG_IGNORE_SCRIPTS=true",
                    ] {
                        assert!(environment.lines().any(|line| line == preserved));
                    }

                    let mut command = Command::new(node_executable);
                    command
                        .args(["-", "--jig-managed-npm", "run-script", ".", "lint"])
                        .current_dir(&repo)
                        .stdin(fs::File::open(repo.join("scripts/web-node.cjs")).unwrap())
                        .env("JIG_WEB_NODE_HELPER", "run_managed_npm_command")
                        .env("PATH", &path)
                        .env("INSTALL_ARGV", &install_argv)
                        .env("INSTALL_ENV", &install_env)
                        .env("NODE_ENV", "test")
                        .env("NPM_CONFIG_OMIT", "dev optional peer")
                        .env("NPM_CONFIG_INCLUDE", "prod")
                        .env("NPM_CONFIG_PRODUCTION", "true")
                        .env("NPM_CONFIG_OPTIONAL", "false")
                        .env("NPM_CONFIG_ONLY", "production")
                        .env("NPM_CONFIG_DEV", "false")
                        .env("NPM_CONFIG_ALSO", "production")
                        .env("NPM_CONFIG_GLOBAL", "true")
                        .env("NPM_CONFIG_WORKSPACE", "other")
                        .env("NPM_CONFIG_WORKSPACES", "false")
                        .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
                        .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
                        .env("Npm_Config_Location", "global")
                        .env("npm_CONFIG_if_present", "true")
                        .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
                        .env(
                            "NPM_CONFIG_//registry.example.invalid/:_authToken",
                            "test-token",
                        )
                        .env("npm_config_install_strategy", "nested")
                        .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
                        .env("NPM_CONFIG_STRICT_PEER_DEPS", "true")
                        .env("NPM_CONFIG_IGNORE_SCRIPTS", "true")
                        .env("NPM_CONFIG_FOREGROUND_SCRIPTS", "true")
                        .env("NPM_CONFIG_SCRIPT_SHELL", "/bin/sh");
                    assert!(command.status().unwrap().success());

                    let expected = [
                        "--prefix=.",
                        "--workspace=.",
                        "--workspaces=true",
                        "--include-workspace-root=true",
                        "--global=false",
                        "--location=project",
                        "--if-present=false",
                        "--include=dev",
                        "--include=optional",
                        "--include=peer",
                        "run",
                        "lint",
                    ];
                    assert_eq!(
                        fs::read_to_string(&install_argv).unwrap(),
                        format!("{}\n", expected.join("\n"))
                    );
                    let environment = fs::read_to_string(&install_env).unwrap();
                    for removed in [
                        "NPM_CONFIG_OMIT=",
                        "NPM_CONFIG_INCLUDE=",
                        "NPM_CONFIG_PRODUCTION=",
                        "NPM_CONFIG_OPTIONAL=",
                        "NPM_CONFIG_ONLY=",
                        "NPM_CONFIG_DEV=",
                        "NPM_CONFIG_ALSO=",
                        "NPM_CONFIG_GLOBAL=",
                        "NPM_CONFIG_WORKSPACE=",
                        "NPM_CONFIG_WORKSPACES=",
                        "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
                        "NPM_CONFIG_PREFIX=",
                        "Npm_Config_Location=",
                        "npm_CONFIG_if_present=",
                    ] {
                        assert!(!environment.lines().any(|line| line.starts_with(removed)));
                    }
                    for preserved in [
                        "NODE_ENV=test",
                        "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                        "npm_config_install_strategy=nested",
                        "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                        "NPM_CONFIG_STRICT_PEER_DEPS=true",
                        "NPM_CONFIG_IGNORE_SCRIPTS=true",
                        "NPM_CONFIG_FOREGROUND_SCRIPTS=true",
                        "NPM_CONFIG_SCRIPT_SHELL=/bin/sh",
                    ] {
                        assert!(
                            environment.lines().any(|line| line == preserved),
                            "npm script launcher removed supported input {preserved}:\n{environment}"
                        );
                    }
                    #[cfg(target_os = "macos")]
                    assert!(environment.lines().any(|line| line
                        == "NPM_CONFIG_//registry.example.invalid/:_authToken=test-token"));
                }
            }
            assert!(web_check.contains(
                "if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\""
            ));
            assert!(web_check.contains(
                "if [ -f \"$app_dir/npm-shrinkwrap.json\" ]; then printf '%s\\n' \"$app_dir/npm-shrinkwrap.json\""
            ));
            assert_eq!(workflow.matches(r#"- "npm-shrinkwrap.json""#).count(), 2);
            for cache_path in [
                "            npm-shrinkwrap.json",
                "            package-lock.json",
                "            ${{ matrix.app.dir }}/npm-shrinkwrap.json",
                "            ${{ matrix.app.dir }}/package-lock.json",
            ] {
                assert!(
                    workflow.contains(cache_path),
                    "npm dependency cache is missing {cache_path}"
                );
            }
        }
        if package_manager == "yarn" {
            assert_eq!(workflow.matches(r#"- ".yarnrc""#).count(), 2);
            assert_eq!(workflow.matches(r#"- "**/.yarnrc""#).count(), 2);
            assert_eq!(workflow.matches(r#"- "**/.yarnrc.yml""#).count(), 2);
            for runtime_path in [
                r#"- ".yarn/patches/**""#,
                r#"- ".yarn/plugins/**""#,
                r#"- ".yarn/releases/**""#,
            ] {
                assert_eq!(
                    workflow.matches(runtime_path).count(),
                    2,
                    "missing Yarn workflow trigger {runtime_path}"
                );
            }
            for runtime_path in [
                r#"- "**/.yarn/patches/**""#,
                r#"- "**/.yarn/plugins/**""#,
                r#"- "**/.yarn/releases/**""#,
            ] {
                assert_eq!(
                    workflow.matches(runtime_path).count(),
                    2,
                    "missing recursive Yarn workflow trigger {runtime_path}"
                );
            }
        }
        if package_manager == "pnpm" {
            assert!(
                workflow.contains(r#"scripts/check-webapps.sh package-manager-spec "$APP_DIR""#)
            );
            assert!(!workflow.contains("corepack prepare pnpm@11.22.0 --activate"));
        }
        if package_manager == "yarn" {
            assert!(
                workflow.contains(r#"scripts/check-webapps.sh package-manager-spec "$APP_DIR""#)
            );
            assert!(!workflow.contains("corepack prepare yarn@4.18.0 --activate"));
        }
        if matches!(package_manager, "pnpm" | "yarn") {
            assert!(workflow.contains(
                r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec "$APP_DIR")" || exit $?"#
            ));
            assert!(workflow.contains("corepack prepare \"$package_manager_spec\" --activate"));
            assert!(
                !workflow.contains(
                    r#"corepack prepare "$(scripts/check-webapps.sh package-manager-spec"#
                )
            );
            let corepack = workflow.find("corepack enable").unwrap();
            let cache = workflow.find(&format!("cache: {package_manager}")).unwrap();
            assert!(
                corepack < cache,
                "corepack must be enabled before {package_manager} cache setup"
            );
        }
    }
}
