use std::io;

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn tempdir() -> io::Result<TempDir> {
    #[cfg(unix)]
    {
        use std::sync::Once;

        static TEST_UMASK: Once = Once::new();
        TEST_UMASK.call_once(|| {
            // SAFETY: this is test-only process setup; the test process exits
            // after the suite, so the private umask does not escape to callers.
            unsafe { libc::umask(0o077) };
        });

        return tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir();
    }

    #[cfg(not(unix))]
    {
        tempfile::tempdir()
    }
}
