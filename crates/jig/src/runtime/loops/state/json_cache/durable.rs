#[derive(Debug)]
struct DurableJsonCommitMayHaveLanded {
    path: String,
    source: anyhow::Error,
}

impl std::fmt::Display for DurableJsonCommitMayHaveLanded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Loop state file {} was replaced, but its durable publication is unconfirmed: {}",
            self.path, self.source
        )
    }
}

impl std::error::Error for DurableJsonCommitMayHaveLanded {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
fn durable_json_commit_may_have_landed(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<DurableJsonCommitMayHaveLanded>()
            .is_some()
    })
}

fn publish_durable_json(
    data_path: &Path,
    write_and_sync_temp: impl FnOnce() -> Result<()>,
    replace: impl FnOnce() -> Result<()>,
    sync_publication: impl FnOnce() -> Result<()>,
) -> Result<()> {
    write_and_sync_temp()?;
    replace()?;
    sync_publication().map_err(|source| {
        anyhow::Error::new(DurableJsonCommitMayHaveLanded {
            path: data_path.display().to_string(),
            source,
        })
    })
}

impl StateDirectory {
    #[cfg(unix)]
    fn sync_durable_json_publication(&self, _data_name: &OsStr, data_path: &Path) -> Result<()> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No);
        self.directory
            .open_with(".", &options)
            .and_then(|directory| directory.sync_all())
            .with_context(|| {
                format!(
                    "Failed to sync loop state directory {}",
                    data_path.parent().unwrap_or(data_path).display()
                )
            })
    }

    #[cfg(not(unix))]
    fn sync_durable_json_publication(&self, data_name: &OsStr, data_path: &Path) -> Result<()> {
        open_regular_file(&self.directory, data_name, true, false, false, data_path)?
            .sync_all()
            .with_context(|| {
                format!(
                    "Failed to sync replaced loop state file {}",
                    data_path.display()
                )
            })
    }
}
