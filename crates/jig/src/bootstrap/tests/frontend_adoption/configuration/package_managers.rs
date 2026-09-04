fn assert_package_manager_rendered_helpers(
    repo: &Path,
    package_manager: &str,
    install_command: &str,
    run_command: &str,
) {
    assert!(!repo.join("Makefile").exists());
    let agent_guidance = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert_text_contains_all(
        &agent_guidance,
        &[
            "Generated install steps select the package-manager project from workspace membership, not root-lock presence",
            "It ignores only real top-level tool-cache directories",
        ],
    );
    assert_text_contains_none(
        &agent_guidance,
        &["Generated install steps use a repo-root lockfile when one exists"],
    );
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let web_node = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let web_sources = format!("{web_check}\n{web_node}");
    if package_manager == "npm" {
        assert_text_contains_all(&web_check, &["run_npm_dependency_install", install_command]);
    } else {
        assert_text_contains_all(&web_check, &[install_command]);
    }
    assert_text_contains_all(
        &web_check,
        &[
            run_command,
            "if dependencies_present \"$app_dir\"",
            "acquire_install_lock",
            "dependency_stamp_path",
            "dependency_fingerprint",
            "jig-web-dependencies-v3",
            "record_dependency_state",
            "bootstrap_dependencies",
            "dependencies-bootstrap",
            "dependencies-ready",
            "dependencies-install",
            "node_version_file",
            r#"app_dir="apps/web""#,
            r#""$app_dir/.node-version""#,
            "start_install_worker",
            "transfer_install_lock_to_worker",
            "recover_stale_install_lock",
        ],
    );
    assert_text_contains_none(&web_check, &["root_lock_exists"]);
    assert_text_contains_all(&web_node, &["maximumEntries = 10_000"]);
    assert_eq!(
        web_node.contains("volatilePnpmWorkspaceState"),
        package_manager == "pnpm",
        "only pnpm checkers should exclude pnpm's volatile workspace-state cache"
    );
    #[cfg(unix)]
    {
        let syntax = std::process::Command::new("/bin/bash")
            .args(["-n", "scripts/check-webapps.sh"])
            .current_dir(repo)
            .status()
            .unwrap();
        assert!(
            syntax.success(),
            "invalid rendered Bash for {package_manager}"
        );
        let node_syntax = std::process::Command::new("node")
            .args(["--check", "scripts/web-node.cjs"])
            .current_dir(repo)
            .status()
            .unwrap();
        assert!(
            node_syntax.success(),
            "invalid rendered Node helper for {package_manager}"
        );
    }
    if package_manager == "yarn" {
        assert_text_contains_all(
            &web_sources,
            &[
                "dependency_artifact_kind",
                "yarn_berry_config_payload",
                "yarn_berry_pnp_artifact_proof",
                "pnpEnableInlining",
                "pnpEnableEsmLoader",
                "installStatePath",
                "pnpUnpluggedFolder",
                "const RAW_RUNTIME_STATE",
                "setupStatePackageLocations",
                "maximumParsedInputBytes = 64 * 1024 * 1024",
                "hashFile(loader, \"loader\", maximumParsedInputBytes)",
                "hashFile(dataPath, \"data\", maximumParsedInputBytes)",
                "too many PnP package locations",
                "yarn_classic_actual_artifact_kind",
                "yarn_classic_pnp_artifact_proof",
                "YARN_PLUGNPLAY_OVERRIDE",
                r#""$scope/.pnp.cjs""#,
                r#""$scope/.pnp.js""#,
                r#"yarn_scope_authority_paths "$scope" "$authority""#,
                "yarn_runtime_identity",
                "yarn@1.22.22",
                ".yarn/patches",
                ".yarn/plugins",
                ".yarn/releases",
            ],
        );
    }
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    let expected_dev_argv = if package_manager == "npm" {
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]".to_string()
    } else {
        format!("argv = [\"{package_manager}\", \"run\", \"dev\"]")
    };
    assert_text_contains_all(&answers, &[&expected_dev_argv]);
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
}

