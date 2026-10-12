use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as JsonValue, json};

#[derive(Clone, Debug)]
pub(super) struct DetectedTool {
    pub(super) name: String,
    pub(super) sources: Vec<String>,
}

pub(super) fn tools_from_map(tools: BTreeMap<String, BTreeSet<String>>) -> Vec<DetectedTool> {
    tools
        .into_iter()
        .map(|(name, sources)| DetectedTool {
            name,
            sources: sources.into_iter().collect(),
        })
        .collect()
}

pub(super) fn dedup_tools(tools: &mut Vec<DetectedTool>) {
    let mut merged = BTreeMap::<String, BTreeSet<String>>::new();
    for tool in tools.drain(..) {
        merged.entry(tool.name).or_default().extend(tool.sources);
    }
    *tools = tools_from_map(merged);
}

pub(super) fn tool_reports(tools: &[DetectedTool]) -> Vec<JsonValue> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "sources": tool.sources,
            })
        })
        .collect()
}
