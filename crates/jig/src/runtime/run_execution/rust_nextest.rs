use super::*;
use jig_contract::PreparedRustInputV1;

pub(super) fn run_rust_nextest_target(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    control: &mut TargetExecutionControl<'_>,
) -> TargetCapture {
    let Some(prepared) = planned
        .prepared_rust_input
        .as_ref()
        .filter(|input| input.schema_version == 1)
    else {
        return TargetCapture::blocked(format!(
            "typed Rust target '{}' has no supported authenticated prepared input",
            planned.target
        ));
    };
    let (command, environment) = rust_command(prepared);
    let capture = target::run_process_target(ctx, planned, command, None, &environment, control);
    classify_empty_selection(capture)
}

fn rust_command(prepared: &PreparedRustInputV1) -> (Command, BTreeMap<String, String>) {
    let mut command = Command::new("cargo");
    command.args(&prepared.args);
    let environment = BTreeMap::from([(
        "CARGO_NET_OFFLINE".into(),
        if prepared.context.offline {
            "true"
        } else {
            "false"
        }
        .into(),
    )]);
    (command, environment)
}

fn classify_empty_selection(mut capture: TargetCapture) -> TargetCapture {
    if capture.exit_code == Some(4) {
        capture.conclusion = RunConclusion::Failure;
        capture.receipt_exit_status = 4;
        let message = "nextest selected no tests; empty selection is not passing test evidence";
        capture.stderr.push_str(message);
        capture.stderr.push('\n');
        capture.findings.push(finding(message, "empty_selection"));
    }
    capture
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::TestRepoBuilder;
    use jig_contract::{ActionIntent, RustNextestConfigV1, RustScopeDispositionV1};

    fn prepared() -> PreparedRustInputV1 {
        PreparedRustInputV1 {
            schema_version: 1,
            disposition: RustScopeDispositionV1::Narrowed,
            reasons: Vec::new(),
            packages: vec!["example-package@0.1.0".into()],
            targets: vec![jig_contract::RustTargetV1::Lib {}],
            context: Default::default(),
            comparison_base: None,
            args: vec![
                "nextest".into(),
                "run".into(),
                "--lib".into(),
                "--package".into(),
                "example-package@0.1.0".into(),
                "-E".into(),
                "test(example); echo untouched".into(),
            ],
        }
    }

    #[test]
    fn rust_nextest_uses_fixed_program_and_literal_prepared_arguments() {
        let mut prepared = prepared();
        let (command, environment) = rust_command(&prepared);
        assert_eq!(command.get_program(), "cargo");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            prepared
                .args
                .iter()
                .map(std::ffi::OsStr::new)
                .collect::<Vec<_>>()
        );
        assert_eq!(environment["CARGO_NET_OFFLINE"], "true");
        prepared.context.offline = false;
        assert_eq!(rust_command(&prepared).1["CARGO_NET_OFFLINE"], "false");
    }

    #[test]
    fn rust_nextest_empty_selection_is_distinct_failed_evidence() {
        let capture = classify_empty_selection(TargetCapture::from_process(
            4,
            String::new(),
            String::new(),
            ResultParser::ExitCode,
        ));
        assert_eq!(capture.conclusion, RunConclusion::Failure);
        assert_eq!(capture.receipt_exit_status, 4);
        assert!(
            capture
                .findings
                .iter()
                .any(|finding| finding.source.as_deref() == Some("empty_selection"))
        );
        assert!(capture.stderr.contains("not passing test evidence"));
        let capture = classify_empty_selection(TargetCapture::from_process(
            0,
            String::new(),
            String::new(),
            ResultParser::ExitCode,
        ));
        assert_eq!(capture.conclusion, RunConclusion::Success);
        assert!(capture.findings.is_empty());
    }

    #[test]
    fn rust_nextest_blocks_missing_and_unknown_prepared_input_without_spawning() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .required_commands(["rust_test_command"])
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut target = PlannedTarget::new(
            "repo:test".parse().unwrap(),
            ActionIntent::Check,
            ActionRunner::RustNextestV1 {
                configuration: RustNextestConfigV1 {
                    workspace_manifest: "Cargo.toml".into(),
                    focused: true,
                    context: Default::default(),
                    cargo_profile: None,
                    nextest_profile: None,
                },
            },
            "sha256:input",
        );
        let mut unknown = prepared();
        unknown.schema_version = 2;
        for input in [None, Some(unknown)] {
            target.prepared_rust_input = input;
            let cancelled = || Ok(false);
            let mut run_control = CancellationOnlyRunControl {
                cancelled: &cancelled,
            };
            let mut control = TargetExecutionControl::new(&ctx, &target, &mut run_control);
            let capture = run_rust_nextest_target(&ctx, &target, &mut control);
            assert_eq!(capture.conclusion, RunConclusion::Blocked);
            assert!(!capture.may_have_executed);
        }
    }
}
