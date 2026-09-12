"""Opt-in client adapter execution and observations; no provider credentials stored."""

import os
from pathlib import Path
import shutil
import subprocess
import time

from .experiment import (REPO, apply_exclusion, digest, dump, environment_versions,
                         experiment_lock, read_json, verify_inputs)
from .fixtures import TASKS
from .grade import command, grade
from .process import exit_status, stop_group
from .workspace import create_workspace, release_workspace, retain_workspace, workspace_path

CHECK_CLASSIFICATION = "exhaustive-checks-v1"


def fingerprints(checkout, names):
    return {name: digest((checkout / name).read_bytes())
            for name in names if (checkout / name).is_file() and not (checkout / name).is_symlink()}


def unavailable(reason):
    return {"value": None, "reason": reason}


def observations(response, requested, tools_hash):
    """Require client-observed execution identity, never infer it from a model alias."""
    actual = response.get("identity", {})
    mismatches = [key for key in ("model", "reasoning", "client", "client_version")
                  if actual.get(key) != requested[key]]
    if actual.get("tools_sha256") != tools_hash:
        mismatches.append("tools_sha256")
    metrics = {key: unavailable("client did not provide this observation") for key in
               ("tool_calls", "repeated_checks", "unnecessary_questions", "usage_tokens", "client_context_tokens")}
    trace = response.get("tool_calls")
    if trace is not None:
        if not isinstance(trace, list) or not all(isinstance(event, dict) and isinstance(event.get("name"), str) for event in trace):
            raise ValueError("tool_calls must be a list of client-observed tool events")
        metrics["tool_calls"] = {"value": len(trace), "source": "client trace",
                                 "complete": response.get("tool_trace_complete")}
        checks = [event for event in trace if event.get("kind") == "check"]
        classified = (response.get("check_classification") == CHECK_CLASSIFICATION
                      and isinstance(response.get("tool_trace_complete"), bool)
                      and all(event.get("kind") in ("check", "non_check") for event in trace)
                      and all(isinstance(event.get("check_key"), str) and event["check_key"].strip()
                              and isinstance(event.get("source_sha256"), str)
                              and len(event["source_sha256"]) == 64
                              and all(char in "0123456789abcdef" for char in event["source_sha256"])
                              for event in checks))
        if classified:
            keys = [(event["check_key"], event["source_sha256"]) for event in checks]
            metrics["repeated_checks"] = {"value": len(keys) - len(set(keys)), "source": "client trace; same classified check and source digest",
                                         "classification": CHECK_CLASSIFICATION,
                                         "complete": response["tool_trace_complete"]}
        else:
            metrics["repeated_checks"] = unavailable("trace lacks supported exhaustive check classification or completeness")
    for key in ("usage_tokens", "client_context_tokens"):
        value = response.get(key)
        if value is not None:
            if not isinstance(value, dict) or not value or not all(
                v is None or (isinstance(v, int) and not isinstance(v, bool) and v >= 0) for v in value.values()
            ):
                raise ValueError(f"{key} must contain nonnegative integer counts or null")
            metrics[key] = {"value": value, "source": "client/provider telemetry",
                            "complete": response.get("tool_trace_complete")}
    # Unnecessary questions require a separate human annotation; a model's own
    # assessment is not an independent measure. Preserve raw questions in response.
    return actual, mismatches, metrics


def verify_trial(trial_dir):
    metadata = read_json(trial_dir / "trial.json")
    if digest((trial_dir / "prompt.txt").read_bytes()) != metadata["prompt_sha256"]:
        raise ValueError("trial prompt changed")
    if digest((trial_dir / "tools.json").read_bytes()) != metadata["tools_sha256"]:
        raise ValueError("trial tool descriptors changed")
    for name, checksum in metadata["starting_files_sha256"].items():
        path = trial_dir / "checkout" / name
        if path.is_symlink() or not path.is_file() or digest(path.read_bytes()) != checksum:
            raise ValueError(f"trial starting file changed: {name}")
    checkout = trial_dir / "checkout"
    if command(["git", "rev-parse", "HEAD"], checkout).strip() != metadata["source_commit"]:
        raise ValueError("trial source commit changed")
    if command(["git", "status", "--porcelain"], checkout) != metadata["initial_git_status"]:
        raise ValueError("trial starting Git state changed")
    current_names = {str(p.relative_to(checkout)) for p in checkout.rglob("*")
                     if (p.is_file() or p.is_symlink()) and ".git" not in p.relative_to(checkout).parts}
    if current_names != set(metadata["starting_files_sha256"]):
        raise ValueError("unexpected files in trial starting checkout")
    return metadata


