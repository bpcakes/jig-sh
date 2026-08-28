pub(super) struct EmbeddedTemplateFile {
    pub(super) relative_path: &'static str,
    pub(super) contents: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_templates.rs"));

#[cfg(test)]
mod snapshot {
    use super::EmbeddedTemplateFile;

    include!("embedded_templates_snapshot.rs");
}

#[cfg(test)]
mod tests {
    use super::{EMBEDDED_TEMPLATE_FILES, EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT, snapshot};

    const REFRESH_COMMAND: &str = "JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh";

    #[test]
    fn embedded_template_snapshot_matches_live_templates() {
        if EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT {
            panic!(
                "embedded templates were compiled from the snapshot; unset JIG_EMBEDDED_TEMPLATE_SNAPSHOT and rebuild before running the snapshot drift test"
            );
        }
        assert_eq!(
            EMBEDDED_TEMPLATE_FILES.len(),
            snapshot::EMBEDDED_TEMPLATE_FILES.len(),
            "embedded template snapshot file count is stale; run {REFRESH_COMMAND}"
        );

        for (live, snapshotted) in EMBEDDED_TEMPLATE_FILES
            .iter()
            .zip(snapshot::EMBEDDED_TEMPLATE_FILES)
        {
            assert_eq!(
                live.relative_path, snapshotted.relative_path,
                "embedded template snapshot paths are stale; run {REFRESH_COMMAND}"
            );
            assert_eq!(
                live.contents, snapshotted.contents,
                "embedded template snapshot contents are stale for {}; run {REFRESH_COMMAND}",
                live.relative_path
            );
        }
    }

    #[test]
    fn legacy_go_postgres_template_renders_the_project_sqlc_command() {
        let config = EMBEDDED_TEMPLATE_FILES
            .iter()
            .find(|template| template.relative_path == ".jig.toml.jinja")
            .unwrap();

        assert!(config.contents.contains(
            r#"sqlc_check_command = "<<[ sqlc_check_command | replace("\\", "\\\\") | replace("\"", "\\\"") ]>>""#
        ));
        assert!(
            !config
                .contents
                .contains("sqlc_check_command = \"go tool sqlc vet && go tool sqlc diff\"")
        );
    }
}
