use super::*;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use jig_execution::NoopExecutionObserver;
use tempfile::tempdir;

use super::review_thread_budget::ReviewThreadUpdateBudget;
use super::review_thread_reply::{ReviewThreadReply, post_review_thread_reply};
use super::review_thread_witness::{
    ReviewThreadWitness, legacy_review_thread_reply_marker, observed_review_thread_witnesses,
    review_reply_generation, review_thread_reply_marker,
};
use super::review_threads::review_thread_comment_with_markers;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

#[test]
fn reply_marker_binds_feedback_generation_but_not_retry_wording() {
    let first = json!({
        "comments": {"nodes": [{
            "id": "COMMENT_1",
            "updatedAt": "2026-09-01T10:00:00Z",
            "body": "Please add a regression test",
            "author": {"trusted": true},
        }]},
    });
    let later = json!({
        "comments": {"nodes": [
            {
                "id": "COMMENT_1",
                "updatedAt": "2026-09-01T10:00:00Z",
                "body": "Please add a regression test",
                "author": {"trusted": true},
            },
            {
                "id": "COMMENT_2",
                "updatedAt": "2026-09-01T11:00:00Z",
                "body": "Please cover the cancellation path too",
                "author": {"trusted": true},
            },
        ]},
    });
    let first = ReviewThreadWitness {
        reply_generation: review_reply_generation(&first),
        ..ReviewThreadWitness::default()
    };
    let later = ReviewThreadWitness {
        reply_generation: review_reply_generation(&later),
        ..ReviewThreadWitness::default()
    };
    let original = review_thread_reply_marker("PRRT_1", "same-head", &first);
    assert_eq!(
        original,
        review_thread_reply_marker("PRRT_1", "same-head", &first)
    );
    assert_ne!(
        original,
        review_thread_reply_marker("PRRT_1", "same-head", &later)
    );

    let legacy =
        legacy_review_thread_reply_marker("PRRT_1", "same-head", &first, "Earlier wording.");
    let state = json!({
        "data": {"node": {"comments": {"nodes": [{
            "id": "PRRC_LEGACY",
            "url": "https://example.invalid/legacy",
            "body": legacy,
            "viewerDidAuthor": true,
        }]}}}
    });
    assert_eq!(
        review_thread_comment_with_markers(&state, &[&original, &legacy])
            .and_then(|comment| comment["id"].as_str()),
        Some("PRRC_LEGACY")
    );
}

#[test]
fn trusted_human_marker_quote_advances_the_reply_generation() {
    let original = json!({
        "id": "PRRC_ORIGINAL",
        "updatedAt": "2026-09-03T10:00:00Z",
        "body": "Please add a regression test",
        "viewerDidAuthor": false,
        "author": {"trusted": true},
    });
    let jig_reply = json!({
        "id": "PRRC_JIG",
        "updatedAt": "2026-09-03T10:05:00Z",
        "body": "Addressed. <!-- jig-pr-manager:review-reply:v3:fixture -->",
        "viewerDidAuthor": true,
        "author": {"trusted": true},
    });
    let before_quote = json!({
        "comments": {"nodes": [original, jig_reply]},
    });
    let original_only = json!({
        "comments": {"nodes": [original]},
    });
    let after_quote = json!({
        "comments": {"nodes": [
            original,
            {
                "id": "PRRC_HUMAN_QUOTE",
                "updatedAt": "2026-09-03T10:10:00Z",
                "body": "This still needs work: <!-- jig-pr-manager:review-reply:v3:fixture -->",
                "viewerDidAuthor": false,
                "author": {"trusted": true},
            },
        ]},
    });
    assert_eq!(
        review_reply_generation(&before_quote),
        review_reply_generation(&original_only)
    );
    assert_ne!(
        review_reply_generation(&before_quote),
        review_reply_generation(&after_quote)
    );
}

#[test]
fn trusted_human_marker_quote_requires_a_new_remote_reply() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    let original = json!({
        "id": "PRRC_ORIGINAL",
        "updatedAt": "2026-09-03T10:00:00Z",
        "body": "Please add a regression test",
        "viewerDidAuthor": false,
        "author": {"trusted": true},
    });
    let original_thread = json!({"comments": {"nodes": [original]}});
    let old_witness = ReviewThreadWitness {
        reply_generation: review_reply_generation(&original_thread),
        ..ReviewThreadWitness::default()
    };
    let old_marker = review_thread_reply_marker("PRRT_1", "pushed-head", &old_witness);
    let observed = json!({
        "review_threads": {"nodes": [{
            "id": "PRRT_1",
            "is_resolved": false,
            "has_trusted_comment": true,
            "comments": {
                "total_count": 3,
                "nodes": [
                    original,
                    {
                        "id": "PRRC_PRIOR",
                        "updatedAt": "2026-09-03T10:05:00Z",
                        "body": format!("Addressed. {old_marker}"),
                        "viewerDidAuthor": true,
                        "author": {"trusted": true},
                    },
                    {
                        "id": "PRRC_HUMAN_QUOTE",
                        "updatedAt": "2026-09-03T10:10:00Z",
                        "body": format!("Still open: {old_marker}"),
                        "viewerDidAuthor": false,
                        "author": {"trusted": true},
                    },
                ],
            },
        }]},
    });
    let witness = observed_review_thread_witnesses(&observed)
        .remove("PRRT_1")
        .unwrap();
    assert_ne!(
        review_thread_reply_marker("PRRT_1", "pushed-head", &witness),
        old_marker
    );
    let calls = temp.path().join("gh-calls");
    let gh = temp.path().join("gh-human-quote-stub.sh");
    fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$JIG_TEST_GH_CALLS"
