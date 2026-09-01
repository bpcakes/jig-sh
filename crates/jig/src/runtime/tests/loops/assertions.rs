use super::*;

#[cfg(unix)]
pub(super) fn assert_pr_manager_tick_output(
    output: &serde_json::Value,
    codex_home: &Path,
    codex_home_log: &Path,
) {
    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["workflow"]["kind"], "pr_manager");
    assert_eq!(output["workflow"]["codex_home_configured"], "./.codex-loop");
    assert_eq!(output["status"], "waiting");
    assert_eq!(output["actions"][0]["status"], "attempted", "{output:#}");
    let canonical_home = codex_home.canonicalize().unwrap().display().to_string();
    assert_eq!(output["actions"][0]["codex_home_resolved"], canonical_home);
    assert_eq!(fs::read_to_string(codex_home_log).unwrap(), canonical_home);
    assert_eq!(output["actions"][0]["push"]["pushed"], true);
    assert_eq!(output["actions"][0]["push"]["force"], false);
    let reasons = output["actions"][0]["reasons"].as_array().unwrap();
    assert!(reasons.iter().any(|reason| reason == "failing_checks"));
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "unresolved_review_threads")
    );
    assert!(output["actions"][0]["worker_receipt_id"].as_str().is_some());
}

#[cfg(unix)]
pub(super) fn assert_pr_manager_attempt_and_review_output(output: &serde_json::Value) {
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["replied"],
        true
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["reply_comment_id"],
        "PRRC_REPLY"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][0]["is_resolved"],
        true
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][1]["status"],
        "skipped"
    );
    assert_eq!(
        output["actions"][0]["review_thread_posts"][1]["reason"],
        "unknown_review_thread"
    );
    assert_eq!(output["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(output["attempts"][0]["item_key"], "pr-7");
    assert_eq!(
        output["attempts"][0]["item_version"],
        output["actions"][0]["push"]["final_head"]
    );
    assert_eq!(output["attempts"][0]["last_status"], "attempted");
}

#[cfg(unix)]
pub(super) fn assert_pr_manager_side_effects(root: &Path, origin: &Path, ctx: &RepoContext) {
    assert!(
        git_stdout(origin, ["show", "refs/heads/codex/widgets:src.rs"])
            .contains("fixed by pr manager")
    );
    let mutations = fs::read_to_string(root.join("gh-mutations.log")).unwrap();
    assert!(mutations.contains("addPullRequestReviewThreadReply"));
    assert!(mutations.contains("resolveReviewThread"));
    assert!(!mutations.contains("PRRT_FOREIGN"));
    let receipts = crate::state::receipts_list(
        ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: None,
            tool_name: Some(WORKER_RUN_TOOL.into()),
            failed_only: false,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(receipts["receipts"][0]["evidence"]["purpose"], "pr_manager");
    assert_eq!(
        receipts["receipts"][0]["evidence"]["codex_home_resolved"],
        "<repository-root>/.codex-loop"
    );
}
