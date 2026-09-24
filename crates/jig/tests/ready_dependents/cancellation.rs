use super::{
    fixture::{Fixture, jig},
    support::*,
};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn cancelled_resource_wave_skips_fallback_source_scan_and_releases_claims() {
    assert_cancellation_releases_claims(false);
}

#[test]
fn cancellation_interrupts_stalled_fallback_source_scan_and_releases_claims() {
    assert_cancellation_releases_claims(true);
}

#[test]
fn cancellation_interrupts_stalled_ordinary_source_scan_and_releases_claims() {
    let fixture = resource_fixture();
    let mut command = blocked_git_command(&fixture);
    let mut run = fixture.spawn_command(&fixture.root, "example-ordinary-scan", &mut command);
    run.wait_named_entry("prerequisite");
    run.wait_named_entry("slow");
    let other = fixture.other_repository("example-resource-waiter", 30, false);
    let mut waiter = fixture.spawn_in(&other, "waiter", &[]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered(), "resource claim must still be held");

    let _unblock = UnblockOnDrop(fixture.signals.join("release-source-scan"));
    signal(&fixture, "block-source-scan");
    release(&fixture, "prerequisite");
    run.wait_signal("entered-source-scan");
    assert!(!fixture.signals.join("entered-dependent").exists());
    run.interrupt();
    let cancelled_at = Instant::now();
    while run.running() {
        assert!(
            cancelled_at.elapsed() < Duration::from_secs(10),
            "cancelled run retained its ordinary source scan/resource claim: {}",
            run.output()
        );
        thread::sleep(Duration::from_millis(20));
    }
    run.finish_failure();
    assert!(!fixture.signals.join("release-source-scan").exists());
    assert!(!fixture.signals.join("entered-dependent").exists());
    for receipt in records(&fixture, "receipts.jsonl") {
        assert_eq!(receipt["target_freshness"]["state"], "incomplete");
    }
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
}

#[test]
fn publication_error_closes_resource_arrivals_and_terminalizes_run() {
    // The resource worker has finished one root, but stays alive for the
    // resource-backed dependent of the ordinary prerequisite.
    let fixture = resource_dependent_fixture();
    let mut run = start(&fixture, &[]);
    release(&fixture, "slow");
    run.wait_target_publication("slow");
    assert!(!fixture.signals.join("entered-dependent").exists());

    let journal = UnavailableReceiptJournal::new(&fixture);
    release(&fixture, "prerequisite");
    let failed_at = Instant::now();
    while run.running() {
        assert!(
            failed_at.elapsed() < Duration::from_secs(10),
            "receipt publication error left the resource worker waiting for arrivals"
        );
        thread::sleep(Duration::from_millis(20));
    }
    run.finish_failure();
    drop(journal);
    run.assert_single_completion("blocked");
    assert!(!fixture.signals.join("entered-dependent").exists());
    assert_eq!(records(&fixture, "receipts.jsonl").len(), 1);
}

fn assert_cancellation_releases_claims(during_scan: bool) {
    // All targets own the same resource, with a dependent to select the ready
    // scheduler. Only the prerequisite can enter the first resource wave.
    let fixture = all_resource_fixture(if during_scan { 8 } else { 30 });
    let mut command = blocked_git_command(&fixture);
    let mut run = fixture.spawn_command(&fixture.root, "example-cancel-scan", &mut command);
    run.wait_named_entry("prerequisite");
    let other = fixture.other_repository("example-resource-waiter", 30, false);
    let mut waiter = fixture.spawn_in(&other, "waiter", &[]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered(), "resource claim must still be held");

    // Unblock before Running's cleanup on assertion failure, including when
    // this regression is run against the old non-cancellable collector.
    let _unblock = UnblockOnDrop(fixture.signals.join("release-source-scan"));
    signal(&fixture, "block-source-scan");
    if during_scan {
        // The prerequisite expires; its budget-limited observation fails
        // before Git starts. This marker therefore belongs to the retry.
        run.wait_signal("entered-source-scan");
        assert!(!waiter.entered());
    }
    run.interrupt();
    let cancelled_at = Instant::now();
    while run.running() {
        assert!(
            cancelled_at.elapsed() < Duration::from_secs(10),
            "cancelled run retained its source scan/resource claim: {}",
            run.output()
        );
        thread::sleep(Duration::from_millis(20));
    }
    run.finish_failure();
    assert_eq!(
        fixture.signals.join("entered-source-scan").exists(),
        during_scan,
        "an already-cancelled run must not start the fallback"
    );
    assert!(!fixture.signals.join("release-source-scan").exists());
    assert!(!fixture.signals.join("entered-dependent").exists());
    let events = records(&fixture, "runs.jsonl");
    let completions = events
        .iter()
        .filter(|event| event["event"] == "target_completed")
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 3);
    assert!(
        completions
            .iter()
            .all(|event| event["result"]["conclusion"] != "success")
    );
    for receipt in records(&fixture, "receipts.jsonl") {
        assert_eq!(receipt["target_freshness"]["state"], "incomplete");
    }
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
}

fn blocked_git_command(fixture: &Fixture) -> Command {
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let bin = fixture.signals.join("bin");
    fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = ls-tree ] && [ -f "$EXAMPLE_SOURCE_SIGNALS/block-source-scan" ]; then
  touch "$EXAMPLE_SOURCE_SIGNALS/entered-source-scan"
  while [ ! -f "$EXAMPLE_SOURCE_SIGNALS/release-source-scan" ]; do sleep 0.02; done
fi
exec "$EXAMPLE_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let path = env::join_paths(
        std::iter::once(bin).chain(env::split_paths(&env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = jig(&fixture.root);
    command
        .args(["check", "--profile", "verify"])
        .env("PATH", path)
        .env("EXAMPLE_SOURCE_SIGNALS", &fixture.signals)
        .env(
            "EXAMPLE_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        );
    command
}

struct UnblockOnDrop(PathBuf);

impl Drop for UnblockOnDrop {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "release\n");
    }
}

struct UnavailableReceiptJournal {
    path: PathBuf,
    backup: PathBuf,
}

impl UnavailableReceiptJournal {
    fn new(fixture: &Fixture) -> Self {
        let path = fixture.root.join(".agent/state/receipts.jsonl");
        let backup = fixture.signals.join("receipts-backup.jsonl");
        fs::rename(&path, &backup).unwrap();
        fs::create_dir(&path).unwrap();
        Self { path, backup }
    }
}

impl Drop for UnavailableReceiptJournal {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
        let _ = fs::rename(&self.backup, &self.path);
    }
}
