#![cfg(unix)]

#[path = "shared/pty.rs"]
mod pty_support;
mod support;

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

fn jig(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(root)
        .env("HOME", root)
        // Prevent PTY inspections from consulting the developer's macOS Keychain.
        .env("CLAUDE_CODE_OAUTH_TOKEN", "fixture-inspection-disabled")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_CLAUDE_BIN");
    command
}

fn report(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn executable(path: PathBuf, script: &str) -> PathBuf {
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn discovery_includes_current_home_deduplicates_aliases_and_ignores_files() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    for name in [".claude", ".claude-work", "custom"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join(".claude-file"), "").unwrap();
    symlink(root.join(".claude-work"), root.join(".claude-z-alias")).unwrap();
    let value = report(
        jig(root)
            .args(["claude", "homes", "--json"])
            .env("CLAUDE_CONFIG_DIR", "custom")
            .env("JIG_CLAUDE_BIN", "/missing/claude")
            .output()
            .unwrap(),
    );
    let homes = value["homes"].as_array().unwrap();
    assert_eq!(homes.len(), 3);
    assert_eq!(homes[0]["name"], "claude");
    let current = homes.iter().find(|home| home["current"] == true).unwrap();
    assert_eq!(current["name"], "custom");
    assert_eq!(value["outcome"], "complete");
    assert_eq!(value["representation_lossy"], false);
    let partial = report(
        jig(root)
            .args(["claude", "homes", "--json"])
            .env("CLAUDE_CONFIG_DIR", root.join("missing"))
            .output()
            .unwrap(),
    );
    assert_eq!(partial["outcome"], "partial");
    assert_eq!(partial["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn empty_discovery_and_empty_environment_use_default_without_creating_it() {
    let temp = support::tempdir().unwrap();
    let value = report(
        jig(temp.path())
            .args(["claude", "homes", "--json"])
            .env("CLAUDE_CONFIG_DIR", "")
            .output()
            .unwrap(),
    );
    let homes = value["homes"].as_array().unwrap();
    assert_eq!(homes.len(), 1);
    assert_eq!(homes[0]["name"], "claude");
    assert_eq!(homes[0]["default_config"], true);
    assert_eq!(homes[0]["current"], true);
    assert_eq!(value["outcome"], "complete");
    assert!(!temp.path().join(".claude").exists());
}

#[test]
fn discovery_warns_when_native_default_is_a_file() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let default = root.join(".claude");
    fs::write(&default, "preserve this file").unwrap();
    fs::create_dir(root.join(".claude-work")).unwrap();
    let warning = format!(
        "Claude configuration home is not a directory: {}",
        default.display()
    );

    for config in [None, Some(""), Some(".claude"), Some(".claude-work")] {
        let mut command = jig(root);
        command.args(["claude", "homes", "--json"]);
        if let Some(config) = config {
            command.env("CLAUDE_CONFIG_DIR", config);
        }
        let value = report(command.output().unwrap());
        assert_eq!(value["outcome"], "partial");
        assert_eq!(value["warnings"], serde_json::json!([warning]));
        assert_eq!(value["homes"][0]["default_config"], true);
    }

    let output = jig(root).args(["claude", "homes"]).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(&format!("Warning: {warning}")));
    assert!(text.contains("[default config]"));
    assert_eq!(fs::read_to_string(default).unwrap(), "preserve this file");
}

#[test]
fn optional_usage_report_is_partial_on_unavailable_credentials_and_stays_machine_readable() {
    let temp = support::tempdir().unwrap();
    fs::create_dir(temp.path().join(".claude-work")).unwrap();
    let value = report(
        jig(temp.path())
            .args(["claude", "homes", "--usage", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(value["usage_included"], true);
    assert_eq!(value["outcome"], "partial");
    for home in value["homes"].as_array().unwrap() {
        assert!(
            home["inspection_error"]
                .as_str()
                .unwrap()
                .contains("CLAUDE_CODE_OAUTH_TOKEN")
        );
        assert_eq!(home["rate_limits"], serde_json::json!([]));
        assert_eq!(home["status"], "unknown");
    }
    assert!(!value.to_string().contains("fixture-inspection-disabled"));
    let output = jig(temp.path())
        .args(["claude", "homes", "--usage"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Usage unavailable:")
    );
    let discovery = report(
        jig(temp.path())
            .args(["claude", "homes", "--json"])
            .output()
            .unwrap(),
    );
    assert!(discovery.get("usage_included").is_none());
    assert_eq!(discovery["outcome"], "complete");
}

#[test]
fn names_aliases_and_explicit_paths_resolve_without_cwd_shadowing() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    for name in [".claude", ".claude-work", "work", "custom"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    for (input, expected) in [
        ("default", ".claude"),
        ("claude", ".claude"),
        ("work", ".claude-work"),
        ("claude-work", ".claude-work"),
        ("~/.claude-work", ".claude-work"),
        ("./work", "work"),
        ("custom", "custom"),
    ] {
        let value = report(
            jig(root)
                .args(["claude", "launch", input, "--dry-run", "--json"])
                .env("CLAUDE_CONFIG_DIR", root.join("custom"))
                .output()
                .unwrap(),
        );
        assert_eq!(
            value["home"],
            root.join(expected)
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
    }
    let value = report(
        jig(root)
            .args(["claude", "launch"])
            .arg(root.join("work"))
            .args(["--dry-run", "--json"])
            .env("HOME", root.join("missing-user-home"))
            .output()
            .unwrap(),
    );
    assert_eq!(
        value["home"],
        root.join("work").canonicalize().unwrap().to_str().unwrap()
    );
    fs::remove_dir(root.join(".claude-work")).unwrap();
    assert!(
        !jig(root)
            .args(["claude", "launch", "work", "--dry-run"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn launch_preserves_argv_environment_streams_cwd_and_exit_status() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join(".claude-work")).unwrap();
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\nprintf '%s\\n' \"$CLAUDE_CONFIG_DIR\" \"$JIG_TEST_PRESERVED\"\npwd -P\nprintf '<%s>\\n' \"$@\"\ncat\nprintf 'child stderr' >&2\nexit 37\n",
    );
    let mut child = jig(root)
        .args([
            "claude",
            "launch",
            "work",
            "--",
            "--model",
            "two words",
            "$(touch forbidden)",
            "",
            "--json",
        ])
        .env("JIG_CLAUDE_BIN", stub)
        .env("CLAUDE_CONFIG_DIR", "/unused/ambient")
        .env("JIG_TEST_PRESERVED", "preserved")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"child stdin\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(37));
    assert_eq!(output.stderr, b"child stderr");
    let expected = format!(
        "{}\npreserved\n{}\n<--model>\n<two words>\n<$(touch forbidden)>\n<>\n<--json>\nchild stdin\n",
        root.join(".claude-work").canonicalize().unwrap().display(),
        root.canonicalize().unwrap().display()
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(!root.join("forbidden").exists());
}

#[test]
fn agent_launch_preserves_failures_and_signal_termination_without_extra_output() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    for (agent, bin_env) in [("claude", "JIG_CLAUDE_BIN"), ("codex", "JIG_CODEX_BIN")] {
        fs::create_dir(root.join(format!(".{agent}-work"))).unwrap();
        for (name, termination) in [("exit", "exit 37"), ("signal", "kill -TERM $$")] {
            let stub = executable(
                root.join(format!("{agent}-{name}")),
                &format!(
                    "#!/bin/sh\nprintf child-stdout\nprintf child-stderr >&2\n{termination}\n"
                ),
            );
            let output = jig(root)
                .args([agent, "launch", "work"])
                .env(bin_env, stub)
                .output()
                .unwrap();
            if name == "exit" {
                assert_eq!(output.status.code(), Some(37));
            } else {
                assert_eq!(output.status.signal(), Some(libc::SIGTERM));
            }
            assert_eq!(output.stdout, b"child-stdout");
            assert_eq!(output.stderr, b"child-stderr");
        }
    }
}

#[test]
fn agent_spawn_failures_keep_provider_diagnostics() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    for (agent, bin_env, message) in [
        (
            "claude",
            "JIG_CLAUDE_BIN",
            "install claude or set JIG_CLAUDE_BIN",
        ),
        ("codex", "JIG_CODEX_BIN", "with CODEX_HOME="),
    ] {
        fs::create_dir(root.join(format!(".{agent}-work"))).unwrap();
        let output = jig(root)
            .args([agent, "launch", "work"])
            .env(bin_env, root.join("missing-executable"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("Failed to launch"), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn invalid_or_noninteractive_launch_fails_before_starting_claude() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join(".claude")).unwrap();
    let marker = root.join("launched");
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\ntouch \"$JIG_TEST_MARKER\"\n",
    );
    for args in [
        vec!["claude", "launch"],
        vec!["claude", "launch", "default", "--json"],
        vec!["claude", "launch", "--dry-run", "--json"],
        vec!["claude", "launch", "missing", "--dry-run", "--json"],
        vec!["claude", "launch", "", "--dry-run", "--json"],
    ] {
        let output = jig(root)
            .args(&args)
            .env("JIG_CLAUDE_BIN", &stub)
            .env("JIG_TEST_MARKER", &marker)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        if args.contains(&"--json") {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
        }
    }
    let dry_run = report(
        jig(root)
            .args([
                "claude",
                "launch",
                "default",
                "--dry-run",
                "--json",
                "--",
                "--resume",
                "two words",
            ])
            .env("JIG_CLAUDE_BIN", &stub)
            .env("JIG_TEST_MARKER", &marker)
            .output()
            .unwrap(),
    );
    assert_eq!(
        dry_run["args"],
        serde_json::json!(["--resume", "two words"])
    );
    assert!(!marker.exists());
}

#[test]
fn non_utf8_paths_and_arguments_are_preserved_and_reports_mark_lossiness() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let name = OsString::from_vec(b".claude-\xff".to_vec());
    let home = root.join(name);
    // macOS filesystems can reject non-UTF-8 directory names; argv still supports them.
    let home = match fs::create_dir(&home) {
        Ok(()) => home,
        Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
            let fallback = root.join(".claude-work");
            fs::create_dir(&fallback).unwrap();
            fallback
        }
        Err(error) => panic!("create native-path fixture: {error}"),
    };
    let value = report(
        jig(root)
            .args(["claude", "homes", "--json"])
            .env("CLAUDE_CONFIG_DIR", &home)
            .output()
            .unwrap(),
    );
    assert_eq!(value["representation_lossy"], home.to_str().is_none());
    let arg = OsString::from_vec(b"arg-\xfe".to_vec());
    let value = report(
        jig(root)
            .args(["claude", "launch"])
            .arg(&home)
            .args(["--dry-run", "--json", "--"])
            .arg(&arg)
            .output()
            .unwrap(),
    );
    assert_eq!(value["representation_lossy"], true);
    let preview = jig(root)
        .args(["claude", "launch"])
        .arg(&home)
        .args(["--dry-run", "--"])
        .arg(&arg)
        .output()
        .unwrap();
    assert!(preview.status.success());
    assert!(
        String::from_utf8(preview.stdout)
            .unwrap()
            .contains("non-UTF-8 values; display is lossy")
    );
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\nprintf '%s\\0%s' \"$CLAUDE_CONFIG_DIR\" \"$1\"\n",
    );
    let output = jig(root)
        .args(["claude", "launch"])
        .arg(&home)
        .arg("--")
        .arg(&arg)
        .env("JIG_CLAUDE_BIN", stub)
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut expected = home.canonicalize().unwrap().into_os_string().into_vec();
    expected.push(0);
    expected.extend(arg.into_vec());
    assert_eq!(output.stdout, expected);
}

#[test]
fn human_dry_run_warns_when_terminal_sanitization_changes_values() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    for (bin, arg, warning) in [
        ("claude", "two words", false),
        (
            "claude",
            "$EXAMPLE_VAR $(example-command) `example-command`",
            false,
        ),
        ("claude", "before\x1b[31mafter", true),
        ("claude", "before\u{202e}after", true),
        ("claude\x1b[31m", "ordinary", true),
    ] {
        let output = jig(root)
            .args(["claude", "launch", "default", "--dry-run", "--", arg])
            .env("JIG_CLAUDE_BIN", bin)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.contains("terminal controls were replaced"), warning);
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains("unset (Claude default configuration)"));
        assert!(text.contains("Arguments (display only; not shell syntax):"));
        assert!(!text.contains("display is lossy"));
        let json = report(
            jig(root)
                .args([
                    "claude",
                    "launch",
                    "default",
                    "--dry-run",
                    "--json",
                    "--",
                    arg,
                ])
                .env("JIG_CLAUDE_BIN", bin)
                .output()
                .unwrap(),
        );
        assert_eq!(json["args"][0], arg);
        assert_eq!(json["claude_bin"], bin);
    }
    let home = root.join(".claude-\u{202e}work");
    fs::create_dir(&home).unwrap();
    let output = jig(root)
        .args(["claude", "launch"])
        .arg(home)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("terminal controls were replaced"));
    assert!(!text.contains('\u{202e}'));
    assert!(!root.join(".claude").exists());
}

