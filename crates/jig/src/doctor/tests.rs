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

const CURRENT_GENERATED_LAUNCHER_TEMPLATE: &str =
    include_str!("../bootstrap/embedded_template_snapshots/scripts/jig.jinja");
const CURRENT_GENERATED_INSTALLER: &str =
    include_str!("../bootstrap/embedded_template_snapshots/scripts/install-jig.sh.jinja");

fn current_generated_launcher() -> String {
    CURRENT_GENERATED_LAUNCHER_TEMPLATE.replace(
        "<<[ _jig.contract_version ]>>",
        &crate::context::CURRENT_CONTRACT_VERSION.to_string(),
    )
}

#[cfg(unix)]
#[test]
fn go_module_requires_a_supervised_doctor_process_session() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("backend_language = \"go\"")
        .write();
    fs::write(
        temp.path().join("go.mod"),
        "module example.com/doctor-fixture\n\ngo 1.24\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert!(go_runtime_probe_required(&ctx));
}

include!("tests_parts/part_01.rs");
include!("tests_parts/part_02.rs");
include!("tests_parts/part_03.rs");
include!("tests_parts/part_04.rs");
include!("tests_parts/part_05.rs");
include!("tests_parts/part_06.rs");
include!("tests_parts/part_07.rs");
include!("tests_parts/part_08.rs");
include!("tests_parts/part_09.rs");

mod root;
mod runtime;
