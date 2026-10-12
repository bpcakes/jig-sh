use super::*;

pub(super) fn serialize_harness_footprint<S>(
    value: &HarnessFootprint,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value.as_str())
}

pub(super) fn default_codex_marketplaces() -> Vec<CodexMarketplaceAnswers> {
    vec![CodexMarketplaceAnswers {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

pub(super) fn merge_option<T>(target: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

pub(super) fn default_repo_name(destination: &Path) -> Option<String> {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}
