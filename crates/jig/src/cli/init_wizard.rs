use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{Context, Result, bail};

use crate::bootstrap::{
    self, InitOpts, ScaffoldDb, ScaffoldFrontend, ScaffoldPreset, parse_scaffold_frontend,
};

pub(super) fn prepare_init_interaction(
    opts: &mut InitOpts,
) -> Result<bootstrap::PreparedInitAnswers> {
    let mut prepared = prepare_init_answers(opts)?;
    prepared.move_effective_to(&mut opts.answers)?;
    let terminals_available = io::stdin().is_terminal() && io::stderr().is_terminal();
    let stdin = io::stdin();
    let stderr = io::stderr();
    let mut input = stdin.lock();
    let mut output = stderr.lock();
    let policy = InitInteractionPolicy::resolve(opts, terminals_available);
    prepare_merged_init_interaction(opts, &mut prepared, policy, &mut input, &mut output)?;
    Ok(prepared)
}

pub(super) fn preflight_init_package_manager(opts: &InitOpts) -> Result<()> {
    preflight_init_package_manager_with(opts, crate::doctor::program_available_on_path)
}

fn preflight_init_package_manager_with(
    opts: &InitOpts,
    available: impl FnOnce(&str) -> bool,
) -> Result<()> {
    if !opts
        .scaffold
        .preset
        .is_some_and(ScaffoldPreset::requires_web_package_manager)
    {
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
) -> Result<bootstrap::PreparedInitAnswers> {
    let mut prepared = prepare_init_answers(opts)?;
    prepared.move_effective_to(&mut opts.answers)?;
    let policy = InitInteractionPolicy::resolve(opts, true);
    prepare_merged_init_interaction(opts, &mut prepared, policy, input, output)?;
    Ok(prepared)
}

#[cfg(test)]
fn prepare_init_interaction_with_terminal<R: BufRead, W: Write>(
    opts: &mut InitOpts,
    terminals_available: bool,
    input: &mut R,
    output: &mut W,
) -> Result<bootstrap::PreparedInitAnswers> {
    let mut prepared = prepare_init_answers(opts)?;
    prepared.move_effective_to(&mut opts.answers)?;
    let policy = InitInteractionPolicy::resolve(opts, terminals_available);
    prepare_merged_init_interaction(opts, &mut prepared, policy, input, output)?;
    Ok(prepared)
}

fn prepare_init_answers(opts: &InitOpts) -> Result<bootstrap::PreparedInitAnswers> {
    bootstrap::prepare_init_answers_for_interaction(&opts.answers).map_err(|error| {
        if let Some(preset @ (ScaffoldPreset::RustLibrary | ScaffoldPreset::RustCli)) =
            opts.scaffold.preset
        {
            anyhow::anyhow!(
                "Failed to prepare --preset {} answers: {error:#}",
                preset.as_str()
            )
        } else {
            error
        }
    })
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
    prepared: &mut bootstrap::PreparedInitAnswers,
    policy: InitInteractionPolicy,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    opts.scaffold.normalize_minimal_harness_shape(&opts.answers);
    prepared.validate_selected_preset(&opts.scaffold, &opts.answers)?;
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
    prepared.validate_selected_preset(&opts.scaffold, &opts.answers)?;
    opts.scaffold.apply_init_answer_defaults(&mut opts.answers);
    opts.scaffold.validate_init_invariants(&opts.answers)
}

fn apply_project_shape_defaults(opts: &mut InitOpts) -> Result<()> {
    if opts.scaffold.preset.is_none() {
        opts.scaffold.preset = Some(ScaffoldPreset::RustReact);
    }
    if opts
        .scaffold
        .preset
        .is_some_and(ScaffoldPreset::requires_database_choice)
    {
        opts.scaffold.db.get_or_insert(ScaffoldDb::None);
    }
    if opts
        .scaffold
        .preset
        .is_some_and(ScaffoldPreset::requires_frontend_choice)
        && !opts.scaffold.has_frontends()
        && opts.answers.frontend_apps.is_empty()
    {
        opts.scaffold
            .frontends
            .push(parse_scaffold_frontend("web").map_err(|error| anyhow::anyhow!(error))?);
    }
    if opts
        .scaffold
        .preset
        .is_some_and(ScaffoldPreset::requires_go_module)
        && opts.answers.go_module.is_none()
    {
        opts.answers.go_module = Some(default_go_module_for_init(opts));
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
            "Init cannot prompt because {mode}; pass --preset rust-react with explicit database and frontend choices, pass --preset go-react with explicit database, frontend, and Go module choices, pass --preset harness-only, --preset rust-library, or --preset rust-cli, or use --defaults"
        );
    };
    if preset.requires_database_choice() && opts.scaffold.db.is_none() {
        bail!(
            "Init cannot prompt because {mode}; the selected application preset requires an explicit --db choice, or use --defaults"
        );
    }
    if preset.requires_frontend_choice()
        && !opts.scaffold.has_frontends()
        && opts.answers.frontend_apps.is_empty()
    {
        bail!(
            "Init cannot prompt because {mode}; the selected application preset requires --frontend/--frontends or frontend_apps in --answers-file, or use --defaults"
        );
    }
    if preset.requires_go_module() && opts.answers.go_module.is_none() {
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
            ScaffoldChoice::RustLibrary => {
                opts.scaffold.preset = Some(ScaffoldPreset::RustLibrary);
            }
            ScaffoldChoice::RustCli => {
                opts.scaffold.preset = Some(ScaffoldPreset::RustCli);
            }
        }
    }

    let Some(preset) = opts.scaffold.preset else {
        return Ok(());
    };
    let needs_database = preset.requires_database_choice() && opts.scaffold.db.is_none();
    let needs_frontends = preset.requires_frontend_choice()
        && !opts.scaffold.has_frontends()
        && opts.answers.frontend_apps.is_empty();
    if !printed_header && (needs_database || needs_frontends) {
        print_project_shape_header(output, &metadata)?;
    }
    if needs_database {
        opts.scaffold.db = Some(prompt_database(
            input,
            output,
            preset == ScaffoldPreset::GoReact,
        )?);
    }
    if needs_frontends {
        opts.scaffold.frontends = prompt_frontends(input, output, &metadata)?;
    }
    if preset.requires_go_module() && opts.answers.go_module.is_none() {
        let default = default_go_module_for_init(opts);
        opts.answers.go_module = Some(prompt_go_module(input, output, &default)?);
    }
    Ok(())
}

