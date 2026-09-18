use super::*;
use crate::repository::freshness::adoption::{Request, preview};
use std::process::{Command, Stdio};

fn apply_adoption(ctx: &RepoContext, exhaustive: bool) -> RepoContext {
    let paths = [".jig.toml", ".agent/jig-contract.json"];
    let before = paths.map(|path| fs::read(ctx.root().join(path)).unwrap());
    let proposed = preview(
        ctx,
        &Request {
            targets: ["api:test", "web:test"]
                .map(|target| target.parse().unwrap())
                .into(),
            assert_worktree: true,
            assert_exhaustive: exhaustive,
            patch: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        paths.map(|path| fs::read(ctx.root().join(path)).unwrap()),
        before,
        "adoption preview mutated repository authority"
    );
    let patch = proposed["patch"].as_str().unwrap();
    assert!(!patch.is_empty());
    let mut child = Command::new("git")
        .args(["apply", "-"])
        .current_dir(ctx.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(patch.as_bytes())
        .unwrap();
    let applied = child.wait_with_output().unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let adopted = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    run_git(
        adopted.root(),
        &["add", ".jig.toml", ".agent/jig-contract.json"],
    );
    run_git(
        adopted.root(),
        &["commit", "-qm", "Example freshness adoption"],
    );
    adopted
}

fn assert_reuses(
    ctx: &RepoContext,
    plan: &str,
    receipts: &BTreeMap<String, Value>,
    executions: &str,
) {
    let inspected = inspect(ctx, plan);
    assert_eq!(inspected["overall"], "passed", "{inspected:#}");
    assert_eq!(inspected["recovery"]["execute"], json!([]));
    assert_eq!(invocations(ctx), executions, "inspection executed a check");
    let reused = check(ctx, plan);
    assert_eq!(reused["ok"], true, "{reused:#}");
    assert!(reused["run"].is_null(), "{reused:#}");
    assert_eq!(&receipt_ids(&reused), receipts);
    assert_eq!(invocations(ctx), executions);
}

fn assert_reruns(
    ctx: &RepoContext,
    plan: &str,
    receipts: &mut BTreeMap<String, Value>,
    executions: &mut String,
    expected: &[&str],
) {
    let inspected = inspect(ctx, plan);
    assert_eq!(inspected["overall"], "blocked", "{inspected:#}");
    let targets = expected
        .iter()
        .map(|component| format!("{component}:test").parse::<TargetId>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(inspected["recovery"]["execute"], json!(targets));
    assert_eq!(invocations(ctx), *executions, "inspection executed a check");
    let checked = check(ctx, plan);
    assert_eq!(checked["ok"], true, "{checked:#}");
    let current = receipt_ids(&checked);
    for component in ["api", "web"] {
        if expected.contains(&component) {
            assert_ne!(
                current[component], receipts[component],
                "{component}: {checked:#}"
            );
        } else {
            assert_eq!(
                current[component], receipts[component],
                "{component}: {checked:#}"
            );
        }
    }
    let current_executions = invocations(ctx);
    assert!(current_executions.starts_with(executions.as_str()));
    let mut added = current_executions[executions.len()..]
        .lines()
        .collect::<Vec<_>>();
    added.sort_unstable();
    assert_eq!(added, expected, "unexpected action executions");
    *receipts = current;
    *executions = current_executions;
    assert_eq!(inspect(ctx, plan)["overall"], "passed");
}

#[test]
fn adopted_freshness_reuses_and_invalidates_real_work_evidence_by_owned_scope() {
    for exhaustive in [false, true] {
        let temp = tempdir().unwrap();
        let (ctx, _) = worktree_fixture(temp.path(), "whole_repository", Some("git"));
        let ctx = apply_adoption(&ctx, exhaustive);
        let plan = crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: "Example adopted freshness".into(),
                body: None,
                body_file: None,
                base: Some("HEAD".into()),
            },
        )
        .unwrap();
        let plan_id = plan["plan_id"].as_str().unwrap();
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// checked edit\n",
        )
        .unwrap();
        let first = check(&ctx, plan_id);
        assert_eq!(first["ok"], true, "exhaustive={exhaustive}: {first:#}");
        let mut receipts = receipt_ids(&first);
        assert_eq!(receipts.len(), 2);
        let mut executions = invocations(&ctx);
        assert_eq!(executions.lines().count(), 2);

        run_git(ctx.root(), &["add", "api/example.go"]);
        assert_reuses(&ctx, plan_id, &receipts, &executions);
        run_git(
            ctx.root(),
            &["commit", "-qm", "Example adopted checked edit"],
        );
        assert_reuses(&ctx, plan_id, &receipts, &executions);

        fs::write(
            ctx.root().join("unrelated.md"),
            "Example documentation edit\n",
        )
        .unwrap();
        if exhaustive {
            assert_reuses(&ctx, plan_id, &receipts, &executions);
        } else {
            assert_reruns(
                &ctx,
                plan_id,
                &mut receipts,
                &mut executions,
                &["api", "web"],
            );
        }
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// changed input\n",
        )
        .unwrap();
        assert_reruns(
            &ctx,
            plan_id,
            &mut receipts,
            &mut executions,
            if exhaustive {
                &["api"]
            } else {
                &["api", "web"]
            },
        );

        if exhaustive {
            let added = ctx.root().join("api/added.go");
            let renamed = ctx.root().join("api/renamed.go");
            fs::write(&added, "package example\n").unwrap();
            assert_reruns(&ctx, plan_id, &mut receipts, &mut executions, &["api"]);
            fs::rename(&added, &renamed).unwrap();
            assert_reruns(&ctx, plan_id, &mut receipts, &mut executions, &["api"]);
            fs::remove_file(renamed).unwrap();
            assert_reruns(&ctx, plan_id, &mut receipts, &mut executions, &["api"]);
        }
    }
}
