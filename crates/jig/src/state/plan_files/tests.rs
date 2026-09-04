use std::cell::Cell;
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

fn context(root: &Path) -> RepoContext {
    TestRepoBuilder::new(root).write();
    RepoContext::load_from(root).unwrap()
}

#[test]
fn canonical_plan_ids_accept_documented_shapes_and_reject_path_syntax() {
    for valid in [
        "plan_01M1PYF12DJ19XYES8WWFW4Y3P",
        "plan-example",
        "ExamplePlan_42",
        &"a".repeat(128),
    ] {
        validate_plan_id(valid).unwrap();
    }
    for invalid in [
        "",
        ".",
        "..",
        "../plan",
        "plan/body",
        "plan\\body",
        "/absolute",
        "plan\0body",
        "plán",
        &"a".repeat(129),
    ] {
        let error = validate_plan_id(invalid).unwrap_err();
        assert_eq!(
            error.downcast_ref::<PlanFileError>().unwrap().kind(),
            PlanFileErrorKind::InvalidId
        );
    }
}

#[test]
fn safe_plan_store_creates_missing_directories_and_round_trips_append() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    fs::remove_dir_all(temp.path().join(".agent")).unwrap();

    let path = create_plan_body(&ctx, "plan_example", "# Example\n").unwrap();
    append_plan_body(&ctx, "plan_example", b"\nMore").unwrap();
    let body = read_plan_body(&ctx, "plan_example", &|| false).unwrap();

    assert_eq!(path, temp.path().join(".agent/plans/plan_example.md"));
    assert_eq!(body.text, "# Example\n\nMore");
    assert!(!body.truncated);
}

#[test]
fn create_refuses_to_replace_an_existing_body() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_existing", "original").unwrap();

    let error = create_plan_body(&ctx, "plan_existing", "replacement").unwrap_err();

    assert!(error.downcast_ref::<PlanFileError>().is_some());
    assert_eq!(
        fs::read_to_string(plan_body_path(&ctx, "plan_existing").unwrap()).unwrap(),
        "original"
    );
}

#[test]
fn append_fails_closed_when_the_original_body_is_missing() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_missing_body", "original").unwrap();
    let path = plan_body_path(&ctx, "plan_missing_body").unwrap();
    fs::remove_file(&path).unwrap();

    let error = append_plan_body(&ctx, "plan_missing_body", b"replacement fragment").unwrap_err();

    assert_eq!(
        error.downcast_ref::<PlanFileError>().unwrap().kind(),
        PlanFileErrorKind::NotFound
    );
    assert!(error.to_string().contains("restore the original body"));
    assert!(!path.exists());
}

#[test]
fn missing_plan_read_is_read_only() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    fs::remove_dir_all(temp.path().join(".agent")).unwrap();

    let error = read_plan_body(&ctx, "plan_missing", &|| false).unwrap_err();

    assert_eq!(
        error.downcast_ref::<PlanFileError>().unwrap().kind(),
        PlanFileErrorKind::NotFound
    );
    assert!(!temp.path().join(".agent").exists());
}

#[test]
fn bounded_body_reader_marks_truncation_and_rejects_invalid_prefix_utf8() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_large", &"é".repeat(40_003)).unwrap();
    let body = read_plan_body(&ctx, "plan_large", &|| false).unwrap();
    assert_eq!(body.text.chars().count(), PLAN_BODY_VISIBLE_CHARS);
    assert!(body.truncated);

    create_plan_body(&ctx, "plan_invalid", "valid").unwrap();
    fs::write(plan_body_path(&ctx, "plan_invalid").unwrap(), [b'a', 0xff]).unwrap();
    let error = read_plan_body(&ctx, "plan_invalid", &|| false).unwrap_err();
    assert_eq!(
        error.downcast_ref::<PlanFileError>().unwrap().kind(),
        PlanFileErrorKind::InvalidUtf8
    );
}

#[test]
fn bounded_body_reader_does_not_inspect_invalid_utf8_beyond_the_visible_prefix() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_invalid_suffix", "seed").unwrap();
    let mut bytes = vec![b'x'; PLAN_BODY_PREFIX_BYTES];
    bytes.push(0xff);
    fs::write(plan_body_path(&ctx, "plan_invalid_suffix").unwrap(), bytes).unwrap();

    let body = read_plan_body(&ctx, "plan_invalid_suffix", &|| false).unwrap();

    assert_eq!(body.text, "x".repeat(PLAN_BODY_VISIBLE_CHARS));
    assert!(body.truncated);
}

