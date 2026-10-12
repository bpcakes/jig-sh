//! The account Claude Code records as signed in for a home.

use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::claude::Home;

/// `.claude.json` also keeps per-project history; a larger file is skipped.
const CONFIG_LIMIT: u64 = 8 * 1024 * 1024;

/// The signed-in account's email from the home's `.claude.json`.
///
/// The file holds account metadata, not credentials. Anything missing,
/// unreadable, or oversized leaves the email unknown instead of failing the
/// inspection.
pub(super) fn email(home: &Home) -> Option<String> {
    parse_email(&read_bounded(&config_path(home)?)?)
}

/// The native default keeps its configuration at `~/.claude.json`; an
/// explicit `CLAUDE_CONFIG_DIR` keeps it inside that directory.
fn config_path(home: &Home) -> Option<PathBuf> {
    if home.default_config {
        super::super::user_home()
            .ok()
            .map(|root| root.join(".claude.json"))
    } else {
        Some(home.path.join(".claude.json"))
    }
}

pub(super) fn parse_email(bytes: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(rename = "oauthAccount")]
        account: Option<Account>,
    }
    #[derive(Deserialize)]
    struct Account {
        #[serde(rename = "emailAddress")]
        email: Option<String>,
    }
    let config: Config = serde_json::from_slice(bytes).ok()?;
    config
        .account?
        .email
        .filter(|email| !email.trim().is_empty())
}

pub(super) fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > CONFIG_LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(CONFIG_LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= CONFIG_LIMIT).then_some(bytes)
}
