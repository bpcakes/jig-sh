use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[allow(dead_code)]
pub(crate) const BEADS_TRACKER_ROOT: &str = ".beads";
const MAX_MANUAL_EXPORT_GUIDANCE_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WorkTrackerConfig {
    Beads {
        workspace_id: String,
        #[serde(default)]
        export: WorkTrackerExport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manual_export_guidance: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkTrackerExport {
    #[default]
    Manual,
}

#[allow(dead_code)]
impl WorkTrackerConfig {
    pub(super) fn validate(&self) -> Result<()> {
        let Self::Beads {
            workspace_id,
            manual_export_guidance,
            ..
        } = self;
        validate_workspace_id(workspace_id)?;
        if let Some(guidance) = manual_export_guidance {
            validate_manual_export_guidance(guidance)?;
        }
        Ok(())
    }

    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Beads { .. } => "beads",
        }
    }

    pub(crate) fn workspace_id(&self) -> &str {
        match self {
            Self::Beads { workspace_id, .. } => workspace_id,
        }
    }

    pub(crate) const fn root(&self) -> &'static str {
        match self {
            Self::Beads { .. } => BEADS_TRACKER_ROOT,
        }
    }

    pub(crate) const fn export(&self) -> WorkTrackerExport {
        match self {
            Self::Beads { export, .. } => *export,
        }
    }

    pub(crate) fn manual_export_guidance(&self) -> Option<&str> {
        match self {
            Self::Beads {
                manual_export_guidance,
                ..
            } => manual_export_guidance.as_deref(),
        }
    }
}

fn validate_workspace_id(workspace_id: &str) -> Result<()> {
    let parsed = workspace_id.parse::<Ulid>().map_err(|_| {
        anyhow::anyhow!("work tracker workspace_id must be a canonical uppercase 26-character ULID")
    })?;
    if workspace_id.len() != 26 || parsed.to_string() != workspace_id {
        bail!("work tracker workspace_id must be a canonical uppercase 26-character ULID");
    }
    Ok(())
}

fn validate_manual_export_guidance(guidance: &str) -> Result<()> {
    if guidance.trim().is_empty() || guidance.len() > MAX_MANUAL_EXPORT_GUIDANCE_BYTES {
        bail!(
            "work tracker manual_export_guidance must be nonblank and contain at most {MAX_MANUAL_EXPORT_GUIDANCE_BYTES} bytes"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::context::WorkConfig;

    const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn parse(source: &str) -> WorkConfig {
        toml::from_str(source).unwrap()
    }

    #[test]
    fn absent_tracker_keeps_the_legacy_authority_shape() {
        let config = parse("");

        config.validate().unwrap();
        assert!(config.tracker().is_none());
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({"checks": [], "gates": [], "refinements": []})
        );
    }

    #[test]
    fn beads_tracker_defaults_and_canonicalizes_manual_export() {
        let implicit = parse(&format!(
            "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\n"
        ));
        let explicit = parse(&format!(
            "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nexport = \"manual\"\n"
        ));

        implicit.validate().unwrap();
        explicit.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&implicit).unwrap(),
            serde_json::to_value(&explicit).unwrap()
        );
        let tracker = implicit.tracker().unwrap();
        assert_eq!(tracker.kind(), "beads");
        assert_eq!(tracker.workspace_id(), WORKSPACE_ID);
        assert_eq!(tracker.root(), ".beads");
        assert_eq!(tracker.export(), WorkTrackerExport::Manual);
        assert_eq!(tracker.manual_export_guidance(), None);
    }

    #[test]
    fn beads_tracker_accepts_bounded_nonblank_display_guidance() {
        let config = parse(&format!(
            "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nmanual_export_guidance = \"Run the repository export helper.\"\n"
        ));

        config.validate().unwrap();
        assert_eq!(
            config.tracker().unwrap().manual_export_guidance(),
            Some("Run the repository export helper.")
        );
    }

    #[test]
    fn tracker_schema_rejects_unknown_kinds_exports_and_fields() {
        for source in [
            format!("[tracker]\nkind = \"other\"\nworkspace_id = \"{WORKSPACE_ID}\"\n"),
            format!(
                "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nexport = \"automatic\"\n"
            ),
            format!(
                "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nroot = \".beads\"\n"
            ),
        ] {
            assert!(toml::from_str::<WorkConfig>(&source).is_err(), "{source}");
        }
    }

    #[test]
    fn tracker_validation_requires_a_canonical_uppercase_ulid() {
        for workspace_id in [
            "",
            "01ARZ3NDEKTSV4RRFFQ69G5FA",
            "01arz3ndektsv4rrffq69g5fav",
            "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        ] {
            let config = parse(&format!(
                "[tracker]\nkind = \"beads\"\nworkspace_id = \"{workspace_id}\"\n"
            ));
            let error = config.validate().unwrap_err().to_string();
            assert!(error.contains("canonical uppercase 26-character ULID"));
        }
    }

    #[test]
    fn tracker_validation_rejects_blank_or_oversized_guidance() {
        for guidance in [" \t ".to_string(), "x".repeat(4097)] {
            let source = format!(
                "[tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\nmanual_export_guidance = {}\n",
                toml::Value::String(guidance)
            );
            let error = parse(&source).validate().unwrap_err().to_string();
            assert!(error.contains("manual_export_guidance"));
        }
    }
}
