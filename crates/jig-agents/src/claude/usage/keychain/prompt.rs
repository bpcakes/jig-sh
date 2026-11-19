use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jig_owned_process::OwnedProcessTreeError;
use jig_owned_process::interaction::{
    OwnedProcessTreeInteractionError,
    run_owned_process_tree_with_cooperative_interaction_and_status,
};
use zeroize::Zeroizing;

use super::{ReadError, ReadResult};

// The security CLI returns the low byte of the OSStatus.
const EXIT_ITEM_NOT_FOUND: i32 = 44; // errSecItemNotFound (-25300)
const EXIT_INTERACTION_NOT_ALLOWED: i32 = 36; // errSecInteractionNotAllowed (-25308)
const EXIT_AUTH_FAILED: i32 = 51; // errSecAuthFailed (-25293)
const EXIT_USER_CANCELED: i32 = 128; // errSecUserCanceled (-128)

pub(super) fn read(
    service: &str,
    account: &str,
    timeout: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> ReadResult {
    let mut command = Command::new("/usr/bin/security");
    command.args(["find-generic-password", "-s", service, "-a", account, "-w"]);
    run(&mut command, timeout, cancelled)
}

fn run(
    command: &mut Command,
    timeout: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> ReadResult {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (bytes, status) = run_owned_process_tree_with_cooperative_interaction_and_status(
        command,
        timeout,
        cancelled,
        |stdin, stdout, deadline| {
            drop(stdin);
            let mut bytes = Zeroizing::new(Vec::new());
            let mut buffer = Zeroizing::new([0u8; 4096]);
            loop {
                if cancelled() {
                    return Err(OwnedProcessTreeError::Cancelled);
                }
                match stdout.read(&mut *buffer) {
                    Ok(0) => return Ok(bytes),
                    Ok(count) => {
                        if bytes.len() + count > 64 * 1024 {
                            return Err(OwnedProcessTreeError::OutputLimitExceeded(
                                jig_owned_process::OwnedProcessOutputStream::Stdout,
                            ));
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            return Err(OwnedProcessTreeError::TimedOut);
                        }
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {
                        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            return Err(OwnedProcessTreeError::TimedOut);
                        }
                    }
                    Err(_) => return Err(OwnedProcessTreeError::Await),
                }
            }
        },
    )
    .map_err(process_error)?;
    match status.code() {
        Some(0) if !bytes.is_empty() => Ok(Some(bytes)),
        Some(EXIT_ITEM_NOT_FOUND) => Ok(None),
        Some(EXIT_INTERACTION_NOT_ALLOWED) => Err(ReadError::InteractionNotAllowed),
        Some(EXIT_AUTH_FAILED | EXIT_USER_CANCELED) => Err(ReadError::Refused),
        _ => Err(ReadError::Unreadable),
    }
}

fn process_error(error: OwnedProcessTreeInteractionError) -> ReadError {
    use OwnedProcessTreeInteractionError::Process;
    match error {
        Process(OwnedProcessTreeError::Start(error)) if error.kind() == ErrorKind::NotFound => {
            ReadError::ToolMissing
        }
        Process(OwnedProcessTreeError::TimedOut) => ReadError::TimedOut,
        Process(OwnedProcessTreeError::Cancelled | OwnedProcessTreeError::CancelledBeforeStart) => {
            ReadError::Cancelled
        }
        _ => ReadError::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::usage::credentials::{parse, prefer_keychain};

    fn command(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    const TIMEOUT: Duration = Duration::from_secs(3);

    #[test]
    fn command_status_determines_the_credential_source_and_diagnostic() {
        let file = || parse(br#"{"claudeAiOauth":{"accessToken":"example-file"}}"#);
        let choose = |read, file: &dyn Fn() -> _| prefer_keychain(read, &|| false, 0, file);
        let unused = || panic!("unexpected credential file read");
        let read = |script: &str| run(&mut command(script), TIMEOUT, &|| false);

        let missing = || read("exec 1>&-; sleep 0.05; exit 44");
        assert_eq!(
            choose(missing(), &file).unwrap().access_token.as_str(),
            "example-file"
        );
        assert_eq!(
            choose(missing(), &|| parse(b"?")).err().unwrap(),
            "Claude credentials could not be decoded"
        );
        let expired =
            || parse(br#"{"claudeAiOauth":{"accessToken":"example-expired","expiresAt":0}}"#);
        assert!(
            choose(missing(), &expired)
                .unwrap()
                .usage_error(0)
                .unwrap()
                .contains("expired")
        );
        assert_eq!(
            choose(read("exit 36"), &file)
                .unwrap()
                .access_token
                .as_str(),
            "example-file"
        );
        let locked = choose(read("exit 36"), &|| parse(b"?")).err().unwrap();
        assert!(locked.contains("security unlock-keychain") && locked.contains("Always Allow"));

        let data = r#"printf '%s' '{"claudeAiOauth":{"accessToken":"example-keychain"}}'"#;
        assert_eq!(
            choose(read(data), &unused).unwrap().access_token.as_str(),
            "example-keychain"
        );
        for script in [
            "exit 0",
            "exit 1",
            "exit 93",
            "head -c 65537 /dev/zero",
            "kill -TERM $$",
            &format!("{data}; exit 1"),
        ] {
            assert_eq!(
                choose(read(script), &unused).err().unwrap(),
                ReadError::Unreadable.message()
            );
        }
        for script in ["exit 51", "exit 128"] {
            let message = choose(read(script), &unused).err().unwrap();
            assert!(message.contains("denied or cancelled"), "{message}");
        }
    }

    #[test]
    fn only_a_missing_executable_permits_file_fallback_after_start_failure() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing-security");
        let result = run(&mut Command::new(missing), TIMEOUT, &|| false);
        assert!(matches!(result, Err(ReadError::ToolMissing)));
        let file = || parse(br#"{"claudeAiOauth":{"accessToken":"example-file"}}"#);
        assert_eq!(
            prefer_keychain(Err(ReadError::ToolMissing), &|| false, 0, file)
                .unwrap()
                .access_token
                .as_str(),
            "example-file"
        );
        let error = prefer_keychain(result, &|| false, 0, || parse(b"?"))
            .err()
            .unwrap();
        assert!(error.contains("missing /usr/bin/security") && !error.contains("unlock"));
        for errno in [libc::EMFILE, libc::EAGAIN, libc::EINTR, libc::EPERM] {
            let error = process_error(OwnedProcessTreeInteractionError::Process(
                OwnedProcessTreeError::Start(std::io::Error::from_raw_os_error(errno)),
            ));
            assert!(matches!(error, ReadError::Unreadable));
            assert!(
                prefer_keychain(Err(error), &|| false, 0, || panic!(
                    "transient failure read the file"
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn closed_stdout_does_not_end_the_deadline_or_cancellation_checks() {
        let stalled = || command("exec 1>&-; exec sleep 10");
        let timeout = run(&mut stalled(), Duration::from_millis(100), &|| false);
        assert!(matches!(timeout, Err(ReadError::TimedOut)));
        let start = Instant::now();
        let cancelled = run(&mut stalled(), TIMEOUT, &|| {
            start.elapsed() >= Duration::from_millis(100)
        });
        assert!(matches!(cancelled, Err(ReadError::Cancelled)));
        assert!(start.elapsed() < TIMEOUT);
    }
}
