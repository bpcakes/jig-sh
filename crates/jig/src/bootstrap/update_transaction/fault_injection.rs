use std::cell::RefCell;

thread_local! {
    static FAILURE_POINT: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) fn with_failure<T>(point: &str, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<String>);

    impl Drop for Reset {
        fn drop(&mut self) {
            FAILURE_POINT.with_borrow_mut(|current| *current = self.0.take());
        }
    }

    let previous = FAILURE_POINT.with_borrow_mut(|current| current.replace(point.to_owned()));
    let _reset = Reset(previous);
    operation()
}

pub(super) fn matches(point: &str) -> bool {
    FAILURE_POINT.with_borrow(|current| current.as_deref() == Some(point))
}
