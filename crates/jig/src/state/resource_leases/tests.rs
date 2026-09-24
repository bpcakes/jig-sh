use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use wait_timeout::ChildExt;

use super::*;

const FIXTURE: &str = "JIG_RESOURCE_LEASE_TEST_FIXTURE";
const SLOT: &str = "JIG_RESOURCE_LEASE_TEST_SLOT";
const ROLE: &str = "JIG_RESOURCE_LEASE_TEST_ROLE";
const KEY: &str = "JIG_RESOURCE_LEASE_TEST_KEY";
const MODE: &str = "JIG_RESOURCE_LEASE_TEST_MODE";
const BOUND: Duration = Duration::from_secs(15);

#[test]
fn lease_process_helper() {
    let Some(root) = std::env::var_os(FIXTURE) else {
        return;
    };
    let root = Path::new(&root);
    let slot = std::env::var(SLOT).unwrap();
    let role = std::env::var(ROLE).unwrap();
    let key = std::env::var(KEY).unwrap();
    let mode = if std::env::var(MODE).unwrap() == "shared" {
        ResourceClaimMode::Shared
    } else {
        ResourceClaimMode::Exclusive
    };
    if role == "inherited" {
        marker(root, &slot, "entered", "ready");
        await_marker(root, &slot, "release");
        return;
    }
    let lease = ResourceLease::try_acquire(&[claim(key.clone(), mode)]).unwrap();
    marker(
        root,
        &slot,
        "outcome",
        if lease.is_some() { "acquired" } else { "busy" },
    );
    let Some(lease) = lease else { return };
    let mut inherited = if role == "crash-owner" {
        let child_slot = format!("{slot}-child");
        let mut command = helper_command(root, &child_slot, "inherited", &key, mode);
        lease.inherit_into(&mut command).unwrap();
        let child = command.spawn().unwrap();
        drop(command);
        await_marker(root, &child_slot, "entered");
        Some((child, child_slot))
    } else {
        None
    };
    marker(root, &slot, "entered", "ready");
    await_marker(root, &slot, "release");
    if let Some((child, child_slot)) = &mut inherited {
        marker(root, child_slot, "release", "release");
        assert!(child.wait_timeout(BOUND).unwrap().unwrap().success());
    }
    drop(lease);
}

#[test]
fn claims_are_sorted_deduplicated_and_exclusive_wins() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let claims = [
        claim(b.clone(), ResourceClaimMode::Shared),
        claim(a.clone(), ResourceClaimMode::Exclusive),
        claim(a.clone(), ResourceClaimMode::Shared),
    ];
    assert_eq!(
        normalized_claims(&claims)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec![
            (a.as_str(), ResourceClaimMode::Exclusive),
            (b.as_str(), ResourceClaimMode::Shared)
        ]
    );
    for invalid in [
        "../private".into(),
        "A".repeat(64),
        "f".repeat(65),
        "".into(),
    ] {
        let error = normalized_claims(&[claim(invalid.clone(), ResourceClaimMode::Shared)])
            .unwrap_err()
            .to_string();
        if !invalid.is_empty() {
            assert!(!error.contains(&invalid));
        }
    }
    assert!(normalized_claims(&vec![claim(a, ResourceClaimMode::Shared); 65]).is_err());
}

#[test]
fn independent_processes_cannot_overlap_exclusive_claims() {
    let fixture = Fixture::new();
    let key = fixture.key("exclusive");
    let mut owner = fixture.spawn("owner", "holder", &key, ResourceClaimMode::Exclusive);
    fixture.wait("owner", "entered");
    assert!(!fixture.0.path().join("ignored-temporary-root").exists());
    let mut contender = fixture.spawn("contender", "holder", &key, ResourceClaimMode::Exclusive);
    assert_eq!(fixture.wait("contender", "outcome"), "busy");
    contender.finish();
    fixture.release("owner");
    owner.finish();
    let mut next = fixture.spawn("next", "holder", &key, ResourceClaimMode::Exclusive);
    assert_eq!(fixture.wait("next", "outcome"), "acquired");
    fixture.release("next");
    next.finish();
}

