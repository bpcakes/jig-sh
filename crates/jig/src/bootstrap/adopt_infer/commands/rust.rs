use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use super::super::scan::{RepoScan, push_scan_warning, read_limited_text, relative_path_string};
use super::CommandCandidate;

#[derive(Default)]
pub(super) struct RustWrapperCommands {
    pub(super) fmt: Option<CommandCandidate>,
    pub(super) clippy: Option<CommandCandidate>,
    pub(super) test: Option<CommandCandidate>,
    pub(super) test_locked: Option<CommandCandidate>,
}

pub(super) struct NextestDetection {
    pub(super) source: String,
    pub(super) confidence: &'static str,
}

#[derive(Debug)]
pub(super) struct NestedManifestCommands {
    pub(super) fmt: CommandCandidate,
    pub(super) clippy: CommandCandidate,
    pub(super) test: CommandCandidate,
}

pub(super) fn infer_rust_wrapper_commands(
    root: &Path,
    warnings: &mut Vec<String>,
) -> RustWrapperCommands {
    let mut out = RustWrapperCommands::default();
    for (path, runner, recipes) in [
        (
            root.join("Justfile"),
            "just",
            just_recipes as fn(&str) -> BTreeSet<String>,
        ),
        (
            root.join("justfile"),
            "just",
            just_recipes as fn(&str) -> BTreeSet<String>,
        ),
        (
            root.join("Makefile"),
            "make",
            make_targets as fn(&str) -> BTreeSet<String>,
        ),
    ] {
        if !path.is_file() {
            continue;
        }
        let text = match read_limited_text(&path) {
            Ok(text) => text,
            Err(error) => {
                push_scan_warning(
                    warnings,
                    &path,
                    &format!("could not read command wrapper for inference: {error:#}"),
                );
                continue;
            }
        };
        let recipes = recipes(&text);
        let source_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("wrapper");
        if out.fmt.is_none() {
            out.fmt = first_recipe(&recipes, &["fmt-check", "check-fmt", "rust-fmt-check"])
                .map(|recipe| wrapper_candidate(runner, recipe, source_name));
        }
        if out.clippy.is_none() {
            out.clippy = first_recipe(&recipes, &["clippy", "rust-clippy", "lint-rust"])
                .map(|recipe| wrapper_candidate(runner, recipe, source_name));
        }
        if out.test.is_none() {
            out.test = first_recipe(&recipes, &["test", "rust-test", "test-rust"])
                .map(|recipe| wrapper_candidate(runner, recipe, source_name));
        }
        if out.test_locked.is_none() {
            out.test_locked = first_recipe(
                &recipes,
                &["test-locked", "locked-test", "rust-test-locked"],
            )
            .map(|recipe| wrapper_candidate(runner, recipe, source_name));
        }
        if out.complete() {
            break;
        }
    }
    out.warn_if_mixed_sources();
    out
}

pub(super) fn nested_manifest_commands(manifest_paths: &[String]) -> NestedManifestCommands {
    NestedManifestCommands {
        fmt: nested_manifest_candidate(
            manifest_paths,
            "cargo fmt --manifest-path \"$jig_manifest\" -- --check",
            "fmt",
        ),
        clippy: nested_manifest_candidate(
            manifest_paths,
            "cargo clippy --manifest-path \"$jig_manifest\" --all-targets -- -D warnings",
            "clippy",
        ),
        test: nested_manifest_candidate(
            manifest_paths,
            "cargo test --manifest-path \"$jig_manifest\"",
            "test",
        ),
    }
}

pub(super) fn detect_nextest(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Option<NextestDetection> {
    for path in [root.join(".config/nextest.toml"), root.join("nextest.toml")] {
        if path.is_file() {
            return Some(NextestDetection {
                source: relative_path_string(path.strip_prefix(root).unwrap_or(&path)),
                confidence: "high",
            });
        }
    }
    first_text_source_matching(root, scan, warnings, |text| text.contains("cargo nextest")).map(
        |source| NextestDetection {
            source,
            confidence: "medium",
        },
    )
}

pub(super) fn first_text_source_matching<F>(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
    predicate: F,
) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    for path in [
        root.join("Justfile"),
        root.join("justfile"),
        root.join("Makefile"),
    ] {
        if !path.is_file() {
            continue;
        }
        match read_limited_text(&path) {
            Ok(text) if predicate(&text) => {
                return Some(relative_path_string(
                    path.strip_prefix(root).unwrap_or(&path),
                ));
            }
            Ok(_) => {}
            Err(error) => push_scan_warning(
                warnings,
                &path,
                &format!("could not read text for command inference: {error:#}"),
            ),
        }
    }
    for path in scan.files_with_extensions(&["sh", "yml", "yaml", "toml"]) {
        match read_limited_text(path) {
            Ok(text) if predicate(&text) => {
                return Some(relative_path_string(
                    path.strip_prefix(root).unwrap_or(path),
                ));
            }
            Ok(_) => {}
            Err(error) => push_scan_warning(
                warnings,
                path,
                &format!("could not read text for command inference: {error:#}"),
            ),
        }
    }
    None
}

