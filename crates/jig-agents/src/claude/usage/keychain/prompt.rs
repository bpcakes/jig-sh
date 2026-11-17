use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jig_owned_process::OwnedProcessTreeError;
use jig_owned_process::interaction::{
    OwnedProcessTreeInteractionError, run_owned_process_tree_with_cooperative_interaction,
};
use zeroize::Zeroizing;

use super::ToolFailure;

/// Reads the item through Apple's `security` tool. A tool still running at the
/// deadline is reported as [`ToolFailure::Waiting`]: it may be showing a
/// permission or unlock dialog that a Claude launch would wait on. Only a tool
/// that ends without the item, or cannot start, is [`ToolFailure::Failed`].
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
    run_owned_process_tree_with_cooperative_interaction(
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
                        return if bytes.is_empty() {
                            Ok(Err(ToolFailure::Failed))
                        } else {
                            Ok(Ok(bytes))
                        };
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
    )
    .unwrap_or_else(|error| {
        Err(match error {
            // A Claude launch could not run the tool either and would use the file.
            OwnedProcessTreeInteractionError::Process(OwnedProcessTreeError::Start(_)) => {
                ToolFailure::Failed
            }
            // Oversized or unreadable output is an item a launch would still
            // read, and a failed cleanup may leave the tool's dialog open.
            _ => ToolFailure::Unreadable,
        })
    })
}
