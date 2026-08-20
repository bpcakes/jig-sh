use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{Context, Result, bail};

use crate::bootstrap::{
    self, InitOpts, ScaffoldDb, ScaffoldFrontend, ScaffoldPreset, parse_scaffold_frontend,
};

pub(super) fn prepare_init_interaction(opts: &mut InitOpts) -> Result<()> {
    bootstrap::merge_init_answer_file_for_interaction(&mut opts.answers)?;
    let terminals_available = io::stdin().is_terminal() && io::stderr().is_terminal();
    let stdin = io::stdin();
    let stderr = io::stderr();
    let mut input = stdin.lock();
    let mut output = stderr.lock();
    let policy = InitInteractionPolicy::resolve(opts, terminals_available);
    prepare_merged_init_interaction(opts, policy, &mut input, &mut output)
}

pub(super) fn preflight_init_package_manager(opts: &InitOpts) -> Result<()> {
    preflight_init_package_manager_with(opts, crate::doctor::program_available_on_path)
}

fn preflight_init_package_manager_with(
    opts: &InitOpts,
    available: impl FnOnce(&str) -> bool,
) -> Result<()> {
    if !matches!(
        opts.scaffold.preset,
        Some(ScaffoldPreset::RustReact | ScaffoldPreset::GoReact)
    ) {
        return Ok(());
    }
    let package_manager = opts.answers.web_package_manager.as_deref().unwrap_or("bun");
    if available(package_manager) {
        return Ok(());
    }
    bail!(
        "Selected web package manager '{package_manager}' is not available on PATH. Install or enable it, or rerun with --web-package-manager bun, npm, pnpm, or yarn. No files were written."
    )
}

#[cfg(test)]
fn prepare_init_interaction_with_io<R: BufRead, W: Write>(
    opts: &mut InitOpts,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    bootstrap::merge_init_answer_file_for_interaction(&mut opts.answers)?;
    let policy = InitInteractionPolicy::resolve(opts, true);
    prepare_merged_init_interaction(opts, policy, input, output)
}

