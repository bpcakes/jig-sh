//! Restore durability: each step is durable before the next, and a retry
//! after a publication whose sync failed re-establishes the barrier it
//! relies on. Every retry forgets this process's durability cache, as a
//! fresh process would. Recording proves ordering and propagation, not
//! real power-loss behavior.

use super::*;
use crate::store::durable::recording::{
    FsOp, Publication, fail_sync_after_publication, forget_durable_entries_under, record,
};

struct Layout {
    _temp: tempfile::TempDir,
    /// This test's own directory; cache resets stay inside it.
    base: PathBuf,
    archive: PathBuf,
    /// A dedicated parent, so its syncs are only the restore's own.
    targets: PathBuf,
    target: PathBuf,
    witness: PathBuf,
}

/// A backup, a fresh witness on its own branch, and a target inside a
/// dedicated parent directory.
fn layout() -> (Layout, impl Drop) {
    let temp = private_temp();
    let base = fs::canonicalize(temp.path()).unwrap();
    let (home, _vault) = source(&base.join("sources"));
    let archive = base.join("vault.backup");
    backup(&home, &archive, &test_passphrase());
    let witness = base.join("w/witness");
    let guard = crate::store::witness::override_root_for_test(witness.clone());
    let targets = base.join("targets");
    fs::create_dir(&targets).unwrap();
    fs::set_permissions(&targets, fs::Permissions::from_mode(0o700)).unwrap();
    let target = targets.join("restored");
    (
        Layout {
            _temp: temp,
            base,
            archive,
            targets,
            target,
            witness,
        },
        guard,
    )
}

fn at(ops: &[FsOp], from: usize, wanted: impl Fn(&FsOp) -> bool) -> usize {
    from + ops[from..]
        .iter()
        .position(wanted)
        .unwrap_or_else(|| panic!("missing after {from}: {ops:?}"))
}

fn in_dir(op: &FsOp, dir: &Path) -> bool {
    let path = match op {
        FsOp::SyncDir(path) | FsOp::SyncFile(path) | FsOp::Rename(path) | FsOp::Remove(path) => {
            path
        }
    };
    path.parent() == Some(dir)
}

fn pending_marker_visible(layout: &Layout) -> bool {
    Vault::status(Some(layout.target.clone()))
        .unwrap()
        .pending_transaction
        && fs::read_dir(layout.witness.join("ids"))
            .unwrap()
            .any(|entry| {
                fs::read_to_string(entry.unwrap().path())
                    .is_ok_and(|text| text.contains("\"pending\": {"))
            })
}

#[test]
fn a_restore_makes_each_step_durable_before_the_next() {
    let (layout, _witness) = layout();
    let (ids, journals) = (layout.witness.join("ids"), layout.witness.join("journals"));

    let (restored, ops) = record(|| restore(&layout.archive, &layout.target));
    restored.unwrap();
    let is_staging = |op: &FsOp| match op {
        FsOp::SyncDir(path) | FsOp::SyncFile(path) => path
            .ancestors()
            .any(|dir| dir.to_string_lossy().contains(".jig-vault-restore-")),
        _ => false,
    };
    let staging_durable = ops.iter().rposition(is_staging).unwrap();
    let journal = at(&ops, 0, |op| {
        matches!(op, FsOp::Rename(_)) && in_dir(op, &journals)
    });
    let journal_synced = at(&ops, journal, |op| *op == FsOp::SyncDir(journals.clone()));
    let pending = at(&ops, journal_synced, |op| {
        matches!(op, FsOp::Rename(_)) && in_dir(op, &ids)
    });
    let pending_synced = at(&ops, pending, |op| *op == FsOp::SyncDir(ids.clone()));
    let installed = at(&ops, pending_synced, |op| {
        *op == FsOp::Rename(layout.target.clone())
    });
    let installed_synced = at(&ops, installed, |op| {
        *op == FsOp::SyncDir(layout.targets.clone())
    });
    let committed = at(&ops, installed_synced, |op| {
        matches!(op, FsOp::Rename(_)) && in_dir(op, &ids)
    });
    let committed_synced = at(&ops, committed, |op| *op == FsOp::SyncDir(ids.clone()));
    let removed = at(&ops, committed_synced, |op| {
        matches!(op, FsOp::Remove(_)) && in_dir(op, &journals)
    });
    at(&ops, removed, |op| *op == FsOp::SyncDir(journals.clone()));
    assert!(staging_durable < journal, "{ops:?}");
}