impl RustWrapperCommands {
    const fn complete(&self) -> bool {
        self.fmt.is_some()
            && self.clippy.is_some()
            && self.test.is_some()
            && self.test_locked.is_some()
    }

    fn warn_if_mixed_sources(&mut self) {
        let sources = [
            self.fmt.as_ref(),
            self.clippy.as_ref(),
            self.test.as_ref(),
            self.test_locked.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(wrapper_source_name)
        .collect::<BTreeSet<_>>();
        if sources.len() <= 1 {
            return;
        }

        let warning = format!(
            "wrapper commands were inferred from multiple files ({}); review .jig.toml before relying on them as one command set",
            sources.into_iter().collect::<Vec<_>>().join(", ")
        );
        for candidate in [
            &mut self.fmt,
            &mut self.clippy,
            &mut self.test,
            &mut self.test_locked,
        ]
        .into_iter()
        .flatten()
        {
            candidate.warnings.push(warning.clone());
        }
    }
}

fn wrapper_candidate(runner: &str, recipe: &str, source_name: &str) -> CommandCandidate {
    CommandCandidate {
        command: format!("{runner} {recipe}"),
        source: format!("{source_name} recipe {recipe}"),
        confidence: "high",
        warnings: vec!["wrapper recipe inferred by name; review the rendered command".into()],
        wrapper_source: Some(source_name.into()),
        from_nested_manifest_scan: false,
    }
}

fn nested_manifest_candidate(
    manifest_paths: &[String],
    cargo_command: &str,
    label: &str,
) -> CommandCandidate {
    CommandCandidate {
        command: nested_manifest_command(manifest_paths, cargo_command, label),
        source: "nested Cargo.toml scan".into(),
        confidence: "medium",
        warnings: vec![
            "Generated from nested Cargo.toml files because no root Cargo.toml was found; review rust_crate_roots before relying on this command."
                .into(),
        ],
        wrapper_source: None,
        from_nested_manifest_scan: true,
    }
}

fn nested_manifest_command(manifest_paths: &[String], cargo_command: &str, label: &str) -> String {
    let mut command = "( found=0".to_string();
    command.push_str("; rc=0");
    for manifest_path in manifest_paths {
        let manifest_path = crate::shell::quote(manifest_path);
        // Keep any cargo failure nonzero while still attempting later inferred manifests.
        let _ = write!(
            command,
            "; jig_manifest={manifest_path}; if [ -f \"$jig_manifest\" ]; then found=1; {cargo_command} || rc=$?; fi"
        );
    }
    let _ = write!(
        command,
        "; if [ \"$found\" -eq 0 ]; then printf '%s\\n' {}; fi",
        crate::shell::quote(&format!("{}{label}.", crate::CARGO_SKIP_OUTPUT_PREFIX))
    );
    command.push_str("; exit \"$rc\" )");
    command
}

fn wrapper_source_name(candidate: &CommandCandidate) -> Option<&str> {
    candidate.wrapper_source.as_deref()
}

fn first_recipe<'a>(recipes: &BTreeSet<String>, candidates: &'a [&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|candidate| recipes.contains(*candidate))
}

fn just_recipes(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            if line.starts_with(char::is_whitespace) {
                return None;
            }
            recipe_name_before_colon(line)
        })
        .collect()
}

fn make_targets(text: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((head, tail)) = line.split_once(':') else {
            continue;
        };
        if head.contains('=') || tail.trim_start().starts_with('=') || head.starts_with('.') {
            continue;
        }
        targets.extend(
            head.split_whitespace()
                .filter(|target| safe_recipe_name(target))
                .map(str::to_string),
        );
    }
    targets
}

fn recipe_name_before_colon(line: &str) -> Option<String> {
    let (head, tail) = line.split_once(':')?;
    if tail.trim_start().starts_with('=') {
        return None;
    }
    let name = head
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(['(', ')']);
    safe_recipe_name(name).then(|| name.to_string())
}

fn safe_recipe_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}