#[test]
fn repository_shared_guards_overlap_but_exclude_fallback() {
    let fixture = Fixture::new();
    let key = fixture.key("repository");
    let mut first = fixture.spawn("first", "holder", &key, ResourceClaimMode::Shared);
    fixture.wait("first", "entered");
    let mut second = fixture.spawn("second", "holder", &key, ResourceClaimMode::Shared);
    fixture.wait("second", "entered");
    assert!(
        ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_none()
    );
    fixture.release("first");
    first.finish();
    assert!(
        ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_none()
    );
    fixture.release("second");
    second.finish();
    let fallback = ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
        .unwrap()
        .unwrap();
    let mut blocked = fixture.spawn("blocked", "holder", &key, ResourceClaimMode::Shared);
    assert_eq!(fixture.wait("blocked", "outcome"), "busy");
    blocked.finish();
    drop(fallback);
}

#[test]
fn a_failed_multi_claim_attempt_releases_every_partial_claim() {
    let fixture = Fixture::new();
    let mut keys = [fixture.key("partial-a"), fixture.key("partial-b")];
    keys.sort();
    let held = ResourceLease::try_acquire(&[claim(keys[1].clone(), ResourceClaimMode::Exclusive)])
        .unwrap()
        .unwrap();
    assert!(
        ResourceLease::try_acquire(&[
            claim(keys[1].clone(), ResourceClaimMode::Exclusive),
            claim(keys[0].clone(), ResourceClaimMode::Exclusive),
        ])
        .unwrap()
        .is_none()
    );
    let mut probe = fixture.spawn(
        "partial-probe",
        "holder",
        &keys[0],
        ResourceClaimMode::Exclusive,
    );
    assert_eq!(fixture.wait("partial-probe", "outcome"), "acquired");
    fixture.release("partial-probe");
    probe.finish();
    drop(held);
}