#[test]
fn a_journal_whose_sync_failed_is_unlinked_and_its_staging_kept() {
    let (layout, _witness) = layout();
    let journals = layout.witness.join("journals");
    fail_sync_after_publication(&journals, Publication::Rename, 0);
    let error = restore(&layout.archive, &layout.target).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("failed to persist the vault transaction journal"),
        "{error}"
    );
    // Published, then its sync failed: the staging is already recovery data.
    assert!(!pending_marker_visible(&layout));
    let [journal] = journal_paths(&layout.target).try_into().unwrap();
    let [staging] = staging_dirs(&layout.targets).try_into().unwrap();
    let staging = layout.targets.join(staging);
    let staged = staged_bytes(&staging);

    forget_durable_entries_under(&layout.base);
    let (restored, ops) = record(|| restore(&layout.archive, &layout.target));
    restored.unwrap();
    let removed = at(&ops, 0, |op| *op == FsOp::Remove(journal.clone()));
    let synced = at(&ops, removed, |op| *op == FsOp::SyncDir(journals.clone()));
    let republished = at(&ops, 0, |op| *op == FsOp::Rename(journal.clone()));
    assert!(synced < republished, "{ops:?}");
    assert_eq!(staged_bytes(&staging), staged);
}

#[test]
fn a_pending_marker_whose_sync_failed_is_made_durable_before_installing() {
    let (layout, _witness) = layout();
    let ids = layout.witness.join("ids");
    // The first record published is the pending marker.
    fail_sync_after_publication(&ids, Publication::Rename, 0);
    restore(&layout.archive, &layout.target).unwrap_err();
    assert!(pending_marker_visible(&layout));
    assert!(!layout.target.exists());
    assert_eq!(staging_dirs(&layout.targets).len(), 1);

    forget_durable_entries_under(&layout.base);
    let (restored, ops) = record(|| restore(&layout.archive, &layout.target));
    restored.unwrap();
    let synced = at(&ops, 0, |op| *op == FsOp::SyncDir(ids.clone()));
    let installed = at(&ops, 0, |op| *op == FsOp::Rename(layout.target.clone()));
    assert!(synced < installed, "{ops:?}");
}

#[test]
fn an_installation_whose_sync_failed_is_made_durable_before_committing() {
    // A restore retry reaches the installed home only through recovery; an
    // ordinary open also resolves the home, which syncs its entry itself.
    for through_restore in [true, false] {
        let (layout, _witness) = layout();
        let ids = layout.witness.join("ids");
        // The only rename into the parent is the installation.
        fail_sync_after_publication(&layout.targets, Publication::Rename, 0);
        restore(&layout.archive, &layout.target).unwrap_err();
        assert!(layout.target.join("vault.json").exists());
        assert!(pending_marker_visible(&layout));

        forget_durable_entries_under(&layout.base);
        let (retried, ops) = record(|| {
            if through_restore {
                restore(&layout.archive, &layout.target).map(|_| ())
            } else {
                Vault::resolve_for_test(Some(layout.target.clone()))
                    .unwrap()
                    .list_fields(&test_passphrase())
                    .map(|_| ())
            }
        });
        retried.unwrap();
        let synced = at(&ops, 0, |op| *op == FsOp::SyncDir(layout.targets.clone()));
        let committed = at(&ops, 0, |op| {
            matches!(op, FsOp::Rename(_)) && in_dir(op, &ids)
        });
        assert!(synced < committed, "{through_restore}: {ops:?}");
        assert!(!pending_marker_visible(&layout));
    }
}

#[test]
fn a_commit_whose_sync_failed_is_made_durable_before_its_journal_is_removed() {
    let (layout, _witness) = layout();
    let (ids, journals) = (layout.witness.join("ids"), layout.witness.join("journals"));
    // Records published: the pending marker, then the committed checkpoint.
    fail_sync_after_publication(&ids, Publication::Rename, 1);
    restore(&layout.archive, &layout.target).unwrap_err();
    assert!(layout.target.join("vault.json").exists());
    assert!(!pending_marker_visible(&layout));
    let [journal] = journal_paths(&layout.target).try_into().unwrap();

    forget_durable_entries_under(&layout.base);
    let (listed, ops) = record(|| {
        Vault::resolve_for_test(Some(layout.target.clone()))
            .unwrap()
            .list_fields(&test_passphrase())
    });
    listed.unwrap();
    let removed = at(&ops, 0, |op| *op == FsOp::Remove(journal.clone()));
    let synced_before = ops[..removed]
        .iter()
        .any(|op| *op == FsOp::SyncDir(ids.clone()));
    assert!(synced_before, "{ops:?}");
    at(&ops, removed, |op| *op == FsOp::SyncDir(journals.clone()));
}

#[test]
fn a_journal_removal_whose_sync_failed_is_made_durable_by_the_next_open() {
    let (layout, _witness) = layout();
    let journals = layout.witness.join("journals");
    fail_sync_after_publication(&journals, Publication::Remove, 0);
    restore(&layout.archive, &layout.target).unwrap_err();
    // Committed and unlinked, but the unlink was never made durable.
    assert!(layout.target.join("vault.json").exists());
    assert!(!pending_marker_visible(&layout));
    assert!(journal_paths(&layout.target).is_empty());

    forget_durable_entries_under(&layout.base);
    let (listed, ops) = record(|| {
        Vault::resolve_for_test(Some(layout.target.clone()))
            .unwrap()
            .list_fields(&test_passphrase())
    });
    listed.unwrap();
    assert!(ops.contains(&FsOp::SyncDir(journals)), "{ops:?}");
}
