//! Resuming a Claude session from the home whose transcripts hold it.

use super::*;

const SESSION_ID: &str = "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f";

fn write_transcript(home: &Path) {
    let project = home.join("projects/-tmp-ExampleProject");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join(format!("{SESSION_ID}.jsonl")), "{}\n").unwrap();
}

#[test]
fn resume_launches_the_owning_home_from_the_invocation_directory() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let caller = root.join("ExampleProject");
    fs::create_dir(&caller).unwrap();
    fs::create_dir(root.join(".claude")).unwrap();
    write_transcript(&root.join(".claude-work"));
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\nprintf '%s\\n' \"${CLAUDE_CONFIG_DIR-unset}\"\npwd -P\nprintf '<%s>\\n' \"$@\"\n",
    );

    let output = jig(&caller)
        .env("HOME", root)
        .args(["claude", "resume", &SESSION_ID.to_ascii_uppercase()])
        .args(["--", "--model", "two words"])
        .env("JIG_CLAUDE_BIN", &stub)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}\n{}\n<--resume>\n<{SESSION_ID}>\n<--model>\n<two words>\n",
            root.join(".claude-work").canonicalize().unwrap().display(),
            caller.canonicalize().unwrap().display()
        )
    );
}

#[test]
fn resume_keeps_the_configuration_mode_of_the_default_directory() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let default = root.join(".claude");
    write_transcript(&default);
    fs::create_dir(root.join(".claude-work")).unwrap();
    let canonical = default.canonicalize().unwrap();
    let dry_run = |config_dir: Option<&Path>| {
        let mut command = jig(root);
        command.args(["claude", "resume", SESSION_ID, "--dry-run", "--json"]);
        if let Some(config_dir) = config_dir {
            command.env("CLAUDE_CONFIG_DIR", config_dir);
        }
        report(command.output().unwrap())
    };

    for current in [None, Some(root.join(".claude-work"))] {
        let value = dry_run(current.as_deref());
        assert_eq!(value["command"], "claude resume");
        assert!(value["config_dir"].is_null(), "{value}");
        assert_eq!(value["home"], canonical.to_str().unwrap());
        assert_eq!(value["args"], serde_json::json!(["--resume", SESSION_ID]));
    }
    let value = dry_run(Some(&default));
    assert_eq!(value["config_dir"], canonical.to_str().unwrap());
}

#[test]
fn resume_needs_one_owning_home_unless_home_is_explicit() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join(".claude")).unwrap();
    fs::create_dir(root.join(".claude-work")).unwrap();
    let marker = root.join("launched");
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\ntouch \"$JIG_TEST_MARKER\"\n",
    );
    let run = |args: &[&str]| {
        jig(root)
            .args(args)
            .env("JIG_CLAUDE_BIN", &stub)
            .env("JIG_TEST_MARKER", &marker)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    let output = run(&["claude", "resume", SESSION_ID]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(&format!("Claude session '{SESSION_ID}' was not found")),
        "{error}"
    );
    assert!(error.contains("Pass --home HOME"), "{error}");

    let value = report(run(&[
        "claude",
        "resume",
        SESSION_ID,
        "--home",
        "work",
        "--dry-run",
        "--json",
        "--",
        "--fork-session",
    ]));
    assert_eq!(
        value["config_dir"],
        root.join(".claude-work")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        value["args"],
        serde_json::json!(["--resume", SESSION_ID, "--fork-session"])
    );

    write_transcript(&root.join(".claude"));
    write_transcript(&root.join(".claude-work"));
    let output = run(&["claude", "resume", SESSION_ID]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("exists in multiple homes"), "{error}");

    for args in [
        vec!["claude", "resume", "not-a-session-id"],
        vec!["claude", "resume", SESSION_ID, "--json"],
        vec!["claude", "resume", SESSION_ID, "--home", "work", "--json"],
    ] {
        let output = run(&args);
        assert!(!output.status.success(), "{args:?}");
        if args.contains(&"--json") {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
        }
    }
    assert!(!marker.exists());
}

#[test]
fn resume_shows_terminal_progress_while_homes_are_searched() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join(".claude")).unwrap();
    write_transcript(&root.join(".claude-work"));
    let (mut master, slave) = pseudo_terminal();
    let mut child = pty_support::ChildGuard::new(
        jig(root)
            .args(["claude", "resume", SESSION_ID, "--dry-run"])
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave)
            .spawn()
            .unwrap(),
    );

    let mut output = Vec::new();
    let status = pty_support::wait_for_child_while_draining(
        &mut child,
        &mut master,
        &mut output,
        Duration::from_secs(5),
    )
    .expect("resume exited");
    let output = String::from_utf8_lossy(&output);

    assert!(status.success(), "{output}");
    assert!(
        output.contains("jig claude resume | find the Claude home containing the session"),
        "{output}"
    );
    assert!(output.contains("inspect homes"), "{output}");
    assert!(output.contains("2/2"), "{output}");
    assert!(
        output.contains("found the session's Claude home"),
        "{output}"
    );
    assert!(output.contains("Claude resume (dry run)"), "{output}");
}
