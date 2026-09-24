#!/usr/bin/env python3
"""Measure dependency scheduling with real invocations, receipts, and source scans.

Run each binary with a distinct --phase (before/after) and a fresh --outdir.
Each case gets three fresh generic Git repositories. Setup and work start are
outside the timed interval; check planning, commands, validation, and publication
are inside it. Keep builds and other tests idle during measurement. Outputs are
retained without overwriting earlier samples. Timing is evidence, not a test gate.
"""
import argparse
import hashlib
import json
import os
import pathlib
import statistics
import subprocess
import time


def literal(value):
    if isinstance(value, dict):
        return '{ ' + ', '.join(json.dumps(k) + ' = ' + literal(v) for k, v in value.items()) + ' }'
    if isinstance(value, list):
        return '[' + ', '.join(literal(v) for v in value) + ']'
    return json.dumps(value)


def environment():
    env = dict(os.environ)
    env.update(NO_COLOR='1', GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    env.pop('JIG_REPO_ROOT', None)
    env.pop('JIG_INVOKE_CWD', None)
    return env


def checked(argv, root):
    result = subprocess.run(argv, cwd=root, env=environment(), text=True, capture_output=True, timeout=90)
    if result.returncode:
        raise RuntimeError(f'{argv!r}: {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result


def make_fixture(root, case):
    root.mkdir(parents=True)
    (root / '.agent').mkdir()
    (root / 'src').mkdir()
    (root / 'src/lib.rs').write_text('pub fn example() {}\n')
    (root / 'Cargo.toml').write_text('[package]\nname="example-velocity-fixture"\nversion="0.1.0"\nedition="2021"\n')
    (root / 'Cargo.lock').write_text('version = 4\n\n[[package]]\nname = "example-velocity-fixture"\nversion = "0.1.0"\n')
    (root / '.gitignore').write_text('.agent/state/\n.agent/plans/\n.agent/.cache/\n')
    if case == 'critical-path':
        names = [('prerequisite', '0.1'), ('dependent', '2'), ('slow', '2')]
        command = 'sleep "$EXAMPLE_DURATION"'
    else:
        names = [('root-' + str(index).zfill(2), '0') for index in range(16)] + [('dependent', '0')]
        command = ':'
    predecessor = names[0][0]
    actions = []
    for name, duration in names:
        action = {
            'target': {'component': 'example', 'action': name},
            'intent': 'check', 'effects': ['read_only', 'process'],
            'inputs': ['src/**'], 'inputs_policy': 'exhaustive', 'timeout_seconds': 30,
            'runner': {'kind': 'shell', 'command': 'example_check_command',
                       'environment': {'EXAMPLE_DURATION': duration}},
        }
        if name == 'dependent':
            action['depends_on'] = [{'component': 'example', 'action': predecessor}]
        actions.append(action)
    components = [{'id': 'example', 'root': '.', 'adapters': ['rust']}]
    profiles = [{'id': 'verify', 'targets': [a['target'] for a in actions]}]
    config = {'_src_path': '/tmp/template', '_commit': 'abc123', 'repo_name': 'ExampleVelocityProject',
              'default_branch': 'main', 'commands': {'example_check_command': command},
              'repository': {'components': components, 'actions': actions, 'profiles': profiles,
                             'default_check_profile': 'verify'},
              'work': {'gates': [{'id': 'full', 'kind': 'evidence', 'profile': 'verify'}]}}
    (root / '.jig.toml').write_text('\n'.join(json.dumps(k) + ' = ' + literal(v) for k, v in config.items()) + '\n')
    manifest = {'contract_version': 11, 'tool_namespace': 'jig', 'required_commands': ['example_check_command'],
                'tools': [], 'components': components, 'actions': actions, 'profiles': profiles,
                'default_check_profile': 'verify'}
    (root / '.agent/jig-contract.json').write_text(json.dumps(manifest, indent=2) + '\n')
    for argv in [['git', 'init', '--quiet'], ['git', 'add', '.'],
                 ['git', '-c', 'user.name=Example Agent', '-c', 'user.email=example@example.invalid',
                  'commit', '--quiet', '-m', 'Example velocity benchmark']]:
        checked(argv, root)


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def find_key(value, key):
    if isinstance(value, dict):
        if key in value:
            return value[key]
        for child in value.values():
            found = find_key(child, key)
            if found is not None:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_key(child, key)
            if found is not None:
                return found
    return None


def run_sample(binary, root, case, phase, repetition):
    make_fixture(root, case)
    plan_id = checked([binary, 'work', 'start', '--title', 'Example velocity benchmark', '--body',
                       'Measure dependency scheduling and source observation overhead.', '--print-plan-id'], root).stdout.strip()
    argv = [binary, 'check', '--profile', 'verify', '--plan-id', plan_id, '--json']
    started_wall_ms = time.time_ns() // 1_000_000
    started = time.perf_counter()
    result = checked(argv, root)
    wall_seconds = time.perf_counter() - started
    cache = root / '.agent/.cache'
    cache.mkdir(parents=True, exist_ok=True)
    (cache / 'check.stdout.json').write_text(result.stdout)
    (cache / 'check.stderr').write_text(result.stderr)
    response = json.loads(result.stdout)
    events = records(root / '.agent/state/runs.jsonl')
    receipts = records(root / '.agent/state/receipts.jsonl')
    finished = [e for e in events if e.get('event') == 'target_completed']
    targets = [{k: e['result'].get(k) for k in ['target', 'conclusion', 'started_at_ms', 'ended_at_ms', 'receipt_id']}
               for e in finished]
    for target in targets:
        target['start_from_invocation_ms'] = target['started_at_ms'] - started_wall_ms
        target['end_from_invocation_ms'] = target['ended_at_ms'] - started_wall_ms
    own_receipts = [r for r in receipts if isinstance(r.get('target'), dict)]
    states = [r.get('target_freshness', {}).get('state') for r in own_receipts]
    dependent = next(r for r in own_receipts if r['target']['action'] == 'dependent')
    proofs = dependent.get('target_freshness', {}).get('dependency_execution_proof', [])
    assert len(targets) == (3 if case == 'critical-path' else 17), targets
    assert all(target['conclusion'] == 'success' for target in targets), targets
    assert all(state == 'complete' for state in states), states
    assert len(proofs) == 1, dependent
    predecessor = next(r for r in own_receipts if r['target']['action'] ==
                       ('prerequisite' if case == 'critical-path' else 'root-00'))
    for key in ['run_id', 'plan_id']:
        assert proofs[0][key] == predecessor[key], proofs
    assert proofs[0]['receipt_id'] == predecessor['id'], proofs
    assert proofs[0]['identity_digest'] == predecessor['target_freshness']['identity']['identity_digest'], proofs
    return {'phase': phase, 'case': case, 'repetition': repetition, 'fixture': str(root), 'argv': argv,
            'wall_seconds': wall_seconds, 'source_observations': find_key(response, 'source_observations'),
            'targets': targets, 'receipt_count': len(own_receipts), 'freshness_states': states,
            'dependent_proofs': proofs}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--phase', choices=['before', 'after'], required=True)
    parser.add_argument('--outdir', default='/tmp/jig-4117-velocity-benchmark')
    args = parser.parse_args()
    binary = str(pathlib.Path(args.binary).resolve())
    binary_sha256 = hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest()
    outdir = pathlib.Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    version = checked([binary, '--version'], outdir).stdout.strip()
    output = outdir / (args.phase + '.jsonl')
    with output.open('x') as stream:
        for case in ['critical-path', 'wide-noop']:
            for repetition in range(1, 4):
                root = outdir / (args.phase + '-' + case + '-' + str(repetition))
                measurement = run_sample(binary, root, case, args.phase, repetition)
                measurement['version'] = version
                measurement['binary_sha256'] = binary_sha256
                stream.write(json.dumps(measurement) + '\n')
                stream.flush()
                print(json.dumps({k: measurement[k] for k in ['phase', 'case', 'repetition', 'wall_seconds', 'source_observations']}), flush=True)
    measurements = records(output)
    summary = {'phase': args.phase, 'binary': binary, 'binary_sha256': binary_sha256,
               'version': version, 'cases': {}}
    for case in ['critical-path', 'wide-noop']:
        values = [m['wall_seconds'] for m in measurements if m['case'] == case]
        summary['cases'][case] = {'median_seconds': statistics.median(values), 'min_seconds': min(values), 'max_seconds': max(values)}
    (outdir / (args.phase + '-summary.json')).write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))

if __name__ == '__main__':
    main()
