use super::*;

impl InitMutationTransaction {
    pub(in crate::bootstrap) fn commit(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let staged_boundary = self
            .staged_publication
            .as_ref()
            .map(|_| verify_tracked_init_directories(&self.directory_identities));
        if let Some(publication) = self.staged_publication.as_mut() {
            let staging_root = publication
                .staging_root
                .take()
                .context("Private init staging root was already consumed")?;
            let publish_source = staging_root.path().to_path_buf();
            debug_assert_eq!(publish_source, publication.publish_source);
            if let Some(Err(error)) = staged_boundary {
                let preserved = staging_root.keep();
                return Err(anyhow::anyhow!(
                    "Private init work tree changed before publication: {error:#}. Preserving the complete staging tree at {}",
                    preserved.display()
                ));
            }
            let publish_parent = publication
                .publish_destination
                .parent()
                .context("Init publication destination has no parent")?;
            let parent_identity = match path::repository_path_identity(publish_parent) {
                Ok(identity) => identity,
                Err(error) => {
                    let primary = anyhow::anyhow!(
                        "Failed to verify init publication parent {}: {error:#}",
                        publish_parent.display()
                    );
                    return Err(close_failed_staging(
                        staging_root,
                        &publication.publish_source_identity,
                        primary,
                    ));
                }
            };
            if parent_identity != publication.publish_parent_identity.identity {
                let primary = anyhow::anyhow!(
                    "Init publication parent changed concurrently: {}",
                    publish_parent.display()
                );
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            let source_identity = match path::repository_path_identity(&publish_source) {
                Ok(identity) => identity,
                Err(error) => {
                    let preserved = staging_root.keep();
                    return Err(anyhow::anyhow!(
                        "Failed to verify private init staging root {}: {error:#}. Preserving unverified staging path {}",
                        publish_source.display(),
                        preserved.display()
                    ));
                }
            };
            if source_identity != publication.publish_source_identity.identity {
                let primary = anyhow::anyhow!(
                    "Private init staging root changed concurrently: {}",
                    publish_source.display()
                );
                // The path is no longer proven to be ours. Disarm recursive
                // cleanup and surface the recovery path rather than deleting a
                // foreign replacement.
                let preserved = staging_root.keep();
                return Err(anyhow::anyhow!(
                    "{primary:#}\nPreserving unverified staging path {}",
                    preserved.display()
                ));
            }
            if let Err(error) =
                fs::set_permissions(&publish_source, publication.publish_permissions.clone())
            {
                let primary = anyhow::Error::new(error).context(format!(
                    "Failed to apply final directory permissions before publishing {}",
                    publication.publish_destination.display()
                ));
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            let post_permissions_identity = (|| -> Result<bool> {
                Ok(path::repository_directory_commit_matches_path(
                    &publication.publish_parent_identity,
                    publish_parent,
                )? && path::repository_directory_commit_matches_path(
                    &publication.publish_source_identity,
                    &publish_source,
                )? && verify_tracked_init_directories(&self.directory_identities).is_ok())
            })();
            if !matches!(post_permissions_identity, Ok(true)) {
                let primary = anyhow::anyhow!(
                    "Init publication boundary changed after final permissions were applied; refusing to publish {}{}",
                    publication.publish_destination.display(),
                    post_permissions_identity
                        .err()
                        .map(|error| format!(": {error:#}"))
                        .unwrap_or_default()
                );
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            if let Err(primary) =
                path::rename_entry_noreplace(&publish_source, &publication.publish_destination)
            {
                let primary = anyhow::Error::new(primary).context(format!(
                    "Failed to publish initialized repository without replacing concurrent path {}",
                    publication.publish_destination.display()
                ));
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            self.armed = false;
            // Disarm TempDir cleanup after the rename. A watcher could recreate
            // the now-missing random source name before Drop; `keep` guarantees
            // Jig never recursively removes that foreign replacement.
            let _published_source_name = staging_root.keep();
            return Ok(());
        }

        self.verify_destination_identity()?;
        let mut cleanup_failures = Vec::new();
        for (relative, mutation) in &self.files {
            if let Err(error) = self.verify_destination_identity() {
                cleanup_failures.push(format!(
                    "stopped retained-preimage cleanup before {}: {error:#}",
                    relative.display()
                ));
                break;
            }
            let Some(preimage) = mutation.original_quarantine.as_ref() else {
                continue;
            };
            let snapshot = match self.snapshot_absolute_path(preimage) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    cleanup_failures.push(format!(
                        "{}: retained preimage {} could not be inspected: {error:#}",
                        relative.display(),
                        preimage.display()
                    ));
                    continue;
                }
            };
            if !init_snapshots_match(&snapshot, &mutation.before) {
                cleanup_failures.push(format!(
                    "{}: retained preimage changed; preserving recovery artifact {}",
                    relative.display(),
                    preimage.display()
                ));
                continue;
            }
            if let Err(error) = self.dispose_snapshot_leaf(relative, preimage, &snapshot) {
                cleanup_failures.push(format!(
                    "{}: failed to remove retained preimage {}: {error:#}",
                    relative.display(),
                    preimage.display()
                ));
            }
        }
        if let Err(error) = self.close_write_staging() {
            cleanup_failures.push(format!("private write staging cleanup failed: {error:#}"));
        }
        self.armed = false;
        if !cleanup_failures.is_empty() {
            bail!(
                "Initialized repository was committed, but cleanup was incomplete:\n{}",
                cleanup_failures.join("\n")
            );
        }
        Ok(())
    }
}