def run_trial(trial_dir, config, environment=None):
    metadata = verify_trial(trial_dir)
    environment = environment if environment is not None else environment_versions()
    authority = create_workspace(trial_dir / "checkout", REPO)
    checkout = workspace_path(authority)
    request = {"schema_version": 1, "workspace": str(checkout),
               "prompt": (trial_dir / "prompt.txt").read_text(),
               "tools": read_json(trial_dir / "tools.json"),
               "tools_sha256": metadata["tools_sha256"],
               "requested": {k: v for k, v in config.items() if k != "argv"},
               "response_path": str((trial_dir / "response.json").resolve())}
    dump(trial_dir / "request.json", request)
    result = {"schema_version": 1, "order": metadata["order"], "status": "running",
              "reason": None, "excluded": False, "driver_pid": os.getpid(),
              "descriptor_bytes": metadata["descriptor_bytes"],
              "actual_identity": None, "grade": None, "execution_workspace": authority,
              "environment_before": environment}
    dump(trial_dir / "result.json", result)
    started = time.monotonic()
    edits = []
    mutable = TASKS[metadata["task"]]["solution"]
    initial = fingerprints(checkout, mutable)
    previous = initial
    interrupted = False
    child = None
    try:
        executable = shutil.which(config["argv"][0])
        if executable is None:
            raise ValueError("configured adapter executable is unavailable")
        result["adapter_executable_sha256"] = digest(Path(executable).read_bytes())
        with (trial_dir / "stdout.log").open("wb") as stdout, (trial_dir / "stderr.log").open("wb") as stderr:
            argv = [str((trial_dir.parents[1] / "inputs/implementation/scripts/harness_eval/openai_adapter.py").resolve())
                    if arg == "@openai-adapter" else arg for arg in config["argv"]]
            child = subprocess.Popen([*argv, str((trial_dir / "request.json").resolve())],
                                     cwd=checkout, stdout=stdout, stderr=stderr,
                                     stdin=subprocess.DEVNULL, start_new_session=True)
            result["child_pid"] = child.pid
            dump(trial_dir / "result.json", result)
            while True:
                current = fingerprints(checkout, mutable)
                if current != previous:
                    edits.append({"elapsed_seconds": time.monotonic() - started, "files": current})
                    previous = current
                returncode = exit_status(child)
                if returncode is not None:
                    break
                if time.monotonic() - started >= config["timeout_seconds"]:
                    raise subprocess.TimeoutExpired(config["argv"][0], config["timeout_seconds"])
                time.sleep(0.05)
            result["returncode"] = returncode
            result["status"] = "completed" if returncode == 0 else "failed"
            if returncode:
                result["reason"] = "client process failed; see retained logs"
    except subprocess.TimeoutExpired:
        result.update(status="timeout", reason="configured wall-time limit reached")
    except KeyboardInterrupt:
        interrupted = True
        result.update(status="interrupted", reason="operator interrupted the driver")
    except (OSError, ValueError) as error:
        result.update(status="failed", reason=str(error))
    finally:
        if child is not None:
            # Let the bundled adapter retire its active command group before
            # forcibly retiring the outer client group. Never reap before signals.
            try:
                stop_group(child, grace_seconds=5)
            except Exception as error:
                reason = "client process-group cleanup unresolved; inspect recorded process and workspace before recovery"
                result.update(execution_status=result["status"], status="cleanup_failed",
                              cleanup_error=str(error), reason=reason, excluded=True,
                              exclusion_reason=reason, initial_files=initial,
                              elapsed_seconds=time.monotonic() - started)
                # Publish before propagating: neither replay nor local finalization
                # is safe while the detached workspace may still be changing.
                dump(trial_dir / "result.json", result)
                dump(trial_dir / "edit-observations.json", edits)
                raise RuntimeError(reason) from error
        result["elapsed_seconds"] = time.monotonic() - started
    dump(trial_dir / "edit-observations.json", edits)
    result.update(execution_status=result["status"], status="finalizing", initial_files=initial)
    dump(trial_dir / "result.json", result)
    result = finalize_trial(trial_dir, config, result)
    if interrupted:
        raise KeyboardInterrupt
    return result


