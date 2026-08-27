#[test]
fn work_refine_requires_explicit_refinement_before_writing() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo_without_refinement(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Refine(
            crate::cli::WorkRefineOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                max_iterations: 1,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["refinement_required"], true);
    assert_eq!(output["iterations"].as_array().unwrap().len(), 0);
    assert!(!temp.path().join("fixed.txt").exists());
}

fn write_review_codex_stub(path: &Path) {
    // Review stubs use .agent sentinel files to model state changes between
    // review and refine iterations inside one fixture repo.
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  if [ -f .agent/clean-review ]; then
    printf '{"summary":"clean","findings":[]}\n' > "$out"
  else
    printf '{"summary":"needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  fi
  exit 0
fi
mkdir -p .agent
touch .agent/clean-review
if [ "$#" -ne 9 ] || [ "$1 $2 $3 $4 $5 $6 $7" != "--ask-for-approval never exec --sandbox workspace-write --ephemeral -o" ] || [ -z "$8" ] || [ "$9" != "-" ]; then
  echo "unexpected refine args: $*" >&2
  exit 2
fi
printf 'stdin' > prompt-source.txt
printf 'refined\n' > "$8"
cat >/dev/null
printf 'fixed\n' > fixed.txt
"#,
    );
}

fn write_verbose_refine_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  if [ -f .agent/clean-review ]; then
    printf '{"summary":"clean","findings":[]}\n' > "$out"
  else
    printf '{"summary":"needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  fi
  exit 0
fi
mkdir -p .agent
touch .agent/clean-review
if [ "$#" -ne 9 ] || [ "$1 $2 $3 $4 $5 $6 $7" != "--ask-for-approval never exec --sandbox workspace-write --ephemeral -o" ] || [ -z "$8" ] || [ "$9" != "-" ]; then
  echo "unexpected verbose refine args: $*" >&2
  exit 2
fi
cat >/dev/null
printf 'refined\n' > "$8"
printf 'fixed\n' > fixed.txt
head -c 4194305 /dev/zero >&2
"#,
    );
}

fn read_receipts(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

fn write_invalid_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf 'not json\n' > "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_many_findings_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"many findings","findings":[' > "$out"
  i=1
  while [ "$i" -le 105 ]; do
    if [ "$i" -gt 1 ]; then
      printf ',' >> "$out"
    fi
    printf '{"severity":"critical","path":"src.rs","line":1,"issue":"issue %s","evidence":"bare propagation","recommendation":"add context"}' "$i" >> "$out"
    i=$((i + 1))
  done
  printf ']}\n' >> "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_missing_review_output_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  printf 'review finished without file output\n'
  exit 0
fi
exit 0
"#,
    );
}

fn write_clean_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"clean","findings":[]}\n' > "$out"
  exit 0
fi
exit 0
"#,
    );
}

fn write_low_finding_failed_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"tool failed with nonblocking finding","findings":[{"severity":"suggestion","path":"src.rs","line":1,"issue":"minor style","evidence":"style only","recommendation":"cleanup later"}]}\n' > "$out"
  exit 2
fi
exit 2
"#,
    );
}

fn write_stubborn_review_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  printf '{"summary":"still needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"still missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  exit 0
fi
cat >/dev/null
printf 'attempted refine\n'
"#,
    );
}

fn write_failing_refine_codex_stub(path: &Path) {
    write_codex_stub(
        path,
        r#"#!/bin/sh
if [ "$1" = "exec" ] && [ "$2" = "review" ]; then
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "-o" ]; then
      out="$arg"
    fi
    prev="$arg"
  done
  if [ -f .agent/refine-failed ]; then
    printf '{"summary":"still needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"post-failure review","evidence":"partial fixer state","recommendation":"repair partial edits"}]}\n' > "$out"
  else
    printf '{"summary":"needs work","findings":[{"severity":"critical","path":"src.rs","line":1,"issue":"missing context","evidence":"bare propagation","recommendation":"add context"}]}\n' > "$out"
  fi
  exit 0
fi
mkdir -p .agent
touch .agent/refine-failed
cat >/dev/null
printf 'refine failed\n' >&2
exit 42
"#,
    );
}

#[test]
fn work_gates_use_direct_receipt_when_prior_batch_ended_in_same_millisecond() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 200,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );
    let direct_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 200,
            ended_at_ms: 200,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], direct_receipt_id);
}

#[test]
fn work_gates_use_legacy_batch_receipt_without_receipt_ids() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let legacy_batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(
        gates["gates"][0]["freshness_receipt_id"],
        legacy_batch_receipt_id
    );
}

#[test]
fn work_gates_use_exact_batch_receipt_id_when_batches_interleave() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    let tool_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": [tool_receipt_id],
            }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );
    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": ["receipt_other_tool"],
            }),
            plan_id: "plan_1",
            started_at_ms: 90,
            ended_at_ms: 130,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], batch_receipt_id);
}

#[test]
fn work_gates_keep_failed_checks_failed_when_freshness_is_unknown() {
    let temp = tempdir().unwrap();
    write_failing_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("jig.custom_check failed with status 7"));

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["gates"][0]["freshness"], "unknown");
    assert_eq!(gates["failed_required"][0], "custom");
}

#[test]
fn old_flat_memory_tool_names_are_not_supported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, "jig.session_start", json!({}))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported tool: jig.session_start"));
}