#[test]
fn body_reader_polls_cancellation_between_chunks() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_cancel", &"x".repeat(PLAN_BODY_INPUT_BYTES)).unwrap();
    let checks = Cell::new(0_usize);

    let error = read_plan_body(&ctx, "plan_cancel", &|| {
        let current = checks.get();
        checks.set(current + 1);
        current >= 5
    })
    .unwrap_err();

    assert!(crate::cancellation::is_status_collection_cancellation(
        &error
    ));
    assert!(checks.get() > 5);
}

#[test]
fn plan_reader_cancels_at_directory_and_body_open_boundaries_without_writes() {
    for cancel_after in 0..=5 {
        let temp = tempdir().unwrap();
        let ctx = context(temp.path());
        create_plan_body(&ctx, "plan_cancel_open", "body").unwrap();
        let before = fs::read(plan_body_path(&ctx, "plan_cancel_open").unwrap()).unwrap();
        let checks = Cell::new(0_usize);

        let error = read_plan_body(&ctx, "plan_cancel_open", &|| {
            let current = checks.get();
            checks.set(current + 1);
            current >= cancel_after
        })
        .unwrap_err();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert_eq!(
            fs::read(plan_body_path(&ctx, "plan_cancel_open").unwrap()).unwrap(),
            before
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn make_fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `path` is a live NUL-terminated pathname and the mode is valid.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn symlinked_ancestors_body_and_lock_never_escape_the_repository() {
    use std::os::unix::fs::symlink;

    for ancestor in [".agent", ".agent/plans"] {
        let temp = tempdir().unwrap();
        let ctx = context(temp.path());
        fs::remove_dir_all(temp.path().join(".agent")).unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        if ancestor == ".agent" {
            symlink(&outside, temp.path().join(".agent")).unwrap();
        } else {
            fs::create_dir(temp.path().join(".agent")).unwrap();
            symlink(&outside, temp.path().join(".agent/plans")).unwrap();
        }
        assert!(create_plan_body(&ctx, "plan_escape", "unsafe").is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
    }

    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    let outside_body = temp.path().join("outside-body");
    fs::write(&outside_body, "unchanged").unwrap();
    fs::create_dir_all(temp.path().join(".agent/plans")).unwrap();
    symlink(&outside_body, plan_body_path(&ctx, "plan_escape").unwrap()).unwrap();
    assert!(read_plan_body(&ctx, "plan_escape", &|| false).is_err());
    assert!(append_plan_body(&ctx, "plan_escape", b"changed").is_err());
    assert_eq!(fs::read_to_string(&outside_body).unwrap(), "unchanged");

    fs::remove_file(plan_body_path(&ctx, "plan_escape").unwrap()).unwrap();
    create_plan_body(&ctx, "plan_escape", "body").unwrap();
    let outside_lock = temp.path().join("outside-lock");
    fs::write(&outside_lock, "unchanged").unwrap();
    symlink(
        &outside_lock,
        temp.path().join(".agent/plans/plan_escape.md.lock"),
    )
    .unwrap();
    assert!(append_plan_body(&ctx, "plan_escape", b"changed").is_err());
    assert_eq!(fs::read_to_string(&outside_lock).unwrap(), "unchanged");
    assert_eq!(
        fs::read_to_string(plan_body_path(&ctx, "plan_escape").unwrap()).unwrap(),
        "body"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn fifo_and_device_targets_fail_without_a_peer() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    fs::create_dir_all(temp.path().join(".agent/plans")).unwrap();
    let body_path = plan_body_path(&ctx, "plan_fifo").unwrap();
    make_fifo(&body_path);
    assert!(read_plan_body(&ctx, "plan_fifo", &|| false).is_err());
    assert!(append_plan_body(&ctx, "plan_fifo", b"data").is_err());

    create_plan_body(&ctx, "plan_lock_fifo", "body").unwrap();
    make_fifo(&temp.path().join(".agent/plans/plan_lock_fifo.md.lock"));
    assert!(append_plan_body(&ctx, "plan_lock_fifo", b"data").is_err());

    let device = Dir::open_ambient_dir("/dev", ambient_authority()).unwrap();
    let mut options = regular_options(false, false, false);
    options.read(true);
    let error = open_regular(
        &device,
        OsStr::new("null"),
        &mut options,
        Path::new("/dev/null"),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<PlanFileError>().unwrap().kind(),
        PlanFileErrorKind::UnsafeType
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn append_waits_for_the_verified_sidecar_lock() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_locked", "body").unwrap();
    let lock_path = temp.path().join(".agent/plans/plan_locked.md.lock");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock_exclusive().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker_ctx = ctx.clone();
    let worker = thread::spawn(move || {
        let result = append_plan_body(&worker_ctx, "plan_locked", b"+append");
        tx.send(result).unwrap();
    });

    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    let body = File::open(plan_body_path(&ctx, "plan_locked").unwrap()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if !FileExt::try_lock_shared(&body).unwrap() {
            break;
        }
        FileExt::unlock(&body).unwrap();
        assert!(
            std::time::Instant::now() < deadline,
            "append never acquired the body lock before the sidecar"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let reader_ctx = ctx.clone();
    let (read_tx, read_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        read_tx
            .send(read_plan_body(&reader_ctx, "plan_locked", &|| false))
            .unwrap();
    });
    assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
    FileExt::unlock(&lock).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
    let body_after_append = read_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    reader.join().unwrap();
    assert_eq!(body_after_append.text, "body+append");
    assert_eq!(
        fs::read_to_string(plan_body_path(&ctx, "plan_locked").unwrap()).unwrap(),
        "body+append"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn body_read_wait_is_cancellable_and_never_returns_a_torn_append() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_read_lock", "before").unwrap();
    let path = plan_body_path(&ctx, "plan_read_lock").unwrap();
    let lock = File::open(&path).unwrap();
    lock.lock_exclusive().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let reader_ctx = ctx.clone();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        tx.send(read_plan_body(&reader_ctx, "plan_read_lock", &|| {
            reader_cancelled.load(Ordering::SeqCst)
        }))
        .unwrap();
    });

    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let error = rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_err();
    assert!(crate::cancellation::is_status_collection_cancellation(
        &error
    ));
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();

    append_plan_body(&ctx, "plan_read_lock", b"+after").unwrap();
    assert_eq!(
        read_plan_body(&ctx, "plan_read_lock", &|| false)
            .unwrap()
            .text,
        "before+after"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn body_read_wait_has_a_finite_deadline_without_cancellation() {
    let temp = tempdir().unwrap();
    let ctx = context(temp.path());
    create_plan_body(&ctx, "plan_read_timeout", "body").unwrap();
    let path = plan_body_path(&ctx, "plan_read_timeout").unwrap();
    let lock = File::open(&path).unwrap();
    lock.lock_exclusive().unwrap();

    let started = std::time::Instant::now();
    let error = read_plan_body(&ctx, "plan_read_timeout", &|| false).unwrap_err();
    let elapsed = started.elapsed();
    FileExt::unlock(&lock).unwrap();

    assert_eq!(
        error.downcast_ref::<PlanFileError>().unwrap().kind(),
        PlanFileErrorKind::Read
    );
    assert!(error.to_string().contains("Timed out"));
    assert!(elapsed >= PLAN_BODY_LOCK_WAIT_LIMIT);
    assert!(elapsed < Duration::from_secs(2));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn ancestor_replacement_between_create_and_open_fails_closed() {
    use std::os::unix::fs::symlink;

    for replace in [".agent", "plans"] {
        let temp = tempdir().unwrap();
        let ctx = context(temp.path());
        fs::remove_dir_all(temp.path().join(".agent")).unwrap();
        if replace == "plans" {
            fs::create_dir(temp.path().join(".agent")).unwrap();
        }
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let root = temp.path().to_path_buf();
        let error = open_plan_directory_with_hook(&root, true, &|| false, |created, name| {
            if name == OsStr::new(replace) {
                let displaced = created.with_extension("displaced");
                fs::rename(created, &displaced).unwrap();
                symlink(&outside, created).unwrap();
            }
        })
        .unwrap_err();
        assert!(error.downcast_ref::<PlanFileError>().is_some());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        drop(ctx);
    }
}
