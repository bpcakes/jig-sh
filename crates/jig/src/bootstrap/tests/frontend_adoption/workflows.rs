use super::*;

#[cfg(unix)]
#[test]
fn generated_npm_run_script_selects_exact_app_without_installing() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("npm-run-script");
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
            repo_name: Some("npm-run-script".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "standalone".into(),
                    dir: "standalone".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("standalone")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();
    for app_dir in ["apps/web", "standalone"] {
        fs::write(
            repo.join(app_dir).join("package.json"),
            format!(
                r#"{{"name":"{}","scripts":{{"probe":"true"}}}}"#,
                app_dir.replace('/', "-")
            ),
        )
        .unwrap();
    }
    fs::write(
        repo.join("standalone/package-lock.json"),
        "standalone lock\n",
    )
    .unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    : > "$UNEXPECTED_INSTALL"
    exit 91
    ;;
esac
printf '%s\n' "$@" > "$RUN_ARGV"
pwd -P > "$RUN_CWD"
env | LC_ALL=C sort > "$RUN_ENV"
count=0
[ ! -f "$RUN_COUNT" ] || count="$(cat "$RUN_COUNT")"
printf '%s\n' "$((count + 1))" > "$RUN_COUNT"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let run_count = repo.join("run-count");
    let unexpected_install = repo.join("unexpected-install");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run_script = |app_dir: &str, script_name: &str, suffix: &str| {
        let argv = repo.join(format!("run-{suffix}-argv"));
        let cwd = repo.join(format!("run-{suffix}-cwd"));
        let environment = repo.join(format!("run-{suffix}-env"));
        let node_environment = if suffix == "standalone" {
            "production"
        } else {
            "test"
        };
        let output = std::process::Command::new("bash")
            .args([
                "scripts/check-webapps.sh",
                "run-script",
                app_dir,
                script_name,
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("RUN_ARGV", &argv)
            .env("RUN_CWD", &cwd)
            .env("RUN_ENV", &environment)
            .env("RUN_COUNT", &run_count)
            .env("UNEXPECTED_INSTALL", &unexpected_install)
            .env("NODE_ENV", node_environment)
            .env("NPM_CONFIG_OMIT", "dev optional peer")
            .env("NPM_CONFIG_INCLUDE", "prod")
            .env("NPM_CONFIG_PRODUCTION", "true")
            .env("NPM_CONFIG_OPTIONAL", "false")
            .env("NPM_CONFIG_ONLY", "production")
            .env("NPM_CONFIG_DEV", "false")
            .env("NPM_CONFIG_ALSO", "production")
            .env("Npm_Config_Workspace", "missing")
            .env("npm_CONFIG_workspaces", "false")
            .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
            .env("npm_config_include-workspace-root", "false")
            .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
            .env("NPM_CONFIG_GLOBAL", "true")
            .env("Npm_Config_Location", "global")
            .env("npm_CONFIG_if_present", "true")
            .env("NPM_CONFIG_IF-PRESENT", "true")
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
            .env("NPM_CONFIG_SCRIPT_SHELL", "/bin/sh")
            .output()
            .unwrap();
        (output, argv, cwd, environment)
    };

    let expected_argv = [
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
        "probe",
    ];
    for (app_dir, suffix) in [("apps/web", "workspace"), ("standalone", "standalone")] {
        let (output, argv, cwd, environment) = run_script(app_dir, "probe", suffix);
        assert!(
            output.status.success(),
            "npm run-script failed for {app_dir}:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(argv).unwrap(),
            format!("{}\n", expected_argv.join("\n"))
        );
        assert_eq!(
            fs::read_to_string(cwd).unwrap().trim(),
            fs::canonicalize(repo.join(app_dir))
                .unwrap()
                .display()
                .to_string()
        );
        let environment = fs::read_to_string(environment).unwrap();
        for removed in [
            "NPM_CONFIG_OMIT=",
            "NPM_CONFIG_INCLUDE=",
            "NPM_CONFIG_PRODUCTION=",
            "NPM_CONFIG_OPTIONAL=",
            "NPM_CONFIG_ONLY=",
            "NPM_CONFIG_DEV=",
            "NPM_CONFIG_ALSO=",
            "Npm_Config_Workspace=",
            "npm_CONFIG_workspaces=",
            "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
            "npm_config_include-workspace-root=",
            "NPM_CONFIG_PREFIX=",
            "NPM_CONFIG_GLOBAL=",
            "Npm_Config_Location=",
            "npm_CONFIG_if_present=",
            "NPM_CONFIG_IF-PRESENT=",
        ] {
            assert!(
                !environment.lines().any(|line| line.starts_with(removed)),
                "npm run-script inherited shaping input {removed}:\n{environment}"
            );
        }
        let expected_node_environment = if suffix == "standalone" {
            "NODE_ENV=production"
        } else {
            "NODE_ENV=test"
        };
        assert!(
            environment
                .lines()
                .any(|line| line == expected_node_environment),
            "npm run-script changed explicit {expected_node_environment}:\n{environment}"
        );
        for preserved in [
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
                "npm run-script removed supported input {preserved}:\n{environment}"
            );
        }
        #[cfg(target_os = "macos")]
        assert!(
            environment
                .lines()
                .any(|line| line == "NPM_CONFIG_//registry.example.invalid/:_authToken=test-token")
        );
    }
    assert_eq!(fs::read_to_string(&run_count).unwrap().trim(), "2");
    assert!(!unexpected_install.exists());
    assert!(!repo.join("node_modules").exists());
    assert!(!repo.join("standalone/node_modules").exists());
    assert!(!repo.join(".agent/tmp/web-dependencies").exists());

    let (missing, _, _, _) = run_script("apps/web", "missing", "missing");
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&run_count).unwrap().trim(), "2");

    let unconfigured = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "run-script",
            "unconfigured",
            "probe",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(unconfigured.status.code(), Some(2));
    let wrong_arity = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "run-script", "apps/web"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(wrong_arity.status.code(), Some(2));
}

