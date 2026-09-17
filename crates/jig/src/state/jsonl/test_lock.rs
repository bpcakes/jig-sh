use std::cell::Cell;

thread_local! {
    static UNSUPPORTED: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn unsupported() -> bool {
    UNSUPPORTED.get()
}

/// Exercise public read paths without changing process-global state or writers.
pub(in crate::state) fn with_unsupported_scan_lock<T>(read: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            UNSUPPORTED.set(self.0);
        }
    }
    let _restore = Restore(UNSUPPORTED.replace(true));
    read()
}
