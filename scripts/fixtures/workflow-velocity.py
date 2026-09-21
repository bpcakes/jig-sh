#!/usr/bin/env python3
"""Fixed, disposable T-03 experiments. No production admission or readiness logic."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlencode

ASSETS = Path(__file__).with_name("workflow_velocity")
CONDITIONS = ("isolated", "same_run", "two_requests")


def command(argv, cwd=None, env=None):
    result = subprocess.run(argv, cwd=cwd, env=env, text=True, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(f"setup command failed: {argv}: {result.stderr}")
    return result.stdout.strip()


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def free_ports():
    with socket.socket() as first, socket.socket() as second:
        first.bind(("127.0.0.1", 0))
        second.bind(("127.0.0.1", 0))
        return first.getsockname()[1], second.getsockname()[1]


def fixture(root, mode):
    write(root / "example/src/lib.rs", "pub fn example() -> u8 { 1 }\n")
    write(root / "example/Cargo.toml", '[package]\nname = "example-project"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n')
    for asset in ("child.py", "server.py", "playwright.config.cjs", "ownership.spec.cjs"):
        shutil.copyfile(ASSETS / asset, root / "example" / asset)
    if mode == "cargo":
        shutil.copyfile(ASSETS / "build.rs", root / "example/build.rs")
    write(root / ".gitignore", ".agent/state/\n.agent/.cache/\n.agent/runtime/\n")
    components = [{"id": "example", "root": "example", "adapters": ["rust"]}]
    actions = []
    config = ['_src_path = "embedded:jig-sh"', '_commit = "fixture"', 'repo_name = "ExampleProject"', 'default_branch = "main"',
              '[execution]', 'command_timeout_seconds = 90',
              '[repository]', 'default_check_profile = "probe"',
              '[[repository.components]]', 'id = "example"', 'root = "example"', 'adapters = ["rust"]']
    for label in ("left", "right"):
        target = {"component": "example", "action": label}
        runner = {"kind": "argv", "program": sys.executable,
                  "args": ["child.py", mode, label], "working_directory": "example"}
        actions.append({"target": target, "intent": "check", "effects": ["read_only", "process"],
                        "runner": runner, "inputs": ["example/**"]})
        config.extend(['[[repository.actions]]', f'target = {{ component = "example", action = "{label}" }}',
                       'intent = "check"', 'effects = ["read_only", "process"]', 'inputs = ["example/**"]',
                       '[repository.actions.runner]', 'kind = "argv"', f'program = {json.dumps(sys.executable)}',
                       f'args = {json.dumps(runner["args"])}', 'working_directory = "example"'])
    profile = {"id": "probe", "targets": [action["target"] for action in actions]}
    config.extend(['[[repository.profiles]]', 'id = "probe"',
                   'targets = [{ component = "example", action = "left" }, { component = "example", action = "right" }]'])
    write(root / ".jig.toml", "\n".join(config) + "\n")
    write(root / ".agent/jig-contract.json", json.dumps({"contract_version": 8, "tool_namespace": "jig",
          "required_commands": [], "tools": [], "components": components, "actions": actions,
          "profiles": [profile], "default_check_profile": "probe"}))
    command(["cargo", "generate-lockfile", "--offline"], cwd=root / "example")
    command(["git", "init", "-q"], cwd=root)
    command(["git", "add", "."], cwd=root)
    command(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "commit", "-qm", "fixture"], cwd=root)


def events(path):
    if not path.exists():
        return []
    lines = path.read_text().splitlines()
    # A writer can be between write and newline; only consume complete records.
    return [json.loads(line) for line in lines if line.endswith("}")]


def measure(root, scratch, jig, mode, condition, env):
    control = scratch / f'{env.get("PROBE_DATABASE_CASE", mode)}-{condition}'
    control.mkdir()
    event_path = control / "events.jsonl"
    env = dict(env, PROBE_CONTROL=str(control), PROBE_EVENTS=str(event_path), PROBE_CONDITION=condition)
    groups = [["left"]] if condition == "isolated" else ([["left", "right"]] if condition == "same_run" else [["left"], ["right"]])
    argv = [[str(jig), "check", *[f"example:{label}" for label in group], "--no-receipt", "--json"] for group in groups]
    started = time.monotonic()
    processes, streams = [], []
    released = None
    try:
        for args in argv:
            pair = (tempfile.TemporaryFile(mode="w+"), tempfile.TemporaryFile(mode="w+"))
            streams.append(pair)
            processes.append(subprocess.Popen(args, cwd=root, env=env, stdout=pair[0], stderr=pair[1],
                                              text=True, start_new_session=True))
        deadline = started + 80
        while any(process.poll() is None for process in processes):
            observed = events(event_path)
            ready = (control / "build-ready").exists() if mode == "cargo" else any(control.glob("browser-ready-*"))
            competitor_observed = any(
                event["kind"] == "cargo_lock_wait" and event["resource"] == "build_directory"
                if mode == "cargo" else event["kind"] == "child_end"
                for event in observed)
            if env.get("PROBE_SEPARATE_BROWSER_PORTS") == "1":
                competitor_observed = all((control / f"browser-ready-{label}").exists() for group in groups for label in group)
            if released is None and ready and (condition == "isolated" or competitor_observed):
                (control / "release").touch()
                released = time.monotonic()
            if time.monotonic() >= deadline:
                raise TimeoutError(f"{mode}/{condition}: no bounded terminal result")
            time.sleep(0.02)
        wall = time.monotonic() - started
        outputs = []
        for pair in streams:
            for output in pair:
                output.seek(0)
            outputs.append(tuple(output.read() for output in pair))
        statuses = [process.returncode for process in processes]
        observed = events(event_path)
        starts = [event for event in observed if event["kind"] == "child_start"]
        ends = [event for event in observed if event["kind"] == "child_end"]
        assert len(starts) == len(ends) == sum(map(len, groups)), (statuses, outputs, observed)
        by_label = {event["label"]: event["status"] for event in ends}
        assert statuses == [int(any(by_label[label] for label in group)) for group in groups], (statuses, outputs, observed)
        result = {"mode": mode, "condition": condition, "state": env.get("PROBE_DATABASE_CASE"),
                  "wall_seconds": round(wall, 3), "request_statuses": statuses,
                  "child_statuses": [event["status"] for event in ends], "child_launches": len(starts),
                  "events": [dict(event, at=round(event["at"] - started, 3)) for event in observed],
                  "child_elapsed_seconds_including_wait": {end["label"]: round(end["at"] - next(start["at"] for start in starts if start["label"] == end["label"]), 3) for end in ends},
                  "controlled_release_seconds": None if released is None else round(released - started, 3)}
        result["build_lock_wait_lower_bound_seconds"] = {
            event["label"]: round(max(0, released - event["at"]), 3)
            for event in observed if released is not None and event["kind"] == "cargo_lock_wait"
            and event["resource"] == "build_directory"}
        result["cargo_check_launches"] = len(starts) if mode == "cargo" else sum(event["kind"] == "cargo_invocation" and event["argv"][0] == "check" for event in observed)
        if mode == "browser":
            result["port_ownership"] = "distinct_pairs" if env.get("PROBE_SEPARATE_BROWSER_PORTS") == "1" else "shared_pair"
        result["requests"] = []
        for args, (out, err) in zip(argv, outputs):
            body = json.loads(out)
            result["requests"].append({"argv": ["<jig>", *args[1:]], "ok": body["ok"],
                "layers": body["plan"]["execution_layers"], "stderr": err,
                "targets": [{"target": item["target"], "result": item["response"]["result"]} for item in body["results"]]})
        return result
    finally:
        for process in processes:
            if process.poll() is None:
                # Let Jig's existing supervisor clean up its owned child groups.
                process.terminate()
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        for pair in streams:
            for output in pair:
                output.close()


def database_setup(scratch, root, env, pg_bin):
    data, sockets = scratch / "postgres", scratch / "socket"
    sockets.mkdir()
    port, _ = free_ports()
    command([str(pg_bin / "initdb"), "-D", str(data), "--auth=trust", "--username=example_admin", "--no-locale"])
    command([str(pg_bin / "pg_ctl"), "-D", str(data), "-l", str(scratch / "postgres.log"),
             "-o", f"-k {sockets} -p {port} -h ''", "-w", "start"])
    psql = [str(pg_bin / "psql"), "-h", str(sockets), "-p", str(port), "-U", "example_admin", "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-c"]
    try:
        command([*psql, "CREATE ROLE example_user LOGIN"])
        command([*psql, "CREATE DATABASE example_database"])
        command([*psql, "REVOKE CONNECT ON DATABASE example_database FROM PUBLIC"])
    except BaseException:
        command([str(pg_bin / "pg_ctl"), "-D", str(data), "-m", "immediate", "-w", "stop"])
        raise
    proxy = scratch / "cargo-proxy"
    write(proxy, '#!/usr/bin/env python3\nimport os, sys\nos.execv(sys.executable, [sys.executable, ' + repr(str(root / "example/child.py")) + ', "cargo-proxy", *sys.argv[1:]])\n')
    proxy.chmod(0o755)
    env.update(CARGO=str(proxy), SQLX_OFFLINE="false", SQLX_OFFLINE_DIR=".sqlx")
    (root / "example/.sqlx").mkdir()
    return data, psql, urlencode({"host": str(sockets), "port": port})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", choices=("cargo", "browser", "database"))
    parser.add_argument("--jig", type=Path, required=True)
    parser.add_argument("--playwright", type=Path, help="Existing @playwright/test 1.62.1 package")
    parser.add_argument("--sqlx", type=Path, help="Existing sqlx-cli 0.9.0 executable")
    parser.add_argument("--separate-browser-ports", action="store_true", help="Control: use the generated E2E port overrides to give each invocation its own pair")
    parser.add_argument("--pg-bin", type=Path, default=Path("/usr/lib/postgresql/18/bin"))
    args = parser.parse_args()
    jig = args.jig.resolve(strict=True)
    if args.probe == "browser":
        assert args.playwright and json.loads((args.playwright / "package.json").read_text())["version"] == "1.62.1"
    if args.probe == "database":
        assert args.sqlx and command([str(args.sqlx.resolve()), "--version"]) == "sqlx-cli 0.9.0"
    versions = {"jig": command([str(jig), "--version"]), "cargo": command(["cargo", "--version"])}
    with tempfile.TemporaryDirectory(prefix="ExampleProject-workflow-") as temporary:
        scratch = Path(temporary)
        root = scratch / "repo"
        root.mkdir()
        fixture(root, args.probe)
        env = dict(os.environ, PROBE_CARGO=shutil.which("cargo"), PROBE_PYTHON=sys.executable,
                   CARGO_TARGET_DIR=str(scratch / "cargo-target"))
        if args.probe == "browser":
            ports = free_ports()
            env.update(PROBE_NODE=shutil.which("node"), PROBE_PLAYWRIGHT=str(args.playwright.resolve()),
                       E2E_API_PORT=str(ports[0]), E2E_WEB_PORT=str(ports[1]), CI="1")
            if args.separate_browser_ports:
                alternate = free_ports()
                assert not set(ports) & set(alternate), "ephemeral port selection collided; setup is inconclusive"
                env.update(PROBE_SEPARATE_BROWSER_PORTS="1", PROBE_ALT_API_PORT=str(alternate[0]), PROBE_ALT_WEB_PORT=str(alternate[1]))
            versions["playwright"] = "1.62.1"
        results = []
        if args.probe == "database":
            env["PROBE_SQLX"] = str(args.sqlx.resolve())
            data, psql, query = database_setup(scratch, root, env, args.pg_bin)
            versions.update(sqlx="0.9.0", postgres=command([str(args.pg_bin / "postgres"), "--version"]))
            try:
                for state in ("missing", "denied_connect", "repaired"):
                    if state == "repaired":
                        command([*psql, "GRANT CONNECT ON DATABASE example_database TO example_user"])
                    database = "example_missing" if state == "missing" else "example_database"
                    env.update(DATABASE_URL=f"postgresql://example_user@localhost/{database}?{query}", PROBE_DATABASE_CASE=state)
                    for condition in CONDITIONS:
                        result = measure(root, scratch, jig, args.probe, condition, env)
                        assert all(status == (0 if state == "repaired" else 1) for status in result["child_statuses"]), result
                        assert result["cargo_check_launches"] == (result["child_launches"] if state == "repaired" else 0), result
                        for request in result["requests"]:
                            for target in request["targets"]:
                                if state != "repaired":
                                    assert ("does not exist" if state == "missing" else "permission denied for database") in target["result"]["stdout"], result
                        results.append(result)
            finally:
                command([str(args.pg_bin / "pg_ctl"), "-D", str(data), "-m", "immediate", "-w", "stop"])
        else:
            for condition in CONDITIONS:
                result = measure(root, scratch, jig, args.probe, condition, env)
                if args.probe == "cargo":
                    assert all(status == 0 for status in result["child_statuses"]), result
                    if condition != "isolated":
                        assert any(event["kind"] == "cargo_lock_wait" and event["resource"] == "build_directory" for event in result["events"]), result
                else:
                    if condition == "isolated" or args.separate_browser_ports:
                        assert not any(result["child_statuses"]), result
                    else:
                        assert any(result["child_statuses"]), result
                    if condition != "isolated" and not args.separate_browser_ports:
                        assert any(event["kind"] == "server_conflict" for event in result["events"]), result
                results.append(result)
        # Stable public fixture output: never record host tool or scratch paths.
        output = json.dumps({"versions": versions, "observations": results}, indent=2)
        for value, replacement in [(str(scratch), "<fixture>"), (env["PROBE_CARGO"], "<cargo>"),
                                   (env.get("PROBE_SQLX", ""), "<sqlx>"),
                                   (env.get("PROBE_PLAYWRIGHT", ""), "<playwright>"),
                                   (env.get("PROBE_NODE", ""), "<node>")]:
            if value:
                output = output.replace(value, replacement)
        print(output)


if __name__ == "__main__":
    main()
