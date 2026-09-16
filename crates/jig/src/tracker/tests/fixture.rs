use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::{TempDir, tempdir};

use super::*;

pub(super) struct Fixture {
    pub(super) _temp: TempDir,
    pub(super) root: PathBuf,
    pub(super) bin: PathBuf,
    pub(super) log: PathBuf,
}

impl Fixture {
    pub(super) fn new(version: &str) -> Self {
        let temp = tempdir().unwrap();
        let root = temp.path().join("ExampleProject");
        let bin = temp.path().join("bin");
        fs::create_dir_all(root.join(".beads")).unwrap();
        fs::create_dir(&bin).unwrap();
        let root = root.canonicalize().unwrap();
        let bin = bin.canonicalize().unwrap();
        fs::write(root.join(".beads/beads.db"), b"fixture database").unwrap();
        fs::write(root.join(".beads/issues.jsonl"), b"").unwrap();
        let log = temp.path().join("br-argv.bin");
        write_executable(
            &bin.join("br"),
            &fake_br_script(&root, temp.path(), version),
        );
        Self {
            _temp: temp,
            root,
            bin,
            log,
        }
    }

    pub(super) fn discover(
        &self,
        policy: TrackerProcessPolicy,
    ) -> (BeadsAdapter, TrackerDiscovery) {
        let mut never_cancelled = || false;
        BeadsAdapter::discover(&self.root, WORKSPACE_ID, policy, &mut never_cancelled).unwrap()
    }
}

pub(super) fn sync_status(
    jsonl_newer: bool,
    db_newer: bool,
    coverage_drift: bool,
    health: &str,
    anomaly_codes: &[&str],
) -> serde_json::Value {
    json!({
        "jsonl_newer": jsonl_newer,
        "db_newer": db_newer,
        "coverage_drift": coverage_drift,
        "workspace_health": health,
        "reliability_audit": {
            "source": "sync.status",
            "health": health,
            "anomaly_count": anomaly_codes.len(),
            "anomalies": anomaly_codes.iter().map(|code| json!({
                "code": code,
                "severity": health
            })).collect::<Vec<_>>()
        }
    })
}

pub(super) fn logged_argument_groups(path: &Path) -> Vec<Vec<String>> {
    let mut groups = vec![Vec::new()];
    for bytes in fs::read(path).unwrap().split(|byte| *byte == 0) {
        if bytes.is_empty() {
            continue;
        }
        let value = String::from_utf8(bytes.to_vec()).unwrap();
        if value == "__END__" {
            groups.push(Vec::new());
        } else {
            groups.last_mut().unwrap().push(value);
        }
    }
    groups.retain(|group| !group.is_empty());
    groups
}

pub(super) fn tree_identity(
    root: &Path,
) -> Vec<(PathBuf, &'static str, Vec<u8>, std::time::SystemTime)> {
    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut Vec<(PathBuf, &'static str, Vec<u8>, std::time::SystemTime)>,
    ) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            "other"
        };
        let bytes = if metadata.is_file() {
            fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        entries.push((
            path.strip_prefix(root).unwrap().to_path_buf(),
            kind,
            bytes,
            metadata.modified().unwrap(),
        ));
        if metadata.is_dir() {
            let mut children = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                visit(root, &child, entries);
            }
        }
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries);
    entries
}

