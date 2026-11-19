use std::time::Duration;

use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

mod prompt;
mod silent;
#[cfg(test)]
mod tests;

use crate::agent_provider::InspectionMode;
use crate::claude::Home;

/// The picker waits while the user answers a permission dialog.
const PICKER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// A tool read with permission already granted returns at once. Headless
/// inspection stops waiting on one that needs the user to answer a dialog.
const HEADLESS_READ_TIMEOUT: Duration = Duration::from_secs(2);
const READ_FAILED_REMEDY: &str = "unlock a locked Keychain with security unlock-keychain, as needed over SSH; an item that does not yet allow /usr/bin/security also needs Always Allow from the jig claude launch picker in a desktop session";

pub(super) type ReadResult = Result<Option<Zeroizing<Vec<u8>>>, ReadError>;

/// Evidence from the read. Confirmed absence is `Ok(None)` regardless of
/// which probe first reported a search miss.
#[derive(Debug)]
pub(super) enum ReadError {
    ToolMissing,
    SearchMiss,
    InteractionNotAllowed,
    Refused,
    TimedOut,
    EmptyOutput,
    OutputTooLarge,
    FailedExit(std::process::ExitStatus),
    Unreadable,
    Cancelled,
    Service(String),
}

impl ReadError {
    pub(super) fn message(&self) -> String {
        match self {
            Self::ToolMissing => "Claude Keychain credentials require the missing /usr/bin/security tool".into(),
            Self::SearchMiss => "Claude Keychain credentials could not be read; the item could not be confirmed absent".into(),
            Self::InteractionNotAllowed => format!("Claude Keychain is locked or cannot ask for permission in this session; {READ_FAILED_REMEDY}"),
            Self::Refused => format!("Claude Keychain access was denied or cancelled; {READ_FAILED_REMEDY}"),
            Self::TimedOut => "Claude Keychain read timed out; if a permission dialog is waiting, answer it from the jig claude launch picker in a desktop session".into(),
            Self::EmptyOutput => "/usr/bin/security returned empty Claude Keychain credentials".into(),
            Self::OutputTooLarge => "Claude Keychain credentials exceeded the inspection size limit".into(),
            Self::FailedExit(status) => format!("Claude Keychain credentials could not be read through /usr/bin/security ({status})"),
            Self::Unreadable => "Claude Keychain credentials could not be read through /usr/bin/security".into(),
            Self::Cancelled => super::CANCELLED.into(),
            Self::Service(message) => message.clone(),
        }
    }
}

pub(super) fn service(home: &Home) -> Result<String, String> {
    if home.default_config {
        return Ok("Claude Code-credentials".into());
    }
    let path = home
        .path
        .to_str()
        .ok_or("Claude Keychain lookup requires a Unicode configuration path")?;
    let normalized: String = path.nfc().collect();
    let hash = Sha256::digest(normalized.as_bytes());
    Ok(format!(
        "Claude Code-credentials-{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3]
    ))
}

pub(super) fn read(
    home: &Home,
    mode: InspectionMode,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> ReadResult {
    // A launch still derives a service name and reads that item, so a path Jig
    // cannot name keeps its error instead of reporting the file login.
    let service = service(home).map_err(ReadError::Service)?;
    let account = std::env::var("USER")
        .ok()
        .filter(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
        .unwrap_or_else(|| "claude-code-user".into());
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service(&service)
        .account(&account)
        .load_data(true)
        .skip_authenticated_items(true);
    read_with(
        query,
        mode,
        cancelled,
        ItemSearchOptions::search,
        |timeout| prompt::read(&service, &account, timeout, cancelled),
    )
}

fn read_with(
    mut query: ItemSearchOptions,
    mode: InspectionMode,
    cancelled: &(dyn Fn() -> bool + Sync),
    mut search: impl FnMut(&ItemSearchOptions) -> security_framework::base::Result<Vec<SearchResult>>,
    tool_read: impl FnOnce(Duration) -> Result<Zeroizing<Vec<u8>>, ReadError>,
) -> ReadResult {
    let check_for_skipped_item = match silent::search(&query, &mut search) {
        Ok(items) => {
            if let Some(bytes) = items.into_iter().find_map(|item| match item {
                SearchResult::Data(bytes) => Some(Zeroizing::new(bytes)),
                _ => None,
            }) {
                return Ok(Some(bytes));
            }
            true
        }
        Err(error) => error.code() == -25300, // errSecItemNotFound
    };

    // Check only attributes, with dialogs disabled and protected items included.
    // A data query that skips authenticated items cannot establish absence.
    query
        .load_data(false)
        .load_attributes(true)
        .skip_authenticated_items(false);
    let mut confirmed_absent = || match silent::search(&query, &mut search) {
        Err(error) => error.code() == -25300,
        Ok(items) => items.is_empty(),
    };
    if check_for_skipped_item && confirmed_absent() {
        return Ok(None);
    }
    if cancelled() {
        return Err(ReadError::Cancelled);
    }
    // Only the system tool may prompt for permission to read the password.
    let timeout = match mode {
        InspectionMode::Interactive => PICKER_READ_TIMEOUT,
        InspectionMode::Headless => HEADLESS_READ_TIMEOUT,
    };
    match tool_read(timeout) {
        // security also returns errSecItemNotFound when its underlying search
        // failed. Recheck after the command: an item present earlier may now
        // be gone, but a failed or inconclusive search cannot select the file.
        Err(ReadError::SearchMiss) if cancelled() => Err(ReadError::Cancelled),
        Err(ReadError::SearchMiss) if confirmed_absent() => Ok(None),
        result => result.map(Some),
    }
}
