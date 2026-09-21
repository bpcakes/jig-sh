use super::*;

/// Preserve the work authority's wire shape while excluding display-only text.
#[derive(Serialize)]
pub(in crate::context) struct WorkExecutionAuthority<'a> {
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    receipt_metadata: &'a [ReceiptMetadata],
    #[serde(skip_serializing_if = "Option::is_none")]
    tracker: Option<TrackerExecutionAuthority<'a>>,
    checks: &'a [String],
    gates: &'a [WorkGateConfig],
    #[serde(skip_serializing_if = "Option::is_none")]
    iteration_profile: Option<&'a ProfileId>,
    refinements: &'a [WorkRefinementConfig],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TrackerExecutionAuthority<'a> {
    Beads {
        workspace_id: &'a str,
        export: WorkTrackerExport,
    },
}

impl WorkConfig {
    pub(in crate::context) fn execution_authority(&self) -> WorkExecutionAuthority<'_> {
        // Exhaustive patterns require new config fields to be classified here.
        let Self {
            receipt_metadata,
            tracker,
            checks,
            gates,
            iteration_profile,
            refinements,
        } = self;
        WorkExecutionAuthority {
            receipt_metadata,
            tracker: tracker.as_ref().map(|config| match config {
                WorkTrackerConfig::Beads {
                    workspace_id,
                    export,
                    manual_export_guidance: _,
                } => TrackerExecutionAuthority::Beads {
                    workspace_id,
                    export: *export,
                },
            }),
            checks,
            gates,
            iteration_profile: iteration_profile.as_ref(),
            refinements,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_authority_preserves_existing_serialization_without_display_guidance() {
        for source in [
            "",
            "iteration_profile = \"iteration\"",
            "receipt_metadata = [\"beads\"]\nchecks = [\"jig.test\"]",
            "[tracker]\nkind = \"beads\"\nworkspace_id = \"01ARZ3NDEKTSV4RRFFQ69G5FAV\"\nexport = \"manual\"",
        ] {
            let config: WorkConfig = toml::from_str(source).unwrap();
            // Byte equality matters: the contract hashes the serialized bytes.
            assert_eq!(
                serde_json::to_vec(&config.execution_authority()).unwrap(),
                serde_json::to_vec(&config).unwrap()
            );
        }
    }
}
