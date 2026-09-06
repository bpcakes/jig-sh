use std::ffi::OsStr;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::*;

#[test]
fn concurrent_cold_lock_creation_serializes_every_caller() {
    const CALLER_COUNT: usize = 16;

    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let barrier = Arc::new(Barrier::new(CALLER_COUNT));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut callers = Vec::new();
    for _ in 0..CALLER_COUNT {
        let root = root.clone();
        let barrier = Arc::clone(&barrier);
        let active = Arc::clone(&active);
        callers.push(std::thread::spawn(move || {
            let directory = StateDirectory::open(&root, &root).unwrap();
            barrier.wait();
            directory
                .with_lock_until(
                    OsStr::new("attempts.lock"),
                    &root.join("attempts.lock"),
                    Instant::now() + Duration::from_secs(5),
                    &|| false,
                    || {
                        assert_eq!(active.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                        std::thread::sleep(Duration::from_millis(2));
                        assert_eq!(active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst), 1);
                        Ok(())
                    },
                )
                .unwrap();
        }));
    }
    for caller in callers {
        caller.join().unwrap();
    }
    assert!(root.join("attempts.lock").is_file());
}
