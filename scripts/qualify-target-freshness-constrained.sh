#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 || ( "$2" != warm && "$2" != cold ) ]]; then
  echo "usage: $0 <release-jig> <warm|cold> <report.json>" >&2
  exit 2
fi
benchmark_binary="$(realpath "$1")"
benchmark_cache="$2"
benchmark_output="$(realpath -m "$3")"
benchmark_scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
benchmark_fixtures="$(mktemp -d /tmp/ExampleFreshnessQualification.XXXXXXXX)"
benchmark_results="$(dirname "$benchmark_output")"
mkdir -p "$benchmark_results"

# A partition's requests are accounted on its containing device. Device-mapper
# mounts keep their mapped queue, which the report verifies using actual rbytes.
benchmark_device="${JIG_BENCH_DEVICE:-}"
if [[ -z "$benchmark_device" ]]; then
  benchmark_device="$(python3 - "$benchmark_fixtures" <<'PY'
import os
from pathlib import Path
import sys
device = os.stat(sys.argv[1]).st_dev
entry = (Path('/sys/dev/block') / f'{os.major(device)}:{os.minor(device)}').resolve(strict=True)
if (entry / 'partition').exists():
    entry = entry.parent
print(Path('/dev') / entry.name)
PY
)"
fi
if [[ ! -b "$benchmark_device" ]]; then
  echo "Backing device is unavailable; set JIG_BENCH_DEVICE to its block-device path." >&2
  exit 2
fi
docker build --quiet --tag jig-target-freshness-benchmark --file "$benchmark_scripts/target-freshness-benchmark.Dockerfile" "$benchmark_scripts"
docker run --rm --network none --cpus 1 \
  --device-read-bps "$benchmark_device:20971520" \
  --cap-drop ALL --security-opt no-new-privileges \
  --user "$(id -u):$(id -g)" \
  -e TMPDIR=/tmp/ExampleBenchmark -e GITHUB_ACTIONS -e GITHUB_RUN_ID \
  -v "$benchmark_fixtures:/tmp/ExampleBenchmark" \
  -v "$benchmark_results:/results" \
  -v "$benchmark_binary:/jig:ro" \
  -v "$benchmark_scripts/benchmark-target-freshness-commands.py:/benchmark.py:ro" \
  jig-target-freshness-benchmark \
  python3 /benchmark.py --binary /jig --cache "$benchmark_cache" --profile constrained \
    --output "/results/$(basename "$benchmark_output")"

if [[ "$benchmark_cache" == cold ]]; then
  docker run --rm --network none --cpus 1 \
    --device-read-bps "$benchmark_device:20971520" \
    --cap-drop ALL --security-opt no-new-privileges \
    --user "$(id -u):$(id -g)" \
    -e TMPDIR=/tmp/ExampleBenchmark -e GITHUB_ACTIONS -e GITHUB_RUN_ID \
    -v "$benchmark_fixtures:/tmp/ExampleBenchmark" \
    -v "$benchmark_results:/results" \
    -v "$benchmark_binary:/jig:ro" \
    -v "$benchmark_scripts:/benchmark-scripts:ro" \
    jig-target-freshness-benchmark \
    python3 /benchmark-scripts/benchmark-target-freshness-limits.py --binary /jig \
      --profile constrained --output /results/limits-constrained.json
fi