pub(super) fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn fake_br_script(root: &Path, fixture_root: &Path, version: &str) -> String {
    let info = json!({
        "database_path": root.join(".beads/beads.db"),
        "path": root.join(".beads"),
        "jsonl_path": root.join(".beads/issues.jsonl")
    });
    let outside = json!({
        "database_path": fixture_root.join("outside.db"),
        "path": root.join(".beads"),
        "jsonl_path": root.join(".beads/issues.jsonl")
    });
    let symlinked = json!({
        "database_path": root.join(".beads/link.db"),
        "path": root.join(".beads"),
        "jsonl_path": root.join(".beads/issues.jsonl")
    });
    let issue = json!([{
        "id": ISSUE_ID,
        "title": "Generic fixture task",
        "description": "Generic fixture description",
        "acceptance_criteria": "It remains portable.",
        "status": "open",
        "assignee": null,
        "updated_at": "2026-01-01T00:00:00Z",
        "comments": [{"id": 6, "text": "ignored by snapshot"}]
    }]);
    let comments = json!([{
        "id": 6,
        "issue_id": ISSUE_ID,
        "author": "ExampleAgent",
        "text": "Generic comment",
        "created_at": "2026-01-01T00:00:00Z"
    }]);
    let comments_with_error = json!([{
        "id": 6,
        "issue_id": ISSUE_ID,
        "author": "ExampleAgent",
        "text": "Generic comment",
        "created_at": "2026-01-01T00:00:00Z",
        "error": {
            "code": "POLICY_VIOLATION",
            "message": "Contradictory comment",
            "hint": null,
            "retryable": false,
            "context": null
        }
    }]);
    let added = json!({
        "id": 7,
        "issue_id": ISSUE_ID,
        "author": ACTOR,
        "text": "Marker; $(touch should-not-exist)\nsecond line",
        "created_at": "2026-01-02T00:00:00Z"
    });
    let mismatched_comment = json!({
        "id": 7,
        "issue_id": ISSUE_ID,
        "author": "DifferentAgent",
        "text": "Different text",
        "created_at": "2026-01-02T00:00:00Z"
    });
    let claim = json!([{
        "id": ISSUE_ID,
        "status": "in_progress",
        "assignee": ACTOR,
        "updated_at": "2026-01-03T00:00:00Z"
    }]);
    let option_actor_claim = json!([{
        "id": ISSUE_ID,
        "status": "in_progress",
        "assignee": "--force",
        "updated_at": "2026-01-03T00:00:00Z"
    }]);
    let close = json!([{
        "id": ISSUE_ID,
        "status": "closed",
        "closed_at": "2026-01-04T00:00:00Z"
    }]);
    let close_blocked = json!({
        "closed": [],
        "skipped": [{"id": ISSUE_ID, "reason": "blocked by: ExampleProject-2"}],
        "warnings": []
    });
    let close_missing = json!({
        "error": {
            "code": "ISSUE_NOT_FOUND",
            "message": "Issue not found",
            "hint": null,
            "retryable": false
            ,"context": null
        }
    });
    let combined_error = json!({
        "updated": [{"id": ISSUE_ID, "status": "in_progress"}],
        "error": {
            "code": "POLICY_VIOLATION",
            "message": "Policy rejected the update",
            "hint": null,
            "retryable": false,
            "context": null
        }
    });
    let wrapped_claim = json!({
        "updated": claim
    });
    let close_with_error = json!([{
        "id": ISSUE_ID,
        "status": "closed",
        "closed_at": "2026-01-04T00:00:00Z",
        "error": {
            "code": "POLICY_VIOLATION",
            "message": "Policy rejected the update",
            "hint": null,
            "retryable": false,
            "context": null
        }
    }]);
    let policy_error = json!({
        "error": {
            "code": "POLICY_VIOLATION",
            "message": "Policy rejected the update",
            "hint": null,
            "retryable": false,
            "context": null
        }
    });
    let close_ambiguous = json!({
        "closed": [{"id": ISSUE_ID, "status": "closed"}],
        "skipped": [{"id": ISSUE_ID, "reason": "blocked"}],
        "warnings": []
    });
    let sync = sync_status(false, false, false, "healthy", &[]);
    let mut sync_with_error = sync.clone();
    sync_with_error["error"] = policy_error["error"].clone();
    let close_conflicting_noop = json!({
        "closed": [],
        "skipped": [{
            "id": ISSUE_ID,
            "reason": "blocked by: ExampleProject-2",
            "status": "closed"
        }],
        "updated": [{"id": ISSUE_ID, "status": "closed"}],
        "warnings": []
    });
    let version = json!({"version": version});
    format!(
        r#"#!/bin/sh
database=
previous=
for argument do
  printf '%s\0' "$argument" >> "$JIG_TRACKER_TEST_LOG"
  if [ "$previous" = --db ]; then database=$argument; fi
  previous=$argument
done
printf '__END__\0' >> "$JIG_TRACKER_TEST_LOG"
if [ "$PWD" != {root} ]; then exit 91; fi
if [ "${{BD_DB+x}}" = x ] || [ "${{BD_DATABASE+x}}" = x ] || [ "${{BEADS_DIR+x}}" = x ] || [ "${{BEADS_CACHE_DIR+x}}" = x ] || [ "${{BR_OUTPUT_FORMAT+x}}" = x ] || [ "${{TOON_DEFAULT_FORMAT+x}}" = x ]; then exit 92; fi
if [ "${{BD_ALLOW_STALE+x}}" = x ] || [ "${{BR_INHERITED_CONTEXT+x}}" = x ] || [ "${{BEADS_REMOTE_SYNC_INTERVAL+x}}" = x ] || [ "${{TOON_STATS+x}}" = x ]; then exit 86; fi
if [ "${{BD_ACTOR-}}" != 'Ambient Actor' ]; then exit 93; fi
if [ "${{BR_STARTUP_CACHE-}}" != 0 ] || [ "${{BR_STARTUP_CACHE_DIR+x}}" = x ] || [ "${{BR_DISABLE_READ_ONLY_FAST_OPEN+x}}" = x ]; then exit 95; fi
if [ "${{LD_AUDIT+x}}" = x ] || [ "${{LD_LIBRARY_PATH+x}}" = x ] || [ "${{LD_PRELOAD+x}}" = x ] || [ "${{DYLD_INSERT_LIBRARIES+x}}" = x ] || [ "${{DYLD_LIBRARY_PATH+x}}" = x ] || [ "${{DYLD_VERSIONED_LIBRARY_PATH+x}}" = x ] || [ "${{DYLD_VERSIONED_FRAMEWORK_PATH+x}}" = x ]; then exit 89; fi
case " $* " in
  *' --no-db where '*) if [ "${{BEADS_JSONL+x}}" = x ] || [ "${{BD_NO_DB-}}" != true ]; then exit 96; fi ;;
  *' version '*) if [ "${{BEADS_JSONL+x}}" = x ] || [ "${{BD_NO_DB-}}" != true ]; then exit 94; fi ;;
  *' sync --allow-external-jsonl --status '*)
     if [ "${{BD_NO_DB-}}" != false ]; then exit 94; fi
     case "${{BEADS_JSONL-}}" in *.jsonl) ;; *) exit 97 ;; esac
     if [ ! -f "$BEADS_JSONL" ]; then exit 98; fi
     if [ "${{JIG_TRACKER_TEST_MODE-}}" = stale_lock ]; then
       lock_mtime=$(stat -c %Y "$(dirname "$database")/.beads.lock" 2>/dev/null || stat -f %m "$(dirname "$database")/.beads.lock" 2>/dev/null) || exit 88
       if [ "$lock_mtime" != "${{JIG_TRACKER_TEST_LOCK_MTIME-}}" ]; then exit 87; fi
     elif [ "${{JIG_TRACKER_TEST_MODE-}}" = migration_state ]; then
       marker="${{database}}.fsqlite-migration-state"
       if [ ! -f "$marker" ] || [ "$(cat "$marker")" != migration-complete ]; then exit 86; fi
       marker_mtime=$(stat -c %Y "$marker" 2>/dev/null || stat -f %m "$marker" 2>/dev/null) || exit 85
       if [ "$marker_mtime" != "${{JIG_TRACKER_TEST_MIGRATION_MTIME-}}" ]; then exit 84; fi
     fi ;;
  *) if [ "${{BEADS_JSONL+x}}" = x ] || [ "${{BD_NO_DB-}}" != false ]; then exit 99; fi ;;
