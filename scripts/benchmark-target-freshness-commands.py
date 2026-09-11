#!/usr/bin/env python3
"""Qualify real CLI inspections using fresh processes and original live receipts.

Build the current jig runtime with --release before running qualification.
This driver owns temporary generic repositories, never an existing checkout.
Run warm and cold separately; cold evicts file data, not directory metadata.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time


CASES = ("clean", "narrow-dirty", "wide-dirty", "staged", "untracked")
COMMANDS = ("status", "gates", "evidence")
BASELINE_EPOCH = 7
TREATMENT_EPOCH = 8


def run(command, root, **kwargs):
    return subprocess.run(command, cwd=root, capture_output=True, text=True,
                          timeout=180, **kwargs)


def git(root, *args):
    return run(["git", *args], root, check=True).stdout.strip()


def target(name):
    component, action = name.split(":")
    return {"component": component, "action": action}


def toml_value(value):
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{k} = {toml_value(v)}" for k, v in value.items()) + " }"
    if isinstance(value, list):
        return "[" + ", ".join(map(toml_value, value)) + "]"
    return json.dumps(value)


def create_fixture(root, epoch):
    root.mkdir()
    for directory in (".agent", "scripts", "web/src", "api/model", "api/fixtures", "docs"):
        (root / directory).mkdir(parents=True, exist_ok=True)
    (root / ".gitignore").write_text("node_modules/\n.agent/state/\n.agent/plans/\n")
    script = root / "scripts/check.sh"
    script.write_text("#!/bin/sh\nexit 0\n")
    script.chmod(0o755)
    for name in ("api/model/example.txt", "api/fixtures/example.txt"):
        (root / name).write_text("Example input\n")
    for index in range(4000):
        directory = "web/src" if index < 100 else "docs"
        (root / directory / f"example-{index:04}.txt").write_text("x" * 512)
    ignored = root / "node_modules"
    ignored.mkdir()
    for index in range(10000):
        (ignored / f"example-{index:05}").write_text("ignored")
    specs = [
        ("api:model", ["api/model/**", "scripts/check.sh"], []),
        ("api:verify", ["api/fixtures/**", "scripts/check.sh"], ["api:model"]),
        ("web:test", ["web/**", "scripts/check.sh"], ["api:verify"]),
        ("web:broad", ["web/**", "api/**", "docs/**", "scripts/**"], ["api:verify"]),
    ]
    actions = []
    for name, inputs, dependencies in specs:
        runner = ({"kind": "command", "command": "benchmark_check_command"}
                  if epoch == BASELINE_EPOCH else
                  {"kind": "argv", "program": "scripts/check.sh", "args": []})
        action = {"target": target(name), "intent": "check", "effects": ["read_only", "process"],
                  "runner": runner,
                  "inputs": inputs, "depends_on": list(map(target, dependencies))}
        if epoch == TREATMENT_EPOCH:
            action["inputs_policy"] = "exhaustive"
            action["source_state"] = "git"
        actions.append(action)
    components = [{"id": "web", "root": "web"}, {"id": "api", "root": "api"}]
    profiles = [{"id": "verify", "targets": [target("web:test"), target("web:broad")]}]
    config = ['_src_path = "/tmp/ExampleTemplate"', '_commit = "example"',
              'repo_name = "ExampleProject"', 'default_branch = "main"']
    if epoch == BASELINE_EPOCH:
        config += ['[commands]', 'benchmark_check_command = "scripts/check.sh"']
    config += ['[repository]', 'default_check_profile = "verify"']
    for kind, entries in (("components", components), ("actions", actions), ("profiles", profiles)):
        for entry in entries:
            config.append(f"[[repository.{kind}]]")
            config.extend(f"{key} = {toml_value(value)}" for key, value in entry.items())
    config += ['[[work.gates]]', 'id = "verify"', 'kind = "evidence"', 'profile = "verify"']
    (root / ".jig.toml").write_text("\n".join(config) + "\n")
    required_commands = ["benchmark_check_command"] if epoch == BASELINE_EPOCH else []
    manifest = {"contract_version": epoch, "tool_namespace": "jig", "tools": [],
                "required_commands": required_commands, "components": components, "actions": actions,
                "profiles": profiles, "default_check_profile": "verify"}
    (root / ".agent/jig-contract.json").write_text(json.dumps(manifest) + "\n")
    git(root, "init", "-q", "-b", "main")
    git(root, "config", "user.name", "Jig Benchmark")
    git(root, "config", "user.email", "jig@example.invalid")
    git(root, "add", ".")
    git(root, "commit", "-q", "-m", "Generic command freshness benchmark")


def cgroup_file(name):
    path = Path("/sys/fs/cgroup") / name
    return path.read_text().strip() if path.exists() else None


def io_counters():
    counters = {}
    for line in (cgroup_file("io.stat") or "").splitlines():
        device, *fields = line.split()
        counters[device] = dict((key, int(value)) for key, value in (field.split("=") for field in fields))
    return counters


def cold_cache(root):
    for directory, names, files in os.walk(root, followlinks=False):
        names[:] = [name for name in names if name != "node_modules"]
        for name in files:
            path = Path(directory) / name
            if path.is_symlink():
                continue
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            try:
                os.fsync(fd)
                os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
            finally:
                os.close(fd)


def cli(binary, root, *args, check=True):
    started = time.monotonic()
    result = run([binary, "--json", *args], root)
    elapsed = (time.monotonic() - started) * 1000
    try:
        value = json.loads(result.stdout)
    except ValueError as error:
        if not check and result.returncode != 0 and not result.stdout.strip():
            return {"ok": False, "error": result.stderr[:1000]}, elapsed, result.returncode
        raise RuntimeError(f"{args}: invalid JSON; status={result.returncode}; stderr={result.stderr[:1000]}") from error
    if check and result.returncode != 0:
        raise RuntimeError(f"{args}: failed with status={result.returncode}: {value}")
    return value, elapsed, result.returncode


def gate_snapshots(value):
    if "work" in value:  # Aggregate status command.
        return [entry["snapshot"] for entry in value["work"]["gates"] if entry.get("snapshot")]
    return [value]


def gate_rows(value):
    return [gate for snapshot in gate_snapshots(value) for gate in snapshot.get("gates", [])]


def sample(binary, root, plan, command, cache):
    if cache == "cold":
        cold_cache(root)
    before = io_counters()
    args = ["status"] if command == "status" else ["work", command, "--plan-id", plan]
    value, elapsed, code = cli(binary, root, *args, check=False)
    after = io_counters()
    rows = gate_rows(value)
    stats = [row["freshness_collection"] for row in rows if row.get("freshness_collection")]
    deltas = {device: {key: count - before.get(device, {}).get(key, 0)
                       for key, count in fields.items()} for device, fields in after.items()}
    return {"elapsed_ms": elapsed, "exit_status": code,
            "gate_statuses": [row["status"] for row in rows],
            "freshness": [row.get("freshness") for row in rows],
            "collection": stats,
            "io_delta": {device: fields for device, fields in deltas.items() if any(fields.values())}}


def summary(values):
    return {"first_ms": values[0], "median_ms": statistics.median(values),
            "p95_ms": sorted(values)[math.ceil(len(values) * .95) - 1]}


def apply_case(root, case):
    git(root, "reset", "--hard", "HEAD")
    (root / "web/src/added.txt").unlink(missing_ok=True)
    if case in ("narrow-dirty", "staged"):
        (root / "web/src/example-0000.txt").write_text("changed\n")
    elif case == "wide-dirty":
        for path in sorted((root / "docs").glob("example-*.txt")):
            path.write_text("changed\n")
    elif case == "untracked":
        (root / "web/src/added.txt").write_text("added\n")
    if case == "staged":
        git(root, "add", "web/src/example-0000.txt")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cache", choices=("warm", "cold"), required=True)
    parser.add_argument("--profile", choices=("local", "ci", "constrained"), required=True)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--cases", default=",".join(CASES))
    parser.add_argument("--smoke", action="store_true", help="Exercise the driver; never claim qualification")
    args = parser.parse_args()
    cases = args.cases.split(",")
    if any(case not in CASES for case in cases) or len(set(cases)) != len(cases):
        parser.error("invalid or repeated case")
    if args.samples < (1 if args.smoke else 20):
        parser.error("qualification requires at least 20 separate processes per case and command")
    if args.profile == "ci" and not (os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("GITHUB_RUN_ID")):
        parser.error("the CI profile requires an actual GitHub Actions run")
    if args.cache == "cold" and not hasattr(os, "posix_fadvise"):
        parser.error("cold-cache measurements require posix_fadvise")
    cpu_max, io_max = cgroup_file("cpu.max"), cgroup_file("io.max")
    if args.profile == "constrained":
        if not cpu_max or cpu_max.split()[0] == "max" or int(cpu_max.split()[0]) != int(cpu_max.split()[1]):
            parser.error("constrained qualification requires a one-CPU cgroup")
        if not io_max or "rbps=20971520" not in io_max:
            parser.error("constrained qualification requires enforced 20 MiB/s backing reads")
    binary = str(args.binary.resolve())
    report = {"schema": "jig.target-freshness-command-benchmark/v1", "profile": args.profile,
              "cache": args.cache, "cache_description": "file data evicted before every sample; directory metadata warm" if args.cache == "cold" else "explicit untimed command warmup before each command series",
              "platform": platform.platform(), "logical_cpus": os.cpu_count(),
              "cpu_max": cpu_max, "io_max": io_max, "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
              "ci_run_id": os.environ.get("GITHUB_RUN_ID") if os.environ.get("GITHUB_ACTIONS") == "true" else None,
              "fixture": "4000 512-byte files; 100 narrow files; 10000 ignored entries; four targets, with explicit exhaustive Git authority at v8, and two roots sharing a two-target dependency chain",
              "samples_per_case_command_epoch": args.samples, "smoke": args.smoke,
              "cases": {}, "failures": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    # Retain generic fixtures on failure for diagnosis, but never accept an
    # externally supplied existing checkout as a mutation target.
    parent = Path(tempfile.mkdtemp(prefix="ExampleFreshnessCommands-"))
    try:
        for epoch in (BASELINE_EPOCH, TREATMENT_EPOCH):
            root = parent / f"epoch-{epoch}"
            create_fixture(root, epoch)
            plan = run([binary, "work", "start", "--title", "Example freshness measurement", "--body", "Generic fixture validation.", "--print-plan-id"], root, check=True).stdout.strip()
            for case in cases:
                apply_case(root, case)
                checked, _, _ = cli(binary, root, "work", "check", "--plan-id", plan)
                if checked.get("ok") is not True:
                    raise RuntimeError(f"epoch {epoch} {case}: original recording failed")
                group = report["cases"].setdefault(case, {}).setdefault(str(epoch), {})
                for command in COMMANDS:
                    if args.cache == "warm":
                        sample(binary, root, plan, command, "warm")
                    samples = [sample(binary, root, plan, command, args.cache) for _ in range(args.samples)]
                    row = {**summary([sample["elapsed_ms"] for sample in samples]), "samples": samples}
                    group[command] = row
                    if any(sample["exit_status"] != 0 or sample["gate_statuses"] != ["passed"] for sample in samples):
                        report["failures"].append(f"{case}/{epoch}/{command}: every inspection must pass with its original receipts")
                    if epoch == TREATMENT_EPOCH:
                        if args.profile == "constrained" and args.cache == "cold":
                            limited = {line.split()[0] for line in io_max.splitlines() if "rbps=20971520" in line}
                            if any(sum(sample["io_delta"].get(device, {}).get("rbytes", 0) for device in limited) < 8 * 1024 * 1024 for sample in samples):
                                report["failures"].append(f"{case}/{command}: cold proof did not read fixture data through the throttled device")
                        phases = [sample["collection"][0]["elapsed_us"] / 1000 for sample in samples if len(sample["collection"]) == 1]
                        if len(phases) != args.samples:
                            report["failures"].append(f"{case}/{command}: missing unique phase instrumentation")
                        else:
                            row["phase"] = summary(phases)
                            if row["phase"]["p95_ms"] >= 1000 or max(phases) >= 2000:
                                report["failures"].append(f"{case}/{command}: new phase exceeds acceptance")
                        if row["p95_ms"] > report["cases"][case][str(BASELINE_EPOCH)][command]["p95_ms"] + 2000:
                            report["failures"].append(f"{case}/{command}: full command exceeds baseline + 2000 ms")
                    save()
                    print(json.dumps({"case": case, "epoch": epoch, "command": command, **{key: value for key, value in row.items() if key != "samples"}}), flush=True)
            if epoch == TREATMENT_EPOCH:
                (root / "unrelated.md").write_text("Example unrelated edit\n")
                retained, _, _ = cli(binary, root, "work", "gates", "--plan-id", plan)
                report["non_leaf_retained_after_unrelated_edit"] = all(row["status"] == "passed" for row in gate_rows(retained)) and bool(gate_rows(retained))
                if not report["non_leaf_retained_after_unrelated_edit"]:
                    report["failures"].append("non-leaf scoped proof did not survive unrelated edit")
                (root / "unrelated.md").unlink()
                finished, _, _ = cli(binary, root, "work", "finish", "--plan-id", plan, "--resolution", "Generic qualification completed")
                report["finish_ok"] = finished.get("ok") is True
                if not report["finish_ok"]:
                    report["failures"].append("finish failed after valid recording and inspections")
        report["qualified_matrix"] = not report["failures"] and not args.smoke and set(cases) == set(CASES)
    except Exception as error:
        report["failures"].append(str(error).replace(str(parent), "<generic-fixture>"))
        report["qualified_matrix"] = False
        raise
    finally:
        save()
        print(f"Generic fixtures retained at {parent}", flush=True)
    if report["failures"]:
        raise SystemExit("command qualification failed; all measurements retained")


if __name__ == "__main__":
    main()
