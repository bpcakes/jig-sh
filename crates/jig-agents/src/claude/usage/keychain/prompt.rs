use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jig_owned_process::OwnedProcessTreeError;
use jig_owned_process::interaction::{
    OwnedProcessTreeInteractionError,
    run_owned_process_tree_with_cooperative_interaction_and_status,
};
use zeroize::Zeroizing;

use super::ToolFailure;

/// `security` exits with the low byte of the Keychain status it hit.
/// errSecInteractionNotAllowed (-25308): this session can show no dialog, as
/// over SSH, so a Claude launch here fails the same way.
const EXIT_INTERACTION_NOT_ALLOWED: i32 = 36;
/// errSecItemNotFound (-25300): the item went away after Jig found it.
const EXIT_ITEM_NOT_FOUND: i32 = 44;

/// Reads the item through Apple's `security` tool. A tool still running at the
/// deadline is reported as [`ToolFailure::Waiting`]: it may be showing a
/// permission or unlock dialog that a Claude launch would wait on. Only a tool
/// that cannot start, or that reports this session cannot be asked or holds no
/// item, is [`ToolFailure::Unavailable`]. Any other exit without the item, such
/// as a Deny or a cancelled unlock dialog, answers only that one request, so it
/// is [`ToolFailure::Refused`].
pub(super) fn read(
    service: &str,
    account: &str,
    timeout: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Zeroizing<Vec<u8>>, ToolFailure> {
    let mut command = Command::new("/usr/bin/security");
    command
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let outcome = run_owned_process_tree_with_cooperative_interaction_and_status(
        &mut command,
        timeout,
        |stdin, mut stdout, deadline| {
            drop(stdin);
            let mut bytes = Zeroizing::new(Vec::new());
            let mut buffer = Zeroizing::new([0u8; 4096]);
            loop {
                if cancelled() {
                    return Err("Claude Keychain access cancelled".into());
                }
                match stdout.read(&mut *buffer) {
                    Ok(0) => {
                        return Ok(Ok((!bytes.is_empty()).then_some(bytes)));
                    }
                    Ok(count) => {
                        if bytes.len() + count > 64 * 1024 {
                            return Err(
                                "Claude Keychain credentials exceeded the size limit".into()
                            );
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        // Output or an exit that arrived before the deadline was
                        // taken above, so only a tool still running is waiting.
                        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            return Ok(Err(ToolFailure::Waiting));
                        }
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return Err("Could not read Claude Keychain credentials".into()),
                }
            }
        },
    );
    match outcome {
        Ok((Ok(Some(bytes)), _)) => Ok(bytes),
        // stdout reached end of file, so the tool had exited with this status.
        Ok((Ok(None), status)) => Err(ended_without_item(status.code())),
        Ok((Err(failure), _)) => Err(failure),
        // A Claude launch could not run the tool either and would use the file.
        Err(OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Start(_))) => {
            Err(ToolFailure::Unavailable)
        }
        // Oversized or unreadable output is an item a launch would still
        // read, and a failed cleanup may leave the tool's dialog open.
        Err(_) => Err(ToolFailure::Unreadable),
    }
}

/// Classifies a tool that exited without the item by its exit code; a signal
/// or an unknown status is a refusal, which keeps the Keychain error.
pub(super) fn ended_without_item(code: Option<i32>) -> ToolFailure {
    match code {
        Some(EXIT_INTERACTION_NOT_ALLOWED | EXIT_ITEM_NOT_FOUND) => ToolFailure::Unavailable,
        _ => ToolFailure::Refused,
    }
}
