//! The status shown after an encrypted backup is restored.

use crate::VaultActionResult;

/// A witnessed restore lands above every other copy of its vault ID, so
/// those copies are refused afterwards; the TUI reports that as the CLI
/// does.
pub(crate) fn restore_status(restored: &VaultActionResult) -> &'static str {
    let other_copies_stale = matches!(
        restored,
        VaultActionResult::Restored {
            other_copies_stale: true,
            ..
        }
    );
    if other_copies_stale {
        "Encrypted backup restored. Other copies of this vault are now stale and will be refused. Enter its vault passphrase to unlock."
    } else {
        "Encrypted backup restored. Enter its vault passphrase to unlock."
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jig_vault::VaultHomeState;

    use crate::model::App;
    use crate::{VaultActionResult, VaultDescriptor};

    fn restored(other_copies_stale: bool) -> String {
        let mut app = App::new(VaultDescriptor {
            scope: "repo".to_owned(),
            scope_id: Some("scope_123".to_owned()),
            repo_name: Some("demo".to_owned()),
            home: PathBuf::from("/tmp/demo-vault"),
            home_state: VaultHomeState::Absent,
        });
        app.apply_restore(&VaultActionResult::Restored {
            root: PathBuf::from("/tmp/demo-vault"),
            vault_id: "01EXAMPLEVAULTID00000000000".to_owned(),
            format_version: 3,
            other_copies_stale,
        });
        app.status.unwrap().text
    }

    #[test]
    fn a_witnessed_restore_warns_that_other_copies_are_stale() {
        assert!(restored(true).contains("Other copies of this vault are now stale"));
        assert!(!restored(false).contains("stale"));
    }
}
