#!/usr/bin/env python3
"""Measure the scoped collector using a separate Rust test process per sample.

Build with cargo test -p jig-sh --lib --no-run. Pass the printed test executable
as --test-binary. The ignored test is only an entrypoint into the real collector;
timings exclude fixture preparation and test-process startup. No persisted cache.
"""
import argparse
import json
import math
import os
import pathlib
import statistics
import subprocess
import tempfile
import time

TEST = "repository::freshness::tests::benchmark::measurement"


def invoke(binary, mode, root=None, timeout=30000):
    env = dict(os.environ, JIG_FRESHNESS_BENCH_MODE=mode,
               JIG_FRESHNESS_BENCH_TIMEOUT_MS=str(timeout))
    if root:
        env["JIG_FRESHNESS_BENCH_ROOT"] = str(root)
    started = time.monotonic()
    result = subprocess.run([binary, "--exact", TEST, "--ignored", "--nocapture", "--format=terse"],
                            env=env, capture_output=True, text=True, check=True, timeout=120)
    data = [json.loads(line[6:]) for line in result.stdout.splitlines() if line.startswith("BENCH ")]
    if len(data) != 1:
        raise RuntimeError("benchmark did not emit exactly one complete measurement")
    data[0]["process_elapsed_ms"] = (time.monotonic() - started) * 1000
    return data[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True)
    parser.add_argument("--fixture", type=pathlib.Path)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--timeout-ms", type=int, default=30000)
    parser.add_argument("--cases", default="clean,narrow-dirty,wide-dirty,staged,untracked")
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--require-qualified", action="store_true")
    args = parser.parse_args()
    if args.samples < 20:
        parser.error("qualification requires at least 20 process samples per case")
    root = args.fixture or pathlib.Path(invoke(args.test_binary, "prepare")["fixture"])
    report = {"fixture_description": "4000 512-byte files, 100 narrow inputs, 10000 unrelated ignored entries, four targets sharing a dependency chain",
              "samples_per_case": args.samples, "timeout_ms": args.timeout_ms,
              "cache_conditions": "ordinary host page cache; first sample separately reported, not a cold-cache claim",
              "cases": {}}
    for case in args.cases.split(","):
        # This script owns its generic temporary fixture. Reusing a fixture is
        # restricted to those created by this driver and its exact commit marker.
        subject = subprocess.check_output(["git", "log", "-1", "--format=%s"], cwd=root, text=True).strip()
        if subject != "Generic 4000-file benchmark" or not str(root.resolve()).startswith(tempfile.gettempdir() + "/"):
            raise RuntimeError("refusing to mutate a non-benchmark checkout")
        subprocess.run(["git", "reset", "--hard", "HEAD"], cwd=root, check=True, capture_output=True)
        (root / "apps/web/src/added.txt").unlink(missing_ok=True)
        if case in {"narrow-dirty", "staged"}:
            (root / "apps/web/src/example-0000.txt").write_text("changed\n")
        elif case == "wide-dirty":
            for path in sorted((root / "docs").glob("example-*.txt")):
                path.write_text("changed\n")
        elif case == "untracked":
            (root / "apps/web/src/added.txt").write_text("added\n")
        elif case != "clean":
            raise ValueError(f"unknown case {case}")
        if case == "staged":
            subprocess.run(["git", "add", "apps/web/src/example-0000.txt"], cwd=root, check=True)
        samples = [invoke(args.test_binary, "collect", root, args.timeout_ms) for _ in range(args.samples)]
        times = [sample["stats"]["elapsed_us"] / 1000 for sample in samples]
        report["cases"][case] = {"first_ms": times[0], "median_ms": statistics.median(times),
                                  "p95_ms": sorted(times)[math.ceil(len(times)*0.95)-1],
                                  "complete_samples": sum(all("Ok" in outcome for outcome in sample["outcomes"].values()) for sample in samples),
                                  "samples": samples}
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"case": case, **{key: value for key, value in report["cases"][case].items() if key != "samples"}}), flush=True)
    print(f"Generic fixture retained for reproducibility: {root}", flush=True)
    if args.require_qualified and any(case["p95_ms"] >= 1000 or case["complete_samples"] != args.samples for case in report["cases"].values()):
        raise SystemExit("collector qualification failed: require p95 < 1000 ms and every sample complete")


if __name__ == "__main__":
    main()
