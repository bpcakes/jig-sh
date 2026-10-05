use std::sync::Mutex;

use security_framework::base::Result;
use security_framework::item::{ItemSearchOptions, SearchResult};
use security_framework::os::macos::keychain::SecKeychain;

// SecKeychainSetUserInteractionAllowed controls the whole process. Serialize
// native reads so one inspection cannot re-enable prompts during another.
static INTERACTION: Mutex<()> = Mutex::new(());

pub(super) fn search(query: &ItemSearchOptions) -> Result<Vec<SearchResult>> {
    without_interaction(|| query.search())
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

#[cfg(test)]
mod tests {
    use super::*;
    use security_framework::base::Error;

    #[test]
    fn native_reads_suppress_dialogs_and_preserve_the_callers_interaction_setting() {
        let original = SecKeychain::user_interaction_allowed().unwrap();
        let result = without_interaction(|| {
            assert!(!SecKeychain::user_interaction_allowed()?);
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);

        let error = without_interaction::<()>(|| {
            assert!(!SecKeychain::user_interaction_allowed()?);
            Err(Error::from_code(-25308))
        });
        assert_eq!(error.unwrap_err().code(), -25308);
        assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);

        let _disabled = if original {
            Some(SecKeychain::disable_user_interaction().unwrap())
        } else {
            None
        };
        without_interaction(|| {
            assert!(!SecKeychain::user_interaction_allowed()?);
            Ok(())
        })
        .unwrap();
        assert!(!SecKeychain::user_interaction_allowed().unwrap());
    }
}
