use super::*;

/// Keep this separate from affected-selection ignores: documentation and other
/// non-code inputs remain freshness authority even when they do not select a
/// command. Tracker exclusion requires an explicit typed repository opt-in.
pub(super) fn receipt_metadata_paths(root: &Path) -> Result<Vec<&'static str>> {
    let contents = match fs::read_to_string(root.join(".jig.toml")) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("Failed to read receipt metadata configuration"),
    };
    #[derive(serde::Deserialize)]
    struct Configuration {
        #[serde(default)]
        work: crate::context::WorkConfig,
    }
    let configuration: Configuration =
        toml::from_str(&contents).context("Failed to parse receipt metadata configuration")?;
    Ok(configuration.work.receipt_metadata_paths())
}

pub(super) fn worktree_source_pathspecs(root: &Path) -> Result<Vec<String>> {
    let mut paths = vec![".".to_owned(), ":(exclude).agent/**".to_owned()];
    for metadata in receipt_metadata_paths(root)? {
        paths.push(format!(":(top,exclude,literal){metadata}"));
    }
    Ok(paths)
}

pub(super) fn committed_source_tree_without_agent_state(
    tree: &[u8],
    metadata_paths: &[&str],
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    let mut source_tree = Vec::with_capacity(tree.len());
    for record in tree
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        collection.ensure_active()?;
        let path_offset = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("Git ls-tree record is missing its path separator")?
            + 1;
        let path = &record[path_offset..];
        if path == b".agent"
            || path.starts_with(b".agent/")
            || metadata_paths
                .iter()
                .any(|metadata| path == metadata.as_bytes())
        {
            continue;
        }
        source_tree.extend_from_slice(record);
        source_tree.push(0);
    }
    Ok(source_tree)
}
