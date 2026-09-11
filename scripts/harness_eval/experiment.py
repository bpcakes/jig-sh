"""Immutable inputs and deterministic paired trial preparation."""

from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import sqlite3
import subprocess
import sys
import tempfile

from .fixtures import TASKS
from .grade import command, write_files
from .workspace import temporary_base

BASELINE = "03e9a9e4e5122b5bc12c66b1f635ae1faac05e15"
REPO = Path(__file__).resolve().parents[2]
ASSETS = REPO / "tests/fixtures/harness-eval"
CONDITIONS = {"baseline": (False, False), "guidance-only": (True, False),
              "tool-schema-only": (False, True), "combined": (True, True)}
ORIENTATION = "This is a minimal synthetic ExampleProject checkout. Repository guidance is the experimental condition; only files actually present are available. Use native Rust, Python/SQLite, or Node commands for the task. The full Jig launcher, Beads database, and services are not installed in this fixture.\n\n"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def dump(path, value):
    temporary = path.with_suffix(".pending")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


@contextmanager
def experiment_lock(output):
    with (output / ".run.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("another driver is running this experiment") from None
        yield


def apply_exclusion(trial_dir, result):
    path = trial_dir / "exclusion.json"
    if path.exists():
        annotation = read_json(path)
        result = dict(result if result is not None else annotation["original_result"])
        result.update(excluded=True, exclusion_reason=annotation["reason"])
        if result["status"] in {"not-run", "running", "finalizing"}:
            result["status"] = "excluded"
    return result


def guidance(revision):
    result = {}
    for name in ("AGENTS.md", ".agent/PLANS.md"):
        result[name] = subprocess.check_output(["git", "show", f"{revision}:{name}"], cwd=REPO).decode()
    return result


def baseline_guidance():
    manifest = read_json(ASSETS / "baseline.json")
    if manifest["revision"] != BASELINE:
        raise ValueError("audited baseline revision changed")
    files = {}
    for name, checksum in manifest["sha256"].items():
        content = (ASSETS / "baseline" / (name + ".snapshot")).read_bytes()
        if digest(content) != checksum:
            raise ValueError(f"baseline checksum mismatch: {name}")
        files[name] = content.decode()
    return files


def environment_versions():
    versions = {"python": sys.version.split()[0], "sqlite": sqlite3.sqlite_version,
                "os": platform.system(), "architecture": platform.machine()}
    # Match the cwd and allowlisted environment of detached clients and graders,
    # so repository-specific rustup overrides cannot misidentify their compiler.
    with tempfile.TemporaryDirectory(prefix="example-toolchain-", dir=temporary_base(REPO)) as cwd:
        for program in ("git", "rustc", "cargo", "node"):
            try:
                versions[program] = command([program, "--version"], cwd).strip()
            except (OSError, ValueError, subprocess.TimeoutExpired):
                versions[program] = None
    return versions


def tools_snapshot(compact=False):
    # This is a controlled synthetic tool surface, not the production Jig catalog.
    return read_json(ASSETS / ("tools-compact.json" if compact else "tools-original.json"))


def commit_source(root, files):
    root.mkdir()
    write_files(root, files)
    command(["git", "init", "--quiet", "--template=", "--initial-branch=main"], root)
    command(["git", "add", "--all"], root)
    env = dict(os.environ, GIT_AUTHOR_NAME="Example Author", GIT_AUTHOR_EMAIL="example@example.invalid",
               GIT_COMMITTER_NAME="Example Author", GIT_COMMITTER_EMAIL="example@example.invalid",
               GIT_AUTHOR_DATE="2000-01-01T00:00:00Z", GIT_COMMITTER_DATE="2000-01-01T00:00:00Z")
    subprocess.run(["git", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null",
                    "commit", "--quiet", "-m", "ExampleProject fixed source"],
                   cwd=root, env=env, capture_output=True, check=True)
    return command(["git", "rev-parse", "HEAD"], root).strip()


def prepare(output, revision, repetitions=3, seed=20260911, config=None):
    if repetitions < 3:
        raise ValueError("comparisons require at least three paired repetitions")
    revision = command(["git", "rev-parse", "--verify", revision + "^{commit}"], REPO).strip()
    original = baseline_guidance()
    treatment = guidance(revision)
    # Fail before creating any trial if required configuration is invalid.
    if config is not None:
        validate_config(config)
        config = {"max_output_tokens": 4096, **config}
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / "inputs"
    inputs.mkdir()
    write_files(inputs / "guidance-original", original)
    write_files(inputs / "guidance-treatment", treatment)
    dump(inputs / "tools-original.json", tools_snapshot())
    dump(inputs / "tools-compact.json", tools_snapshot(True))
    dump(inputs / "tasks.json", TASKS)
    # Retain the exact uncommitted driver/grader bytes as well as the Git revision.
    sources = [REPO / "scripts/evaluate-harness.py", *sorted((REPO / "scripts/harness_eval").glob("*.py"))]
    for source in sources:
        destination = inputs / "implementation" / source.relative_to(REPO)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(source.read_bytes())
    dump(inputs / "execution.json", config)
    executables = {}
    if config is not None:
        executable = shutil.which(config["argv"][0])
        if executable:
            executables["0"] = digest(Path(executable).read_bytes())
        for index, arg in enumerate(config["argv"][1:], 1):
            if Path(arg).is_file():
                destination = inputs / "adapter" / str(index)
                destination.parent.mkdir(exist_ok=True)
                destination.write_bytes(Path(arg).read_bytes())
                executables[str(index)] = digest(Path(arg).read_bytes())
    rng = random.Random(seed)
    schedule = []
    for repetition in range(1, repetitions + 1):
        names = list(TASKS)
        rng.shuffle(names)
        for name in names:
            comparisons = list(CONDITIONS)[1:]
            rng.shuffle(comparisons)
            for comparison in comparisons:
                pair = f"{name}-{repetition}-{comparison}"
                arms = ["baseline", comparison]
                rng.shuffle(arms)
                for condition in arms:
                    schedule.append({"task": name, "condition": condition, "pair": pair,
                                     "repetition": repetition, "order": len(schedule) + 1})
    manifest = {"schema_version": 1, "baseline_revision": BASELINE, "guidance_revision": revision,
                "implementation_revision": command(["git", "rev-parse", "HEAD"], REPO).strip(),
                "python_version": sys.version.split()[0], "seed": seed, "repetitions": repetitions,
                "environment": environment_versions(),
                "adapter_files_sha256": executables,
                "tool_surface": "synthetic fixture v1 (not production Jig tools/list)",
                "stopping_rule": "Complete the fixed schedule; never stop early for favorable speed or usage. Keep every failed, timed-out, interrupted or excluded trial. Resolve correctness regressions before any rollout claim.",
                "inputs_sha256": {str(p.relative_to(inputs)): digest(p.read_bytes())
                                  for p in sorted(inputs.rglob("*")) if p.is_file()},
                "schedule": schedule}
    trials = output / "trials"
    trials.mkdir()
    for trial in schedule:
        trial_dir = trials / f"{trial['order']:03d}"
        trial_dir.mkdir()
        task = TASKS[trial["task"]]
        checkout = trial_dir / "checkout"
        source_commit = commit_source(checkout, task["files"])
        write_files(checkout, task.get("interrupted_edits", {}))
        use_guidance, use_tools = CONDITIONS[trial["condition"]]
        write_files(checkout, treatment if use_guidance else original)
        prompt = ORIENTATION + task["prompt"] + "\n"
        (trial_dir / "prompt.txt").write_text(prompt)
        dump(trial_dir / "tools.json", tools_snapshot(use_tools))
        metadata = {**trial, "source_commit": source_commit,
                    "starting_files_sha256": {str(p.relative_to(checkout)): digest(p.read_bytes())
                                              for p in sorted(checkout.rglob("*"))
                                              if p.is_file() and ".git" not in p.relative_to(checkout).parts},
                    "prompt_sha256": digest(prompt.encode()),
                    "tools_sha256": digest((trial_dir / "tools.json").read_bytes()),
                    "descriptor_bytes": len((trial_dir / "tools.json").read_bytes()),
                    "initial_git_status": command(["git", "status", "--porcelain"], checkout)}
        dump(trial_dir / "trial.json", metadata)
    manifest["trials_sha256"] = {str(p.parent.name): digest(p.read_bytes())
                                 for p in sorted(trials.glob("*/trial.json"))}
    dump(output / "experiment.json", manifest)
    (output / "experiment.sha256").write_text(digest((output / "experiment.json").read_bytes()) + "\n")
    return manifest


def validate_config(config):
    required = {"argv", "model", "reasoning", "client", "client_version", "timeout_seconds"}
    if not isinstance(config, dict) or not required <= set(config) or set(config) - required - {"max_output_tokens"}:
        raise ValueError("execution config requires " + ", ".join(sorted(required)) + "; optional: max_output_tokens")
    if not isinstance(config["argv"], list) or not config["argv"] or not all(isinstance(x, str) and x for x in config["argv"]):
        raise ValueError("argv must be a nonempty array of strings; no shell expansion")
    for key in ("model", "reasoning", "client", "client_version"):
        if not isinstance(config[key], str) or not config[key].strip():
            raise ValueError(f"{key} must explicitly identify the requested execution")
    timeout = config["timeout_seconds"]
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or not 0 < timeout <= 3600:
        raise ValueError("timeout_seconds must be greater than 0 and at most 3600")
    tokens = config.get("max_output_tokens", 4096)
    if isinstance(tokens, bool) or not isinstance(tokens, int) or tokens <= 0:
        raise ValueError("max_output_tokens must be a positive integer")


def verify_inputs(output):
    if digest((output / "experiment.json").read_bytes()) != (output / "experiment.sha256").read_text().strip():
        raise ValueError("experiment manifest changed")
    manifest = read_json(output / "experiment.json")
    for name, checksum in manifest["inputs_sha256"].items():
        if digest((output / "inputs" / name).read_bytes()) != checksum:
            raise ValueError(f"frozen input changed: {name}")
    for source in (output / "inputs/implementation/scripts").rglob("*.py"):
        current = REPO / "scripts" / source.relative_to(output / "inputs/implementation/scripts")
        if source.read_bytes() != current.read_bytes():
            raise ValueError("driver changed; run the retained implementation or prepare a new experiment")
    for name, checksum in manifest["trials_sha256"].items():
        if digest((output / "trials" / name / "trial.json").read_bytes()) != checksum:
            raise ValueError(f"trial metadata changed: {name}")
    config = read_json(output / "inputs/execution.json")
    if config:
        for index, checksum in manifest["adapter_files_sha256"].items():
            arg = config["argv"][int(index)]
            path = Path(shutil.which(arg) or arg) if index == "0" else Path(arg)
            if not path.is_file() or digest(path.read_bytes()) != checksum:
                raise ValueError("configured adapter changed; prepare a new experiment")
    return manifest
