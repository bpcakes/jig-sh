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
const PICKER_READ_FAILED: &str = "Claude Keychain access was denied or is locked";
const PICKER_READ_WAITING: &str = "Claude Keychain permission request timed out";
const HEADLESS_READ_FAILED: &str = "Claude Keychain login is locked or needs permission";
const HEADLESS_READ_WAITING: &str = "Claude Keychain login needs permission";
/// Unlocking and item permission are separate: a session without desktop
/// access, such as SSH, can do neither through a dialog, so name both remedies.
const READ_FAILED_REMEDY: &str = "unlock a locked Keychain with security unlock-keychain, as needed over SSH; an item that does not yet allow /usr/bin/security also needs Always Allow from the jig claude launch picker in a desktop session";
const TOOL_UNREADABLE: &str =
    "Claude Keychain credentials could not be read through /usr/bin/security";

/// How the `security` tool failed to return the item.
#[derive(Debug)]
pub(super) enum ToolFailure {
    /// The tool ended without the item, as when the Keychain is locked in a
    /// session that cannot unlock it or the user denied access.
    Failed,
    /// The tool was still running at the deadline, typically waiting on a
    /// dialog.
    Waiting,
    /// The tool's output could not be taken, being oversized or unreadable, or
    /// the tool could not be stopped. A launch would still read the item.
    Unreadable,
}

/// A Keychain item that exists but could not be read.
#[derive(Debug)]
pub(super) struct ReadError {
    pub(super) message: String,
    /// Whether a Claude launch from this session would also go without the
    /// item and use `.credentials.json`. False while a dialog may still be
    /// open: a launch waits on it and then uses the Keychain login.
    pub(super) file_fallback: bool,
}

impl ReadError {
    /// A read that did not show the tool ending without the item, so the file
    /// login cannot stand in for the one a launch would read.
    fn without_fallback(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file_fallback: false,
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
) -> Result<Option<Zeroizing<Vec<u8>>>, ReadError> {
    // A launch still derives a service name and reads that item, so a path Jig
    // cannot name keeps its error instead of reporting the file login.
    let service = service(home).map_err(ReadError::without_fallback)?;
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
    tool_read: impl FnOnce(Duration) -> Result<Zeroizing<Vec<u8>>, ToolFailure>,
) -> Result<Option<Zeroizing<Vec<u8>>>, ReadError> {
    match silent::search(&query, &mut search) {
        Ok(items) => {
            return Ok(items.into_iter().find_map(|item| match item {
                SearchResult::Data(bytes) => Some(Zeroizing::new(bytes)),
                _ => None,
            }));
        }
        Err(error) if error.code() == -25300 => {
            // A skipped protected item is also reported as not found. Inspect
            // attributes so an existing item is read through the security tool
            // instead of being treated as missing.
            query.load_data(false).load_attributes(true);
            match silent::search(&query, &mut search) {
                Err(error) if error.code() == -25300 => return Ok(None),
                Ok(items) if items.is_empty() => return Ok(None),
                _ => {}
            }
        }
        Err(_) => {}
    }
    if cancelled() {
        return Err(ReadError::without_fallback(super::CANCELLED));
    }
    // Jig's own read never asks for trust, so a protected item is read through
    // Apple's security tool, whose permission survives Jig rebuilds and upgrades.
    let (timeout, failed, waiting) = match mode {
        InspectionMode::Interactive => {
            (PICKER_READ_TIMEOUT, PICKER_READ_FAILED, PICKER_READ_WAITING)
        }
        InspectionMode::Headless => (
            HEADLESS_READ_TIMEOUT,
            HEADLESS_READ_FAILED,
            HEADLESS_READ_WAITING,
        ),
    };
    tool_read(timeout).map(Some).map_err(|failure| {
        let (summary, file_fallback) = match failure {
            ToolFailure::Failed => (failed, true),
            ToolFailure::Waiting => (waiting, false),
            ToolFailure::Unreadable => return ReadError::without_fallback(TOOL_UNREADABLE),
        };
        ReadError {
            message: format!("{summary}; {READ_FAILED_REMEDY}"),
            file_fallback,
        }
    })
}