fn print_project_shape_header<W: Write>(
    output: &mut W,
    metadata: &InitPresetMetadata,
) -> Result<()> {
    writeln!(output, "Project shape")?;
    writeln!(output, "  1. rust-react — {}", metadata.preset_summary)?;
    writeln!(
        output,
        "  2. harness-only — Jig harness without starter application code."
    )?;
    writeln!(
        output,
        "  3. go-react — Go 1.26, chi, Huma, pgx/sqlc/Goose, and React."
    )?;
    writeln!(
        output,
        "  4. rust-library — {}",
        ScaffoldPreset::RustLibrary.descriptor().summary()
    )?;
    writeln!(
        output,
        "  5. rust-cli — {}",
        ScaffoldPreset::RustCli.descriptor().summary()
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScaffoldChoice {
    RustReact,
    HarnessOnly,
    GoReact,
    RustLibrary,
    RustCli,
}

fn prompt_scaffold_choice<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
) -> Result<ScaffoldChoice> {
    loop {
        let answer = prompt_line(
            input,
            output,
            "Project shape? [1 rust-react / 2 harness-only / 3 go-react / 4 rust-library / 5 rust-cli] (1): ",
            "1",
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
            "4" | "rust-library" => return Ok(ScaffoldChoice::RustLibrary),
            "5" | "rust-cli" => return Ok(ScaffoldChoice::RustCli),
            _ => writeln!(
                output,
                "  Enter 1, 2, 3, 4, 5, rust-react, harness-only, go-react, rust-library, or rust-cli."
            )?,
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

fn default_go_module_for_init(opts: &InitOpts) -> String {
    let repo_name = opts
        .answers
        .repo_name
        .as_deref()
        .or_else(|| opts.path.file_name().and_then(|value| value.to_str()))
        .unwrap_or("app");
    bootstrap::default_go_module(repo_name)
}

fn prompt_go_module<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    default: &str,
) -> Result<String> {
    let prompt = format!("Go module [{default}] (for example github.com/acme/my-app): ");
    loop {
        let module = prompt_value(input, output, &prompt, default, "Go module")?;
        match bootstrap::validate_go_module(&module) {
            Ok(()) => return Ok(module),
            Err(error) => writeln!(output, "  {error}")?,
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
    Ok(prompt_value(input, output, prompt, default, label)?.to_ascii_lowercase())
}

fn prompt_value<R: BufRead, W: Write>(
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
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
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

#[cfg(test)]
#[path = "init_wizard_rust_library_tests.rs"]
mod rust_library_tests;

#[cfg(test)]
#[path = "init_wizard_rust_cli_tests.rs"]
mod rust_cli_tests;

#[cfg(test)]
#[path = "init_wizard_discovery_tests.rs"]
mod discovery_tests;
