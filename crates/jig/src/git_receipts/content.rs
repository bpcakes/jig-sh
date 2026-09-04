use super::*;

/// Reads one optional index blob through the same bounded, scrubbed Git process
/// used by repository evidence. The caller supplies a validated
/// repository-relative path and receives complete bytes, absence, or an error.
pub(crate) fn read_index_blob_v1(root: &Path, path: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    let listing = git_bounded_proof_stdout(
        root,
        &["--literal-pathspecs", "ls-files", "-z", "--", path],
        "git list optional index blob",
        path.len().saturating_mul(4).saturating_add(4),
        "index-blob-presence",
        GitReceiptCollection::Blocking,
    )?;
    if listing.is_empty() {
        return Ok(None);
    }
    let mut expected = path.as_bytes().to_vec();
    expected.push(0);
    if listing != expected {
        bail!("index path `{path}` did not resolve to one stage-zero entry");
    }
    let object = format!(":{path}");
    git_bounded_proof_stdout(
        root,
        &["--no-replace-objects", "cat-file", "blob", &object],
        "git cat-file index blob",
        limit,
        "index-blob",
        GitReceiptCollection::Blocking,
    )
    .map(Some)
}

/// Reads an authenticated blob object through the bounded, scrubbed Git
/// process used by repository evidence. The caller supplies an object ID
/// obtained from a resolved comparison or scope entry.
pub(crate) fn read_git_blob_v1_with_cancellation(
    root: &Path,
    oid: &str,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout(
        root,
        &["--no-replace-objects", "cat-file", "blob", oid],
        "git cat-file authenticated blob",
        limit,
        "file-budget-blob",
        GitReceiptCollection::Cancellable(cancelled),
    )
}

/// Reads one optional regular-file blob from an exact tree. Absence is
/// distinct from an unsupported tree entry and from an incomplete Git probe.
pub(crate) fn read_tree_path_blob_v1_with_cancellation(
    root: &Path,
    tree_oid: &str,
    path: &str,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Vec<u8>>> {
    let Some(oid) =
        resolve_tree_path_blob_oid_v1_with_cancellation(root, tree_oid, path, cancelled)?
    else {
        return Ok(None);
    };
    read_git_blob_v1_with_cancellation(root, &oid, limit, cancelled).map(Some)
}

pub(crate) fn resolve_tree_path_blob_oid_v1_with_cancellation(
    root: &Path,
    tree_oid: &str,
    path: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    let listing_limit = path.len().saturating_mul(4).saturating_add(256);
    let listing = git_bounded_proof_stdout(
        root,
        &[
            "--no-replace-objects",
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "--full-tree",
            tree_oid,
            "--",
            path,
        ],
        "git list optional exact-tree blob",
        listing_limit,
        "file-budget-tree-path",
        GitReceiptCollection::Cancellable(cancelled),
    )?;
    if listing.is_empty() {
        return Ok(None);
    }
    if !listing.ends_with(&[0]) || listing[..listing.len() - 1].contains(&0) {
        bail!("exact tree path `{path}` did not resolve to one entry");
    }
    let record = &listing[..listing.len() - 1];
    let separator = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("malformed exact-tree path entry"))?;
    let metadata = &record[..separator];
    let listed_path = &record[separator + 1..];
    if listed_path != path.as_bytes() {
        bail!("exact tree path query returned an unexpected path");
    }
    let metadata = std::str::from_utf8(metadata).context("exact-tree metadata was not UTF-8")?;
    let mut fields = metadata.split(' ');
    let mode = fields.next().unwrap_or_default();
    let kind = fields.next().unwrap_or_default();
    let oid = fields.next().unwrap_or_default();
    if fields.next().is_some() || !matches!(mode, "100644" | "100755") || kind != "blob" {
        bail!("exact tree path `{path}` is not a regular file");
    }
    Ok(Some(oid.to_owned()))
}

pub(crate) fn resolve_index_blob_oid_v1_with_cancellation(
    root: &Path,
    path: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    let limit = path.len().saturating_mul(4).saturating_add(256);
    let output = git_bounded_proof_stdout(
        root,
        &[
            "--no-replace-objects",
            "--literal-pathspecs",
            "ls-files",
            "--stage",
            "-z",
            "--",
            path,
        ],
        "git resolve optional index blob",
        limit,
        "file-budget-index-path",
        GitReceiptCollection::Cancellable(cancelled),
    )?;
    if output.is_empty() {
        return Ok(None);
    }
    if !output.ends_with(&[0]) || output[..output.len() - 1].contains(&0) {
        bail!("index path `{path}` did not resolve to one entry");
    }
    let record = &output[..output.len() - 1];
    let separator = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("malformed index path entry"))?;
    if &record[separator + 1..] != path.as_bytes() {
        bail!("index path query returned an unexpected path");
    }
    let metadata =
        std::str::from_utf8(&record[..separator]).context("index path metadata was not UTF-8")?;
    let mut fields = metadata.split(' ');
    let mode = fields.next().unwrap_or_default();
    let oid = fields.next().unwrap_or_default();
    let stage = fields.next().unwrap_or_default();
    if fields.next().is_some() || !matches!(mode, "100644" | "100755") || stage != "0" {
        bail!("index path `{path}` is not one regular stage-zero file");
    }
    Ok(Some(oid.to_owned()))
}
