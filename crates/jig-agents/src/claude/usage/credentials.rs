use std::fs::OpenOptions;
use std::io::Read;

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::claude::Home;

const CREDENTIAL_LIMIT: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Credential {
    #[serde(deserialize_with = "secret")]
    pub(super) access_token: Zeroizing<String>,
    pub(super) expires_at: Option<i64>,
    pub(super) subscription_type: Option<String>,
    pub(super) scopes: Option<Vec<String>>,
}

fn secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(deserializer).map(Zeroizing::new)
}

pub(super) fn parse(bytes: &[u8]) -> Result<Credential, String> {
    if bytes.len() as u64 > CREDENTIAL_LIMIT {
        return Err("Claude credentials exceeded the size limit".into());
    }
    #[derive(Deserialize)]
    struct Stored {
        #[serde(rename = "claudeAiOauth")]
        oauth: Option<Credential>,
    }
    let stored: Stored =
        serde_json::from_slice(bytes).map_err(|_| "Claude credentials could not be decoded")?;
    let credential = stored
        .oauth
        .ok_or("No Claude subscription login in this home")?;
    if credential.access_token.is_empty() {
        return Err("No Claude subscription login in this home".into());
    }
    Ok(credential)
}

impl Credential {
    pub(super) fn usage_error(&self, now_millis: i64) -> Option<String> {
        if self.expires_at.is_some_and(|expiry| expiry <= now_millis) {
            Some("Claude login expired; sign in again with Claude".into())
        } else if self
            .scopes
            .as_ref()
            .is_some_and(|scopes| !scopes.iter().any(|scope| scope == "user:profile"))
        {
            Some("Claude login lacks the user:profile scope required for usage".into())
        } else {
            None
        }
    }
}

pub(super) fn load(
    home: &Home,
    allow_keychain_prompt: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Credential, String> {
    // These selectors can change credential authority independently of HOME. Do not
    // present an unrelated stored subscription as the account a launch would use.
    for key in [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        "CLAUDE_CODE_CUSTOM_OAUTH_URL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        if std::env::var_os(key).is_some_and(|value| !value.is_empty()) {
            return Err(format!(
                "Per-home subscription usage unavailable while {key} is set"
            ));
        }
    }
    // Explicit launch paths are canonicalized before CLAUDE_CONFIG_DIR is set.
    let home = Home {
        path: crate::home_paths::canonical_key(&home.path),
        default_config: home.default_config,
    };
    #[cfg(target_os = "macos")]
    if let Some(bytes) = super::keychain::read(&home, allow_keychain_prompt, cancelled)? {
        return parse(&bytes);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (allow_keychain_prompt, cancelled);
    read_file(&home)
}

pub(super) fn read_file(home: &Home) -> Result<Credential, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(home.path.join(".credentials.json"))
        .map_err(|_| "No readable Claude login in this home; sign in with Claude")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Could not inspect Claude credentials")?;
    if !metadata.is_file() || metadata.len() > CREDENTIAL_LIMIT {
        return Err("Claude credentials must be a regular file within the size limit".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(CREDENTIAL_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Claude credentials")?;
    if bytes.len() as u64 > CREDENTIAL_LIMIT {
        return Err("Claude credentials exceeded the size limit".into());
    }
    parse(&bytes)
}