def finalize_trial(trial_dir, config, result):
    """Resume local finalization only; never launch or replay a client here."""
    if result.get("cleanup_error") is not None or result["status"] == "cleanup_failed":
        raise ValueError("cannot finalize a trial with unresolved client process-group cleanup")
    metadata = read_json(trial_dir / "trial.json")
    checkout = trial_dir / "checkout"
    if result.get("execution_workspace") and not result.get("checkout_retained"):
        retain_workspace(result["execution_workspace"], checkout)
        result["checkout_retained"] = True
        dump(trial_dir / "result.json", result)
    edits_path = trial_dir / "edit-observations.json"
    edits = read_json(edits_path) if edits_path.exists() else []
    mutable = TASKS[metadata["task"]]["solution"]
    initial = result.get("initial_files", {
        name: checksum for name, checksum in metadata["starting_files_sha256"].items()
        if name in mutable
    })
    execution_status = result.get("execution_status", result["status"])
    result["grade"] = grade(metadata["task"], checkout)
    for name in ("AGENTS.md", ".agent/PLANS.md"):
        path = checkout / name
        if not path.is_file() or path.is_symlink() or digest(path.read_bytes()) != metadata["starting_files_sha256"][name]:
            result["grade"]["invariant_violations"].append(f"trial guidance changed: {name}")
            result["grade"]["passed"] = False
    result["first_useful_edit_seconds"] = unavailable("no observed retained edit in a passing outcome")
    if result["grade"]["passed"]:
        final = fingerprints(checkout, mutable)
        retained = [edit["elapsed_seconds"] for edit in edits if any(
            checksum == final.get(name) and checksum != initial.get(name)
            for name, checksum in edit["files"].items())]
        if retained:
            result["first_useful_edit_seconds"] = {"value": min(retained),
                "source": "50ms polling; earliest changed implementation retained in a passing final outcome"}
    try:
        response_path = trial_dir / "response.json"
        response = read_json(response_path) if response_path.exists() else {}
        actual, mismatches, metrics = observations(response, config, metadata["tools_sha256"])
        result.update(actual_identity=actual, metrics=metrics)
        outcome = response.get("provider_outcome") or {}
        if outcome.get("status") == "incomplete":
            result.update(provider_outcome=outcome, excluded=True,
                          exclusion_reason="provider response incomplete: " + str(outcome.get("details")))
            execution_status = "incomplete"
        client_outcome = response.get("client_outcome") or {}
        if client_outcome.get("status") == "execution_failed":
            result.update(client_outcome=client_outcome, excluded=True,
                          exclusion_reason="client execution failed: " + str(client_outcome.get("reason")))
        if mismatches:
            result.update(excluded=True, exclusion_reason="missing or different observed execution: " + ", ".join(mismatches))
    except (OSError, ValueError, TypeError, AttributeError) as error:
        result.update(excluded=True, exclusion_reason=f"invalid client observations: {error}")
    if result.get("environment_before") is not None:
        result["environment_after"] = environment_versions()
        if result["environment_after"] != result["environment_before"]:
            result.update(excluded=True, exclusion_reason="fixture toolchain or host environment changed during trial")
    result["status"] = execution_status
    result = apply_exclusion(trial_dir, result)
    dump(trial_dir / "result.json", result)
    if result.get("execution_workspace"):
        release_workspace(result["execution_workspace"])
    return result


def run(output, limit=None):
    with experiment_lock(output):
        return run_locked(output, limit)


def run_locked(output, limit):
    manifest = verify_inputs(output)
    check_environment(manifest["environment"])
    config = read_json(output / "inputs/execution.json")
    if config is None:
        raise ValueError("prepare with --config before selecting model execution")
    results = []
    count = 0
    for trial in manifest["schedule"]:
        trial_dir = output / "trials" / f"{trial['order']:03d}"
        result_path = trial_dir / "result.json"
        result = read_json(result_path) if result_path.exists() else None
        result = apply_exclusion(trial_dir, result)
        if result is not None:
            if result["status"] == "running":
                raise ValueError("trial remains running; inspect its recorded process before excluding it")
            if result["status"] == "finalizing" or (
                result.get("grade") is None and not result.get("excluded")
            ):
                check_environment(manifest["environment"])
                result = finalize_trial(trial_dir, config, result)
        elif limit is None or count < limit:
            result = run_trial(trial_dir, config, environment=check_environment(manifest["environment"]))
            count += 1
        else:
            continue
        annotation = trial_dir / "annotations.json"
        if annotation.exists():
            result.setdefault("metrics", {})["unnecessary_questions"] = read_json(annotation)
        results.append(result)
    return {"schema_version": 1, "scheduled": len(manifest["schedule"]),
            "finished": len(results), "results": results}


def check_environment(expected):
    observed = environment_versions()
    if observed != expected:
        raise ValueError("fixture toolchain or host environment changed; prepare a new experiment")
    return observed