#[test]
fn killed_owner_keeps_its_claim_until_inherited_child_exits() {
    let fixture = Fixture::new();
    let key = fixture.key("crash");
    let mut owner = fixture.spawn("owner", "crash-owner", &key, ResourceClaimMode::Exclusive);
    fixture.wait("owner", "entered");
    owner.child.kill().unwrap();
    assert!(!owner.child.wait_timeout(BOUND).unwrap().unwrap().success());
    assert!(
        ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_none()
    );
    fixture.release("owner-child");
    let deadline = Instant::now() + BOUND;
    loop {
        if ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_some()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "inherited child did not release its claim"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn unrelated_exec_does_not_inherit_parent_claim() {
    const ISOLATED: &str = "JIG_RESOURCE_LEASE_UNRELATED_EXEC_HELPER";
    if std::env::var_os(ISOLATED).is_none() {
        // Concurrent tests can fork while we hold the lease, retaining its FD
        // until they exec. Keep the lease owner in a process running only this test.
        let fixture = Fixture::new();
        let mut isolated = OwnedHelper {
            child: Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "state::resource_leases::tests::unrelated_exec_does_not_inherit_parent_claim",
                    "--nocapture",
                ])
                .env(ISOLATED, "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
            root: fixture.0.path().to_path_buf(),
            slot: "isolated".into(),
        };
        isolated.finish();
        return;
    }

    let fixture = Fixture::new();
    let key = fixture.key("unrelated");
    let lease = ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
        .unwrap()
        .unwrap();
    let mut unrelated = fixture.spawn("unrelated", "inherited", &key, ResourceClaimMode::Exclusive);
    fixture.wait("unrelated", "entered");
    drop(lease);
    let next = ResourceLease::try_acquire(&[claim(key, ResourceClaimMode::Exclusive)])
        .unwrap()
        .expect("unrelated exec retained the parent's claim");
    assert!(unrelated.child.try_wait().unwrap().is_none());
    fixture.release("unrelated");
    unrelated.finish();
    drop(next);
}

#[test]
fn dropping_owner_does_not_unlock_a_live_inherited_child() {
    let fixture = Fixture::new();
    let key = fixture.key("close-only");
    let lease = ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
        .unwrap()
        .unwrap();
    let mut command = helper_command(
        fixture.0.path(),
        "child",
        "inherited",
        &key,
        ResourceClaimMode::Exclusive,
    );
    lease.inherit_into(&mut command).unwrap();
    // The command must retain the exact FD even if the original owner drops
    // before spawn. Its child then independently keeps the close-only claim.
    drop(lease);
    let mut child = OwnedHelper {
        child: command.spawn().unwrap(),
        root: fixture.0.path().to_path_buf(),
        slot: "child".into(),
    };
    drop(command);
    fixture.wait("child", "entered");
    assert!(
        ResourceLease::try_acquire(&[claim(key.clone(), ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_none()
    );
    fixture.release("child");
    child.finish();
    assert!(
        ResourceLease::try_acquire(&[claim(key, ResourceClaimMode::Exclusive)])
            .unwrap()
            .is_some()
    );
}

fn claim(opaque_key: String, mode: ResourceClaimMode) -> ResourceClaim {
    ResourceClaim { opaque_key, mode }
}

struct Fixture(TempDir);

impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn key(&self, label: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(self.0.path().as_os_str().as_encoded_bytes());
        hash.update(label.as_bytes());
        format!("{:x}", hash.finalize())
    }

    fn spawn(&self, slot: &str, role: &str, key: &str, mode: ResourceClaimMode) -> OwnedHelper {
        OwnedHelper {
            child: helper_command(self.0.path(), slot, role, key, mode)
                .spawn()
                .unwrap(),
            root: self.0.path().to_path_buf(),
            slot: slot.into(),
        }
    }

    fn wait(&self, slot: &str, event: &str) -> String {
        await_marker(self.0.path(), slot, event)
    }
    fn release(&self, slot: &str) {
        marker(self.0.path(), slot, "release", "release");
    }
}

struct OwnedHelper {
    child: Child,
    root: std::path::PathBuf,
    slot: String,
}

impl OwnedHelper {
    fn finish(&mut self) {
        assert!(self.child.wait_timeout(BOUND).unwrap().unwrap().success());
    }
}

impl Drop for OwnedHelper {
    fn drop(&mut self) {
        marker(&self.root, &self.slot, "release", "release");
        marker(
            &self.root,
            &format!("{}-child", self.slot),
            "release",
            "release",
        );
        if self.child.wait_timeout(BOUND).ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn helper_command(
    root: &Path,
    slot: &str,
    role: &str,
    key: &str,
    mode: ResourceClaimMode,
) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "state::resource_leases::tests::lease_process_helper",
            "--nocapture",
        ])
        .env(FIXTURE, root)
        .env(SLOT, slot)
        .env(ROLE, role)
        .env(KEY, key)
        .env("TMPDIR", root.join("ignored-temporary-root"))
        .env(
            MODE,
            if mode == ResourceClaimMode::Shared {
                "shared"
            } else {
                "exclusive"
            },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn marker(root: &Path, slot: &str, event: &str, value: &str) {
    let path = root.join(format!("{slot}.{event}"));
    let temporary = path.with_extension(format!("{event}.pending"));
    fs::write(&temporary, value).unwrap();
    fs::rename(temporary, path).unwrap();
}

fn await_marker(root: &Path, slot: &str, event: &str) -> String {
    let deadline = Instant::now() + BOUND;
    loop {
        match fs::read_to_string(root.join(format!("{slot}.{event}"))) {
            Ok(value) => return value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("could not inspect helper barrier: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "helper barrier was not reached: {slot}.{event}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
