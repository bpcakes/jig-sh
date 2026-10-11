#![cfg(unix)]

mod support;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

fn jig(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", root)
        .current_dir(root)
        .stdin(Stdio::null());
    command
}

fn codex_fixture(root: &Path) -> Command {
    for name in [".codex", ".codex-example"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let stub = root.join("codex-stub");
    fs::write(&stub, r#"#!/bin/sh
if [ "${1:-}" = app-server ]; then
  read -r initialize
  printf '%s\n' '{"id":0,"result":{}}'
  read -r initialized
  read -r account
  read -r usage
  printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt","planType":"pro"}}}'
  case "$CODEX_HOME" in
    */.codex-example) short=45; weekly=40; banked=${JIG_TEST_EXAMPLE_BANKED:-1} ;;
    *) short=10; weekly=90; banked=3 ;;
  esac
  if [ "${JIG_TEST_TIED_HEADROOM:-}" = 1 ]; then short=20; weekly=20; fi
  if [ "${JIG_TEST_EXHAUSTED:-}" = 1 ]; then weekly=100; fi
  now=$(date +%s)
  short_duration=300; weekly_duration=10080
  short_reset=$((now+9000)); weekly_reset=$((now+300000))
  if [ "${JIG_TEST_NULL_TIMING:-}" = 1 ]; then
    short_duration=null; weekly_duration=null; short_reset=null; weekly_reset=null
  fi
  printf '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":%s,"windowDurationMins":%s,"resetsAt":%s},"secondary":{"usedPercent":%s,"windowDurationMins":%s,"resetsAt":%s}},"rateLimitResetCredits":{"availableCount":%s,"credits":[]}}}\n' "$short" "$short_duration" "$short_reset" "$weekly" "$weekly_duration" "$weekly_reset" "$banked"
  sleep 30
  exit 0
fi
printf '%s\n' "$CODEX_HOME"
pwd -P
printf '<%s>\n' "$@"
cat
printf child-stderr >&2
exit 37
"#).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = jig(root);
    command.env("JIG_CODEX_BIN", stub);
    command
}

