use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use jig_vault::SecretBytes;

// Keep one explicit buffer over an unbuffered descriptor. Mixing StdinLock's
// hidden read-ahead with descriptor readiness can strand already-read bytes.
#[cfg(unix)]
pub(super) fn read(output: &mut impl Write) -> Result<SecretBytes> {
    use std::os::fd::{AsFd, AsRawFd};
    let input = std::fs::File::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    let fd = input.as_raw_fd();
    super::read_text_input(
        &mut BufReader::with_capacity(4096, input),
        output,
        |timeout| {
            let deadline = Instant::now() + timeout;
            loop {
                let mut descriptor = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let remaining = deadline.saturating_duration_since(Instant::now());
                // SAFETY: descriptor is initialized and its fd remains owned by
                // the reader for this entire call; poll does not retain the pointer.
                let ready = unsafe {
                    libc::poll(
                        &mut descriptor,
                        1,
                        remaining.as_millis().min(i32::MAX as u128) as i32,
                    )
                };
                if ready >= 0 {
                    return Ok(ready > 0);
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error);
                }
                if Instant::now() >= deadline {
                    return Ok(false);
                }
            }
        },
    )
}

#[cfg(not(unix))]
pub(super) fn read(_output: &mut impl Write) -> Result<SecretBytes> {
    bail!("interactive text entry requires a supported Unix host; use --value-stdin");
}

pub(super) fn skip_escape(
    input: &mut BufReader<impl Read>,
    wait: &mut impl FnMut(Duration) -> std::io::Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(100);
    let Some(first) = peek(input, wait, deadline)? else {
        bail!("text field input cancelled; value was not saved");
    };
    input.consume(1);
    if matches!(first, b'[' | b'O') {
        for index in 0..64 {
            let Some(byte) = peek(input, wait, deadline)? else {
                // Alt+[ and Alt+O share these prefixes. Only bare Escape
                // cancels; a prefix timeout completes an ignored Alt key.
                return Ok(());
            };
            if first == b'[' && index == 0 && byte == b'[' {
                input.consume(1);
                // Linux console F1-F5 use ESC [[ A through ESC [[ E.
                if matches!(peek(input, wait, deadline)?, Some(b'A'..=b'E')) {
                    input.consume(1);
                }
                return Ok(());
            }
            match byte {
                0x40..=0x7e => {
                    input.consume(1);
                    return Ok(());
                }
                0x20..=0x3f => input.consume(1),
                // A control/editing byte or UTF-8 character cannot continue
                // CSI/SS3. Leave it buffered for the main input loop after
                // the ignored Alt prefix, including immediate submission.
                _ => return Ok(()),
            }
        }
        bail!("invalid terminal key sequence; value was not saved");
    }
    // Alt-modified characters are ESC followed by one complete UTF-8 scalar.
    let width = super::utf8_width(first)?;
    let mut bytes = [first; 4];
    for byte in &mut bytes[1..width] {
        let Some(next) = peek(input, wait, deadline)? else {
            bail!("incomplete terminal key sequence; value was not saved");
        };
        *byte = next;
        input.consume(1);
    }
    std::str::from_utf8(&bytes[..width])?;
    Ok(())
}

fn peek(
    input: &mut BufReader<impl Read>,
    wait: &mut impl FnMut(Duration) -> std::io::Result<bool>,
    deadline: Instant,
) -> Result<Option<u8>> {
    if input.buffer().is_empty() && !wait(deadline.saturating_duration_since(Instant::now()))? {
        return Ok(None);
    }
    let byte = input.fill_buf()?.first().copied();
    if matches!(byte, Some(3 | 4 | 27)) {
        bail!("text field input cancelled; value was not saved");
    }
    Ok(byte)
}
