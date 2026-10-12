use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use jig_context::RepoContext;
use jig_contract::{
    ActionId, ActionRunner, ComponentId, NativeActionConfigurationV1, RunConclusion, TargetId, tool,
};
use jig_repository::{RepositoryCatalog, target_input_digest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::managed_paths;
use super::repository_model::generated_file_budget_action;
use super::staged_render::{FILE_BUDGET_POLICY_PATH, StagedRender};

pub(super) const LEGACY_CHECKER_PATH: &str = "scripts/check-rust-file-loc.sh";
const LEGACY_REGISTRY_PATH: &str = ".agent/jig-legacy-assets.json";
const RERUN_COMMAND: &str = "scripts/jig check repo:file-budget";
const REGISTRY_VERSION: u32 = 1;
const NATIVE_EVALUATION_DEADLINE: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy)]
struct KnownLegacyAsset {
    generation: &'static str,
    path: &'static str,
    sha256: &'static str,
    executable: bool,
}

// This table deliberately contains identities only, never checker source. The
// generations cover the published self-contained checker and its preceding
// generated forms for the standard Rust workspace layout.
const KNOWN_LEGACY_ASSETS: &[KnownLegacyAsset] = &[
    KnownLegacyAsset {
        generation: "rust-loc-v5-source",
        path: LEGACY_CHECKER_PATH,
        sha256: "56fc9fe067912c47aa939f9f0044a34111b9361f3ef9e3bb47274e17cd735b8c",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v5-crates-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "0a17951b7214a5581f6ba2cb6b107d51c931d243cfc6b486f407100a9cfc88cc",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v5-root-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "056948ecaca33b4a43a5263993d43192748f8c9e2a5e83e2f9c8370546108bb7",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v4-source",
        path: LEGACY_CHECKER_PATH,
        sha256: "516f0a1622fd5a9b88f535173cfda78bb065dbe66a08e9034e907719f83cbb3b",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v4-crates-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "bc6b2624b5db47831de43faaeb01eb97c13c7c7001c50420128329b350966d2b",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v4-root-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "9ed6373594492624feda716533cc6c9161b0317fd77a267be2bfe547cc8ae2f7",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v3-source",
        path: LEGACY_CHECKER_PATH,
        sha256: "6acff8e0c10623be7aca405a4f76fbde2b76f77e3fb2e9bcea025fba10b8c8cd",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v3-crates-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "1f8bad97de5ce6e9ddb859816ebfbf62985f23a71343afdba2023a1c76d7e8cf",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v3-root-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "a6291371136a01e8c5f966070e681837e93d42615543df9a31c77076c080b80f",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v2-source",
        path: LEGACY_CHECKER_PATH,
        sha256: "46b7669ecb3f57098bc3cd8664173faa73cca96983fdeb8cd43e2a5934e66e16",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v2-crates-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "99bf536f62b154ef335f81d52a4526795f76f91b5fe7d28bf28d694968844529",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v2-root-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "e9d53cd9dec49a438678fcae807948895addffc3189bf36b91695f8fdf8f3ba8",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v1-source",
        path: LEGACY_CHECKER_PATH,
        sha256: "f49a2391b04f7af63b4cd80fdeb763a97daf87c1af71de4f6b8e9a8b02dc1155",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v1-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "adc0388851ada643e1dcbcf8373c455473fec9a4b25a2fd34d2b38673d24065c",
        executable: true,
    },
    KnownLegacyAsset {
        generation: "rust-loc-v0-rendered",
        path: LEGACY_CHECKER_PATH,
        sha256: "f1a0a1a36b213e53768198c62fd5b4689d00839ce49b4e7964940797e84265a0",
        executable: true,
    },
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyAssetRecord {
    generation: String,
    path: String,
    sha256: String,
    file_type: String,
    executable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyAssetRegistry {
    version: u32,
    assets: Vec<LegacyAssetRecord>,
}

/// Proof that the generated native file budget passed on the current source.
/// Transaction manifests written by earlier runtimes also carried receipt
/// fields, which are ignored on read.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct LifecycleProof {
    pub(super) config_digest: String,
    pub(super) input_digest: String,
    pub(super) source_fingerprint: String,
    pub(super) policy_raw_digest: String,
    pub(super) comparison: Value,
    pub(super) evaluation_digest: String,
    pub(super) valid_until_ms: Option<u64>,
}

impl LifecycleProof {
    /// Whether two passing evaluations covered the same authority, policy and
    /// source. Each evaluation digests its own evaluation time, so neither the
    /// evaluation digest nor the validity window is compared.
    fn covers_same_source(&self, other: &Self) -> bool {
        self.config_digest == other.config_digest
            && self.input_digest == other.input_digest
            && self.source_fingerprint == other.source_fingerprint
            && self.policy_raw_digest == other.policy_raw_digest
            && self.comparison == other.comparison
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct LegacyMigrationReport {
    pub(super) asset: String,
    pub(super) status: &'static str,
    pub(super) generation: Option<String>,
    pub(super) reason: String,
    pub(super) rerun_command: Option<&'static str>,
    #[serde(skip_serializing)]
    pub(super) proof: Option<LifecycleProof>,
}

impl LegacyMigrationReport {
    fn absent(reason: impl Into<String>) -> Self {
        Self {
            asset: LEGACY_CHECKER_PATH.into(),
            status: "absent",
            generation: None,
            reason: reason.into(),
            rerun_command: None,
            proof: None,
        }
    }
}

pub(super) fn prepare_legacy_migration(
    destination: &Path,
    staged: &mut StagedRender,
    prior_managed_paths: &BTreeSet<PathBuf>,
) -> Result<LegacyMigrationReport> {
    let relative = Path::new(LEGACY_CHECKER_PATH);
    let destination_path = destination.join(relative);
    let metadata = match fs::symlink_metadata(&destination_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            omit_fresh_legacy_asset(staged, relative)?;
            return Ok(LegacyMigrationReport::absent(
                "no legacy Bash checker is present",
            ));
        }
        Err(error) => return Err(error).context("Failed to inspect legacy Bash checker"),
    };

    let recognized = recognize_asset(destination, staged, &metadata)?;
    let Some(record) = recognized else {
        // A changed checker is authored state. Relinquish any old generated
        // ownership without deleting or replacing the destination.
        staged.active_paths.remove(relative);
        staged.retirement_paths.remove(relative);
        remove_staged_file(staged, relative)?;
        rewrite_manifest(staged)?;
        return Ok(LegacyMigrationReport {
            asset: LEGACY_CHECKER_PATH.into(),
            status: "preserved_authored",
            generation: None,
            reason: "legacy checker bytes, type, or executable metadata are not an exact recognized generation; preserved as authored state".into(),
            rerun_command: None,
            proof: None,
        });
    };

    preserve_recognized_asset(destination, staged, relative, &metadata)?;
    stage_registry(staged, &record)?;
    let proof = retirement_proof(destination, staged);
    match proof {
        Ok(proof) => {
            remove_staged_file(staged, relative)?;
            staged.active_paths.remove(relative);
            staged.retirement_paths.insert(relative.to_path_buf());
            rewrite_manifest(staged)?;
            Ok(LegacyMigrationReport {
                asset: LEGACY_CHECKER_PATH.into(),
                status: "retire",
                generation: Some(record.generation),
                reason: "repo:file-budget passes on the current source and the staged update keeps its authority".into(),
                rerun_command: None,
                proof: Some(proof),
            })
        }
        Err(reason) => {
            staged.retirement_paths.remove(relative);
            rewrite_manifest(staged)?;
            let phase_one = !prior_managed_paths.contains(relative);
            Ok(LegacyMigrationReport {
                asset: LEGACY_CHECKER_PATH.into(),
                status: if phase_one {
                    "phase_one_retained"
                } else {
                    "retained"
                },
                generation: Some(record.generation),
                reason,
                rerun_command: Some(RERUN_COMMAND),
                proof: None,
            })
        }
    }
}

pub(super) fn revalidate_lifecycle_proof(root: &Path, proof: &LifecycleProof) -> Result<()> {
    let current = native_proof(root)?;
    if !current.covers_same_source(proof) {
        bail!(
            "file-budget retirement proof changed after transaction preparation; the uncommitted update will be rolled back and the legacy checker retained"
        );
    }
    Ok(())
}

fn retirement_proof(
    destination: &Path,
    staged: &StagedRender,
) -> std::result::Result<LifecycleProof, String> {
    if staged_changes_native_authority(destination, staged)
        .map_err(|error| format!("could not compare staged authority: {error:#}"))?
    {
        return Err(
            "the staged update changes native authority or evaluated repository source; commit this update, then update again"
                .into(),
        );
    }
    let proof = native_proof(destination).map_err(|error| format!("{error:#}"))?;
    let staged_context = RepoContext::load_from_root(staged.destination.clone())
        .map_err(|error| format!("staged native authority is invalid: {error:#}"))?;
    let staged_catalog = RepositoryCatalog::from_context(&staged_context)
        .map_err(|error| format!("staged native catalog is invalid: {error:#}"))?;
    if staged_catalog.config_digest() != proof.config_digest
        || !catalog_has_generated_action(&staged_catalog)
    {
        return Err(
            "the staged update changes or replaces generated repo:file-budget authority; commit this update, then update again"
                .into(),
        );
    }
    Ok(proof)
}

fn native_proof(root: &Path) -> Result<LifecycleProof> {
    let ctx = RepoContext::load_from_root(root.to_path_buf())?;
    native_proof_with_context(&ctx)
}

/// Evaluates the generated native file budget against the current source and
/// binds the passing result to the authority it was evaluated under.
fn native_proof_with_context(ctx: &RepoContext) -> Result<LifecycleProof> {
    let root = ctx.root();
    let catalog = RepositoryCatalog::from_context(ctx)?;
    if !catalog_has_generated_action(&catalog) {
        bail!("generated repo:file-budget authority is absent or authored");
    }
    let target = file_budget_target()?;
    let action = catalog
        .action(&target)
        .context("file-budget action disappeared")?;
    let configuration = action_file_budget_configuration(action)?;
    let source = jig_repository::source_identity::repository_source_snapshot(root)?;
    let result = jig_repository::file_budget::run_direct_file_budget(
        ctx,
        None,
        configuration,
        jig_repository::file_budget::FileBudgetEvaluationMode::Check,
        Instant::now() + NATIVE_EVALUATION_DEADLINE,
        &|| false,
    )?;
    if result.conclusion != RunConclusion::Success {
        bail!("repo:file-budget does not pass on the current repository source");
    }
    if jig_repository::source_identity::repository_source_snapshot(root)?.worktree_fingerprint
        != source.worktree_fingerprint
    {
        bail!("repository source changed while repo:file-budget was evaluated");
    }

    let evidence = result
        .evidence
        .as_ref()
        .and_then(|value| value.get("file_budget"))
        .context("the repo:file-budget evaluation returned no native evidence")?;
    if evidence.get("schema").and_then(Value::as_str) != Some("jig.file_budget/evidence-v1")
        || evidence.get("complete").and_then(Value::as_bool) != Some(true)
    {
        bail!("the repo:file-budget evaluation evidence is incomplete");
    }
    let evaluation_digest = evidence
        .get("evaluation_digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_identity(digest))
        .context("the repo:file-budget evaluation digest is missing or invalid")?;
    let policy_raw_digest = evidence
        .get("policy_raw_digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_identity(digest))
        .context("the repo:file-budget policy digest is missing or invalid")?;
    let valid_until_ms = evidence.get("valid_until_ms").and_then(Value::as_u64);
    let active_waivers = evidence
        .get("active_waiver_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if active_waivers > 0 && valid_until_ms.is_none() {
        bail!("the repo:file-budget evaluation used waivers without bounded validity");
    }
    let current_policy = fs::read(root.join(FILE_BUDGET_POLICY_PATH))?;
    if format!("sha256:{}", digest(&current_policy)) != policy_raw_digest {
        bail!("the authored file-budget policy changed while it was evaluated");
    }
    Ok(LifecycleProof {
        config_digest: catalog.config_digest().into(),
        input_digest: target_input_digest(&catalog, &target, &source.worktree_fingerprint)?,
        source_fingerprint: source.worktree_fingerprint,
        policy_raw_digest: policy_raw_digest.into(),
        comparison: evidence
            .get("comparison")
            .cloned()
            .context("the repo:file-budget comparison evidence is missing")?,
        evaluation_digest: evaluation_digest.into(),
        valid_until_ms,
    })
}

fn catalog_has_generated_action(catalog: &RepositoryCatalog) -> bool {
    let Ok(target) = file_budget_target() else {
        return false;
    };
    let Ok(mut expected) = generated_file_budget_action() else {
        return false;
    };
    let Some(mut actual) = catalog.action(&target).cloned() else {
        return false;
    };
    for action in [&mut expected, &mut actual] {
        if super::repository_model::prepare_action_inputs_policy(action, catalog.contract_version())
            .is_err()
        {
            return false;
        }
    }
    actual == expected
}

fn action_file_budget_configuration(
    action: &jig_contract::ActionSpec,
) -> Result<jig_contract::NativeFileBudgetConfigV1> {
    match &action.runner {
        ActionRunner::Native {
            operation,
            configuration: Some(NativeActionConfigurationV1::FileBudget { config }),
        } if operation == tool::FILE_BUDGET => Ok(config.clone()),
        _ => bail!("repo:file-budget is not the generated configured native action"),
    }
}

fn file_budget_target() -> Result<TargetId> {
    Ok(TargetId::new(
        ComponentId::parse("repo")?,
        ActionId::parse("file-budget")?,
    ))
}

fn staged_changes_native_authority(destination: &Path, staged: &StagedRender) -> Result<bool> {
    for relative in staged.authored_seed_paths() {
        if relative == Path::new(FILE_BUDGET_POLICY_PATH) && !destination.join(relative).exists() {
            return Ok(true);
        }
    }
    for relative in staged
        .active_paths
        .iter()
        .chain(staged.retirement_paths.iter())
    {
        if relative == Path::new(LEGACY_CHECKER_PATH)
            || relative == Path::new(managed_paths::MANIFEST_PATH)
            || (relative.starts_with(".agent") && relative != Path::new(".agent/jig-contract.json"))
        {
            continue;
        }
        if !entries_match(destination, &staged.destination, relative)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn entries_match(left_root: &Path, right_root: &Path, relative: &Path) -> Result<bool> {
    let left = fs::symlink_metadata(left_root.join(relative));
    let right = fs::symlink_metadata(right_root.join(relative));
    match (left, right) {
        (Err(left), Err(right))
            if left.kind() == ErrorKind::NotFound && right.kind() == ErrorKind::NotFound =>
        {
            Ok(true)
        }
        (Ok(left), Ok(right)) if left.file_type().is_file() && right.file_type().is_file() => {
            Ok(executable(&left) == executable(&right)
                && fs::read(left_root.join(relative))? == fs::read(right_root.join(relative))?)
        }
        (Ok(left), Ok(right))
            if left.file_type().is_symlink() && right.file_type().is_symlink() =>
        {
            Ok(fs::read_link(left_root.join(relative))?
                == fs::read_link(right_root.join(relative))?)
        }
        (Err(error), _) | (_, Err(error)) if error.kind() == ErrorKind::NotFound => Ok(false),
        (Err(error), _) | (_, Err(error)) => Err(error.into()),
        _ => Ok(false),
    }
}

fn recognize_asset(
    destination: &Path,
    staged: &StagedRender,
    metadata: &fs::Metadata,
) -> Result<Option<LegacyAssetRecord>> {
    if !metadata.file_type().is_file() || !executable(metadata) {
        return Ok(None);
    }
    let bytes = fs::read(destination.join(LEGACY_CHECKER_PATH))?;
    let sha256 = digest(&bytes);
    if let Some(known) = KNOWN_LEGACY_ASSETS.iter().find(|asset| {
        asset.path == LEGACY_CHECKER_PATH
            && asset.sha256 == sha256
            && asset.executable == executable(metadata)
    }) {
        return Ok(Some(asset_record(known.generation, sha256)));
    }
    if let Some(record) = read_registry(destination)?
        .assets
        .into_iter()
        .find(|record| {
            record.path == LEGACY_CHECKER_PATH
                && record.sha256 == sha256
                && record.file_type == "regular"
                && record.executable
        })
    {
        return Ok(Some(record));
    }
    let rendered = staged.destination.join(LEGACY_CHECKER_PATH);
    if fs::read(&rendered).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(Some(asset_record("rust-loc-rendered-v1", sha256)));
    }
    Ok(None)
}

fn read_registry(root: &Path) -> Result<LegacyAssetRegistry> {
    let path = root.join(LEGACY_REGISTRY_PATH);
    match fs::read(&path) {
        Ok(bytes) => {
            let registry: LegacyAssetRegistry = serde_json::from_slice(&bytes)
                .with_context(|| format!("Invalid legacy asset registry {}", path.display()))?;
            if registry.version != REGISTRY_VERSION || registry.assets.len() > 16 {
                bail!(
                    "Unsupported or oversized legacy asset registry {}",
                    path.display()
                );
            }
            Ok(registry)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(LegacyAssetRegistry {
            version: REGISTRY_VERSION,
            assets: Vec::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

fn asset_record(generation: impl Into<String>, sha256: String) -> LegacyAssetRecord {
    LegacyAssetRecord {
        generation: generation.into(),
        path: LEGACY_CHECKER_PATH.into(),
        sha256,
        file_type: "regular".into(),
        executable: true,
    }
}

fn preserve_recognized_asset(
    destination: &Path,
    staged: &mut StagedRender,
    relative: &Path,
    metadata: &fs::Metadata,
) -> Result<()> {
    let target = staged.destination.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(destination.join(relative), &target)?;
    fs::set_permissions(&target, metadata.permissions())?;
    staged.active_paths.insert(relative.to_path_buf());
    staged.retirement_paths.remove(relative);
    Ok(())
}

fn stage_registry(staged: &mut StagedRender, record: &LegacyAssetRecord) -> Result<()> {
    let registry = LegacyAssetRegistry {
        version: REGISTRY_VERSION,
        assets: vec![record.clone()],
    };
    let mut bytes = serde_json::to_vec_pretty(&registry)?;
    bytes.push(b'\n');
    let relative = PathBuf::from(LEGACY_REGISTRY_PATH);
    let path = staged.destination.join(&relative);
    fs::create_dir_all(path.parent().context("legacy registry has no parent")?)?;
    fs::write(path, bytes)?;
    staged.active_paths.insert(relative.clone());
    staged.retirement_paths.remove(&relative);
    Ok(())
}

fn omit_fresh_legacy_asset(staged: &mut StagedRender, relative: &Path) -> Result<()> {
    staged.active_paths.remove(relative);
    staged.retirement_paths.remove(relative);
    remove_staged_file(staged, relative)?;
    rewrite_manifest(staged)
}

fn remove_staged_file(staged: &StagedRender, relative: &Path) -> Result<()> {
    match fs::remove_file(staged.destination.join(relative)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn rewrite_manifest(staged: &StagedRender) -> Result<()> {
    managed_paths::write_manifest(&staged.destination, &staged.active_paths)
}

fn valid_sha256_identity(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests;
