use super::*;

#[test]
fn work_review_records_structured_codex_review_findings() {
    #[derive(Default)]
    struct PhaseObserver(Vec<(String, String, usize, usize)>);

    impl crate::execution::ExecutionObserver for PhaseObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => {
                    self.0.push((
                        "started".into(),
                        label.into(),
                        position.current(),
                        position.total(),
                    ))
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.0.push(("finished".into(), label.into(), 0, 0));
                }
                _ => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for PhaseObserver {}

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = PhaseObserver::default();

    let output = crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Review(
            crate::command::WorkReviewRequest {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(output["status"], "failed", "{output:#}");
    assert_eq!(output["reviews"][0]["gate_id"], "rust-error-handling");
    assert_eq!(output["reviews"][0]["actionable_count"], 1);
    assert_eq!(
        output["reviews"][0]["actionable_findings"][0]["severity"],
        "critical"
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["kind"], "codex_review");
    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");

    let receipts = read_receipts(temp.path());
    let worker_receipt = receipts
        .iter()
        .find(|receipt| {
            receipt["tool_name"] == WORKER_RUN_TOOL
                && receipt["evidence"]["purpose"] == "work_review"
        })
        .expect("work review should record a worker receipt");
    assert_eq!(
        output["reviews"][0]["worker_receipt_id"],
        worker_receipt["id"]
    );
    assert_eq!(worker_receipt["evidence"]["provider"], "codex");
    assert_eq!(worker_receipt["evidence"]["runner"], "codex_exec");
    assert_eq!(worker_receipt["evidence"]["mode"], "review");
    assert_eq!(
        observer.0,
        [
            ("started".into(), "rust-error-handling".into(), 1, 1),
            ("finished".into(), "rust-error-handling".into(), 0, 0),
        ]
    );
}

#[test]
fn work_review_surfaces_raw_counts_when_findings_are_truncated() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_many_findings_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    let review = &output["reviews"][0];
    assert_eq!(review["status"], "failed", "{output:#}");
    assert_eq!(review["finding_count"], 105);
    assert_eq!(review["actionable_count"], 105);
    assert_eq!(review["retained_finding_count"], 100);
    assert_eq!(review["retained_actionable_count"], 100);
    assert_eq!(review["findings_truncated"], true);
    assert_eq!(review["actionable_findings_truncated"], true);
    assert_eq!(review["findings"].as_array().unwrap().len(), 100);
    assert_eq!(review["actionable_findings"].as_array().unwrap().len(), 100);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    let gate = &gates["gates"][0];
    assert_eq!(gate["finding_count"], 105);
    assert_eq!(gate["actionable_count"], 105);
    assert_eq!(gate["retained_finding_count"], 100);
    assert_eq!(gate["retained_actionable_count"], 100);
    assert_eq!(gate["findings_truncated"], true);
    assert_eq!(gate["actionable_findings_truncated"], true);
}

#[test]
fn work_review_fails_when_codex_exits_nonzero_with_below_threshold_findings() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_low_finding_failed_review_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    let review = &output["reviews"][0];
    assert_eq!(review["status"], "failed", "{output:#}");
    assert_eq!(review["actionable_count"], 0);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "failed", "{gates:#}");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");
}

#[test]
fn work_review_records_invalid_output_when_codex_writes_no_structured_output() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_missing_review_output_codex_stub(&codex_path);
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Review(
            crate::cli::WorkReviewOpts {
                plan_id: "plan_1".into(),
                gates: Vec::new(),
            },
        )),
    )
    .unwrap();

    assert_eq!(
        output["reviews"][0]["status"], "invalid_output",
        "{output:#}"
    );
    assert!(
        output["reviews"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );
}

#[test]
fn work_refine_runs_fixer_then_review_and_check_gates() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
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

    assert_eq!(output["status"], "passed", "{output:#}");
    assert_eq!(output["iterations"].as_array().unwrap().len(), 1);
    assert!(temp.path().join("fixed.txt").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("prompt-source.txt")).unwrap(),
        "stdin"
    );
    assert_eq!(output["review"]["status"], "passed");
    assert_eq!(output["checks"]["checks"][0]["result"]["exit_status"], 0);

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed", "{gates:#}");
}

#[test]
fn work_refine_keeps_edit_and_iteration_evidence_after_transcript_overflow() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_verbose_refine_codex_stub(&codex_path);
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

    assert_eq!(output["status"], "passed", "{output:#}");
    assert_eq!(output["iterations"].as_array().unwrap().len(), 1);
    assert!(output["iterations"][0]["receipt_id"].as_str().is_some());
    assert_eq!(
        fs::read_to_string(temp.path().join("fixed.txt")).unwrap(),
        "fixed\n"
    );
    let receipts = read_receipts(temp.path());
    let worker_receipt = receipts
        .iter()
        .find(|receipt| {
            receipt["tool_name"] == WORKER_RUN_TOOL
                && receipt["evidence"]["purpose"] == "work_refine"
        })
        .expect("verbose refinement should record its worker receipt");
    assert_eq!(worker_receipt["evidence"]["stderr_truncated"], true);
}

#[test]
fn work_refine_fails_when_review_gate_returns_invalid_output() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_invalid_review_codex_stub(&codex_path);
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
    assert_eq!(output["iterations"].as_array().unwrap().len(), 0);
    assert_eq!(output["failed_review_gates"][0], "rust-error-handling");
    assert_eq!(output["review"]["reviews"][0]["status"], "invalid_output");
    assert_eq!(output["review"]["reviews"][0]["actionable_count"], 0);
    assert!(
        output["review"]["reviews"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["status"], "invalid_output", "{gates:#}");
    assert_eq!(gates["failed_required"][0], "rust-error-handling");
    assert!(
        gates["gates"][0]["parse_error"]
            .as_str()
            .unwrap()
            .contains("valid structured JSON")
    );
}

#[test]
fn work_refine_reports_failed_checks_without_aborting() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo_with_check(temp.path(), "printf 'check failed\\n'; exit 9");
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_clean_review_codex_stub(&codex_path);
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
    assert_eq!(output["review"]["status"], "passed");
    assert_eq!(output["checks"]["checks"][0]["result"]["exit_status"], 9);
    assert!(
        output["checks"]["checks"][0]["receipt_id"]
            .as_str()
            .is_some()
    );
}

#[test]
fn work_refine_reports_remaining_findings_after_max_iterations() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_stubborn_review_codex_stub(&codex_path);
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
    assert_eq!(output["iterations"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["remaining_actionable_findings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(output["failed_review_gates"][0], "rust-error-handling");
}

#[test]
fn work_refine_reports_fixer_failure_without_aborting() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_review_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("src.rs"), "fn changed() {}\n").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_failing_refine_codex_stub(&codex_path);
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
    assert_eq!(output["fixer_failed"], true);
    assert_eq!(output["iterations"][0]["status"], "failed");
    assert_eq!(output["iterations"][0]["exit_status"], 42);
    assert!(output["iterations"][0]["receipt_id"].as_str().is_some());
    assert_eq!(
        output["remaining_actionable_findings"][0]["issue"],
        "post-failure review"
    );
}

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
