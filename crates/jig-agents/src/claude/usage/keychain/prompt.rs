use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jig_owned_process::interaction::run_owned_process_tree_with_cooperative_interaction;
use zeroize::Zeroizing;

pub(super) fn read(
    service: &str,
    account: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut command = Command::new("/usr/bin/security");
    command
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    run_owned_process_tree_with_cooperative_interaction(
        &mut command,
        Duration::from_secs(30),
        |stdin, mut stdout, deadline| {
            drop(stdin);
            let mut bytes = Zeroizing::new(Vec::new());
            let mut buffer = Zeroizing::new([0u8; 4096]);
            loop {
                if cancelled() || deadline.is_none_or(|deadline| Instant::now() >= deadline) {
                    return Err("Claude Keychain access cancelled or timed out".into());
                }
                match stdout.read(&mut *buffer) {
                    Ok(0) => {
                        return if bytes.is_empty() {
                            Err("Claude Keychain access was denied or unavailable".into())
                        } else {
                            Ok(bytes)
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
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return Err("Could not read Claude Keychain credentials".into()),
                }
            }
        },
    )
    .map_err(|_| "Claude Keychain access was denied, cancelled, or timed out".into())
}