esac
case " $* " in
  *' version '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = trailing ]; then printf '%s trailing\n' {version}
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = duplicate ]; then printf '%s\n' '{{"version":"0.5.7","version":"0.5.7"}}'
    else printf '%s\n' {version}; fi ;;
  *' --no-db where '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = outside ]; then printf '%s\n' {outside}
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = symlink ]; then printf '%s\n' {symlinked}
    else printf '%s\n' {info}; fi ;;
  *' sync --allow-external-jsonl --status '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = split_budget ]; then /bin/sleep 0.4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = stale_lock ]; then printf '%s\n' {stale_lock}
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = sync_conflicting_error ]; then printf '%s\n' {sync_with_error}
    else printf '%s\n' {sync}; fi ;;
	  *' show -- {issue_id} '*)
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = shm_not_copied ] && [ -e "${{database}}-shm" ]; then exit 81; fi
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = database_read ]; then
	      if grep -q provider-read "$database"; then exit 80; fi
	      printf provider-read >> "$database"
	    fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = missing ]; then printf '%s\n' '{{"error":{{"code":"ISSUE_NOT_FOUND","message":"Issue not found","hint":null,"retryable":false,"context":{{"path":"/private/path"}}}}}}' >&2; exit 4
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = unstructured_failure ]; then printf 'provider details\n' >&2; exit 5
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = overflow ]; then i=0; while [ "$i" -lt 512 ]; do printf 'xxxxxxxx'; i=$((i + 1)); done
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = timeout ]; then /bin/sleep 2; printf '%s\n' {issue}
    else printf '%s\n' {issue}; fi ;;
  *' show -- '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = missing ]; then printf '%s\n' '{{"error":{{"code":"ISSUE_NOT_FOUND","message":"Issue not found","hint":null,"retryable":false,"context":{{"path":"/private/path"}}}}}}' >&2; exit 4
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = unstructured_failure ]; then printf 'provider details\n' >&2; exit 5
    else exit 6; fi ;;
	  *' comments list -- {issue_id} '*)
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = database_read ]; then
	      if grep -q provider-read "$database"; then exit 80; fi
	      printf provider-read >> "$database"
	    fi
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = comments_error ]; then printf '%s\n' {comments_with_error}
	    else printf '%s\n' {comments}; fi ;;
  *' comments add '*' -- {issue_id} '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = success_stderr ]; then printf '%s\n' {added}; printf '%s\n' {policy_error} >&2
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = comment_mismatch ]; then printf '%s\n' {mismatched_comment}
    else printf '%s\n' {added}; fi ;;
	  *' update --claim '*' -- {issue_id} '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = claim_timeout ]; then /bin/sleep 2; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = split_budget ]; then /bin/sleep 0.4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = blocked ]; then printf '%s\n' '{{"error":{{"code":"POLICY_VIOLATION","message":"Policy rejected the update","hint":null,"retryable":false,"context":null}}}}' >&2; exit 4; fi
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = assignment ]; then printf '%s\n' '{{"error":{{"code":"WORKFLOW_CAPACITY_EXCEEDED","message":"Capacity exceeded","hint":null,"retryable":true,"context":null}}}}' >&2; exit 4; fi
	    if [ "${{JIG_TRACKER_TEST_MODE-}}" = ambiguous_id ]; then printf '%s\n' '{{"error":{{"code":"AMBIGUOUS_ID","message":"Ambiguous issue ID","hint":null,"retryable":true,"context":null}}}}' >&2; exit 4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = conflicting_error ]; then printf '%s\n' {claim}; printf '%s\n' '{{"error":{{"code":"POLICY_VIOLATION","message":"Policy rejected the update","hint":null,"retryable":false,"context":null}}}}' >&2; exit 4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = retryability_mismatch ]; then printf '%s\n' '{{"error":{{"code":"POLICY_VIOLATION","message":"Policy rejected the update","hint":null,"retryable":true,"context":null}}}}' >&2; exit 4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = malformed_error ]; then printf '%s\n' '{{"error":{{"code":"POLICY_VIOLATION","retryable":false}}}}' >&2; exit 4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = combined_error ]; then printf '%s\n' {combined_error}; exit 4; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = database_write ]; then printf provider-write >> "$database"; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = success_combined_error ]; then printf '%s\n' {combined_error}
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = success_wrapper ]; then printf '%s\n' {wrapped_claim}
    elif [ "${{JIG_TRACKER_TEST_MODE-}}" = option_actor ]; then printf '%s\n' {option_actor_claim}
    else printf '%s\n' {claim}; fi ;;
  *' close '*' -- {issue_id} '*)
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = close_conflicting_noop ]; then printf '%s\n' {close_conflicting_noop}; printf '%s\n' {nothing_to_do}; exit 3; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = close_blocked ]; then printf '%s\n' {close_blocked}; printf '%s\n' {nothing_to_do}; exit 3; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = close_missing ]; then printf '%s\n' {close_missing}; exit 3; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = close_incomplete_error ]; then printf '%s\n' {close_blocked}; printf '%s\n' '{{"error":{{"code":"NOTHING_TO_DO","message":"Nothing to do","hint":null,"context":null}}}}'; exit 3; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = close_ambiguous ]; then printf '%s\n' {close_ambiguous}; printf '%s\n' {nothing_to_do}; exit 3; fi
    if [ "${{JIG_TRACKER_TEST_MODE-}}" = success_item_error ]; then printf '%s\n' {close_with_error}
    else printf '%s\n' {close}; fi ;;
  *) printf '%s\n' '{{"error":{{"code":"ISSUE_NOT_FOUND","message":"Issue not found","hint":null,"retryable":false,"context":null}}}}' >&2; exit 4 ;;
