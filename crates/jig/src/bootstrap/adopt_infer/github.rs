use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value as JsonValue, json};
use serde_yaml_ng::Value as YamlValue;

use super::scan::{
    RepoScan, push_scan_warning, read_yaml_for_inference, relative_path_string, yaml_mapping_get,
};

const DEFAULT_SUPPORTED_GITHUB_RUNNER: &str = "ubuntu-latest";

#[derive(Clone, Debug, Default)]
pub(super) struct GithubCiInference {
    pub(super) runner: Option<String>,
    pub(super) runner_was_synthesized: bool,
    pub(super) runner_warnings: Vec<String>,
    pub(super) sources: Vec<String>,
    pub(super) shape: GithubCiShapeInference,
}

#[derive(Clone, Debug, Default)]
pub(super) struct GithubCiShapeInference {
    workflow_files: Vec<String>,
    required_checks: CiSignalSet,
    lockfile_behavior: CiSignalSet,
    cache_strategy: CiSignalSet,
    matrix_os: CiSignalSet,
    matrix_toolchain: CiSignalSet,
    existing_jig_checks: CiSignalSet,
}

#[derive(Clone, Debug, Default)]
struct CiSignalSet {
    values: BTreeMap<String, Vec<String>>,
}

pub(super) fn infer_ci_github_runner_with_metadata(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> GithubCiInference {
    let workflows = root.join(".github/workflows");
    if !workflows.is_dir() {
        return GithubCiInference::default();
    }
    let mut files = scan
        .files_with_extensions(&["yml", "yaml"])
        .filter(|path| path.parent() == Some(workflows.as_path()))
        .cloned()
        .collect::<Vec<_>>();
    files.sort();
    let mut runners = BTreeMap::<String, usize>::new();
    let mut sources_by_runner = BTreeMap::<String, Vec<String>>::new();
    let mut shape = GithubCiShapeInference::default();
    for path in files {
        let Some(workflow) = read_yaml_for_inference(&path, warnings) else {
            continue;
        };
        let source = relative_path_string(path.strip_prefix(root).unwrap_or(&path));
        collect_github_ci_shape(
            &path,
            &source,
            &workflow,
            warnings,
            &mut shape,
            &mut |runner| {
                *runners.entry(runner.clone()).or_default() += 1;
                sources_by_runner
                    .entry(runner)
                    .or_default()
                    .push(format!("{source} jobs.*.runs-on"));
            },
        );
    }
    if runners.len() > 1 {
        push_scan_warning(
            warnings,
            &workflows,
            "multiple GitHub Actions runners detected; using the most common non-Windows runner with an ubuntu tie-break",
        );
    }
    let unsupported_windows_runners = runners
        .keys()
        .filter(|runner| runner_is_windows_host(runner))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported_windows_runners.is_empty() {
        push_scan_warning(
            warnings,
            &workflows,
            "Windows GitHub Actions runners are unsupported by Jig; excluding them from generated-check runner inference and using ubuntu-latest when no non-Windows static runner is available",
        );
    }
    let selected_runner = select_github_runner(&runners);
    let fallback_to_ubuntu = selected_runner.is_none() && !unsupported_windows_runners.is_empty();
    let runner = selected_runner
        .clone()
        .or_else(|| fallback_to_ubuntu.then(|| DEFAULT_SUPPORTED_GITHUB_RUNNER.to_string()));
    let runner_warnings = fallback_to_ubuntu
        .then(|| {
            "ubuntu-latest was synthesized because every statically detected GitHub Actions runner targets unsupported Windows hosts"
                .to_string()
        })
        .into_iter()
        .collect();
    let sources = if fallback_to_ubuntu {
        unsupported_windows_runners
            .iter()
            .flat_map(|runner| sources_by_runner.remove(runner).unwrap_or_default())
            .collect()
    } else {
        selected_runner
            .as_ref()
            .and_then(|runner| sources_by_runner.remove(runner))
            .unwrap_or_default()
    };
    GithubCiInference {
        runner,
        runner_was_synthesized: fallback_to_ubuntu,
        runner_warnings,
        sources,
        shape,
    }
}

pub(super) fn select_github_runner(runners: &BTreeMap<String, usize>) -> Option<String> {
    runners
        .iter()
        .filter(|(runner, _)| !runner_is_windows_host(runner))
        .max_by(|(left, left_count), (right, right_count)| {
            // Prefer the most common non-Windows runner; use ubuntu labels as
            // the stable tie-break because generated workflows are POSIX-oriented.
            // The lexical tie-break keeps newer labels such as ubuntu-24.04
            // ahead of older labels.
            left_count
                .cmp(right_count)
                .then_with(|| runner_preference(left).cmp(&runner_preference(right)))
                .then_with(|| left.cmp(right))
        })
        .map(|(runner, _)| runner.clone())
}

fn runner_preference(runner: &str) -> u8 {
    if runner.starts_with("ubuntu-") { 1 } else { 0 }
}

fn runner_is_windows_host(runner: &str) -> bool {
    runner.to_ascii_lowercase().starts_with("windows-")
}

fn collect_github_ci_shape<F>(
    path: &Path,
    source: &str,
    workflow: &YamlValue,
    warnings: &mut Vec<String>,
    shape: &mut GithubCiShapeInference,
    out: &mut F,
) where
    F: FnMut(String),
{
    shape.workflow_files.push(source.to_string());
    let Some(jobs) = yaml_mapping_get(workflow, "jobs").and_then(YamlValue::as_mapping) else {
        return;
    };
    for (job_key, job) in jobs {
        let job_id = job_key.as_str().unwrap_or("<unknown>");
        collect_job_check_name(source, job_id, job, shape);
        let matrix_axes = collect_job_matrix(source, job_id, job, shape);
        let Some(runs_on) = yaml_mapping_get(job, "runs-on") else {
            collect_job_steps(source, job_id, job, &matrix_axes, shape);
            continue;
        };
        collect_github_runner_value(path, runs_on, &matrix_axes, warnings, out);
        collect_job_steps(source, job_id, job, &matrix_axes, shape);
    }
}

fn collect_github_runner_value<F>(
    path: &Path,
    value: &YamlValue,
    matrix_axes: &BTreeMap<String, Vec<String>>,
    warnings: &mut Vec<String>,
    out: &mut F,
) where
    F: FnMut(String),
{
    match value {
        YamlValue::String(runner) => {
            let runner = runner.trim();
            if runner.is_empty() {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses empty runs-on value",
                );
            } else if let Some(axis) = matrix_expression_axis(runner) {
                if let Some(values) = matrix_axes.get(axis) {
                    for runner in values {
                        if runner_is_static_scalar(runner) {
                            out(runner.clone());
                        }
                    }
                } else {
                    push_scan_warning(
                        warnings,
                        path,
                        "GitHub Actions runner uses unsupported dynamic runs-on expression",
                    );
                }
            } else if runner_is_static_scalar(runner) {
                out(runner.to_string());
            } else {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses unsupported dynamic runs-on expression",
                );
            }
        }
        YamlValue::Sequence(items) => {
            let runners = items
                .iter()
                .filter_map(YamlValue::as_str)
                .map(str::trim)
                .filter(|runner| !runner.is_empty())
                .collect::<Vec<_>>();
            if runners.len() == 1 && runner_is_static_scalar(runners[0]) {
                out(runners[0].to_string());
            } else {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses unsupported composite runs-on labels",
                );
            }
        }
        YamlValue::Null => {
            push_scan_warning(
                warnings,
                path,
                "GitHub Actions runner uses empty runs-on value",
            );
        }
        _ => {
            push_scan_warning(
                warnings,
                path,
                "GitHub Actions runner uses unsupported runs-on shape",
            );
        }
    }
}

