#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
CHANGELOG_PATH="$ROOT_DIR/CHANGELOG.md"

print_usage() {
  cat <<'EOF'
Usage:
  scripts/release-notes.sh update [VERSION]
  scripts/release-notes.sh print [VERSION]

Commands:
  update    Promote ## Unreleased notes into the VERSION section when present;
            otherwise generate notes from git history.
  print     Print the CHANGELOG.md section for VERSION.

VERSION may be prefixed with v. It defaults to scripts/release.sh version.
EOF
}

usage() {
  print_usage >&2
  exit 2
}

normalize_version() {
  local version="$1"
  version="${version#v}"
  if [[ -z "$version" ]]; then
    echo "Version must not be empty." >&2
    exit 2
  fi
  printf '%s\n' "$version"
}

release_version() {
  if [[ $# -gt 1 ]]; then
    usage
  fi
  if [[ $# -eq 1 ]]; then
    normalize_version "$1"
  else
    "$ROOT_DIR/scripts/release.sh" version
  fi
}

generate_section() {
  local version="$1"
  local tag="v$version"
  local to_ref="${RELEASE_NOTES_TO:-HEAD}"
  local from_ref="${RELEASE_NOTES_FROM:-}"
  local range_args=()
  local subjects_file

  if [[ -z "$from_ref" ]]; then
    if git -C "$ROOT_DIR" rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
      to_ref="$tag"
      from_ref="$(git -C "$ROOT_DIR" describe --tags --abbrev=0 --match 'v[0-9]*' "$tag^" 2>/dev/null || true)"
    else
      from_ref="$(git -C "$ROOT_DIR" describe --tags --abbrev=0 --match 'v[0-9]*' "$to_ref" 2>/dev/null || true)"
    fi
  fi

  if [[ -n "$from_ref" ]]; then
    range_args=("$from_ref..$to_ref")
  else
    range_args=("$to_ref")
  fi

  subjects_file="$(mktemp)"
  if ! git -C "$ROOT_DIR" log --no-merges --reverse --format='%s' "${range_args[@]}" >"$subjects_file"; then
    rm -f "$subjects_file"
    return 1
  fi
  if ! python3 - "$version" "${RELEASE_DATE:-$(date -u +%F)}" "$subjects_file" <<'PY'
import pathlib
import re
import sys

version = sys.argv[1]
release_date = sys.argv[2]
subjects = [
    line.strip()
    for line in pathlib.Path(sys.argv[3]).read_text().splitlines()
    if line.strip()
    and not re.fullmatch(
        r"release v?\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?",
        line.strip(),
        re.IGNORECASE,
    )
]

groups = [
    ("Added", {"feat"}),
    ("Fixed", {"fix"}),
    ("Changed", {"refactor", "perf", "build", "ci", "chore"}),
    ("Documentation", {"docs"}),
    ("Tests", {"test", "tests"}),
    ("Other", set()),
]
known_prefixes = set().union(*(prefixes for _, prefixes in groups))
bucketed = {heading: [] for heading, _ in groups}
prefix_pattern = re.compile(r"^([A-Za-z]+)(?:\([^)]+\))?!?:\s*(.+)$")

for subject in subjects:
    match = prefix_pattern.match(subject)
    prefix = match.group(1).lower() if match else ""
    text = match.group(2).strip() if match else subject
    text = text[:1].upper() + text[1:] if text else subject

    for heading, prefixes in groups:
        if prefix in prefixes or (heading == "Other" and prefix not in known_prefixes):
            bucketed[heading].append(text)
            break

print(f"## v{version} - {release_date}")
print()
if not subjects:
    print("### Other")
    print("- No user-facing changes recorded.")
else:
    for heading, _ in groups:
        entries = bucketed[heading]
        if not entries:
            continue
        print(f"### {heading}")
        for entry in entries:
            print(f"- {entry}")
        print()
PY
  then
    rm -f "$subjects_file"
    return 1
  fi
  rm -f "$subjects_file"
}

update_changelog() {
  local version="$1"
  local tmp_section
  tmp_section="$(mktemp)"
  if ! generate_section "$version" >"$tmp_section"; then
    rm -f "$tmp_section"
    return 1
  fi

  if ! python3 - "$CHANGELOG_PATH" "$tmp_section" "$version" "${RELEASE_DATE:-$(date -u +%F)}" <<'PY'
import pathlib
import re
import sys
import os

changelog_path = pathlib.Path(sys.argv[1])
section_path = pathlib.Path(sys.argv[2])
version = sys.argv[3]
release_date = sys.argv[4]
generated = section_path.read_text().rstrip() + "\n"

if changelog_path.exists():
    text = changelog_path.read_text()
else:
    text = "# Changelog\n"

if not text.startswith("# Changelog"):
    raise SystemExit("CHANGELOG.md must start with '# Changelog'.")

unreleased_pattern = re.compile(
    r"^## Unreleased\n+(.*?)(?=^## |\Z)",
    re.MULTILINE | re.DOTALL,
)
version_pattern = re.compile(
    rf"^## v{re.escape(version)} - .+?(?=^## |\Z)",
    re.MULTILINE | re.DOTALL,
)
force = bool(int(os.environ.get("RELEASE_NOTES_FORCE", "0")))
version_exists = version_pattern.search(text)
if version_exists and not force:
    raise SystemExit(
        f"CHANGELOG.md already has a v{version} section. Set RELEASE_NOTES_FORCE=1 to replace it."
    )

section = generated
unreleased = unreleased_pattern.search(text)
if unreleased:
    body_lines = unreleased.group(1).strip().splitlines()
    heading_idx = next(
        (index for index, line in enumerate(body_lines) if line.startswith("### ")),
        None,
    )
    curated = (
        "\n".join(body_lines[heading_idx:]).strip()
        if heading_idx is not None
        else ""
    )
    if curated:
        section = f"## v{version} - {release_date}\n\n{curated}\n"
        text = unreleased_pattern.sub("", text, count=1)

if version_pattern.search(text):
    text = version_pattern.sub(lambda _match: section.rstrip() + "\n\n", text)
else:
    lines = text.rstrip().splitlines()
    if len(lines) == 1:
        text = lines[0] + "\n\n" + section
    else:
        text = lines[0] + "\n\n" + section + "\n" + "\n".join(lines[1:]).lstrip() + "\n"

changelog_path.write_text(re.sub(r"\n{3,}", "\n\n", text).rstrip() + "\n")
PY
  then
    rm -f "$tmp_section"
    return 1
  fi

  rm -f "$tmp_section"
}

print_changelog_section() {
  local version="$1"
  python3 - "$CHANGELOG_PATH" "$version" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
version = sys.argv[2]
if not path.exists():
    raise SystemExit("CHANGELOG.md does not exist.")

text = path.read_text()
match = re.search(
    rf"^## v{re.escape(version)} - .+?(?=^## |\Z)",
    text,
    re.MULTILINE | re.DOTALL,
)
if not match:
    raise SystemExit(f"CHANGELOG.md is missing section for v{version}.")
print(match.group(0).rstrip())
PY
}

if [[ $# -lt 1 ]]; then
  usage
fi

command="$1"
shift
version="$(release_version "$@")"

case "$command" in
  update)
    update_changelog "$version"
    ;;
  print)
    print_changelog_section "$version"
    ;;
  -h|--help|help)
    print_usage
    ;;
  *)
    usage
    ;;
esac