#[cfg(test)]
fn prepare_init_interaction_with_terminal<R: BufRead, W: Write>(
    opts: &mut InitOpts,
    terminals_available: bool,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    bootstrap::merge_init_answer_file_for_interaction(&mut opts.answers)?;
    let policy = InitInteractionPolicy::resolve(opts, terminals_available);
    prepare_merged_init_interaction(opts, policy, input, output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitInteractionPolicy {
    Interactive,
    Defaults,
    Strict { non_terminal: bool },
}

impl InitInteractionPolicy {
    const fn resolve(opts: &InitOpts, terminals_available: bool) -> Self {
        if opts.defaults {
            Self::Defaults
        } else if opts.no_input {
            Self::Strict {
                non_terminal: false,
            }
        } else if !terminals_available {
            Self::Strict { non_terminal: true }
        } else {
            Self::Interactive
        }
    }
}

fn prepare_merged_init_interaction<R: BufRead, W: Write>(
    opts: &mut InitOpts,
    policy: InitInteractionPolicy,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    opts.scaffold.normalize_minimal_harness_shape(&opts.answers);
    opts.scaffold.validate_init_invariants(&opts.answers)?;
    match policy {
        InitInteractionPolicy::Interactive => {
            guide_project_shape(opts, input, output)?;
            opts.scaffold.validate_init_invariants(&opts.answers)?;
            confirm_custom_frontend_names(opts, input, output)?;
        }
        InitInteractionPolicy::Defaults => apply_project_shape_defaults(opts)?,
        InitInteractionPolicy::Strict { non_terminal } => {
            validate_project_shape_resolved(opts, non_terminal)?;
        }
    }
    opts.scaffold.apply_init_answer_defaults(&mut opts.answers);
    opts.scaffold.validate_init_invariants(&opts.answers)
}

fn apply_project_shape_defaults(opts: &mut InitOpts) -> Result<()> {
    if opts.scaffold.preset.is_none() {
        opts.scaffold.preset = Some(ScaffoldPreset::RustReact);
    }
    if matches!(
        opts.scaffold.preset,
        Some(ScaffoldPreset::RustReact | ScaffoldPreset::GoReact)
    ) {
        opts.scaffold.db.get_or_insert(ScaffoldDb::None);
        if !opts.scaffold.has_frontends() && opts.answers.frontend_apps.is_empty() {
            opts.scaffold
                .frontends
                .push(parse_scaffold_frontend("web").map_err(|error| anyhow::anyhow!(error))?);
        }
    }
    if opts.scaffold.preset == Some(ScaffoldPreset::GoReact) && opts.answers.go_module.is_none() {
        let repo_name = opts
            .answers
            .repo_name
            .as_deref()
            .or_else(|| opts.path.file_name().and_then(|value| value.to_str()))
            .unwrap_or("app");
        opts.answers.go_module = Some(bootstrap::default_go_module(repo_name));
    }
    Ok(())
}

fn validate_project_shape_resolved(opts: &InitOpts, non_terminal: bool) -> Result<()> {
    let mode = if non_terminal {
        "stdin or stderr is not a terminal"
    } else {
        "--no-input was supplied"
    };
    let Some(preset) = opts.scaffold.preset else {
        bail!(
            "Init cannot prompt because {mode}; pass an application preset with explicit database and frontend choices, pass --preset harness-only, or use --defaults"
        );
    };
    if matches!(preset, ScaffoldPreset::RustReact | ScaffoldPreset::GoReact) {
        if opts.scaffold.db.is_none() {
            bail!(
                "Init cannot prompt because {mode}; the selected application preset requires an explicit --db choice, or use --defaults"
            );
        }
        if !opts.scaffold.has_frontends() && opts.answers.frontend_apps.is_empty() {
            bail!(
                "Init cannot prompt because {mode}; the selected application preset requires --frontend/--frontends or frontend_apps in --answers-file, or use --defaults"
            );
        }
    }
    if preset == ScaffoldPreset::GoReact && opts.answers.go_module.is_none() {
        bail!(
            "Init cannot prompt because {mode}; --preset go-react requires --go-module <module>, or use --defaults"
        );
    }
    Ok(())
}

fn guide_project_shape<R: BufRead, W: Write>(
    opts: &mut InitOpts,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let metadata = InitPresetMetadata::load();
    let mut printed_header = false;

    if opts.scaffold.preset.is_none() {
        print_project_shape_header(output, &metadata)?;
        printed_header = true;
        match prompt_scaffold_choice(input, output)? {
            ScaffoldChoice::RustReact => {
                opts.scaffold.preset = Some(ScaffoldPreset::RustReact);
            }
            ScaffoldChoice::GoReact => {
                opts.scaffold.preset = Some(ScaffoldPreset::GoReact);
            }
            ScaffoldChoice::HarnessOnly => {
                opts.scaffold.preset = Some(ScaffoldPreset::HarnessOnly);
            }
        }
    }

    if !matches!(
        opts.scaffold.preset,
        Some(ScaffoldPreset::RustReact | ScaffoldPreset::GoReact)
    ) {
        return Ok(());
    }
    let needs_frontends = !opts.scaffold.has_frontends() && opts.answers.frontend_apps.is_empty();
    if !printed_header && (opts.scaffold.db.is_none() || needs_frontends) {
        print_project_shape_header(output, &metadata)?;
    }
    if opts.scaffold.db.is_none() {
        opts.scaffold.db = Some(prompt_database(
            input,
            output,
            opts.scaffold.preset == Some(ScaffoldPreset::GoReact),
        )?);
    }
    if needs_frontends {
        opts.scaffold.frontends = prompt_frontends(input, output, &metadata)?;
    }
    if opts.scaffold.preset == Some(ScaffoldPreset::GoReact) && opts.answers.go_module.is_none() {
        opts.answers.go_module = Some(prompt_line(
            input,
            output,
            "Go module (for example github.com/acme/my-app): ",
            "example.com/app",
            "Go module",
        )?);
    }
    Ok(())
}

fn print_project_shape_header<W: Write>(
    output: &mut W,
    metadata: &InitPresetMetadata,
) -> Result<()> {
    writeln!(output, "Project shape")?;
    writeln!(output, "  rust-react — {}", metadata.preset_summary)?;
    writeln!(
        output,
        "  go-react — Go 1.26, chi, Huma, pgx/sqlc/Goose, and React."
    )?;
    writeln!(
        output,
        "  harness-only — Jig harness without starter application code."
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScaffoldChoice {
    RustReact,
    GoReact,
    HarnessOnly,
}

fn prompt_scaffold_choice<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
) -> Result<ScaffoldChoice> {
    loop {
        let answer = prompt_line(
            input,
            output,
            "Scaffold an app? [rust-react/go-react/harness-only] (rust-react): ",
            "rust-react",
            "project scaffold",
        )?;
        match answer.as_str() {
            "1" | "rust-react" | "app" | "yes" | "y" => {
                return Ok(ScaffoldChoice::RustReact);
            }
            "2" | "harness" | "harness-only" | "no" | "n" => {
                return Ok(ScaffoldChoice::HarnessOnly);
            }
            "3" | "go" | "go-react" => return Ok(ScaffoldChoice::GoReact),
            _ => writeln!(output, "  Enter rust-react, go-react, or harness-only.")?,
        }
    }
}

fn prompt_database<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    go_backend: bool,
) -> Result<ScaffoldDb> {
    loop {
        let answer = prompt_line(
            input,
            output,
            if go_backend {
                "Database? [none/postgres] (none): "
            } else {
                "Database? [none/postgres/sqlite] (none): "
            },
            "none",
            "database choice",
        )?;
        match answer.as_str() {
            "1" | "none" | "no" => return Ok(ScaffoldDb::None),
            "2" | "postgres" | "postgresql" => return Ok(ScaffoldDb::Postgres),
            "3" | "sqlite" if !go_backend => return Ok(ScaffoldDb::Sqlite),
            _ if go_backend => writeln!(output, "  Enter none or postgres.")?,
            _ => writeln!(output, "  Enter none, postgres, or sqlite.")?,
        }
    }
}

fn prompt_frontends<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    metadata: &InitPresetMetadata,
) -> Result<Vec<ScaffoldFrontend>> {
    writeln!(output, "Frontends (choose one or more):")?;
    for (name, description) in &metadata.frontends {
        writeln!(output, "  {name} — {description}")?;
    }
    let choices = metadata
        .frontends
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let default = choices.first().copied().unwrap_or("web");
    let prompt = format!(
        "Frontends? [{}] ({default}; comma-separated): ",
        choices.join(",")
    );
    loop {
        let answer = prompt_line(input, output, &prompt, default, "frontend choice")?;
        let mut seen = HashSet::new();
        let mut frontends = Vec::new();
        let mut invalid = None;
        for value in answer
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if !choices.contains(&value) {
                invalid = Some(value.to_string());
                break;
            }
            if seen.insert(value) {
                frontends
                    .push(parse_scaffold_frontend(value).map_err(|error| anyhow::anyhow!(error))?);
            }
        }
        if let Some(value) = invalid {
            writeln!(
                output,
                "  '{value}' is not a wizard choice; enter one or more of {}.",
                choices.join(", ")
            )?;
            continue;
        }
        if frontends.is_empty() {
            writeln!(output, "  Choose at least one frontend.")?;
            continue;
        }
        return Ok(frontends);
    }
}

