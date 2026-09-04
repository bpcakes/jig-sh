use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionId, ActionRunner, ComparisonRequestV1, ComponentId, NativeActionConfigurationV1,
    TargetId, tool,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::managed_paths;
use super::repository_model::generated_file_budget_action;
use super::staged_render::{FILE_BUDGET_POLICY_PATH, StagedRender};
use crate::context::RepoContext;
use crate::repository::{RepositoryCatalog, target_input_digest};

pub(super) const LEGACY_CHECKER_PATH: &str = "scripts/check-rust-file-loc.sh";
const LEGACY_REGISTRY_PATH: &str = ".agent/jig-legacy-assets.json";
const RERUN_COMMAND: &str = "scripts/jig check repo:file-budget";
const REGISTRY_VERSION: u32 = 1;

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LifecycleProof {
    pub(super) receipt_id: String,
    pub(super) config_digest: String,
    pub(super) input_digest: String,
    pub(super) source_fingerprint: String,
    pub(super) policy_raw_digest: String,
    pub(super) comparison: Value,
    pub(super) evaluation_digest: String,
    pub(super) valid_until_ms: Option<u64>,
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
                reason: "latest successful native receipt matches current and staged post-update authority".into(),
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
    let current = validate_receipt_proof(root)?;
    if &current != proof {
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
    if staged_changes_receipt_authority(destination, staged)
        .map_err(|error| format!("could not compare staged authority: {error:#}"))?
    {
        return Err(
            "the staged update changes native authority or evaluated repository source; commit this update, run the native action, then update again"
                .into(),
        );
    }
    let proof = validate_receipt_proof(destination).map_err(|error| format!("{error:#}"))?;
    let staged_context = RepoContext::load_from_root(staged.destination.clone())
        .map_err(|error| format!("staged native authority is invalid: {error:#}"))?;
    let staged_catalog = RepositoryCatalog::from_context(&staged_context)
        .map_err(|error| format!("staged native catalog is invalid: {error:#}"))?;
    if staged_catalog.config_digest() != proof.config_digest
        || !catalog_has_generated_action(&staged_catalog)
    {
        return Err(
            "the staged update changes or replaces generated repo:file-budget authority; a fresh native receipt is required"
                .into(),
        );
    }
    Ok(proof)
}

fn validate_receipt_proof(root: &Path) -> Result<LifecycleProof> {
    let ctx = RepoContext::load_from_root(root.to_path_buf())?;
    let catalog = RepositoryCatalog::from_context(&ctx)?;
    if !catalog_has_generated_action(&catalog) {
        bail!("generated repo:file-budget authority is absent or authored");
    }
    let receipt = crate::state::latest_file_budget_lifecycle_receipt(&ctx)?
        .context("no repo:file-budget receipt exists")?;
    if receipt.exit_status != 0 {
        bail!("the latest repo:file-budget receipt did not succeed");
    }
    if receipt.worktree_fingerprint_error.is_some() {
        bail!("the latest repo:file-budget receipt could not attest repository source");
    }
    let source = crate::git_receipts::repository_source_snapshot(root)?;
    if receipt.worktree_fingerprint.as_deref() != Some(&source.worktree_fingerprint) {
        bail!("the latest repo:file-budget receipt is stale for current repository source");
    }
    let target = file_budget_target()?;
    let expected_input = target_input_digest(&catalog, &target, &source.worktree_fingerprint)?;
    if receipt.input_digest.as_deref() != Some(&expected_input)
        || receipt.config_digest.as_deref() != Some(catalog.config_digest())
    {
        bail!("the latest repo:file-budget receipt is bound to different action authority");
    }

    let evidence = receipt
        .evidence
        .as_ref()
        .and_then(|value| value.get("file_budget"))
        .context("the latest repo:file-budget receipt has no native evidence")?;
    if evidence.get("schema").and_then(Value::as_str) != Some("jig.file_budget/evidence-v1")
        || evidence.get("complete").and_then(Value::as_bool) != Some(true)
    {
        bail!("the latest repo:file-budget receipt evidence is incomplete");
    }
    let evaluation_digest = evidence
        .get("evaluation_digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_identity(digest))
        .context("the latest repo:file-budget evaluation digest is missing or invalid")?;
    let policy_raw_digest = evidence
        .get("policy_raw_digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_identity(digest))
        .context("the latest repo:file-budget policy digest is missing or invalid")?;
    let evaluated_at_ms = evidence
        .get("evaluated_at_ms")
        .and_then(Value::as_u64)
        .context("the latest repo:file-budget evaluation time is missing")?;
    if receipt.evaluated_at_ms != Some(evaluated_at_ms) {
        bail!("the latest repo:file-budget receipt and evidence evaluation times differ");
    }
    let valid_until_ms = evidence.get("valid_until_ms").and_then(Value::as_u64);
    if receipt.valid_until_ms != valid_until_ms {
        bail!("the latest repo:file-budget receipt and evidence validity differ");
    }
    let active_waivers = evidence
        .get("active_waiver_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if active_waivers > 0 && valid_until_ms.is_none() {
        bail!("the latest repo:file-budget receipt used waivers without bounded validity");
    }
    if valid_until_ms.is_some_and(|deadline| crate::state::now_ms() > deadline) {
        bail!("the latest repo:file-budget receipt has expired");
    }

    let request: ComparisonRequestV1 = serde_json::from_value(
        evidence
            .get("request")
            .cloned()
            .context("receipt request is missing")?,
    )?;
    let action = catalog
        .action(&target)
        .context("file-budget action disappeared")?;
    let configuration = action_file_budget_configuration(action)?;
    let prepared =
        crate::repository::prepare_file_budget_input_v1(&ctx, Some(request), configuration, None)?;
    for (field, current) in [
        (
            "policy_preparation",
            serde_json::to_value(&prepared.policy)?,
        ),
        (
            "comparison_preparation",
            serde_json::to_value(&prepared.comparison)?,
        ),
        ("request", serde_json::to_value(&prepared.request)?),
        ("view", serde_json::to_value(prepared.view)?),
        (
            "configuration",
            serde_json::to_value(&prepared.configuration)?,
        ),
    ] {
        if evidence.get(field) != Some(&current) {
            bail!("the latest repo:file-budget receipt has stale {field}");
        }
    }
    let current_policy = fs::read(root.join(FILE_BUDGET_POLICY_PATH))?;
    if format!("sha256:{}", digest(&current_policy)) != policy_raw_digest {
        bail!("the authored file-budget policy changed after the latest receipt");
    }
    Ok(LifecycleProof {
        receipt_id: receipt.receipt_id,
        config_digest: catalog.config_digest().into(),
        input_digest: expected_input,
        source_fingerprint: source.worktree_fingerprint,
        policy_raw_digest: policy_raw_digest.into(),
        comparison: evidence
            .get("comparison")
            .cloned()
            .context("the latest repo:file-budget comparison evidence is missing")?,
        evaluation_digest: evaluation_digest.into(),
        valid_until_ms,
    })
}

fn catalog_has_generated_action(catalog: &RepositoryCatalog) -> bool {
    let Ok(target) = file_budget_target() else {
        return false;
    };
    let Ok(expected) = generated_file_budget_action() else {
        return false;
    };
    catalog.action(&target) == Some(&expected)
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

fn staged_changes_receipt_authority(destination: &Path, staged: &StagedRender) -> Result<bool> {
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
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    fn write_executable(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn staged_checker(bytes: &[u8]) -> StagedRender {
        let root = tempdir().unwrap();
        let destination = root.path().join("render");
        write_executable(&destination.join(LEGACY_CHECKER_PATH), bytes);
        let active_paths = BTreeSet::from([
            PathBuf::from(LEGACY_CHECKER_PATH),
            PathBuf::from(managed_paths::MANIFEST_PATH),
        ]);
        managed_paths::write_manifest(&destination, &active_paths).unwrap();
        StagedRender {
            _root: root,
            destination,
            active_paths,
            retirement_paths: BTreeSet::new(),
        }
    }

    fn destination_with_checker(bytes: &[u8]) -> TempDir {
        let root = tempdir().unwrap();
        write_executable(&root.path().join(LEGACY_CHECKER_PATH), bytes);
        root
    }

    #[test]
    fn durable_generation_table_contains_only_bounded_identities() {
        assert!(!KNOWN_LEGACY_ASSETS.is_empty());
        assert!(KNOWN_LEGACY_ASSETS.len() <= 16);
        for asset in KNOWN_LEGACY_ASSETS {
            assert_eq!(asset.path, LEGACY_CHECKER_PATH);
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(asset.executable);
        }
        let serialized = serde_json::to_string(&KNOWN_LEGACY_ASSETS.len()).unwrap();
        assert!(!serialized.contains("#!/"));
    }

    #[test]
    fn durable_table_retains_the_last_published_source_identity() {
        let asset = KNOWN_LEGACY_ASSETS
            .iter()
            .find(|asset| asset.generation == "rust-loc-v5-source")
            .expect("last published source generation");
        assert_eq!(
            asset.sha256,
            "56fc9fe067912c47aa939f9f0044a34111b9361f3ef9e3bb47274e17cd735b8c"
        );
    }

    #[test]
    fn recognized_checker_is_retained_in_phase_one_and_registered_without_source_copy() {
        let destination = destination_with_checker(b"generated checker\n");
        let mut staged = staged_checker(b"generated checker\n");

        let report =
            prepare_legacy_migration(destination.path(), &mut staged, &BTreeSet::new()).unwrap();

        assert_eq!(report.status, "phase_one_retained");
        assert_eq!(report.generation.as_deref(), Some("rust-loc-rendered-v1"));
        assert_eq!(report.rerun_command, Some(RERUN_COMMAND));
        assert_eq!(
            fs::read(staged.destination.join(LEGACY_CHECKER_PATH)).unwrap(),
            b"generated checker\n"
        );
        assert!(staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
        assert!(
            staged
                .active_paths
                .contains(Path::new(LEGACY_REGISTRY_PATH))
        );
        let registry = read_registry(&staged.destination).unwrap();
        assert_eq!(registry.assets.len(), 1);
        assert_eq!(registry.assets[0].sha256, digest(b"generated checker\n"));
    }

    #[test]
    fn modified_checker_is_preserved_as_authored_and_deowned() {
        let destination = destination_with_checker(b"authored checker\n");
        let mut staged = staged_checker(b"generated checker\n");
        let prior = BTreeSet::from([PathBuf::from(LEGACY_CHECKER_PATH)]);

        let report = prepare_legacy_migration(destination.path(), &mut staged, &prior).unwrap();

        assert_eq!(report.status, "preserved_authored");
        assert!(!staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
        assert!(
            !staged
                .retirement_paths
                .contains(Path::new(LEGACY_CHECKER_PATH))
        );
        assert!(!staged.destination.join(LEGACY_CHECKER_PATH).exists());
        assert_eq!(
            fs::read(destination.path().join(LEGACY_CHECKER_PATH)).unwrap(),
            b"authored checker\n"
        );
    }

    #[test]
    fn fresh_repository_omits_the_bash_checker_and_its_managed_ownership() {
        let destination = tempdir().unwrap();
        let mut staged = staged_checker(b"generated checker\n");

        let report =
            prepare_legacy_migration(destination.path(), &mut staged, &BTreeSet::new()).unwrap();

        assert_eq!(report.status, "absent");
        assert!(!staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
        assert!(!staged.destination.join(LEGACY_CHECKER_PATH).exists());
        let managed = managed_paths::load_manifest(&staged.destination)
            .unwrap()
            .unwrap();
        assert!(!managed.contains(Path::new(LEGACY_CHECKER_PATH)));
    }
}
