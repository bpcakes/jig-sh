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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn symlinked_ancestors_and_bodies_never_escape_the_repository() {
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
    assert_eq!(fs::read_to_string(&outside_body).unwrap(), "unchanged");
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
fn body_read_wait_is_cancellable() {
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

    assert_eq!(
        read_plan_body(&ctx, "plan_read_lock", &|| false)
            .unwrap()
            .text,
        "before"
    );
}