fn confirm_custom_frontend_names<R: BufRead, W: Write>(
    opts: &InitOpts,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let notices = opts.scaffold.custom_frontend_notices();
    if notices.is_empty() {
        return Ok(());
    }

    let multiple = notices.len() > 1;
    writeln!(
        output,
        "Custom frontend name{}",
        if multiple { "s" } else { "" }
    )?;
    for notice in notices {
        writeln!(output, "  {notice}")?;
    }
    loop {
        let answer = prompt_line(
            input,
            output,
            if multiple {
                "Continue with the custom frontend names? [y/N]: "
            } else {
                "Continue with the custom frontend name? [y/N]: "
            },
            "no",
            "custom frontend confirmation",
        )?;
        match answer.as_str() {
            "y" | "yes" => return Ok(()),
            "n" | "no" => {
                bail!(
                    "init cancelled before files were written; fix the name or use an explicit kind such as name:spa to declare a custom frontend"
                );
            }
            _ => writeln!(output, "  Enter yes or no.")?,
        }
    }
}

fn prompt_line<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
    default: &str,
    label: &str,
) -> Result<String> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input
        .read_line(&mut line)
        .with_context(|| format!("Failed to read init wizard {label}"))?
        == 0
    {
        bail!(
            "init wizard ended before the {label} was answered; rerun interactively or pass --defaults for default answers"
        );
    }
    let answer = line.trim().to_ascii_lowercase();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

#[derive(Debug)]
struct InitPresetMetadata {
    preset_summary: String,
    frontends: Vec<(String, String)>,
}

impl InitPresetMetadata {
    fn load() -> Self {
        let descriptor = ScaffoldPreset::RustReact.descriptor();
        let preset_summary = descriptor.summary().to_string();
        let frontends = descriptor
            .frontend_shorthands()
            .iter()
            .map(|frontend| {
                (
                    frontend.name().to_string(),
                    frontend.expands_to().to_string(),
                )
            })
            .collect();
        Self {
            preset_summary,
            frontends,
        }
    }
}

#[cfg(test)]
#[path = "init_wizard_tests.rs"]
mod tests;