fn runner_is_static_scalar(value: &str) -> bool {
    !value.is_empty() && !value.contains("${{")
}

fn collect_job_check_name(
    source: &str,
    job_id: &str,
    job: &YamlValue,
    shape: &mut GithubCiShapeInference,
) {
    let check_name = yaml_mapping_get(job, "name")
        .and_then(yaml_scalar_string)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| job_id.to_string());
    shape
        .required_checks
        .insert(check_name, format!("{source} jobs.{job_id}.name"));
}

fn collect_job_matrix(
    source: &str,
    job_id: &str,
    job: &YamlValue,
    shape: &mut GithubCiShapeInference,
) -> BTreeMap<String, Vec<String>> {
    let mut axes = BTreeMap::<String, Vec<String>>::new();
    let Some(matrix) = yaml_mapping_get(job, "strategy")
        .and_then(|strategy| yaml_mapping_get(strategy, "matrix"))
        .and_then(YamlValue::as_mapping)
    else {
        return axes;
    };

    for (axis, value) in matrix {
        let Some(axis) = axis.as_str() else {
            continue;
        };
        if axis == "include" {
            collect_matrix_include(source, job_id, value, &mut axes, shape);
            continue;
        }
        if axis == "exclude" {
            continue;
        }
        let values = collect_static_values(value);
        if values.is_empty() {
            continue;
        }
        for item in &values {
            collect_matrix_signal(source, job_id, axis, item, shape);
        }
        axes.insert(axis.to_string(), values);
    }

    axes
}

