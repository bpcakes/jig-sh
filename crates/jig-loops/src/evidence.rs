//! Loop evidence: one durable record per loop occurrence describing what its
//! tick observed and did, including any worker run.
//!
//! Records live beside the protected schedule, under `<git-dir>/jig/loop/`,
//! so they are never part of the checkout a worker can modify. Repositories
//! without Git metadata use the ignored `.agent/runtime/loop/`. A record is
//! kept only while its occurrence is in the schedule, so evidence follows the
//! occurrence history's retention.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use jig_context::RepoContext;

use super::authority::resolve_protected_loop_authority;
use super::occurrence::OccurrenceStore;
use super::state::{LOOP_RUNTIME_DIR, StateDirectory};

const EVIDENCE_DIR: &str = "evidence";
const EVIDENCE_SCHEMA_VERSION: u32 = 1;
/// Records above this size drop the observed snapshot, then action detail,
/// rather than failing the occurrence.
const MAX_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct OccurrenceEvidence {
    pub(super) schema_version: u32,
    pub(super) occurrence_id: String,
    pub(super) workflow_id: String,
    pub(super) started_at_ms: u64,
    pub(super) ended_at_ms: u64,
    /// The tick's status, observations, actions, and worker runs.
    pub(super) tick: Value,
}

impl OccurrenceEvidence {
    pub(super) fn new(
        occurrence_id: &str,
        workflow_id: &str,
        started_at_ms: u64,
        ended_at_ms: u64,
        tick: Value,
    ) -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            occurrence_id: occurrence_id.to_owned(),
            workflow_id: workflow_id.to_owned(),
            started_at_ms,
            ended_at_ms,
            tick,
        }
    }
}

struct EvidenceLocation {
    root: PathBuf,
    dir: PathBuf,
}

impl EvidenceLocation {
    fn resolve(ctx: &RepoContext) -> Result<Self> {
        Ok(match resolve_protected_loop_authority(ctx.root())? {
            Some(authority) => Self {
                dir: authority.dir.join(EVIDENCE_DIR),
                root: authority.root,
            },
            None => Self {
                root: ctx.root().to_path_buf(),
                dir: ctx.root().join(LOOP_RUNTIME_DIR).join(EVIDENCE_DIR),
            },
        })
    }

    fn path(&self, name: &OsStr) -> PathBuf {
        self.dir.join(name)
    }
}

/// Durably records an occurrence's evidence, replacing an earlier record for
/// the same occurrence, then drops records whose occurrence left the schedule.
pub(super) fn record(ctx: &RepoContext, evidence: &OccurrenceEvidence) -> Result<()> {
    let location = EvidenceLocation::resolve(ctx)?;
    let directory = StateDirectory::open(&location.root, &location.dir)?;
    let name = file_name(&evidence.occurrence_id);
    directory
        .write_json_durable(&name, &location.path(&name), &within_size_limit(evidence))
        .with_context(|| {
            format!(
                "Failed to record evidence for loop occurrence {}",
                evidence.occurrence_id
            )
        })?;
    // Pruning only reclaims space; a later record retries it.
    let _ = prune_orphans(ctx, &location, &directory);
    Ok(())
}

pub(super) fn read(
    ctx: &RepoContext,
    occurrence_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<OccurrenceEvidence>> {
    let location = EvidenceLocation::resolve(ctx)?;
    let Some(directory) = StateDirectory::open_existing(&location.root, &location.dir)? else {
        return Ok(None);
    };
    let name = file_name(occurrence_id);
    directory.read_json(&name, &location.path(&name), cancelled)
}

/// Removes orphan evidence under the schedule locks. Names are listed first
/// so newly created names are not considered; holding the locks also prevents
/// a previously listed name from being reclaimed and replaced before unlink.
fn prune_orphans(
    ctx: &RepoContext,
    location: &EvidenceLocation,
    directory: &StateDirectory,
) -> Result<()> {
    let names = directory.regular_file_names(&location.dir)?;
    remove_orphans(ctx, location, directory, &names, || {})
}

fn remove_orphans(
    ctx: &RepoContext,
    location: &EvidenceLocation,
    directory: &StateDirectory,
    names: &[OsString],
    after_snapshot: impl FnOnce(),
) -> Result<()> {
    OccurrenceStore::new(ctx).with_retained_occurrences(|occurrences| {
        let retained = occurrences
            .iter()
            .map(|occurrence| file_name(&occurrence.occurrence_id))
            .collect::<BTreeSet<_>>();
        after_snapshot();
        for name in names {
            if is_evidence_file_name(name) && !retained.contains(name) {
                directory.remove_file(name, &location.path(name))?;
            }
        }
        Ok(())
    })
}

fn file_name(occurrence_id: &str) -> OsString {
    OsString::from(format!(
        "{:x}.json",
        Sha256::digest(occurrence_id.as_bytes())
    ))
}

fn is_evidence_file_name(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.strip_suffix(".json").is_some_and(|stem| {
            stem.len() == 64
                && stem
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    })
}

/// Keeps a record under the size limit by replacing the observed snapshot,
/// then each action, with a summary of what was omitted.
fn within_size_limit(evidence: &OccurrenceEvidence) -> OccurrenceEvidence {
    if encoded_len(evidence) <= MAX_EVIDENCE_BYTES {
        return evidence.clone();
    }
    let mut reduced = evidence.clone();
    let observed_bytes =
        serde_json::to_vec(&reduced.tick["observed"]).map_or(0, |bytes| bytes.len());
    reduced.tick["observed"] = json!({
        "omitted": true,
        "reason": "the observed snapshot exceeded the loop evidence size limit",
        "bytes": observed_bytes,
    });
    if encoded_len(&reduced) <= MAX_EVIDENCE_BYTES {
        return reduced;
    }
    if let Some(actions) = reduced.tick["actions"].as_array_mut() {
        for action in actions {
            *action = json!({
                "kind": action["kind"],
                "status": action["status"],
                "item_key": action["item_key"],
                "error": action["error"],
                "omitted": true,
                "reason": "action detail exceeded the loop evidence size limit",
            });
        }
    }
    reduced
}

fn encoded_len(evidence: &OccurrenceEvidence) -> usize {
    serde_json::to_vec_pretty(evidence).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn directory_for_test(ctx: &RepoContext) -> Result<PathBuf> {
    Ok(EvidenceLocation::resolve(ctx)?.dir)
}

#[cfg(test)]
pub(super) fn path_for_test(ctx: &RepoContext, occurrence_id: &str) -> Result<PathBuf> {
    let location = EvidenceLocation::resolve(ctx)?;
    Ok(location.path(&file_name(occurrence_id)))
}

#[cfg(test)]
#[path = "evidence/tests.rs"]
mod tests;
