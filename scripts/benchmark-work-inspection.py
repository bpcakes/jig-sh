#!/usr/bin/env python3
"""Bounded T-05 inspection profile; owns generic temporary repositories only.

Each invocation runs one sample per matrix cell and command. Baseline and repaired
runs together count toward the task's maximum of three trials per condition.
Cold evicts regular-file data with fadvise; it does not evict directory metadata.
No timing threshold is an acceptance oracle. Results record incomplete gates too.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location(
    "freshness_fixture", Path(__file__).with_name("benchmark-target-freshness-commands.py")
)
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)
TEST = "repository::freshness::observations::measurement"


def populate(binary, root, plans, receipts):
    FIXTURE.create_fixture(root, 8, source_files=128, ignored_files=0, narrow_files=64)
    plan_ids = []
    for index in range(plans):
        value, _, _ = FIXTURE.cli(
            binary, root, "work", "start", "--title", f"Example observation {index}",
            "--body", "Generic bounded inspection fixture."
        )
        plan_ids.append(value["plan"]["plan_id"])
    FIXTURE.cli(binary, root, "work", "check", "--plan-id", plan_ids[0])
    journal = root / ".agent/state/receipts.jsonl"
    lines = journal.read_text().splitlines()
    assert len(lines) < receipts
    prototype = json.loads(lines[0])
    assert prototype.get("target") is None
    prototype.update(tool_name="jig.example_observation", plan_id=None, args={})
    with journal.open("a") as output:
        for index in range(receipts - len(lines)):
            prototype["id"] = f"receipt_example_padding_{index:05}"
            output.write(json.dumps(prototype, separators=(",", ":")) + "\n")
    assert sum(1 for _ in journal.open()) == receipts
    return plan_ids[0]


def warm_cache(root):
    for directory, _, files in os.walk(root, followlinks=False):
        for name in files:
            path = Path(directory) / name
            if path.is_symlink():
                continue
            with path.open("rb") as stream:
                while stream.read(64 * 1024):
                    pass


def sample(test_binary, root, plan, command, cache):
    if cache == "cold":
        FIXTURE.cold_cache(root)
    else:
        warm_cache(root)
    env = os.environ.copy()
    env.update(JIG_INSPECTION_PROFILE_ROOT=str(root),
               JIG_INSPECTION_PROFILE_PLAN=plan, JIG_INSPECTION_PROFILE_COMMAND=command)
    started = time.monotonic()
    process = subprocess.run(
        [str(test_binary), "--exact", TEST, "--ignored", "--nocapture"],
        env=env, capture_output=True, text=True, timeout=90,
    )
    elapsed_ms = (time.monotonic() - started) * 1000
    if process.returncode:
        raise RuntimeError(f"measurement failed: {process.stdout[-4000:]} {process.stderr[-1000:]}")
    rows = [line.removeprefix("PROFILE ") for line in process.stdout.splitlines()
            if line.startswith("PROFILE ")]
    if len(rows) != 1:
        raise RuntimeError("measurement did not emit exactly one PROFILE record")
    result = json.loads(rows[0])
    result.update(cache=cache, process_elapsed_ms=elapsed_ms)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--label", required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output already exists; retained measurements must not be overwritten")
    if not hasattr(os, "posix_fadvise"):
        parser.error("the cold profile requires posix_fadvise")
    binary, test_binary = args.binary.resolve(strict=True), args.test_binary.resolve(strict=True)
    results = {"label": args.label, "schema_version": 1,
               "source_files": 128, "source_bytes_per_file": 512,
               "fixture_targets": 4, "inspection_budget_ms": 30000,
               "trials_per_condition": 1,
               "limitations": ["Uncontrolled host contention; descriptive observations, not latency guarantees.",
                                "Fresh test process per sample; debug/test build with test-only counters.",
                                "Cold evicts file data only; directory metadata remains warm.",
                                "Identity timing includes source capture; phase timings must not be summed.",
                                "API elapsed includes context and command work; process elapsed also includes startup."],
               "samples": [], "failures": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + "\n")
    with tempfile.TemporaryDirectory(prefix="ExampleInspection-") as directory:
        for plans in (1, 20):
            for receipts in (1000, 10000):
                root = Path(directory) / f"ExampleProject-{plans}-{receipts}"
                plan = populate(str(binary), root, plans, receipts)
                for cache in ("cold", "warm"):
                    for command in ("status", "compact"):
                        try:
                            value = sample(test_binary, root, plan, command, cache)
                        except Exception as error:
                            results["failures"].append({"plans": plans, "receipts": receipts,
                                "cache": cache, "command": command, "error_type": type(error).__name__})
                            args.output.write_text(json.dumps(results, indent=2) + "\n")
                            raise
                        assert len(value["gate_statuses"]) == (plans if command == "status" else 1)
                        value.update(plans=plans, receipts=receipts)
                        results["samples"].append(value)
                        args.output.write_text(json.dumps(results, indent=2) + "\n")
                        print(json.dumps({"plans": plans, "receipts": receipts, "cache": cache,
                                          "command": command, "ms": value["process_elapsed_ms"],
                                          "metrics": value["metrics"], "gate_statuses": value["gate_statuses"]}), flush=True)


if __name__ == "__main__":
    main()
