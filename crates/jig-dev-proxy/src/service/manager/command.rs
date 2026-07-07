#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::time::Duration;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use anyhow::Result;
use serde_json::{Value, json};

const SERVICE_MANAGER_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const SERVICE_MANAGER_KILL_GRACE_TIMEOUT: Duration = Duration::from_secs(1);
pub(super) const SERVICE_STATUS_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(target_os = "macos", target_os = "linux"))]
static COMMAND_OUTPUT_CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn command_status_json(program: &str, args: &[String]) -> Value {
    command_status_json_with_timeout(program, args, SERVICE_MANAGER_COMMAND_TIMEOUT)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_status_json_with_timeout(program: &str, args: &[String], timeout: Duration) -> Value {
    let mut command = match service_command(program) {
        Ok(command) => command,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    command.args(args);
    command_status_from_command_with_timeout(command, timeout)
}

#[cfg(target_os = "macos")]
pub(super) fn command_output_json(program: &str, args: &[String]) -> Value {
    command_output_json_with_timeout(program, args, SERVICE_MANAGER_COMMAND_TIMEOUT)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn command_output_json_with_timeout(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Value {
    let mut command = match service_command(program) {
        Ok(command) => command,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    command.args(args);
    command_output_from_command_with_timeout(command, timeout)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::service) fn command_status_from_command_with_timeout(
    command: Command,
    timeout: Duration,
) -> Value {
    command_output_from_command_with_timeout(command, timeout)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(in crate::service) fn command_output_from_command_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Value {
    let (capture, stdout, stderr) = match CommandOutputCapture::new() {
        Ok(capture) => capture,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    command.stdout(stdout).stderr(stderr);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return child_output_json(status, capture.read_current(), false),
            Ok(None) => {}
            Err(error) => {
                return json!({
                    "ok": false,
                    "error": error.to_string(),
                });
            }
        }
        if Instant::now() >= deadline {
            let kill_error = child.kill().err().map(|error| error.to_string());
            let wait_after_kill =
                wait_for_child_exit(&mut child, SERVICE_MANAGER_KILL_GRACE_TIMEOUT);
            let mut value = command_timeout_json(wait_after_kill, capture.read_current());
            if let Some(error) = kill_error {
                value["kill_error"] = json!(error);
            }
            return value;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_for_child_exit(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_timeout_json(
    wait_after_kill: std::io::Result<Option<std::process::ExitStatus>>,
    output: io::Result<CapturedCommandOutput>,
) -> Value {
    let (stdout, stderr, output_error) = output_fields(output);
    match wait_after_kill {
        Ok(Some(status)) => {
            let mut value = json!({
                "ok": false,
                "status": status.code(),
                "timed_out": true,
                "stdout": stdout,
                "stderr": stderr,
            });
            if let Some(error) = output_error {
                value["output_error"] = json!(error);
            }
            value
        }
        Ok(None) => {
            let mut value = json!({
                "ok": false,
                "timed_out": true,
                "still_running_after_kill": true,
                "stdout": stdout,
                "stderr": stderr,
            });
            if let Some(error) = output_error {
                value["output_error"] = json!(error);
            }
            value
        }
        Err(error) => {
            let mut value = json!({
                "ok": false,
                "timed_out": true,
                "error": error.to_string(),
                "stdout": stdout,
                "stderr": stderr,
            });
            if let Some(error) = output_error {
                value["output_error"] = json!(error);
            }
            value
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn child_output_json(
    status: std::process::ExitStatus,
    output: io::Result<CapturedCommandOutput>,
    timed_out: bool,
) -> Value {
    let (stdout, stderr, output_error) = output_fields(output);
    if let Some(error) = output_error {
        return json!({
            "ok": false,
            "timed_out": timed_out,
            "error": error,
        });
    }
    json!({
        "ok": status.success() && !timed_out,
        "status": status.code(),
        "timed_out": timed_out,
        "stdout": stdout,
        "stderr": stderr,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn output_fields(output: io::Result<CapturedCommandOutput>) -> (String, String, Option<String>) {
    match output {
        Ok(output) => (
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
            None,
        ),
        Err(error) => (String::new(), String::new(), Some(error.to_string())),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct CommandOutputCapture {
    dir: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct CapturedCommandOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl CommandOutputCapture {
    fn new() -> io::Result<(Self, Stdio, Stdio)> {
        let dir = create_command_output_capture_dir()?;
        let stdout_path = dir.join("stdout");
        let stderr_path = dir.join("stderr");
        let result = (|| {
            let stdout = open_command_output_capture_file(&stdout_path)?;
            let stderr = open_command_output_capture_file(&stderr_path)?;
            Ok((
                Self {
                    dir: dir.clone(),
                    stdout_path,
                    stderr_path,
                },
                Stdio::from(stdout),
                Stdio::from(stderr),
            ))
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&dir);
        }
        result
    }

    fn read_current(&self) -> io::Result<CapturedCommandOutput> {
        Ok(CapturedCommandOutput {
            stdout: fs::read(&self.stdout_path)?,
            stderr: fs::read(&self.stderr_path)?,
        })
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for CommandOutputCapture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn create_command_output_capture_dir() -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..100 {
        let sequence = COMMAND_OUTPUT_CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!(
            "jig-proxy-command-output-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate temporary command output directory",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn open_command_output_capture_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn service_command(program: &str) -> Result<Command> {
    let path = service_tool_path(program)?;
    let mut command = Command::new(path);
    command.env_clear();
    preserve_service_command_env(&mut command);
    Ok(command)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn preserve_service_command_env(command: &mut Command) {
    for key in [
        "HOME",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

#[cfg(target_os = "macos")]
fn service_tool_path(program: &str) -> Result<PathBuf> {
    match program {
        "launchctl" => Ok(PathBuf::from("/bin/launchctl")),
        other => anyhow::bail!("Unsupported Jig proxy service manager command: {other}"),
    }
}

#[cfg(target_os = "linux")]
fn service_tool_path(program: &str) -> Result<PathBuf> {
    match program {
        "systemctl" => fixed_system_tool_path("systemctl"),
        other => anyhow::bail!("Unsupported Jig proxy service manager command: {other}"),
    }
}

#[cfg(target_os = "linux")]
fn fixed_system_tool_path(program: &str) -> Result<PathBuf> {
    let candidates = fixed_system_tool_candidates(program);
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if executable_regular_file(&path) {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "Could not find {program} at fixed system tool paths: {}",
        candidates.join(", ")
    )
}

#[cfg(target_os = "linux")]
fn fixed_system_tool_candidates(program: &str) -> &'static [&'static str] {
    match program {
        "systemctl" => &["/usr/bin/systemctl", "/bin/systemctl"],
        _ => &[],
    }
}

#[cfg(target_os = "linux")]
fn executable_regular_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub(super) fn command_completed_without_error_or_timeout(output: &Value) -> bool {
    output.get("error").is_none() && output["timed_out"].as_bool() != Some(true)
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub(super) fn command_succeeded(output: &Value) -> bool {
    command_completed_without_error_or_timeout(output) && output["ok"].as_bool() == Some(true)
}