case "$*" in
  *ReviewThreadState*)
    printf '%s\n' "{\"data\":{\"node\":{\"id\":\"PRRT_1\",\"comments\":{\"pageInfo\":{\"hasPreviousPage\":false,\"startCursor\":null},\"nodes\":[{\"id\":\"PRRC_PRIOR\",\"url\":\"https://example.invalid/prior\",\"body\":\"Addressed. $JIG_TEST_OLD_MARKER\",\"viewerDidAuthor\":true},{\"id\":\"PRRC_HUMAN_QUOTE\",\"url\":\"https://example.invalid/quote\",\"body\":\"Still open: $JIG_TEST_OLD_MARKER\",\"viewerDidAuthor\":false}]}}}}"
    ;;
  *ReviewThreadWitnessState*)
    printf '%s\n' "{\"data\":{\"node\":{\"id\":\"PRRT_1\",\"isResolved\":false,\"pullRequest\":{\"headRefOid\":\"pushed-head\"},\"comments\":{\"totalCount\":3,\"pageInfo\":{\"hasPreviousPage\":false,\"startCursor\":null},\"nodes\":[{\"id\":\"PRRC_ORIGINAL\",\"updatedAt\":\"2026-09-03T10:00:00Z\",\"body\":\"Please add a regression test\"},{\"id\":\"PRRC_PRIOR\",\"updatedAt\":\"2026-09-03T10:05:00Z\",\"body\":\"Addressed. $JIG_TEST_OLD_MARKER\"},{\"id\":\"PRRC_HUMAN_QUOTE\",\"updatedAt\":\"2026-09-03T10:10:00Z\",\"body\":\"Still open: $JIG_TEST_OLD_MARKER\"}]}}}}"
    ;;
  *addPullRequestReviewThreadReply*)
    printf '%s\n' '{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_NEW","url":"https://example.invalid/new"}}}}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
    let _calls = EnvVarGuard::set("JIG_TEST_GH_CALLS", calls.as_os_str());
    let _marker = EnvVarGuard::set("JIG_TEST_OLD_MARKER", old_marker.as_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);
    let response = post_review_thread_reply(
        &ctx,
        "PRRT_1",
        "Addressed after the follow-up.",
        "pushed-head",
        &witness,
        &mut NoopExecutionObserver,
        &mut budget,
    )
    .unwrap();
    let ReviewThreadReply::Posted(response) = response else {
        panic!("trusted human follow-up should receive a new reply");
    };
    assert_eq!(
        response["data"]["addPullRequestReviewThreadReply"]["comment"]["id"],
        "PRRC_NEW"
    );
    assert!(
        fs::read_to_string(calls)
            .unwrap()
            .contains("addPullRequestReviewThreadReply")
    );
}

#[test]
fn changed_retry_wording_reconciles_the_reply_from_the_prior_tick() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    let calls = temp.path().join("gh-calls");
    let gh = temp.path().join("gh-retry-reconciliation-stub.sh");
    fs::write(
            &gh,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$JIG_TEST_GH_CALLS"
case "$*" in
  *ReviewThreadState*)
    printf '%s\n' "{\"data\":{\"node\":{\"id\":\"PRRT_1\",\"comments\":{\"pageInfo\":{\"hasPreviousPage\":false,\"startCursor\":null},\"nodes\":[{\"id\":\"PRRC_PRIOR\",\"url\":\"https://example.invalid/prior\",\"body\":\"$JIG_TEST_REPLY_MARKER\",\"viewerDidAuthor\":true}]}}}}"
    ;;
  *addPullRequestReviewThreadReply*) exit 9 ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    let witness = ReviewThreadWitness::default();
    let marker = review_thread_reply_marker("PRRT_1", "pushed-head", &witness);
    let _gh = EnvVarGuard::set("JIG_GH_BIN", gh.as_os_str());
    let _calls = EnvVarGuard::set("JIG_TEST_GH_CALLS", calls.as_os_str());
    let _marker = EnvVarGuard::set("JIG_TEST_REPLY_MARKER", marker.as_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), 1);

    let response = post_review_thread_reply(
        &ctx,
        "PRRT_1",
        "Different wording generated by the retry.",
        "pushed-head",
        &witness,
        &mut NoopExecutionObserver,
        &mut budget,
    )
    .unwrap();
    let ReviewThreadReply::Posted(response) = response else {
        panic!("prior reply should be reconciled");
    };

    assert_eq!(response["_jig"]["reconciled"], true);
    assert_eq!(
        response["data"]["addPullRequestReviewThreadReply"]["comment"]["id"],
        "PRRC_PRIOR"
    );
    let calls = fs::read_to_string(calls).unwrap();
    assert!(calls.contains("ReviewThreadState"), "{calls}");
    assert!(
        !calls.contains("addPullRequestReviewThreadReply"),
        "{calls}"
    );
}

#[test]
fn reply_reconciliation_requires_githubs_viewer_authorship_fact() {
    let marker =
        review_thread_reply_marker("PRRT_1", "pushed-head", &ReviewThreadWitness::default());
    let spoofed = json!({
        "data": {"node": {"comments": {"nodes": [{
            "id": "PRRC_SPOOFED",
            "url": "https://example.invalid/spoofed",
            "body": marker,
            "viewerDidAuthor": false,
        }]}}}
    });
    let owned = json!({
        "data": {"node": {"comments": {"nodes": [{
            "id": "PRRC_OWNED",
            "url": "https://example.invalid/owned",
            "body": marker,
            "viewerDidAuthor": true,
        }]}}}
    });

    assert!(review_thread_comment_with_markers(&spoofed, &[&marker]).is_none());
    assert_eq!(
        review_thread_comment_with_markers(&owned, &[&marker])
            .and_then(|comment| comment["id"].as_str()),
        Some("PRRC_OWNED")
    );
}
