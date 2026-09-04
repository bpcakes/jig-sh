use std::path::Path;

use serde_json::{Value as JsonValue, json};

use super::scan::RepoScan;

mod rust;
mod sqlx;
mod tool;
mod web;

use self::rust::{
    detect_nextest, first_text_source_matching, infer_rust_wrapper_commands,
    nested_manifest_commands,
};
use self::sqlx::infer_sqlx_command_tools;
use self::tool::{DetectedTool, dedup_tools, tool_reports};
use self::web::infer_web_tools;

#[derive(Clone, Debug, Default)]
pub(super) struct CommandInference {
    pub(super) rust_fmt_check_command: Option<CommandCandidate>,
    pub(super) rust_clippy_command: Option<CommandCandidate>,
    pub(super) rust_test_command: Option<CommandCandidate>,
    pub(super) rust_test_locked_command: Option<CommandCandidate>,
    rust_tools: Vec<DetectedTool>,
    web_tools: Vec<DetectedTool>,
    sqlx_tools: Vec<DetectedTool>,
}

#[derive(Clone, Debug)]
pub(super) struct CommandCandidate {
    pub(super) command: String,
    pub(super) source: String,
    pub(super) confidence: &'static str,
    pub(super) warnings: Vec<String>,
    // Typed provenance for wrapper commands so mixed-source checks do not parse
    // the user-facing `source` text.
    wrapper_source: Option<String>,
    from_nested_manifest_scan: bool,
}

pub(super) fn infer_commands(
    root: &Path,
    scan: &RepoScan,
    nested_manifest_paths: Option<&[String]>,
    warnings: &mut Vec<String>,
) -> CommandInference {
    let wrappers = infer_rust_wrapper_commands(root, warnings);
    let mut out = CommandInference {
        rust_fmt_check_command: wrappers.fmt,
        rust_clippy_command: wrappers.clippy,
        rust_test_command: wrappers.test,
        rust_test_locked_command: wrappers.test_locked,
        rust_tools: Vec::new(),
        web_tools: infer_web_tools(root, scan, warnings),
        sqlx_tools: infer_sqlx_command_tools(root, scan, warnings),
    };
    if let Some(manifest_paths) = nested_manifest_paths {
        let nested = nested_manifest_commands(manifest_paths);
        out.rust_fmt_check_command.get_or_insert(nested.fmt);
        out.rust_clippy_command.get_or_insert(nested.clippy);
        out.rust_test_command.get_or_insert(nested.test);
    }
    warn_if_generated_clippy_checks_all_features(&mut out, warnings);

    if let Some(nextest) = detect_nextest(root, scan, warnings) {
        out.rust_tools.push(DetectedTool {
            name: "cargo-nextest".into(),
            sources: vec![nextest.source.clone()],
        });
        if out.rust_test_command.is_none() {
            out.rust_test_command = Some(nextest_candidate(
                "cargo nextest run --workspace",
                nextest.source.clone(),
                nextest.confidence,
            ));
        }
        if out.rust_test_locked_command.is_none() {
            out.rust_test_locked_command = Some(nextest_candidate(
                "cargo nextest run --workspace --locked",
                nextest.source,
                nextest.confidence,
            ));
        }
    }

    if let Some(source) =
        first_text_source_matching(root, scan, warnings, |text| text.contains("cargo hack"))
    {
        out.rust_tools.push(DetectedTool {
            name: "cargo-hack".into(),
            sources: vec![source],
        });
    }

    warn_if_mixed_rust_test_runners(&mut out);
    dedup_tools(&mut out.rust_tools);
    out
}

fn warn_if_generated_clippy_checks_all_features(
    out: &mut CommandInference,
    warnings: &mut Vec<String>,
) {
    let Some(candidate) = out
        .rust_clippy_command
        .as_mut()
        .filter(|candidate| candidate.from_nested_manifest_scan)
    else {
        return;
    };
    let warning = crate::bootstrap::clippy_policy::ALL_FEATURES_ADOPTION_WARNING.to_owned();
    candidate.warnings.push(warning.clone());
    warnings.push(warning);
}

impl CommandInference {
    pub(super) fn uses_nested_manifest_commands(&self) -> bool {
        [
            self.rust_fmt_check_command.as_ref(),
            self.rust_clippy_command.as_ref(),
            self.rust_test_command.as_ref(),
            self.rust_test_locked_command.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|candidate| candidate.from_nested_manifest_scan)
    }

    pub(super) fn report(&self) -> JsonValue {
        json!({
            "rust": {
                "commands": {
                    "rust_fmt_check_command": self.rust_fmt_check_command.as_ref().map(CommandCandidate::report),
                    "rust_clippy_command": self.rust_clippy_command.as_ref().map(CommandCandidate::report),
                    "rust_test_command": self.rust_test_command.as_ref().map(CommandCandidate::report),
                    "rust_test_locked_command": self.rust_test_locked_command.as_ref().map(CommandCandidate::report),
                },
                "tools": tool_reports(&self.rust_tools),
            },
            "web": {
                "tools": tool_reports(&self.web_tools),
            },
            "sqlx": {
                "tools": tool_reports(&self.sqlx_tools),
            },
        })
    }
}

impl CommandCandidate {
    pub(super) fn command(&self) -> String {
        self.command.clone()
    }

    pub(super) fn report(&self) -> JsonValue {
        json!({
            "command": self.command,
            "source": self.source,
            "confidence": self.confidence,
            "warnings": self.warnings,
        })
    }
}

fn nextest_candidate(command: &str, source: String, confidence: &'static str) -> CommandCandidate {
    CommandCandidate {
        command: command.into(),
        source,
        confidence,
        warnings: Vec::new(),
        wrapper_source: None,
        from_nested_manifest_scan: false,
    }
}

fn warn_if_mixed_rust_test_runners(out: &mut CommandInference) {
    let Some(test_runner) = out
        .rust_test_command
        .as_ref()
        .and_then(|candidate| command_runner(&candidate.command))
    else {
        return;
    };
    let Some(locked_runner) = out
        .rust_test_locked_command
        .as_ref()
        .and_then(|candidate| command_runner(&candidate.command))
    else {
        return;
    };
    if test_runner == locked_runner {
        return;
    }

    let warning = format!(
        "rust test commands use different runners ({test_runner}, {locked_runner}); review .jig.toml before relying on the pair"
    );
    if let Some(candidate) = &mut out.rust_test_command {
        candidate.warnings.push(warning.clone());
    }
    if let Some(candidate) = &mut out.rust_test_locked_command {
        candidate.warnings.push(warning);
    }
}

fn command_runner(command: &str) -> Option<&str> {
    command.split_whitespace().next()
}
