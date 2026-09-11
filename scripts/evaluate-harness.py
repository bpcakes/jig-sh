#!/usr/bin/env python3
"""Prepare paired harness trials, verify fixtures offline, or explicitly run a client adapter.

All results are JSON. Model execution requires both a frozen config and --execute.
See tests/fixtures/harness-eval/README.md for the adapter and artifact contracts.
"""

import argparse
import json
import os
from pathlib import Path
import signal
import sys

sys.dont_write_bytecode = True

from harness_eval.experiment import (BASELINE, apply_exclusion, baseline_guidance,
                                     commit_source, dump, experiment_lock, prepare, read_json)
from harness_eval.fixtures import TASKS
from harness_eval.grade import grade, write_files
from harness_eval.runner import run
from harness_eval.workspace import release_workspace


def smoke(output):
    baseline_guidance()
    output.mkdir(parents=True, exist_ok=False)
    results = {}
    for name, task in TASKS.items():
        root = output / name
        commit_source(root, task["files"])
        write_files(root, task.get("interrupted_edits", {}))
        # A self-report saying all checks passed must not rescue an incorrect result.
        dump(root / "agent-report.json", {"all_checks_passed": True})
        rejected = grade(name, root)
        write_files(root, task["solution"])
        accepted = grade(name, root)
        protected = next(path for path in task["files"] if path not in task["solution"])
        (root / protected).write_text("incorrect invariant\n")
        invariant = grade(name, root)
        results[name] = {"incorrect_rejected": not rejected["passed"],
                         "reference_accepted": accepted["passed"],
                         "invariant_rejected": bool(invariant["invariant_violations"]),
                         "reference_failure": accepted["failure"]}
        dump(output / f"{name}.json", {"incorrect": rejected, "reference": accepted, "invariant": invariant})
    result = {"schema_version": 1, "mode": "fixture-only", "baseline_revision": BASELINE,
              "model_calls": 0, "cases": results,
              "passed": all(all(case[key] for key in ("incorrect_rejected", "reference_accepted", "invariant_rejected")) for case in results.values())}
    dump(output / "summary.json", result)
    return result


def exclude(output, order, reason):
    with experiment_lock(output):
        return exclude_locked(output, order, reason)


def exclude_locked(output, order, reason):
    trial_dir = output / "trials" / f"{order:03d}"
    read_json(trial_dir / "trial.json")
    path = trial_dir / "exclusion.json"
    if path.exists():
        raise ValueError("trial already has a durable exclusion annotation")
    result_path = trial_dir / "result.json"
    result = read_json(result_path) if result_path.exists() else {"status": "not-run"}
    if result["status"] == "running":
        for key in ("driver_pid", "child_pid"):
            if key not in result:
                continue
            try:
                os.kill(result[key], 0)
            except ProcessLookupError:
                continue
            raise ValueError("recorded process is still live; stop or wait for it before exclusion")
    annotation = {"order": order, "reason": reason, "original_result": dict(result)}
    dump(path, annotation)
    dump(result_path, apply_exclusion(trial_dir, result))
    if result.get("checkout_retained") and result.get("execution_workspace"):
        release_workspace(result["execution_workspace"])
    return annotation


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    commands = parser.add_subparsers(dest="command", required=True)
    smoke_parser = commands.add_parser("smoke", help="run all five positive and negative graders without model access", allow_abbrev=False)
    smoke_parser.add_argument("--output", type=Path, required=True, help="new artifact directory; never overwritten")
    prepare_parser = commands.add_parser("prepare", help="freeze inputs and create isolated paired checkouts", allow_abbrev=False)
    prepare_parser.add_argument("--output", type=Path, required=True)
    prepare_parser.add_argument("--guidance-revision", required=True, help="Git revision containing treatment guidance; resolved to an immutable commit")
    prepare_parser.add_argument("--repetitions", type=int, default=3)
    prepare_parser.add_argument("--seed", type=int, default=20260911)
    prepare_parser.add_argument("--config", type=Path, help="explicit client adapter JSON; no execution during prepare")
    run_parser = commands.add_parser("run", help="execute pending trials through the explicitly configured adapter", allow_abbrev=False)
    run_parser.add_argument("--output", type=Path, required=True)
    run_parser.add_argument("--execute", action="store_true", required=True, help="authorize the configured external model calls")
    run_parser.add_argument("--limit", type=int, help="pause after N new trials; remaining trials stay pending")
    grade_parser = commands.add_parser("grade", help="independently grade a checkout", allow_abbrev=False)
    grade_parser.add_argument("--task", choices=TASKS, required=True)
    grade_parser.add_argument("--checkout", type=Path, required=True)
    exclude_parser = commands.add_parser("exclude", help="preserve an exclusion reason and any original result", allow_abbrev=False)
    exclude_parser.add_argument("--output", type=Path, required=True)
    exclude_parser.add_argument("--order", type=int, required=True)
    exclude_parser.add_argument("--reason", required=True)
    annotate_parser = commands.add_parser("annotate", help="record an independent review of unnecessary questions", allow_abbrev=False)
    annotate_parser.add_argument("--output", type=Path, required=True)
    annotate_parser.add_argument("--order", type=int, required=True)
    annotate_parser.add_argument("--unnecessary-questions", type=int, required=True)
    annotate_parser.add_argument("--reviewer", required=True, help="generic reviewer ID; no private names")
    annotate_parser.add_argument("--reason", required=True, help="cite message indices and explain the classification")
    args = parser.parse_args(argv)
    if args.command == "run" and args.limit is not None and args.limit < 1:
        parser.error("--limit must be positive")
    if args.command == "exclude" and (args.order < 1 or not args.reason.strip()):
        parser.error("--order must be positive and --reason must be nonempty")
    if args.command == "annotate" and (args.order < 1 or args.unnecessary_questions < 0 or not args.reviewer.strip() or not args.reason.strip()):
        parser.error("annotation requires a positive order, nonnegative count, reviewer and reason")
    try:
        if args.command == "smoke":
            result = smoke(args.output)
        elif args.command == "prepare":
            result = prepare(args.output, args.guidance_revision, args.repetitions, args.seed,
                             read_json(args.config) if args.config else None)
        elif args.command == "run":
            result = run(args.output, args.limit)
        elif args.command == "exclude":
            result = exclude(args.output, args.order, args.reason)
        elif args.command == "annotate":
            with experiment_lock(args.output):
                trial_dir = args.output / "trials" / f"{args.order:03d}"
                read_json(trial_dir / "result.json")
                path = trial_dir / "annotations.json"
                if path.exists():
                    raise ValueError("annotation already exists; preserve the original review")
                result = {"value": args.unnecessary_questions, "source": "independent human review",
                          "reviewer": args.reviewer, "reason": args.reason}
                dump(path, result)
        else:
            result = grade(args.task, args.checkout.resolve())
        print(json.dumps(result, sort_keys=True, allow_nan=False))
        if result.get("passed") is False:
            return 1
        if args.command == "run" and any(r["status"] != "completed" or r.get("excluded") or not (r.get("grade") or {}).get("passed") for r in result["results"]):
            return 1
        return 0
    except KeyboardInterrupt:
        print("evaluation interrupted; existing artifacts retained", file=sys.stderr)
        return 130
    except (OSError, ValueError) as error:
        print("evaluation error: " + ascii(str(error)), file=sys.stderr)
        return 2


if __name__ == "__main__":
    def terminate(signum, frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, terminate)
    sys.exit(main())
