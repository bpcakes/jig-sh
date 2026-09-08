#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn succeeded(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn jig() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD");
    command
}

#[test]
fn postgres_bootstrap_and_database_setup_have_separate_authority() {
    assert_bootstrap_and_database_setup_authority("postgres");
}

#[test]
fn sqlite_bootstrap_and_database_setup_have_separate_authority() {
    assert_bootstrap_and_database_setup_authority("sqlite");
}

#[test]
fn no_database_bootstrap_omits_database_setup() {
    assert_bootstrap_and_database_setup_authority("none");
}

fn assert_bootstrap_and_database_setup_authority(db: &str) {
    let temp = tempfile::tempdir().unwrap();
    let template = temp.path().join("ExampleTemplate");
    materialize_template(&template);
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    executable(
        &bin.join("cargo"),
        r#"#!/bin/sh
set -eu
case "$*" in
  fetch) printf 'cargo fetch\n' >> "$EXAMPLE_CALLS" ;;
  'run -p exampleproject-api -- --bootstrap-database')
    printf 'database setup\n' >> "$EXAMPLE_CALLS"
    if [ "${EXAMPLE_PERMISSION_DENIED:-}" = 1 ]; then
      printf 'permission denied to create database\n' >&2
      exit 7
    fi ;;
  *) printf 'unexpected cargo invocation: %s\n' "$*" >&2; exit 2 ;;
esac
"#,
    );
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let search_path = std::env::join_paths(paths).unwrap();

    let repo = temp.path().join(db).join("ExampleProject");
    let report = succeeded(
        jig()
            .args(["--json", "init"])
            .arg(&repo)
            .args([
                "--preset",
                "rust-react",
                "--db",
                db,
                "--frontend",
                "web:spa",
                "--repo-name",
                "ExampleProject",
                "--template",
            ])
            .arg(&template)
            .args(["--template-mode", "committed", "--no-input", "--no-vault"])
            .output()
            .unwrap(),
    );
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    let package: Value =
        serde_json::from_slice(&fs::read(repo.join("package.json")).unwrap()).unwrap();
    let version = package["packageManager"]
        .as_str()
        .unwrap()
        .strip_prefix("bun@")
        .unwrap();
    executable(
        &bin.join("bun"),
        &format!(
            r#"#!/bin/sh
set -eu
case "${{1:-}}" in
  --version) printf '%s\n' '{version}' ;;
  install)
printf 'bun install\n' >> "$EXAMPLE_CALLS"
printf 'example lock\n' > bun.lock
mkdir -p node_modules/example-dependency
printf '{{"name":"example-dependency","version":"1.0.0"}}\n' > node_modules/example-dependency/package.json ;;
  *) printf 'unexpected bun invocation: %s\n' "$*" >&2; exit 2 ;;
esac
"#
        ),
    );
    let calls = temp.path().join(format!("{db}-calls"));
    let run = |script: &str, url: Option<&str>, deny: bool| {
        let mut command = Command::new("bash");
        command
            .args(["-c", script])
            .current_dir(&repo)
            .env("PATH", &search_path)
            .env("JIG_DEV_BIN", env!("CARGO_BIN_EXE_jig"))
            .env("EXAMPLE_CALLS", &calls)
            .env("EXAMPLE_PERMISSION_DENIED", if deny { "1" } else { "0" })
            .env_remove("DATABASE_URL")
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_INVOKE_CWD");
        if let Some(url) = url {
            command.env("DATABASE_URL", url);
        }
        command.output().unwrap()
    };
    assert!(!repo.join(".env").exists());
    succeeded(run(
        package["scripts"]["bootstrap"].as_str().unwrap(),
        None,
        false,
    ));
    let first = fs::read_to_string(&calls).unwrap();
    assert_eq!(first, "cargo fetch\nbun install\n");
    assert!(repo.join("bun.lock").exists());
    assert!(!repo.join(".env").exists());
    fs::write(&calls, "").unwrap();
    succeeded(run("bash scripts/jig bootstrap", None, false));
    // The second bootstrap reuses the verified frontend install.
    assert_eq!(fs::read_to_string(&calls).unwrap(), "cargo fetch\n");

    if db == "none" {
        assert!(package["scripts"].get("database:setup").is_none());
        assert!(!repo.join("scripts/setup-database.sh").exists());
        return;
    }
    let steps = report["next_steps"].as_array().unwrap();
    let setup = steps
        .iter()
        .position(|step| step == "scripts/jig setup")
        .unwrap();
    let database = steps
        .iter()
        .position(|step| step == "bash scripts/setup-database.sh")
        .unwrap();
    assert!(setup < database);
    fs::write(&calls, "").unwrap();
    let database_script = package["scripts"]["database:setup"].as_str().unwrap();
    let missing = run(database_script, None, false);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing DATABASE_URL"));
    assert!(fs::read_to_string(&calls).unwrap().is_empty());
    let url = if db == "postgres" {
        "postgres://example@localhost/example"
    } else {
        "sqlite:example.db"
    };
    succeeded(run(database_script, Some(url), false));
    assert_eq!(fs::read_to_string(&calls).unwrap(), "database setup\n");
    let denied = run(database_script, Some(url), true);
    assert_eq!(denied.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission denied"));
    fs::write(repo.join(".env"), format!("DATABASE_URL={url}\n")).unwrap();
    succeeded(run(database_script, None, false));

    assert_authored_commands_survive_updates(&repo, &package);
}

fn assert_authored_commands_survive_updates(repo: &Path, package: &Value) {
    let config_path = repo.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["repo_bootstrap_command"] = "printf 'authored bootstrap\\n'".into();
    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    let authored_script = "#!/bin/sh\nprintf 'authored database setup\\n'\n";
    fs::write(repo.join("scripts/setup-database.sh"), authored_script).unwrap();
    let mut authored_package = package.clone();
    authored_package["scripts"]["bootstrap"] =
        Value::String("printf 'authored package bootstrap\\n'".into());
    fs::write(
        repo.join("package.json"),
        serde_json::to_vec_pretty(&authored_package).unwrap(),
    )
    .unwrap();
    for recopy in [false, true] {
        let mut update = jig();
        update
            .current_dir(repo)
            .args(["--json", "update", "--force", "--no-input"]);
        if recopy {
            update.arg("--recopy");
        }
        succeeded(update.output().unwrap());
        let updated: toml::Value =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(
            updated["commands"]["repo_bootstrap_command"],
            config["commands"]["repo_bootstrap_command"]
        );
        assert_eq!(
            fs::read_to_string(repo.join("scripts/setup-database.sh")).unwrap(),
            authored_script
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(repo.join("package.json")).unwrap()).unwrap(),
            authored_package
        );
    }
}

fn materialize_template(destination: &Path) {
    // Recopy needs a committed template, but no source history or Rust workspace.
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates"),
        &destination.join("templates"),
    );
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["add", "templates"],
        vec![
            "-c",
            "user.name=Example",
            "-c",
            "user.email=example@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Example template",
        ],
    ] {
        succeeded(
            Command::new("git")
                .args(args)
                .current_dir(destination)
                .output()
                .unwrap(),
        );
    }
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
