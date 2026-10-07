use std::sync::Mutex;

use security_framework::base::Result;
use security_framework::item::{ItemSearchOptions, SearchResult};
use security_framework::os::macos::keychain::SecKeychain;

// SecKeychainSetUserInteractionAllowed controls the whole process. Serialize
// native reads so one inspection cannot re-enable prompts during another.
static INTERACTION: Mutex<()> = Mutex::new(());

pub(super) fn search(
    query: &ItemSearchOptions,
    read: impl FnOnce(&ItemSearchOptions) -> Result<Vec<SearchResult>>,
) -> Result<Vec<SearchResult>> {
    without_interaction(|| read(query))
}

fn without_interaction<T>(read: impl FnOnce() -> Result<T>) -> Result<T> {
    let _lock = INTERACTION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // kSecUseAuthenticationUISkip alone does not suppress the legacy file
    // Keychain's ACL dialogs. Never let the native probe request trust in Jig:
    // its ad-hoc code identity can change on every rebuild or upgrade. Only the
    // separate /usr/bin/security process may prompt, with its stable identity.
    // The framework guard re-enables interaction on drop, so only create it
    // when interaction was enabled on entry.
    let _interaction = if SecKeychain::user_interaction_allowed()? {
        Some(SecKeychain::disable_user_interaction()?)
    } else {
        None
    };
    read()
}
