use super::*;
use std::cell::Cell;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::tempdir;

const REDIRECT_HELPER_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_HELPER";
const REDIRECT_HELPER_ROOT_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_ROOT";
const REDIRECT_HELPER_WHOLE_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_WHOLE";
const REDIRECT_HELPER_SCOPE_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_SCOPE";
const REDIRECT_HELPER_TEST: &str = "git_receipts::tests::repository_redirect_environment_helper";
// Apple rejects invalid-byte path components with EILSEQ before these
// filesystem-backed fixtures can exercise Jig's path handling.
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_HELPER";
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ROOT_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_ROOT";
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_TEST: &str =
    "git_receipts::tests::canonical_diff_order_file_preserves_non_utf8_temporary_directory_helper";

include!("tests_parts/part_01.rs");
include!("tests_parts/part_02.rs");
include!("tests_parts/part_03.rs");
include!("tests_parts/part_04.rs");