#[test]
fn codex_usage_reports_each_accounts_banked_resets() {
    let root = support::tempdir().unwrap();
    let output = codex_fixture(root.path())
        .args(["codex", "homes", "--usage", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let homes = report["homes"].as_array().unwrap();
    assert_eq!(homes.len(), 2);
    for home in homes {
        let expected = if home["home"].as_str().unwrap().ends_with(".codex-example") {
            1
        } else {
            3
        };
        assert_eq!(home["banked_resets"], expected, "{home}");
    }
}

#[test]
fn codex_auto_breaks_headroom_ties_with_banked_resets() {
    let root = support::tempdir().unwrap();
    let mut command = codex_fixture(root.path());
    command
        .env("JIG_TEST_TIED_HEADROOM", "1")
        .env("JIG_TEST_EXAMPLE_BANKED", "5");
    let output = command
        .args(["codex", "launch", "--auto", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["home"],
        root.path()
            .join(".codex-example")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    let decision = &report["selection"];
    let index = decision["selected_index"].as_u64().unwrap() as usize;
    assert_eq!(decision["candidates"][index]["banked_resets"], 5);
    assert_eq!(
        decision["candidates"][index]["assessment"]["remaining_percent"],
        80.0
    );
    let output = jig(root.path())
        .env("JIG_CODEX_BIN", root.path().join("codex-stub"))
        .env("JIG_TEST_TIED_HEADROOM", "1")
        .env("JIG_TEST_EXAMPLE_BANKED", "5")
        .args(["codex", "launch", "--auto", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("banked resets: 5")
    );
}

#[test]
fn auto_dry_run_selects_by_limiting_window_and_explains_the_decision() {
    let root = support::tempdir().unwrap();
    let output = codex_fixture(root.path())
        .args([
            "codex",
            "launch",
            "--auto",
            "--dry-run",
            "--json",
            "--",
            "exec",
            "--json",
            "Example review",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["home"],
        root.path()
            .join(".codex-example")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        report["args"],
        serde_json::json!(["exec", "--json", "Example review"])
    );
    let decision = &report["selection"];
    assert_eq!(decision["strategy"], "headroom");
    assert_eq!(decision["partial"], false);
    let index = decision["selected_index"].as_u64().unwrap() as usize;
    assert_eq!(
        decision["candidates"][index]["assessment"]["remaining_percent"],
        55.0
    );
}

#[test]
fn codex_auto_selects_known_usage_with_nullable_protocol_timing() {
    let root = support::tempdir().unwrap();
    let output = codex_fixture(root.path())
        .env("JIG_TEST_NULL_TIMING", "1")
        .args(["codex", "launch", "--auto", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["home"],
        root.path()
            .join(".codex-example")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    let decision = &report["selection"];
    assert_eq!(decision["partial"], false);
    let index = decision["selected_index"].as_u64().unwrap() as usize;
    let assessment = &decision["candidates"][index]["assessment"];
    assert_eq!(assessment["remaining_percent"], 55.0);
    assert!(assessment["limiting_window"]["duration_minutes"].is_null());
    assert!(assessment["limiting_window"]["resets_at"].is_null());
}

#[test]
fn auto_launch_preserves_stdin_argv_cwd_streams_and_child_exit_status() {
    let root = support::tempdir().unwrap();
    let mut child = codex_fixture(root.path())
        .args([
            "codex",
            "launch",
            "--auto",
            "--",
            "exec",
            "--json",
            "two words",
            "",
            "$(touch forbidden)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"example stdin\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(37));
    assert_eq!(output.stderr, b"child-stderr");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}\n{}\n<exec>\n<--json>\n<two words>\n<>\n<$(touch forbidden)>\nexample stdin\n",
            root.path()
                .join(".codex-example")
                .canonicalize()
                .unwrap()
                .display(),
            root.path().canonicalize().unwrap().display(),
        )
    );
    assert!(!root.path().join("forbidden").exists());
}

#[test]
fn exhausted_homes_return_one_structured_failure_without_launching() {
    let root = support::tempdir().unwrap();
    let output = codex_fixture(root.path())
        .env("JIG_TEST_EXHAUSTED", "1")
        .args(["codex", "launch", "--auto", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["error"]["kind"], "no_eligible_home");
    assert!(report["selection"]["selected_index"].is_null());
    assert!(
        report["selection"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["assessment"]["reason"] == "exhausted")
    );
}

#[test]
fn auto_conflicts_and_real_launch_json_fail_before_inspection_for_both_providers() {
    let root = support::tempdir().unwrap();
    for provider in ["codex", "claude"] {
        for args in [
            vec![provider, "launch", "default", "--auto"],
            vec![provider, "launch", "--auto", "--json"],
        ] {
            let output = jig(root.path()).args(args).output().unwrap();
            assert!(!output.status.success());
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                text.contains("cannot be used with") || text.contains("only when --dry-run"),
                "{text}"
            );
        }
    }
}

#[test]
fn ambient_credentials_never_get_ranked_as_per_home_subscriptions() {
    let root = support::tempdir().unwrap();
    let output = codex_fixture(root.path())
        .env("CODEX_API_KEY", "example-token")
        .args(["codex", "launch", "--auto", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.to_string().contains("CODEX_API_KEY"));
    assert!(!report.to_string().contains("example-token"));

    // Avoid accessing even the developer's system Keychain on macOS.
    let output = jig(root.path())
        .env("CLAUDE_CODE_OAUTH_TOKEN", "example-token")
        .args(["claude", "launch", "--auto", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"]["kind"], "no_eligible_home");
    assert_eq!(
        report["selection"]["candidates"][0]["assessment"]["reason"],
        "inspection_error"
    );
    assert!(!report.to_string().contains("example-token"));
}
