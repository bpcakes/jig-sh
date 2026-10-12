use jig_policy::test_support::write_v6_schema_policy_repo;

use super::common::init_git_repo;
use super::*;
use crate::command::{CheckCommand, RepositoryCheckRequest, RuntimeCommand};

#[test]
fn v6_repository_schema_failure_preserves_the_generator_exit_and_output() {
    let temp = tempdir().unwrap();
    write_v6_schema_policy_repo(
        temp.path(),
        "true",
        "printf 'generator stdout'; printf 'generator stderr' >&2; exit 7",
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Check(CheckCommand::Repository(RepositoryCheckRequest {
            selectors: vec!["api:schema".into()],
            profile: None,
            affected_base: None,
            comparison: None,
            explain: false,
            fail_fast: false,
        })),
    )
    .unwrap();

    assert_eq!(output["run"]["conclusion"], "failure");
    assert_eq!(output["run"]["targets"][0]["conclusion"], "failure");
    assert_eq!(output["run"]["targets"][0]["exit_code"], 7);
    assert_eq!(output["results"][0]["response"]["result"]["exit_status"], 7);
    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "generator stdout"
    );
    assert_eq!(
        output["results"][0]["response"]["result"]["stderr"],
        "generator stderr"
    );
}
