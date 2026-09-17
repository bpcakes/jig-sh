use std::io::Write;

use anyhow::{Context, Result};

use crate::cli::FreshnessOpts;
use crate::cli::output::print_json;
use crate::context::RepoContext;
use crate::repository::freshness::adoption;

pub(super) fn run(opts: &FreshnessOpts, json_output: bool) -> Result<()> {
    let output = adoption::preview(
        &RepoContext::load()?,
        &adoption::Request {
            targets: opts.targets.clone(),
            assert_worktree: opts.assert_worktree,
            assert_exhaustive: opts.assert_exhaustive,
            inputs: opts.inputs.clone(),
            patch: opts.patch,
        },
    )?;
    if json_output {
        print_json(&output)?;
    } else if opts.patch {
        write!(
            std::io::stdout().lock(),
            "{}",
            output["patch"]
                .as_str()
                .context("freshness preview omitted its patch")?
        )?;
    } else {
        writeln!(
            std::io::stdout().lock(),
            "{}",
            adoption::format_report(&output)
        )?;
    }
    Ok(())
}
