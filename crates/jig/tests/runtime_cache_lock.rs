#![cfg(unix)]

use std::fs::{self, File};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use fs4::fs_std::FileExt;
use tempfile::tempdir;
use wait_timeout::ChildExt;

#[test]
fn installer_waits_for_an_old_but_live_rust_lock_owner() {
    let temp = tempdir().unwrap();
    let install_root = temp.path().join("runtime");
    let lock_directory = temp.path().join("runtime.lock");
    let guard_path = temp.path().join("runtime.lock.guard");
    fs::create_dir(&lock_directory).unwrap();
    fs::write(lock_directory.join("owner-v1"), "owner-v1 1 preexisting\n").unwrap();
    fs::write(&guard_path, "owner-v1 1 preexisting\n").unwrap();
    File::options()
        .write(true)
        .open(&guard_path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
        )
        .unwrap();
    let lock = File::options()
        .read(true)
        .write(true)
        .open(&guard_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut installer = Command::new(repo.join("scripts/install-jig.sh"))
        .args(["--profile", "runtime", "--seed-dev-bin"])
        .arg(&install_root)
        .env("JIG_DEV_BIN", env!("CARGO_BIN_EXE_jig"))
        .env_remove("JIG_INSTALL_LOCK_TOKEN")
        .current_dir(&repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(500));
    if installer.try_wait().unwrap().is_some() {
        let output = installer.wait_with_output().unwrap();
        panic!(
            "installer bypassed the live lock: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_file(lock_directory.join("owner-v1")).unwrap();
    fs::remove_dir(&lock_directory).unwrap();
    FileExt::unlock(&lock).unwrap();
    let status = installer
        .wait_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap_or_else(|| {
            let _ = installer.kill();
            panic!("installer did not continue after the live owner released its lock")
        });
    assert!(status.success(), "installer exited with {status}");
    assert!(install_root.join("bin/jig").is_file());
}

#[test]
fn installer_restores_a_stale_owner_when_the_directory_cannot_be_removed() {
    let temp = tempdir().unwrap();
    let install_root = temp.path().join("runtime");
    let lock_directory = temp.path().join("runtime.lock");
    let owner = lock_directory.join("owner-v1");
    let owner_record = "owner-v1 1 stale-owner\n";
    fs::create_dir(&lock_directory).unwrap();
    fs::write(&owner, owner_record).unwrap();
    fs::write(lock_directory.join("unexpected"), "blocks rmdir\n").unwrap();

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(repo.join("scripts/install-jig.sh"))
        .args(["--profile", "runtime", "--seed-dev-bin"])
        .arg(&install_root)
        .env("JIG_DEV_BIN", env!("CARGO_BIN_EXE_jig"))
        .env_remove("JIG_INSTALL_LOCK_TOKEN")
        .current_dir(&repo)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(owner).unwrap(), owner_record);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Failed to recover stale Jig installer lock")
    );
}

#[test]
fn installer_rejects_a_directory_at_the_guard_path_without_retrying() {
    let temp = tempdir().unwrap();
    let install_root = temp.path().join("runtime");
    fs::create_dir(temp.path().join("runtime.lock.guard")).unwrap();

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(repo.join("scripts/install-jig.sh"))
        .args(["--profile", "runtime", "--seed-dev-bin"])
        .arg(&install_root)
        .env("JIG_DEV_BIN", env!("CARGO_BIN_EXE_jig"))
        .env_remove("JIG_INSTALL_LOCK_TOKEN")
        .current_dir(&repo)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("guard path is a directory instead of a file")
    );
}
