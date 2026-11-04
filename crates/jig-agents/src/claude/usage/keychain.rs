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
const PICKER_READ_FAILED: &str = "Claude Keychain access was denied, timed out, or is locked";
const HEADLESS_READ_FAILED: &str = "Claude Keychain login is locked or needs permission";
/// Unlocking and item permission are separate: a session without desktop
/// access, such as SSH, can do neither through a dialog, so name both remedies.
const READ_FAILED_REMEDY: &str = "unlock a locked Keychain with security unlock-keychain, as needed over SSH, and grant access by choosing Always Allow in the jig claude launch picker from a desktop session";

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
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let service = service(home)?;
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
    tool_read: impl FnOnce(Duration) -> Result<Zeroizing<Vec<u8>>, String>,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    match silent::search(&query, &mut search) {
        Ok(items) => {
            return Ok(items.into_iter().find_map(|item| match item {
                SearchResult::Data(bytes) => Some(Zeroizing::new(bytes)),
                _ => None,
            }));
        }
        Err(error) if error.code() == -25300 => {
            // A skipped protected item is also reported as not found. Inspect
            // attributes before falling back to a potentially stale file token.
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
        return Err("Claude usage inspection was cancelled".into());
    }
    // Jig's own read never asks for trust, so a protected item is read through
    // Apple's security tool, whose permission survives Jig rebuilds and upgrades.
    let (timeout, failure) = match mode {
        InspectionMode::Interactive => (PICKER_READ_TIMEOUT, PICKER_READ_FAILED),
        InspectionMode::Headless => (HEADLESS_READ_TIMEOUT, HEADLESS_READ_FAILED),
    };
    tool_read(timeout)
        .map(Some)
        .map_err(|_| format!("{failure}; {READ_FAILED_REMEDY}"))
}
