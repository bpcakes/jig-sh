#![cfg(unix)]

#[path = "cargo_resources/disjoint_wave.rs"]
mod disjoint_wave;
#[path = "cargo_resources/support.rs"]
mod fixture;
#[path = "cargo_resources/mixed_layer.rs"]
mod mixed_layer;
#[path = "cargo_resources/wave_fixture.rs"]
mod wave_fixture;

use fixture::*;
use std::fs;

#[test]
fn declared_shared_artifacts_serialize_independent_jig_processes() {
    let fixture = Fixture::new(true, 60);
    let mut owner = fixture.spawn("owner", &[]);
    owner.wait_entered();
    let mut waiter = fixture.spawn("waiter", &[]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered(), "waiter started before admission");
    owner.release();
    owner.finish_success();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    assert!(!fixture.signals.join("overlap").exists());
    assert_eq!(fixture.launches(), "owner\nwaiter\n");
}

#[test]
fn canceled_resource_waiter_never_starts_or_releases_owner_claim() {
    let fixture = Fixture::new(true, 60);
    let mut owner = fixture.spawn("owner", &[]);
    owner.wait_entered();
    let mut canceled = fixture.spawn("canceled", &[]);
    canceled.wait_resource_notice();
    canceled.cancel();
    canceled.finish_failure();
    assert!(!canceled.entered());
    assert!(owner.running());
    let mut next = fixture.spawn("next", &[]);
    next.wait_resource_notice();
    assert!(!next.entered());
    owner.release();
    owner.finish_success();
    next.wait_entered();
    next.release();
    next.finish_success();
    assert_eq!(fixture.launches(), "owner\nnext\n");
    assert!(!fixture.signals.join("overlap").exists());
}

#[test]
fn resource_wait_timeout_never_starts_child_or_disrupts_other_repository_owner() {
    let fixture = Fixture::new(true, 60);
    let mut owner = fixture.spawn("owner", &[]);
    owner.wait_entered();
    let other = fixture.other_repository("example-timeout", 8, false);
    let mut waiter = fixture.spawn_in(&other, "timeout", &[]);
    waiter.wait_resource_notice();
    waiter.finish_failure();
    assert!(!waiter.entered());
    assert!(owner.running());
    waiter.assert_single_completion("timed_out");
    owner.release();
    owner.finish_success();
    assert_eq!(fixture.launches(), "owner\n");
    assert!(!fixture.signals.join("overlap").exists());
}

#[test]
fn source_edit_during_resource_wait_prevents_child_start() {
    let fixture = Fixture::new(true, 60);
    let mut owner = fixture.spawn("owner", &[]);
    owner.wait_entered();
    let other = fixture.other_repository("example-source", 60, false);
    let mut waiter = fixture.spawn_in(&other, "changed", &[]);
    waiter.wait_resource_notice();
    fs::write(other.join("src/lib.rs"), "pub fn changed_example() {}\n").unwrap();
    owner.release();
    owner.finish_success();
    waiter.finish_failure();
    assert!(!waiter.entered());
    assert_eq!(fixture.launches(), "owner\n");
    waiter.assert_single_completion("blocked");
}

#[test]
fn cross_repository_symlink_artifact_aliases_collide_without_receipt_sharing() {
    let fixture = Fixture::new(true, 60);
    let other = fixture.other_repository("example-alias", 60, true);
    let mut owner = fixture.spawn("owner", &[]);
    owner.wait_entered();
    let mut waiter = fixture.spawn_in(&other, "alias", &[]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered());
    owner.release();
    owner.finish_success();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    assert_eq!(fixture.launches(), "owner\nalias\n");
    assert!(!fixture.signals.join("overlap").exists());
    for root in [&fixture.root, &other] {
        let receipts = fs::read_to_string(root.join(".agent/state/receipts.jsonl")).unwrap();
        assert_eq!(
            receipts
                .lines()
                .filter(|line| {
                    serde_json::from_str::<serde_json::Value>(line).unwrap()["target"].is_object()
                })
                .count(),
            1,
            "each repository must retain only its own execution receipt"
        );
    }
}

#[test]
fn ordinary_checks_still_enter_concurrently() {
    let fixture = Fixture::new(false, 60);
    let mut first = fixture.spawn("first", &[]);
    first.wait_entered();
    let mut second = fixture.spawn("second", &[]);
    second.wait_entered();
    assert!(first.running(), "first check must still hold its barrier");
    assert!(second.running());
    first.release();
    second.release();
    first.finish_success();
    second.finish_success();
    assert_eq!(fixture.launches(), "first\nsecond\n");
}
