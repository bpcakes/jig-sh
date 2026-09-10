use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

mod prompt;

use crate::claude::Home;

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
    allow_prompt: bool,
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
    match query.search() {
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
            match query.search() {
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
    if allow_prompt {
        prompt::read(&service, &account, cancelled).map(Some)
    } else {
        Err("Claude Keychain credentials require permission; open jig claude launch to allow access".into())
    }
}
