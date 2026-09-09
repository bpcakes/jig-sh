use super::{CheckComparisonOpts, ToolOpts};
use clap::{Args, ValueEnum};
use jig_contract::ActionEffect;

#[derive(Args, Debug)]
pub(crate) struct RepositoryRunOpts {
    #[command(flatten)]
    tool: ToolOpts,
    #[arg(value_name = "SELECTOR", help = "Component action or target selectors")]
    selectors: Vec<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Select a checked-in target profile"
    )]
    profile: Option<String>,
    #[arg(
        long,
        value_name = "GIT_REF",
        help = "Select targets affected since a Git ref"
    )]
    affected: Option<String>,
    #[arg(
        long,
        help = "Print the plan without creating a run or executing commands"
    )]
    explain: bool,
    #[arg(long, help = "Stop scheduling targets after the first failure")]
    fail_fast: bool,
    #[arg(
        long = "approve-effect",
        value_enum,
        help = "Approve each worktree or external effect in this exact plan"
    )]
    approved_effects: Vec<ApprovedEffect>,
    #[arg(
        long = "arg",
        value_name = "TARGET:NAME=VALUE",
        help = "Bind a declared literal string (repeatable)"
    )]
    arguments: Vec<String>,
    #[command(flatten)]
    comparison: CheckComparisonOpts,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ApprovedEffect {
    Worktree,
    External,
}

impl TryFrom<RepositoryRunOpts> for crate::command::RepositoryRunRequest {
    type Error = anyhow::Error;
    fn try_from(opts: RepositoryRunOpts) -> Result<Self, Self::Error> {
        Ok(Self {
            arguments: crate::repository::arguments::parse_cli(opts.arguments)?,
            comparison: opts.comparison.request()?,
            selectors: opts.selectors,
            profile: opts.profile,
            affected_base: opts.affected,
            explain: opts.explain,
            fail_fast: opts.fail_fast,
            approved_effects: opts
                .approved_effects
                .into_iter()
                .map(|effect| match effect {
                    ApprovedEffect::Worktree => ActionEffect::Worktree,
                    ApprovedEffect::External => ActionEffect::External,
                })
                .collect(),
            tool: opts.tool.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, CommandKind};
    use clap::Parser;

    #[test]
    fn foreground_run_parses_repeatable_target_arguments() {
        let cli = Cli::try_parse_from([
            "jig",
            "run",
            "api:generate",
            "--arg",
            "api:generate:message=a=b",
            "--arg",
            "api:generate:optional=",
        ])
        .unwrap();
        let CommandKind::Run(opts) = cli.command else {
            panic!("expected run");
        };
        let request = crate::command::RepositoryRunRequest::try_from(opts).unwrap();
        assert_eq!(
            request.arguments[&"api:generate".parse().unwrap()]["message"],
            "a=b"
        );
        let cli = Cli::try_parse_from([
            "jig",
            "run",
            "api:generate",
            "--arg",
            "api:generate:message=a",
            "--arg",
            "api:generate:message=b",
        ])
        .unwrap();
        let CommandKind::Run(opts) = cli.command else {
            panic!("expected run");
        };
        assert!(
            crate::command::RepositoryRunRequest::try_from(opts)
                .unwrap_err()
                .to_string()
                .contains("duplicate argument")
        );
    }

    #[test]
    fn foreground_run_parses_selection_approval_and_native_comparison() {
        let cli = Cli::try_parse_from([
            "jig",
            "--json",
            "run",
            "api:generate",
            "--approve-effect",
            "worktree",
            "--plan-id",
            "plan_fixture",
            "--fail-fast",
            "--comparison-strict-inventory",
        ])
        .unwrap();
        assert!(cli.json);
        let CommandKind::Run(opts) = cli.command else {
            panic!("expected run");
        };
        let request = crate::command::RepositoryRunRequest::try_from(opts).unwrap();
        assert_eq!(request.selectors, ["api:generate"]);
        assert_eq!(request.approved_effects, [ActionEffect::Worktree]);
        assert!(request.fail_fast);
        assert!(request.comparison.is_some());
        assert_eq!(
            request.tool.into_parts(),
            (Some("plan_fixture".into()), true)
        );
        for args in [
            vec!["jig", "run", "--approve-effect", "read-only"],
            vec![
                "jig",
                "run",
                "--comparison-staged",
                "--comparison-base",
                "HEAD",
            ],
            vec!["jig", "run", "--comparison-exact-tree", "HEAD"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
}