#[test]
fn repository_launcher_preserves_invocation_directory_and_bypasses_repo_policy() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let repo = root.join("ExampleProject");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::write(repo.join(".jig.toml"), "invalid configuration").unwrap();
    fs::create_dir(root.join(".claude-work")).unwrap();
    let launcher = executable(
        repo.join("scripts/jig"),
        include_str!("../../../scripts/jig"),
    );
    executable(
        repo.join("scripts/install-jig.sh"),
        "#!/bin/sh\nprintf '%s\\n' \"$JIG_DEV_BIN\"\n",
    );
    let output = Command::new(launcher)
        .current_dir(root)
        .env("HOME", root)
        .env("JIG_DEV_BIN", env!("CARGO_BIN_EXE_jig"))
        .args(["--json", "claude", "launch", "./.claude-work", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(
        report(output)["home"],
        root.join(".claude-work")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
}

#[test]
fn native_default_unsets_override_while_an_explicit_default_path_sets_it() {
    let temp = support::tempdir().unwrap();
    let root = temp.path();
    let home = root.join(".claude");
    let stub = executable(
        root.join("claude-stub"),
        "#!/bin/sh\nprintf '%s' \"${CLAUDE_CONFIG_DIR-unset}\"\n",
    );
    for alias in ["default", "claude"] {
        let output = jig(root)
            .args(["claude", "launch", alias])
            .env("CLAUDE_CONFIG_DIR", "/unused/ambient")
            .env("JIG_CLAUDE_BIN", &stub)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"unset");
        let value = report(
            jig(root)
                .args(["claude", "launch", alias, "--dry-run", "--json"])
                .output()
                .unwrap(),
        );
        assert!(value["config_dir"].is_null());
        assert!(!home.exists());
    }
    let missing = jig(root)
        .args(["claude", "launch"])
        .arg(&home)
        .env("JIG_CLAUDE_BIN", &stub)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
    fs::create_dir(&home).unwrap();
    let output = jig(root)
        .args(["claude", "launch"])
        .arg(&home)
        .env("JIG_CLAUDE_BIN", &stub)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        home.canonicalize().unwrap().to_str().unwrap()
    );
    let value = report(
        jig(root)
            .args(["claude", "homes", "--json"])
            .env("CLAUDE_CONFIG_DIR", &home)
            .output()
            .unwrap(),
    );
    let homes = value["homes"].as_array().unwrap();
    assert_eq!(homes.len(), 2);
    assert_eq!(homes[0]["default_config"], true);
    assert_eq!(homes[0]["current"], false);
    assert_eq!(homes[1]["default_config"], false);
    assert_eq!(homes[1]["current"], true);
}