esac
"#,
        root = shell_quote(&root.to_string_lossy()),
        version = shell_quote(&version.to_string()),
        outside = shell_quote(&outside.to_string()),
        symlinked = shell_quote(&symlinked.to_string()),
        info = shell_quote(&info.to_string()),
        sync = shell_quote(&sync.to_string()),
        sync_with_error = shell_quote(&sync_with_error.to_string()),
        stale_lock = shell_quote(
            &sync_status(false, false, false, "degraded", &["orphaned_lock_file"]).to_string()
        ),
        issue_id = ISSUE_ID,
        issue = shell_quote(&issue.to_string()),
        comments = shell_quote(&comments.to_string()),
        comments_with_error = shell_quote(&comments_with_error.to_string()),
        added = shell_quote(&added.to_string()),
        mismatched_comment = shell_quote(&mismatched_comment.to_string()),
        claim = shell_quote(&claim.to_string()),
        option_actor_claim = shell_quote(&option_actor_claim.to_string()),
        combined_error = shell_quote(&combined_error.to_string()),
        wrapped_claim = shell_quote(&wrapped_claim.to_string()),
        close_with_error = shell_quote(&close_with_error.to_string()),
        policy_error = shell_quote(&policy_error.to_string()),
        close = shell_quote(&close.to_string()),
        close_blocked = shell_quote(&close_blocked.to_string()),
        close_missing = shell_quote(&close_missing.to_string()),
        close_ambiguous = shell_quote(&close_ambiguous.to_string()),
        close_conflicting_noop = shell_quote(&close_conflicting_noop.to_string()),
        nothing_to_do = shell_quote(
            &json!({
                "error": {
                    "code": "NOTHING_TO_DO",
                    "message": "Nothing to do",
                    "hint": null,
                    "retryable": false,
                    "context": null
                }
            })
            .to_string()
        ),
    )
}