#[test]
fn generated_project_workflows_serialize_dynamic_yaml_scalars_and_shell_branch_values() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("yaml-scalars");
    let default_branch = "release#candidate";
    let runner = "self-hosted # primary";

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
            repo_name: Some("yaml-scalars".into()),
            default_branch: Some(default_branch.into()),
            ci_github_runner: Some(runner.into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "null".into(),
                dir: "null".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    for workflow in [
        "webapp-checks.yml",
        "repo-policy.yml",
        "rust-tests.yml",
        "agent-map-check.yml",
    ] {
        let text = fs::read_to_string(repo.join(".github/workflows").join(workflow)).unwrap();
        let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|error| panic!("{workflow} was invalid YAML: {error}\n{text}"));
        assert_eq!(yaml["on"]["push"]["branches"][0], default_branch);
        let jobs = yaml["jobs"].as_object().unwrap();
        for job in jobs.values() {
            assert_eq!(job["runs-on"], runner, "wrong runner in {workflow}");
        }
    }

    let webapp = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    let webapp = serde_yaml_ng::from_str::<serde_json::Value>(&webapp).unwrap();
    let app = &webapp["jobs"]["checks"]["strategy"]["matrix"]["app"][0];
    assert_eq!(app["name"], "null");
    assert_eq!(app["dir"], "null");

    let policy = fs::read_to_string(repo.join(".github/workflows/repo-policy.yml")).unwrap();
    assert!(policy.contains("JIG_DEFAULT_BRANCH:"));
    assert!(policy.contains(r#""origin/$JIG_DEFAULT_BRANCH""#));
    assert!(!policy.contains(&format!("origin/{default_branch}")));
}

#[cfg(unix)]
#[test]
fn generated_node_version_query_rejects_every_present_invalid_authority() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("node-version-authority");
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
            repo_name: Some("node-version-authority".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    fs::create_dir(repo.join("web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["web"]}"#,
    )
    .unwrap();
    fs::write(repo.join("web/package.json"), r#"{"private":true}"#).unwrap();

    let query = || {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "node-version-file", "web"])
            .current_dir(&repo)
            .output()
            .unwrap()
    };
    let assert_query_status = |expected: i32, label: &str| {
        let output = query();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{label}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };

    assert_query_status(1, "true absence");

    fs::write(repo.join("web/.node-version"), ">=22\n").unwrap();
    let app_valid = assert_query_status(0, "valid app selector");
    assert_eq!(
        String::from_utf8_lossy(&app_valid.stdout).trim(),
        "web/.node-version"
    );

    fs::write(repo.join(".node-version"), "22.22.2\r\n").unwrap();
    let root_valid = assert_query_status(0, "valid root selector");
    assert_eq!(
        String::from_utf8_lossy(&root_valid.stdout).trim(),
        ".node-version"
    );

    fs::write(repo.join(".node-version"), "invalid selector with spaces\n").unwrap();
    assert_query_status(
        2,
        "invalid root must not fall through to valid app selector",
    );
    fs::write(repo.join(".node-version"), "").unwrap();
    assert_query_status(2, "empty authority");
    fs::write(repo.join(".node-version"), "22.22.2\nsecond\n").unwrap();
    assert_query_status(2, "multiline authority");
    fs::write(repo.join(".node-version"), [0xff]).unwrap();
    assert_query_status(2, "non-UTF-8 authority");
    fs::write(repo.join(".node-version"), b"22.22.2\x7f\n").unwrap();
    assert_query_status(2, "control character authority");
    fs::write(repo.join(".node-version"), vec![b'2'; 129]).unwrap();
    assert_query_status(2, "oversized authority");

    fs::remove_file(repo.join(".node-version")).unwrap();
    fs::create_dir(repo.join(".node-version")).unwrap();
    assert_query_status(2, "directory authority");
    fs::remove_dir(repo.join(".node-version")).unwrap();

    let fifo = repo.join(".node-version");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a live, NUL-terminated CString for the duration of
    // the call, and the mode contains only valid permission bits.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
    assert_query_status(2, "FIFO authority");
    fs::remove_file(&fifo).unwrap();

    let outside = temp.path().join("outside-node-version");
    fs::write(&outside, "20.0.0\n").unwrap();
    symlink(&outside, repo.join(".node-version")).unwrap();
    assert_query_status(2, "root symlink authority");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "20.0.0\n");
    fs::remove_file(repo.join(".node-version")).unwrap();

    fs::remove_file(repo.join("web/.node-version")).unwrap();
    symlink(&outside, repo.join("web/.node-version")).unwrap();
    assert_query_status(2, "app symlink authority");
    fs::remove_file(repo.join("web/.node-version")).unwrap();

    fs::rename(repo.join("web"), repo.join("web-real")).unwrap();
    let outside_app = temp.path().join("outside-app");
    fs::create_dir(&outside_app).unwrap();
    fs::write(outside_app.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(outside_app.join(".node-version"), "20.0.0\n").unwrap();
    symlink(&outside_app, repo.join("web")).unwrap();
    assert_query_status(2, "symlinked authority parent");

    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
    assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
    assert!(workflow.contains("set -o noclobber"));
    assert!(!workflow.contains("> .node-version"));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "20.0.0\n");
}