fn assert_package_manager_workflow_common(workflow: &str, package_manager: &str) {
    assert_text_contains_all(
        workflow,
        &[
            "Classic required status checks can remain pending",
            "APP_DIR: ${{ matrix.app.dir }}",
            r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" lint"#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" typecheck"#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" build:bundle"#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" test:coverage"#,
            "scripts/check-webapps.sh node-version-file \"$APP_DIR\"",
            "${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}",
            "mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\"",
            "set -o noclobber",
            "'24.19.0' > \"$node_version_file\"",
            "status=$?",
            "if [ \"$status\" -eq 1 ]",
            "exit \"$status\"",
            "$node_version_file\" >> \"$GITHUB_OUTPUT",
        ],
    );
    assert_text_contains_none(
        workflow,
        &[
            r#"cd "$APP_DIR" &&"#,
            "if [ -f package.json ]",
            "> .node-version",
            "if ! node_version_file=",
            "node-version: 22.12.0",
            "node-version-file: .node-version",
        ],
    );
    assert_text_occurrences(
        workflow,
        &[(r#"scripts/check-webapps.sh run-script "$APP_DIR""#, 4)],
    );
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
    assert_text_occurrences(
        workflow,
        &[
            (r#"- "**/package.json""#, 2),
            (r#"- "**/package.json5""#, 2),
            (r#"- "**/package.yaml""#, 2),
            (r#"- "**/.node-version""#, 2),
            (r#"- "**/.npmrc""#, 2),
            (r#"- "**/*.patch""#, 2),
            (r#"- "**/*.diff""#, 2),
        ],
    );
    let bootstrap_node = workflow
        .find("Bootstrap Node for dependency metadata")
        .unwrap();
    let resolve_node = workflow.find("Resolve Node version file").unwrap();
    assert!(bootstrap_node < resolve_node);
    if package_manager == "bun" {
        assert_text_contains_all(
            workflow,
            &["oven-sh/setup-bun@v2", "bun-version: \"1.3.14\""],
        );
        assert_text_occurrences(workflow, &[(r#"- "bun.lockb""#, 2)]);
    } else {
        assert_text_contains_all(workflow, &[&format!("cache: {package_manager}")]);
    }
}

#[cfg(unix)]
fn assert_npm_launcher_contract(repo: &Path) {
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
            .current_dir(repo)
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
        assert_environment_contains_none(
            &environment,
            &[
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
            ],
        );
        assert_environment_contains_all(
            &environment,
            &[
                "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                "npm_config_install_strategy=nested",
                "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                "NPM_CONFIG_IGNORE_SCRIPTS=true",
            ],
        );

        let mut command = Command::new(node_executable);
        command
            .args(["-", "--jig-managed-npm", "run-script", ".", "lint"])
            .current_dir(repo)
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
        assert_environment_contains_none(
            &environment,
            &[
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
            ],
        );
        assert_environment_contains_all(
            &environment,
            &[
                "NODE_ENV=test",
                "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                "npm_config_install_strategy=nested",
                "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                "NPM_CONFIG_STRICT_PEER_DEPS=true",
                "NPM_CONFIG_IGNORE_SCRIPTS=true",
                "NPM_CONFIG_FOREGROUND_SCRIPTS=true",
                "NPM_CONFIG_SCRIPT_SHELL=/bin/sh",
            ],
        );
        #[cfg(target_os = "macos")]
        assert!(
            environment
                .lines()
                .any(|line| line == "NPM_CONFIG_//registry.example.invalid/:_authToken=test-token")
        );
    }
}

fn assert_npm_setting_names(contents: &str, settings: &[&str]) {
    for setting in settings {
        assert_text_contains_all(contents, &[&format!("  \"{setting}\",")]);
    }
}

fn assert_npm_package_manager_contract(
    repo: &Path,
    package_manager: &str,
    web_check: &str,
    web_sources: &str,
    workflow: &str,
) {
    if package_manager == "npm" {
        assert_text_contains_all(
            web_sources,
            &[
                "run_managed_npm_command install \"$1\" \"$2\"",
                "run_managed_npm_command run-script \"$1\" \"$2\"",
                "const result = spawnSync(\"npm\", args, {",
                "stdio: \"inherit\"",
                "shell: false",
                "const match = /^npm_config_(.*)$/i.exec(key);",
                "match[1].replaceAll(\"_\", \"-\").toLowerCase()",
            ],
        );
        assert_npm_setting_names(
            web_sources,
            &[
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
            ],
        );
        assert_text_contains_none(
            web_sources,
            &["  \"ignore-scripts\",", "  \"install-strategy\","],
        );
        assert_text_contains_all(
            web_sources,
            &[
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
            ],
        );
        #[cfg(unix)]
        assert_npm_launcher_contract(repo);
        assert_text_contains_all(
            web_check,
            &[
                "if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\"",
                "if [ -f \"$app_dir/npm-shrinkwrap.json\" ]; then printf '%s\\n' \"$app_dir/npm-shrinkwrap.json\"",
            ],
        );
        assert_text_occurrences(workflow, &[(r#"- "npm-shrinkwrap.json""#, 2)]);
        assert_text_contains_all(
            workflow,
            &[
                "            npm-shrinkwrap.json",
                "            package-lock.json",
                "            ${{ matrix.app.dir }}/npm-shrinkwrap.json",
                "            ${{ matrix.app.dir }}/package-lock.json",
            ],
        );
    }
}

fn assert_yarn_and_corepack_workflow(workflow: &str, package_manager: &str) {
    if package_manager == "yarn" {
        assert_text_occurrences(
            workflow,
            &[
                (r#"- ".yarnrc""#, 2),
                (r#"- "**/.yarnrc""#, 2),
                (r#"- "**/.yarnrc.yml""#, 2),
                (r#"- ".yarn/patches/**""#, 2),
                (r#"- ".yarn/plugins/**""#, 2),
                (r#"- ".yarn/releases/**""#, 2),
                (r#"- "**/.yarn/patches/**""#, 2),
                (r#"- "**/.yarn/plugins/**""#, 2),
                (r#"- "**/.yarn/releases/**""#, 2),
            ],
        );
    }
    if matches!(package_manager, "pnpm" | "yarn") {
        assert_text_contains_all(
            workflow,
            &[
                r#"scripts/check-webapps.sh package-manager-spec "$APP_DIR""#,
                r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec "$APP_DIR")" || exit $?"#,
                "corepack prepare \"$package_manager_spec\" --activate",
            ],
        );
        assert_text_contains_none(
            workflow,
            &[
                &format!("corepack prepare {package_manager}@"),
                r#"corepack prepare "$(scripts/check-webapps.sh package-manager-spec"#,
            ],
        );
        let corepack = workflow.find("corepack enable").unwrap();
        let cache = workflow.find(&format!("cache: {package_manager}")).unwrap();
        assert!(
            corepack < cache,
            "corepack must be enabled before {package_manager} cache setup"
        );
    }
}

use super::*;

fn assert_package_manager_case(
    root: &Path,
    template: &Path,
    package_manager: &str,
    install_command: &str,
    run_command: &str,
) {
    let repo = root.join(package_manager);
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.display().to_string()),
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

    assert_package_manager_rendered_helpers(&repo, package_manager, install_command, run_command);
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let web_node = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let web_sources = format!("{web_check}\n{web_node}");
    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_package_manager_workflow_common(&workflow, package_manager);
    assert_npm_package_manager_contract(
        &repo,
        package_manager,
        &web_check,
        &web_sources,
        &workflow,
    );
    assert_yarn_and_corepack_workflow(&workflow, package_manager);
}

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
        assert_package_manager_case(
            temp.path(),
            template.path(),
            package_manager,
            install_command,
            run_command,
        );
    }
}
