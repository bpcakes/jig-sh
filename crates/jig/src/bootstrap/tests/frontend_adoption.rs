use super::*;

#[cfg(unix)]
fn wait_for_positive_pid_file(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> std::result::Result<u32, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let observation = match fs::read_to_string(path) {
            Ok(contents) => match contents.trim().parse::<u32>() {
                Ok(pid) if pid > 0 => return Ok(pid),
                Ok(pid) => format!("invalid non-positive PID {pid}"),
                Err(error) => format!("unparseable contents {contents:?}: {error}"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                "file does not exist yet".to_owned()
            }
            Err(error) => format!("could not read file: {error}"),
        };
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for a parseable PID in {} ({observation})",
                path.display()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

mod adoption_validation;
mod configuration;
#[cfg(unix)]
mod dependency_receipts;
#[cfg(unix)]
mod dependency_state;
#[cfg(unix)]
mod install_locking;
#[cfg(unix)]
mod pnpm;
mod workflows;
#[cfg(unix)]
mod yarn;