fn collect_matrix_include(
    source: &str,
    job_id: &str,
    value: &YamlValue,
    axes: &mut BTreeMap<String, Vec<String>>,
    shape: &mut GithubCiShapeInference,
) {
    let Some(items) = value.as_sequence() else {
        return;
    };
    for item in items {
        let Some(mapping) = item.as_mapping() else {
            continue;
        };
        for (axis, value) in mapping {
            let Some(axis) = axis.as_str() else {
                continue;
            };
            let Some(value) = yaml_scalar_string(value) else {
                continue;
            };
            if !runner_is_static_scalar(&value) {
                continue;
            }
            collect_matrix_signal(source, job_id, axis, &value, shape);
            let entry = axes.entry(axis.to_string()).or_default();
            if !entry.iter().any(|existing| existing == &value) {
                entry.push(value);
            }
        }
    }
}

fn collect_matrix_signal(
    source: &str,
    job_id: &str,
    axis: &str,
    value: &str,
    shape: &mut GithubCiShapeInference,
) {
    let signal_source = format!("{source} jobs.{job_id}.strategy.matrix.{axis}");
    if matches!(axis, "os" | "runner" | "runs-on" | "runs_on") {
        shape.matrix_os.insert(value.to_string(), signal_source);
    } else if matches!(
        axis,
        "toolchain"
            | "rust"
            | "rust-toolchain"
            | "rust_toolchain"
            | "rust-version"
            | "rust_version"
            | "channel"
    ) {
        shape
            .matrix_toolchain
            .insert(value.to_string(), signal_source);
    }
}

fn collect_static_values(value: &YamlValue) -> Vec<String> {
    match value {
        YamlValue::Sequence(items) => items
            .iter()
            .filter_map(yaml_scalar_string)
            .filter(|value| runner_is_static_scalar(value))
            .collect(),
        _ => yaml_scalar_string(value)
            .filter(|value| runner_is_static_scalar(value))
            .into_iter()
            .collect(),
    }
}

fn collect_job_steps(
    source: &str,
    job_id: &str,
    job: &YamlValue,
    matrix_axes: &BTreeMap<String, Vec<String>>,
    shape: &mut GithubCiShapeInference,
) {
    let Some(steps) = yaml_mapping_get(job, "steps").and_then(YamlValue::as_sequence) else {
        return;
    };
    for (index, step) in steps.iter().enumerate() {
        let step_source = format!("{source} jobs.{job_id}.steps[{index}]");
        if let Some(uses) = yaml_mapping_get(step, "uses").and_then(YamlValue::as_str) {
            collect_step_uses(&step_source, uses, step, matrix_axes, shape);
        }
        if let Some(run) = yaml_mapping_get(step, "run").and_then(YamlValue::as_str) {
            collect_step_run(&step_source, run, shape);
        }
    }
}

fn collect_step_uses(
    source: &str,
    uses: &str,
    step: &YamlValue,
    matrix_axes: &BTreeMap<String, Vec<String>>,
    shape: &mut GithubCiShapeInference,
) {
    let normalized = uses.to_ascii_lowercase();
    if normalized.starts_with("actions/cache@") {
        shape.cache_strategy.insert("actions/cache", source);
    } else if normalized.starts_with("swatinem/rust-cache@") {
        shape.cache_strategy.insert("Swatinem/rust-cache", source);
    } else if normalized.starts_with("actions/setup-node@") {
        if let Some(cache) = step_with_value(step, "cache") {
            shape.cache_strategy.insert(
                format!("setup-node dependency cache: {cache}"),
                format!("{source}.with.cache"),
            );
        }
    } else if normalized.starts_with("actions-rust-lang/setup-rust-toolchain@") {
        if let Some(cache) = step_with_value(step, "cache") {
            let value = match cache.as_str() {
                "false" => "setup-rust-toolchain cache disabled".to_string(),
                "true" => "setup-rust-toolchain cache enabled".to_string(),
                value => format!("setup-rust-toolchain cache: {value}"),
            };
            shape
                .cache_strategy
                .insert(value, format!("{source}.with.cache"));
        }
        if let Some(toolchain) = step_with_value(step, "toolchain") {
            collect_toolchain_value(
                &toolchain,
                &format!("{source}.with.toolchain"),
                matrix_axes,
                shape,
            );
        }
    }
}