#[test]
fn generated_project_workflow_jobs_force_bash_on_windows_runners() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("windows-workflows");

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
            repo_name: Some("windows-workflows".into()),
            ci_github_runner: Some("windows-latest".into()),
            sqlx_enabled: Some(true),
            schema_dump_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    for workflow in [
        "webapp-checks.yml",
        "repo-policy.yml",
        "rust-tests.yml",
        "agent-map-check.yml",
    ] {
        let text = fs::read_to_string(repo.join(".github/workflows").join(workflow)).unwrap();
        let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|error| panic!("{workflow} was invalid YAML: {error}\n{text}"));
        for (job_name, job) in yaml["jobs"].as_object().unwrap() {
            assert_eq!(
                job["runs-on"], "windows-latest",
                "wrong Windows runner for {workflow}:{job_name}"
            );
            assert_eq!(
                job["defaults"]["run"]["shell"], "bash",
                "{workflow}:{job_name} must force Bash for generated run steps"
            );
        }
    }

    let source = fs::read_to_string(
        template
            .path()
            .join("templates/project/.github/workflows/webapp-checks.yml.jinja"),
    )
    .unwrap();
    let mut environment = minijinja::Environment::new();
    environment.set_syntax(
        minijinja::syntax::SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()
            .unwrap(),
    );
    environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let text = environment
        .render_str(
            &source,
            serde_json::json!({
                "frontend_apps": [],
                "ci_github_runner": "windows-latest",
            }),
        )
        .unwrap();
    let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text).unwrap_or_else(|error| {
        panic!("disabled webapp workflow was invalid YAML: {error}\n{text}")
    });
    assert_eq!(yaml["jobs"]["disabled"]["runs-on"], "windows-latest");
    assert_eq!(yaml["jobs"]["disabled"]["defaults"]["run"]["shell"], "bash");
}
