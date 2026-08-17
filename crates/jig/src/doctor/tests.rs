use super::*;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{
    TestProcessIdentity, assert_test_process_stopped, publish_test_process_identity,
    read_test_process_identity,
};
use serde_json::json;
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

include!("tests_parts/part_01.rs");
include!("tests_parts/part_02.rs");
include!("tests_parts/part_03.rs");
include!("tests_parts/part_04.rs");
include!("tests_parts/part_05.rs");
include!("tests_parts/part_06.rs");
include!("tests_parts/part_07.rs");
include!("tests_parts/part_08.rs");
include!("tests_parts/part_09.rs");