#[test]
fn terminal_picker_searches_launches_exact_modes_and_restores_terminal() {
    for (input, expected, explicit_default) in [
        (b"j\r".as_slice(), Some(".claude-work"), false),
        (b"/work\r".as_slice(), Some(".claude-work"), false),
        (b"\r".as_slice(), Some("unset"), false),
        (b"q".as_slice(), None, false),
        (b"\x1b".as_slice(), None, false),
        (b"\x03".as_slice(), None, false),
        (b"".as_slice(), None, false),
        (b"kk\r".as_slice(), Some("unset"), true),
        (b"\r".as_slice(), Some(".claude"), true),
    ] {
        let temp = support::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join(".claude-work")).unwrap();
        if explicit_default {
            fs::create_dir(root.join(".claude")).unwrap();
        }
        let marker = root.join("launched");
        let stub = executable(
            root.join("claude-stub"),
            "#!/bin/sh\nprintf '%s' \"${CLAUDE_CONFIG_DIR-unset}\" > \"$JIG_TEST_MARKER\"\n",
        );
        let (mut master, slave) = pseudo_terminal();
        let observer = slave.try_clone().unwrap();
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: observer owns a live terminal descriptor and original is writable.
        assert_eq!(
            unsafe { libc::tcgetattr(observer.as_raw_fd(), original.as_mut_ptr()) },
            0
        );
        // SAFETY: tcgetattr initialized original successfully.
        let original = unsafe { original.assume_init() };
        let mut command = jig(root);
        command
            .args(["claude", "launch"])
            .env("JIG_CLAUDE_BIN", stub)
            .env("JIG_TEST_MARKER", &marker)
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave);
        if explicit_default {
            command.env("CLAUDE_CONFIG_DIR", root.join(".claude"));
        }
        let mut child = pty_support::ChildGuard::new(command.spawn().unwrap());
        let mut output = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&output).contains("Claude Home Picker") {
            pty_support::read_available(&mut master, &mut output);
            assert!(
                Instant::now() < deadline,
                "{}",
                String::from_utf8_lossy(&output)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if input.is_empty() {
            // SAFETY: the guarded child is still alive and owned by this test.
            assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        } else {
            master.write_all(input).unwrap();
        }
        let status = pty_support::wait_for_child_while_draining(
            &mut child,
            &mut master,
            &mut output,
            Duration::from_secs(5),
        )
        .expect("picker exited");
        if input.is_empty() {
            assert_eq!(status.signal(), Some(libc::SIGTERM));
        } else {
            assert!(status.success(), "{}", String::from_utf8_lossy(&output));
        }
        let mut restored = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: observer remains open after the child exits.
        assert_eq!(
            unsafe { libc::tcgetattr(observer.as_raw_fd(), restored.as_mut_ptr()) },
            0
        );
        // SAFETY: tcgetattr initialized restored successfully.
        let restored = unsafe { restored.assume_init() };
        // Ignore kernel-maintained pending-input flags; verify the interactive modes.
        let modes = libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN;
        assert_eq!(restored.c_lflag & modes, original.c_lflag & modes);
        assert!(
            output
                .windows(b"\x1b[?1049l".len())
                .any(|window| window == b"\x1b[?1049l")
        );
        if let Some(expected) = expected {
            assert_eq!(
                fs::read_to_string(&marker).unwrap(),
                if expected == "unset" {
                    "unset".to_owned()
                } else {
                    root.join(expected)
                        .canonicalize()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                }
            );
        } else {
            assert!(!marker.exists());
        }
        assert_eq!(root.join(".claude").exists(), explicit_default);
    }
}

fn pseudo_terminal() -> (File, File) {
    let mut master = -1;
    let mut slave = -1;
    let size = libc::winsize {
        ws_row: 30,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty initializes both owned descriptors; no optional settings are supplied.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "PTY required: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: openpty returned two distinct, valid owned file descriptors.
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // SAFETY: slave owns a terminal descriptor and size is a valid winsize.
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
    // SAFETY: the live master descriptor supports fcntl; preserve its existing flags.
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    (master, slave)
}
