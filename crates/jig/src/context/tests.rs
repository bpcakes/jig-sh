use super::*;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, lock_env};
use serde_json::json;
use tempfile::tempdir;

include!("tests/repo_loop_and_commands.rs");
include!("tests/gates_and_dev.rs");
include!("tests/frontend_and_work.rs");
include!("tests_parts/part_01.rs");

mod runtime;
