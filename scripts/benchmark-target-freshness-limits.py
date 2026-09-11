#!/usr/bin/env python3
"""Exercise large, predeclared bounded outcomes through recording and inspection.

content-near: 480 MiB plus the ordinary fixture fits the 512 MiB phase ceiling;
30-second inspection, recording, and finish must succeed. A default inspection
may finish or reach its two-second deadline, with an explicit collection limit.
content-over: 513 MiB must never produce usable proof at any deadline.
entries-over: 27000 ordinary files exceed 250000 charged observations (Git,
matches, and final path checks); proof must remain unknown, including at 30 s.
ignored and symlink: a required broad input cannot be observed; default
inspection must report unobservable_input within its two-second phase budget.
All inspections begin with file data evicted. No elapsed-time bound is asserted
for the pre-existing global checks outside the new bounded freshness phase.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile


spec = importlib.util.spec_from_file_location("commands", Path(__file__).with_name("benchmark-target-freshness-commands.py"))
commands = importlib.util.module_from_spec(spec)
spec.loader.exec_module(commands)

CASES = ("content-near", "content-over", "entries-over", "ignored", "symlink")


def evidence(value):
    rows = commands.gate_rows(value)
    if len(rows) != 1:
        raise RuntimeError("limit inspection did not return the one required gate")
    row = rows[0]
    return {key: row.get(key) for key in ("status", "freshness", "freshness_reasons", "freshness_collection")}


def limit_report(row):
    return row["status"] == "unknown" and any(reason["code"] == "collection_limit" for reason in row["freshness_reasons"] or [])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=("local", "ci", "constrained"), required=True)
    parser.add_argument("--cases", default=",".join(CASES))
    args = parser.parse_args()
    cases = args.cases.split(",")
    if not cases or len(set(cases)) != len(cases) or any(case not in CASES for case in cases):
        parser.error("unknown limit case")
    if args.profile == "ci" and not (os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("GITHUB_RUN_ID")):
        parser.error("the CI profile requires an actual GitHub Actions run")
    binary = str(args.binary.resolve())
    report = {"schema": "jig.target-freshness-limits/v1", "profile": args.profile,
              "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
              "ci_run_id": os.environ.get("GITHUB_RUN_ID") if os.environ.get("GITHUB_ACTIONS") == "true" else None,
              "cpu_max": commands.cgroup_file("cpu.max"), "io_max": commands.cgroup_file("io.max"),
              "expectations": __doc__, "cases": {}, "failures": []}
    if args.profile == "constrained":
        cpu = (report["cpu_max"] or "").split()
        if len(cpu) != 2 or cpu[0] != cpu[1] or "rbps=20971520" not in (report["io_max"] or ""):
            parser.error("constrained limits require the verified one-CPU/20-MiB/s cgroup")
    parent = Path(tempfile.mkdtemp(prefix="ExampleFreshnessLimits-"))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    try:
        for case in cases:
            root = parent / case
            commands.create_fixture(root, commands.TREATMENT_EPOCH)
            if case.startswith("content-"):
                with (root / "docs/boundary.bin").open("wb") as stream:
                    for _ in range(480 if case == "content-near" else 513):
                        stream.write(b"x" * (1024 * 1024))
            elif case == "entries-over":
                for index in range(23000):
                    folder = root / "docs" / f"entries-{index // 1000:02}"
                    folder.mkdir(exist_ok=True)
                    (folder / f"example-{index:05}.txt").write_text("")
            elif case == "ignored":
                with (root / ".gitignore").open("a") as stream:
                    stream.write("docs/required-ignored.txt\n")
                (root / "docs/required-ignored.txt").write_text("Example required but ignored input\n")
            else:
                (root / "docs/required-link.txt").symlink_to("example-0100.txt")
            commands.git(root, "add", ".")
            commands.git(root, "commit", "-q", "-m", "Generic bounded freshness fixture")
            plan = commands.run([binary, "work", "start", "--title", "Example bounded proof", "--body", "Predeclared collection limit case.", "--print-plan-id"], root, check=True).stdout.strip()
            result = report["cases"][case] = {}
            commands.cold_cache(root)
            checked, elapsed, code = commands.cli(binary, root, "work", "check", "--plan-id", plan, check=False)
            result["recording"] = {"ok": checked.get("ok"), "exit_status": code, "elapsed_ms": elapsed,
                                   "collection": checked.get("freshness_collection")}
            originals = [json.loads(line) for line in (root / ".agent/state/receipts.jsonl").read_text().splitlines() if line.strip()]
            originals = [row for row in originals if row.get("target")]
            result["original_metadata_states"] = [row.get("target_freshness", {}).get("state") for row in originals]
            for timeout in (None, 30000):
                commands.cold_cache(root)
                extra = [] if timeout is None else ["--freshness-timeout-ms", str(timeout)]
                value, elapsed, code = commands.cli(binary, root, "work", "gates", "--plan-id", plan, *extra, check=False)
                row = evidence(value)
                result["default" if timeout is None else "override"] = {**row, "elapsed_ms": elapsed, "exit_status": code}
                if case == "content-near":
                    if timeout is None:
                        if row["status"] != "passed" and not (limit_report(row) and row["freshness_collection"]["elapsed_us"] >= 2_000_000):
                            report["failures"].append(f"{case}: default inspection did not pass or report its elapsed deadline")
                    elif row["status"] != "passed":
                        report["failures"].append(f"{case}: 30-second inspection did not produce complete proof")
                elif case in ("ignored", "symlink"):
                    if row["status"] != "unknown" or not any(reason["code"] == "unobservable_input" for reason in row["freshness_reasons"] or []):
                        report["failures"].append(f"{case}: required unobservable input did not expose its reason")
                    if timeout is None and row["freshness_collection"]["elapsed_us"] >= 2_000_000:
                        report["failures"].append(f"{case}: default unknown exceeded its phase deadline")
                elif not limit_report(row):
                    report["failures"].append(f"{case}: unusable proof did not expose collection_limit")
                if timeout == 30000 and case == "content-over" and row["freshness_collection"]["content_bytes_read"] <= 512 * 1024 * 1024:
                    report["failures"].append(f"{case}: override did not reach the byte ceiling")
                if timeout == 30000 and case == "entries-over" and row["freshness_collection"]["discovered_entries"] <= 250000:
                    report["failures"].append(f"{case}: override did not reach the entry ceiling")
                save()
            commands.cold_cache(root)
            finished, elapsed, code = commands.cli(binary, root, "work", "finish", "--plan-id", plan, "--resolution", "Generic limit exercise", check=False)
            result["finish"] = {"ok": finished.get("ok"), "exit_status": code, "elapsed_ms": elapsed}
            if case == "content-near":
                if checked.get("ok") is not True or code != 0 or finished.get("ok") is not True or len(originals) != 4 or any(state != "complete" for state in result["original_metadata_states"]):
                    report["failures"].append(f"{case}: recording or finish rejected a proof within the recording ceilings")
            elif case in ("content-over", "entries-over"):
                if checked.get("ok") is not False or code == 0 or finished.get("ok") is True or any(state == "complete" for state in result["original_metadata_states"]):
                    report["failures"].append(f"{case}: recording or finish accepted a partial proof")
            else:
                if checked.get("ok") is not False or code == 0 or finished.get("ok") is True or "incomplete" not in result["original_metadata_states"]:
                    report["failures"].append(f"{case}: recording or finish accepted unobservable required input")
            print(json.dumps({"case": case, "recording": result["recording"], "default": result["default"]["status"], "override": result["override"]["status"], "finish": result["finish"]}), flush=True)
            save()
        report["qualified_limits"] = not report["failures"] and set(cases) == set(CASES)
    except Exception as error:
        report["failures"].append(str(error).replace(str(parent), "<generic-fixture>"))
        report["qualified_limits"] = False
        raise
    finally:
        save()
        print(f"Generic limit fixtures retained at {parent}", flush=True)
    if report["failures"]:
        raise SystemExit("collection-limit qualification failed; evidence retained")


if __name__ == "__main__":
    main()