fn collect_step_run(source: &str, run: &str, shape: &mut GithubCiShapeInference) {
    for line in run.lines() {
        let command = line.trim();
        let lower = command.to_ascii_lowercase();
        if lower.contains("scripts/jig check") {
            shape
                .existing_jig_checks
                .insert(command.to_string(), source);
        }
        if lower.contains("cargo ") && lower.contains("--locked") {
            shape
                .lockfile_behavior
                .insert("Cargo lockfile enforced with --locked", source);
        }
        if lower.contains("npm ci") {
            shape
                .lockfile_behavior
                .insert("npm lockfile install via npm ci", source);
        }
        if lower.contains("pnpm install") && lower.contains("frozen-lockfile") {
            shape
                .lockfile_behavior
                .insert("pnpm frozen lockfile install", source);
        }
        if lower.contains("yarn install")
            && (lower.contains("--immutable") || lower.contains("frozen-lockfile"))
        {
            shape
                .lockfile_behavior
                .insert("yarn immutable lockfile install", source);
        }
        if lower.contains("bun install") && lower.contains("frozen-lockfile") {
            shape
                .lockfile_behavior
                .insert("bun frozen lockfile install", source);
        }
    }
}

fn collect_toolchain_value(
    value: &str,
    source: &str,
    matrix_axes: &BTreeMap<String, Vec<String>>,
    shape: &mut GithubCiShapeInference,
) {
    if let Some(axis) = matrix_expression_axis(value) {
        if let Some(values) = matrix_axes.get(axis) {
            for value in values {
                shape.matrix_toolchain.insert(value.clone(), source);
            }
        }
    } else if runner_is_static_scalar(value) {
        shape.matrix_toolchain.insert(value.to_string(), source);
    }
}

fn step_with_value(step: &YamlValue, key: &str) -> Option<String> {
    yaml_mapping_get(step, "with")
        .and_then(|with| yaml_mapping_get(with, key))
        .and_then(yaml_scalar_string)
        .filter(|value| !value.trim().is_empty())
}

fn yaml_scalar_string(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::String(value) => Some(value.trim().to_string()),
        YamlValue::Bool(value) => Some(value.to_string()),
        YamlValue::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn matrix_expression_axis(value: &str) -> Option<&str> {
    let expression = value.trim().strip_prefix("${{")?.strip_suffix("}}")?.trim();
    let axis = expression.strip_prefix("matrix.")?.trim();
    if axis
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        Some(axis)
    } else {
        None
    }
}

impl GithubCiShapeInference {
    pub(super) fn has_workflows(&self) -> bool {
        !self.workflow_files.is_empty()
    }

    pub(super) fn workflow_file_count(&self) -> usize {
        self.workflow_files.len()
    }

    pub(super) fn generated_jig_checks_role(&self) -> &'static str {
        if self.workflow_files.is_empty() {
            "establish_new_ci"
        } else if self.existing_jig_checks.is_empty() {
            "supplement_existing_ci"
        } else {
            "replace_existing_jig_ci"
        }
    }

    pub(super) fn report(&self) -> JsonValue {
        json!({
            "workflow_files": self.workflow_files,
            "required_checks": self.required_checks.report(),
            "lockfile_behavior": self.lockfile_behavior.report(),
            "cache_strategy": self.cache_strategy.report(),
            "matrix": {
                "os": self.matrix_os.report(),
                "toolchain": self.matrix_toolchain.report(),
            },
            "existing_jig_checks": self.existing_jig_checks.report(),
            "generated_jig_checks_role": self.generated_jig_checks_role(),
        })
    }

    pub(super) fn sources(&self) -> Vec<String> {
        let mut sources = self.workflow_files.clone();
        sources.extend(self.required_checks.sources());
        sources.extend(self.lockfile_behavior.sources());
        sources.extend(self.cache_strategy.sources());
        sources.extend(self.matrix_os.sources());
        sources.extend(self.matrix_toolchain.sources());
        sources.extend(self.existing_jig_checks.sources());
        sources.sort();
        sources.dedup();
        sources
    }
}

impl CiSignalSet {
    fn insert(&mut self, value: impl Into<String>, source: impl Into<String>) {
        let source = source.into();
        let sources = self.values.entry(value.into()).or_default();
        if !sources.iter().any(|existing| existing == &source) {
            sources.push(source);
        }
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn report(&self) -> JsonValue {
        JsonValue::Array(
            self.values
                .iter()
                .map(|(value, sources)| {
                    json!({
                        "value": value,
                        "sources": sources,
                    })
                })
                .collect(),
        )
    }

    fn sources(&self) -> Vec<String> {
        self.values
            .values()
            .flat_map(|sources| sources.iter().cloned())
            .collect()
    }
}
