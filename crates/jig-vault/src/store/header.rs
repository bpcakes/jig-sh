//! Unauthenticated public-header discovery for read-only status reporting.

use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde::Deserialize;

use crate::format::MAGIC;

use super::{VAULT_FILE, VAULT_TEXT_READ_LIMIT};

#[derive(Deserialize)]
struct PublicEnvelopeProbe {
    header: PublicHeaderProbe,
}

#[derive(Deserialize)]
struct PublicHeaderProbe {
    magic: String,
    version: u32,
}

#[derive(Deserialize)]
struct PublicIdProbe {
    header: PublicIdHeader,
}

#[derive(Deserialize)]
struct PublicIdHeader {
    vault_id: String,
}

/// Reads the unauthenticated vault ID from a home's public header. Like the
/// version probe it never blocks, creates, or fails; malformed files report
/// `None`.
pub(super) fn public_vault_id(root: &Path) -> Option<String> {
    let text = read_regular_text(&root.join(VAULT_FILE))?;
    let probe: PublicIdProbe = serde_json::from_str(&text).ok()?;
    let id = probe.header.vault_id;
    super::witness::record_vault_id_is_valid(&id).then_some(id)
}

/// Reads the format version recorded in a vault home's public header.
///
/// The value is unauthenticated discovery metadata: it proves neither
/// integrity nor freshness. Nothing is created or locked, a FIFO or other
/// special file is never opened for blocking reads, and a missing,
/// unreadable, oversized, or malformed file reports `None`.
pub(super) fn public_format_version(root: &Path) -> Option<u32> {
    let text = read_regular_text(&root.join(VAULT_FILE))?;
    let probe: PublicEnvelopeProbe = serde_json::from_str(&text).ok()?;
    (probe.header.magic == MAGIC).then_some(probe.header.version)
}

fn read_regular_text(path: &Path) -> Option<String> {
    let before = fs::symlink_metadata(path).ok()?;
    if !before.is_file() || before.len() > VAULT_TEXT_READ_LIMIT {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).ok()?;
    let opened = file.metadata().ok()?;
    #[cfg(unix)]
    if opened.dev() != before.dev() || opened.ino() != before.ino() {
        return None;
    }
    if !opened.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(VAULT_TEXT_READ_LIMIT + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= VAULT_TEXT_READ_LIMIT).then_some(text)
}
