use super::process::*;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
use super::process_unix::*;
use super::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const PIPE_ESCAPE_MODE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MODE";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PIPE_ESCAPE_MARKER_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_MARKER";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PIPE_ESCAPE_RELEASE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_RELEASE";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PIPE_ESCAPE_DONE_VAR: &str = "JIG_VAULT_PIPE_ESCAPE_DONE";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PIPE_ESCAPE_HELPER_TEST: &str = "run::tests::execution::brokered_run_pipe_escape_helper";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn test_env_mapping(var: &str, secret_name: &str, value: &[u8]) -> ResolvedBrokeredEnv {
    ResolvedBrokeredEnv {
        var: EnvVarName::parse(var).unwrap(),
        secret_name: SecretName::parse(secret_name).unwrap(),
        value: SecretBytes::new(value.to_vec()),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_path(path: &std::path::Path, timeout: Duration) {
    let deadline = Instant::now().checked_add(timeout).unwrap();
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for test marker {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn assert_path_stays_absent(path: &std::path::Path, duration: Duration) {
    let deadline = Instant::now().checked_add(duration).unwrap();
    while Instant::now() < deadline {
        assert!(
            !path.exists(),
            "terminated descendant wrote unexpected marker {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

mod execution;
mod process;
