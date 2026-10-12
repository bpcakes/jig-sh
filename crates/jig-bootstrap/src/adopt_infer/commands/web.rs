use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value as JsonValue;

use super::super::scan::{RepoScan, read_json_for_inference, relative_path_string};
use super::tool::{DetectedTool, tools_from_map};

pub(super) fn infer_web_tools(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Vec<DetectedTool> {
    let mut tools = BTreeMap::<String, BTreeSet<String>>::new();
    for package_path in scan.named_files("package.json") {
        let relative =
            relative_path_string(package_path.strip_prefix(root).unwrap_or(package_path));
        let Some(package) = read_json_for_inference(package_path, warnings) else {
            continue;
        };
        for section_name in ["dependencies", "devDependencies"] {
            if let Some(section) = package.get(section_name).and_then(JsonValue::as_object) {
                for (dependency, tool) in [
                    ("turbo", "turbo"),
                    ("nx", "nx"),
                    ("vitest", "vitest"),
                    ("@playwright/test", "playwright"),
                    ("playwright", "playwright"),
                    ("eslint", "eslint"),
                    ("biome", "biome"),
                    ("@biomejs/biome", "biome"),
                ] {
                    if section.contains_key(dependency) {
                        tools
                            .entry(tool.into())
                            .or_default()
                            .insert(format!("{relative} [{section_name}].{dependency}"));
                    }
                }
            }
        }
        if let Some(scripts) = package.get("scripts").and_then(JsonValue::as_object) {
            for (script_name, script) in scripts
                .iter()
                .filter_map(|(name, value)| value.as_str().map(|script| (name.as_str(), script)))
            {
                for (needle, tool) in [
                    ("turbo", "turbo"),
                    ("nx", "nx"),
                    ("vitest", "vitest"),
                    ("playwright", "playwright"),
                    ("eslint", "eslint"),
                    ("biome", "biome"),
                ] {
                    if command_mentions_token(script, needle) {
                        tools
                            .entry(tool.into())
                            .or_default()
                            .insert(format!("{relative} scripts.{script_name}"));
                    }
                }
            }
        }
    }
    for (name, tool) in [
        ("turbo.json", "turbo"),
        ("nx.json", "nx"),
        ("biome.json", "biome"),
        ("biome.jsonc", "biome"),
        (".eslintrc", "eslint"),
        (".eslintrc.json", "eslint"),
        ("eslint.config.js", "eslint"),
        ("eslint.config.mjs", "eslint"),
        ("eslint.config.cjs", "eslint"),
        ("vitest.config.ts", "vitest"),
        ("vitest.config.js", "vitest"),
        ("playwright.config.ts", "playwright"),
        ("playwright.config.js", "playwright"),
    ] {
        for path in scan.named_files(name) {
            tools
                .entry(tool.into())
                .or_default()
                .insert(relative_path_string(
                    path.strip_prefix(root).unwrap_or(path),
                ));
        }
    }
    tools_from_map(tools)
}

fn command_mentions_token(command: &str, token: &str) -> bool {
    command
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .any(|candidate| candidate == token)
}
