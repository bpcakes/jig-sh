pub(super) struct EmbeddedScaffoldTemplateFile {
    pub(super) relative_path: &'static str,
    pub(super) contents: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_scaffold_templates.rs"));

#[cfg(test)]
mod snapshot {
    use super::EmbeddedScaffoldTemplateFile;

    include!("embedded_templates_snapshot.rs");
}

#[cfg(test)]
pub(super) fn snapshot_scaffold_template_files() -> &'static [EmbeddedScaffoldTemplateFile] {
    snapshot::EMBEDDED_SCAFFOLD_TEMPLATE_FILES
}

#[cfg(test)]
mod tests {
    use super::{
        EMBEDDED_SCAFFOLD_TEMPLATE_FILES, EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT, snapshot,
    };

    const REFRESH_COMMAND: &str = "JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh";

    #[test]
    fn embedded_scaffold_template_snapshot_matches_live_templates() {
        if EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT {
            panic!(
                "embedded scaffold templates were compiled from the snapshot; unset JIG_EMBEDDED_TEMPLATE_SNAPSHOT and rebuild before running the snapshot drift test"
            );
        }
        assert_eq!(
            EMBEDDED_SCAFFOLD_TEMPLATE_FILES.len(),
            snapshot::EMBEDDED_SCAFFOLD_TEMPLATE_FILES.len(),
            "embedded scaffold template snapshot file count is stale; run {REFRESH_COMMAND}"
        );

        for (live, snapshotted) in EMBEDDED_SCAFFOLD_TEMPLATE_FILES
            .iter()
            .zip(snapshot::EMBEDDED_SCAFFOLD_TEMPLATE_FILES)
        {
            assert_eq!(
                live.relative_path, snapshotted.relative_path,
                "embedded scaffold template snapshot paths are stale; run {REFRESH_COMMAND}"
            );
            assert_eq!(
                live.contents, snapshotted.contents,
                "embedded scaffold template snapshot contents are stale for {}; run {REFRESH_COMMAND}",
                live.relative_path
            );
        }
    }
}
