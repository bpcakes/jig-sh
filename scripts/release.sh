#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PACKAGE_NAME="jig-sh"
# Publish workspace library crates before the CLI; cargo publish strips path
# dependencies, so every exact-version internal crate must exist on crates.io
# before dependents can be published.
PUBLISH_PACKAGE_NAMES=(
  "jig-contract"
  "jig-core"
  "jig-rust"
  "jig-sqlx"
  "jig-typescript"
  "jig-features"
  "jig-vault"
  "jig-dev-proxy"
  "jig-tui"
  "jig-status-tui"
  "jig-codex-tui"
  "jig-ui"
  "$PACKAGE_NAME"
)
BIN_NAME="jig"
RELEASE_FIXTURE_FILES=(
  examples/adopted-custom-commands.toml
  examples/backend-only.toml
  examples/full-stack.toml
  examples/rust-backend-only.toml
  examples/rust-sqlx-schema-dump.toml
  examples/tooling-only.toml
  examples/vite-frontend.toml
  tests/fixtures/backend-only.toml
  tests/fixtures/full-stack.toml
  tests/fixtures/tooling-only.toml
)
EXAMPLE_FIXTURE_BASENAMES=(
  backend-only.toml
  full-stack.toml
  tooling-only.toml
)

print_usage() {
  cat <<'EOF'
Usage:
  scripts/release.sh check [VERSION]
  scripts/release.sh prepare [VERSION]
  scripts/release.sh notes [VERSION]
  scripts/release.sh stage
  scripts/release.sh tag [VERSION]
  scripts/release.sh publish [VERSION]
  scripts/release.sh github [VERSION]
  scripts/release.sh next-version [major|minor|patch]
  scripts/release.sh version

Commands:
  check     Run the full local release validation and cargo publish dry run.
  prepare   Update pinned versions and CHANGELOG.md for VERSION.
  notes     Generate or replace the CHANGELOG.md section for VERSION.
  stage     Stage files updated by release-prepare.
  tag       Run release validation, then create annotated tag vVERSION.
  publish   Run release validation, cargo publish all crates, then push vVERSION to origin.
  github    Create the GitHub Release for vVERSION from CHANGELOG.md.
  next-version
            Print the next semantic version using the requested bump. Defaults to patch.
  version   Print the jig-sh package version from Cargo metadata.

VERSION defaults to the package version from Cargo metadata.

Set ALLOW_DIRTY=1 only with `check` to validate working-tree release tooling before committing it.
EOF
}

usage() {
  print_usage >&2
  exit 2
}

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  "$@"
}

crate_dir_for_package() {
  case "$1" in
    jig-contract) printf '%s\n' "crates/jig-contract" ;;
    jig-core) printf '%s\n' "crates/jig-core" ;;
    jig-rust) printf '%s\n' "crates/jig-rust" ;;
    jig-sqlx) printf '%s\n' "crates/jig-sqlx" ;;
    jig-typescript) printf '%s\n' "crates/jig-typescript" ;;
    jig-features) printf '%s\n' "crates/jig-features" ;;
    jig-vault) printf '%s\n' "crates/jig-vault" ;;
    jig-dev-proxy) printf '%s\n' "crates/jig-dev-proxy" ;;
    jig-tui) printf '%s\n' "crates/jig-tui" ;;
    jig-status-tui) printf '%s\n' "crates/jig-status-tui" ;;
    jig-codex-tui) printf '%s\n' "crates/jig-codex-tui" ;;
    jig-ui) printf '%s\n' "crates/jig-ui" ;;
    jig-sh) printf '%s\n' "crates/jig" ;;
    *)
      echo "Unknown publish package: $1" >&2
      exit 1
      ;;
  esac
}

require_publish_manifest_consistency() {
  python3 - "$ROOT_DIR" "${PUBLISH_PACKAGE_NAMES[@]}" <<'PY'
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1])
publish_order = sys.argv[2:]
metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"],
        cwd=root,
        text=True,
    )
)
workspace_members = set(metadata["workspace_members"])
packages = {
    package["name"]: package
    for package in metadata["packages"]
    if package["id"] in workspace_members and package.get("publish") != []
}

if len(publish_order) != len(set(publish_order)):
    raise SystemExit("PUBLISH_PACKAGE_NAMES contains duplicate package names.")

missing = sorted(set(packages) - set(publish_order))
extra = sorted(set(publish_order) - set(packages))
if missing or extra:
    details = []
    if missing:
        details.append(f"missing publishable workspace packages: {', '.join(missing)}")
    if extra:
        details.append(f"unknown or non-publishable packages: {', '.join(extra)}")
    raise SystemExit("PUBLISH_PACKAGE_NAMES is inconsistent with Cargo metadata: " + "; ".join(details))

positions = {name: index for index, name in enumerate(publish_order)}
for package_name, package in packages.items():
    for dependency in package["dependencies"]:
        dependency_name = dependency["name"]
        if (
            dependency.get("source") is None
            and dependency.get("kind") != "dev"
            and dependency_name in packages
            and positions[dependency_name] >= positions[package_name]
        ):
            raise SystemExit(
                "PUBLISH_PACKAGE_NAMES must publish "
                f"{dependency_name} before dependent package {package_name}."
            )
PY
}

manifest_version() {
  local package_name="${1:-$PACKAGE_NAME}"
  cargo metadata --locked --format-version 1 --no-deps |
    python3 -c '
import json
import sys

package_name = sys.argv[1]
metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    if package["name"] == package_name:
        print(package["version"])
        break
else:
    raise SystemExit(f"Package {package_name!r} not found in Cargo metadata.")
' "$package_name"
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
    manifest_version
  fi
}

next_version() {
  local bump="${1:-patch}"
  if [[ $# -gt 1 ]]; then
    usage
  fi

  python3 - "$(manifest_version)" "$bump" <<'PY'
import re
import sys

version = sys.argv[1]
bump = sys.argv[2]
match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?", version)
if not match:
    raise SystemExit(f"Cannot bump non-semver version: {version}")

major, minor, patch = (int(part) for part in match.groups())
if bump == "major":
    major += 1
    minor = 0
    patch = 0
elif bump == "minor":
    minor += 1
    patch = 0
elif bump == "patch":
    patch += 1
else:
    raise SystemExit("Bump must be major, minor, or patch.")

print(f"{major}.{minor}.{patch}")
PY
}

require_clean_tree() {
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 set; skipping clean working tree requirement." >&2
    return 0
  fi

  if [[ -n "$(git status --short --untracked-files=all)" ]]; then
    echo "Working tree is not clean. Commit or discard changes before releasing." >&2
    git status --short --untracked-files=all >&2
    exit 1
  fi
}

require_version_consistency() {
  local version="$1"
  local cargo_version
  local contract_version
  local launcher_version
  local dependency_version

  require_publish_manifest_consistency

  local package_name
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    cargo_version="$(manifest_version "$package_name")"
    if [[ "$cargo_version" != "$version" ]]; then
      echo "Cargo package $package_name version is $cargo_version, expected $version." >&2
      exit 1
    fi
  done

  local dependency_name
  for dependency_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    if [[ "$dependency_name" == "$PACKAGE_NAME" ]]; then
      continue
    fi
    dependency_version="$(python3 - "$ROOT_DIR/Cargo.toml" "$dependency_name" <<'PY'
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text()
name = re.escape(sys.argv[2])
match = re.search(rf'(?m)^{name}\s*=\s*\{{[^}}\n]*version\s*=\s*"=([^"]+)"', text)
if match:
    print(match.group(1))
PY
)"
    if [[ -z "$dependency_version" ]]; then
      echo "Could not read $dependency_name exact dependency version from workspace Cargo.toml." >&2
      exit 1
    fi
    if [[ "$dependency_version" != "$version" ]]; then
      echo "$dependency_name dependency version is $dependency_version, expected $version." >&2
      exit 1
    fi
  done

  contract_version="$(python3 - "$ROOT_DIR/.agent/jig-contract.json" <<'PY'
import json
import pathlib
import sys

print(json.loads(pathlib.Path(sys.argv[1]).read_text())["jig_version"])
PY
)"
  if [[ "$contract_version" != "$version" ]]; then
    echo ".agent/jig-contract.json jig_version is $contract_version, expected $version." >&2
    exit 1
  fi

  launcher_version="$(python3 - "$ROOT_DIR/scripts/jig" <<'PY'
import pathlib
import re
import sys

pattern = re.compile(r"""^JIG_VERSION\s*=\s*["']([^"']+)["']\s*$""")
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    match = pattern.match(line)
    if match:
        print(match.group(1))
        break
PY
)"
  if [[ -z "$launcher_version" ]]; then
    echo "Could not read JIG_VERSION from scripts/jig." >&2
    exit 1
  fi
  if [[ "$launcher_version" != "$version" ]]; then
    echo "scripts/jig JIG_VERSION is $launcher_version, expected $version." >&2
    exit 1
  fi

  local answer_files
  local fixture_files
  mapfile -t answer_files < <(git ls-files | awk '$0 ~ /(^|\/)\.jig\.toml$/ { print }')
  if [[ "${#answer_files[@]}" -eq 0 ]]; then
    echo "No tracked jig answer files found." >&2
    exit 1
  fi

  mapfile -t fixture_files < <(git ls-files -- "${RELEASE_FIXTURE_FILES[@]}")
  if [[ "${#fixture_files[@]}" -eq 0 ]]; then
    echo "No tracked fixture answer files found." >&2
    exit 1
  fi
  for fixture_file in "${fixture_files[@]}"; do
    if ! grep -Eq '^jig_version\s*=' "$ROOT_DIR/$fixture_file"; then
      echo "$fixture_file is a release-pinned fixture and must include jig_version." >&2
      exit 1
    fi
  done

  local version_files=("${answer_files[@]}" "${fixture_files[@]}")

  local version_file
  for version_file in "${version_files[@]}"; do
    if ! git ls-files --error-unmatch "$version_file" >/dev/null 2>&1; then
      echo "Release-pinned jig answer file is not tracked: $version_file" >&2
      exit 1
    fi

    local file_version
    file_version="$(python3 - "$ROOT_DIR/$version_file" <<'PY'
import ast
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text()

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

if tomllib is not None:
    print(tomllib.loads(text).get("jig_version", ""))
    raise SystemExit(0)

# Release-pinned answer files keep jig_version at top level. tomllib remains
# authoritative when available.
def strip_inline_comment(value):
    quote = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            continue
        if quote is not None:
            if char == quote:
                quote = None
            continue
        if char in {"'", '"'}:
            quote = char
            continue
        if char == "#":
            return value[:index].rstrip()
    return value.strip()

pattern = re.compile(r"^\s*jig_version\s*=\s*(.*?)\s*$")
for line in text.splitlines():
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if stripped.startswith("["):
        break
    match = pattern.match(line)
    if match:
        print(ast.literal_eval(strip_inline_comment(match.group(1))))
        break
else:
    print("")
PY
)"
    if [[ -z "$file_version" ]]; then
      echo "$version_file is missing jig_version." >&2
      exit 1
    fi
    if [[ "$file_version" != "$version" ]]; then
      echo "$version_file jig_version is $file_version, expected $version." >&2
      exit 1
    fi
  done

  local answer_name
  for answer_name in "${EXAMPLE_FIXTURE_BASENAMES[@]}"; do
    if ! cmp -s "$ROOT_DIR/examples/$answer_name" "$ROOT_DIR/tests/fixtures/$answer_name"; then
      echo "examples/$answer_name must match tests/fixtures/$answer_name. Update both copies together." >&2
      exit 1
    fi
  done
}

require_expected_binary_name() {
  local metadata_bin_names
  metadata_bin_names="$(
    cargo metadata --locked --format-version 1 --no-deps |
      python3 -c '
import json
import sys

package_name = sys.argv[1]
metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    if package["name"] != package_name:
        continue
    names = sorted(
        target["name"]
        for target in package["targets"]
        if "bin" in target.get("kind", [])
    )
    print("\n".join(names))
    break
' "$PACKAGE_NAME"
  )"

  if ! grep -qx "$BIN_NAME" <<<"$metadata_bin_names"; then
    echo "Expected package $PACKAGE_NAME to expose binary $BIN_NAME." >&2
    echo "Found binaries:" >&2
    printf '%s\n' "$metadata_bin_names" >&2
    exit 1
  fi
}

require_changelog_entry() {
  local version="$1"

  if [[ ! -f "$ROOT_DIR/CHANGELOG.md" ]]; then
    echo "CHANGELOG.md is missing. Run scripts/release.sh notes $version." >&2
    exit 1
  fi

  if ! "$ROOT_DIR/scripts/release-notes.sh" print "$version" >/dev/null; then
    echo "CHANGELOG.md is missing release notes for v$version." >&2
    echo "Run scripts/release.sh notes $version before release validation." >&2
    exit 1
  fi
}

update_version_files() {
  local version="$1"

  python3 - \
    "$ROOT_DIR" \
    "$version" \
    "$PACKAGE_NAME" \
    "${#PUBLISH_PACKAGE_NAMES[@]}" \
    "${PUBLISH_PACKAGE_NAMES[@]}" \
    "${RELEASE_FIXTURE_FILES[@]}" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1])
version = sys.argv[2]
package_name = sys.argv[3]
publish_package_count = int(sys.argv[4])
publish_package_names = sys.argv[5:5 + publish_package_count]
release_fixture_files = sys.argv[5 + publish_package_count:]
semver_release = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
if not re.fullmatch(semver_release, version):
    raise SystemExit(f"Version must be MAJOR.MINOR.PATCH[-PRERELEASE], got {version!r}.")

def replace_required(path, pattern, replacement, label=None, flags=0):
    text = path.read_text()
    next_text, count = re.subn(pattern, replacement, text, flags=flags)
    if count == 0:
        raise SystemExit(f"Could not update {label or pattern!r} in {path}.")
    path.write_text(next_text)

def replace_exactly_once(path, pattern, replacement, label=None, flags=0):
    text = path.read_text()
    next_text, count = re.subn(pattern, replacement, text, flags=flags)
    if count != 1:
        raise SystemExit(f"Expected to update {label or pattern!r} exactly once in {path}; updated {count}.")
    path.write_text(next_text)

def update_jig_toml(path):
    replace_required(
        path,
        r'(?m)^jig_version\s*=\s*"[^"]*"\s*$',
        f'jig_version = "{version}"',
        "jig_version",
    )

def has_jig_version(path):
    return bool(re.search(r'(?m)^jig_version\s*=', path.read_text()))

def git_ls_files(*patterns):
    return subprocess.check_output(
        ["git", "-C", str(root), "ls-files", "--", *patterns],
        text=True,
    ).splitlines()

replace_exactly_once(
    root / "Cargo.toml",
    r'(?ms)(^\[workspace\.package\]\n(?:(?!^\[).)*?^version\s*=\s*)"[^"]*"',
    rf'\g<1>"{version}"',
    "workspace package version",
)
for dependency in publish_package_names:
    if dependency == package_name:
        continue
    escaped_dependency = re.escape(dependency)
    replace_exactly_once(
        root / "Cargo.toml",
        rf'(?m)^({escaped_dependency}\s*=\s*\{{[^}}\n]*version\s*=\s*)"=[^"]*"',
        rf'\g<1>"={version}"',
        f"workspace {dependency} exact dependency version",
    )
for package in publish_package_names:
    replace_exactly_once(
        root / "Cargo.lock",
        rf'(?ms)(\[\[package\]\]\nname = "{re.escape(package)}"\nversion = )"[^"]*"',
        rf'\g<1>"{version}"',
        f"Cargo.lock {package} package version",
    )
replace_exactly_once(
    root / "scripts" / "jig",
    r'(?m)^JIG_VERSION="[^"]*"$',
    f'JIG_VERSION="{version}"',
    "launcher JIG_VERSION",
)

contract_path = root / ".agent" / "jig-contract.json"
contract = json.loads(contract_path.read_text())
contract["jig_version"] = version
contract_path.write_text(json.dumps(contract, indent=2) + "\n")

jig_toml_paths = set()
for relative_path in git_ls_files():
    if pathlib.Path(relative_path).name != ".jig.toml":
        continue
    path = root / relative_path
    jig_toml_paths.add(path)
for relative_path in git_ls_files(*release_fixture_files):
    path = root / relative_path
    if not has_jig_version(path):
        raise SystemExit(f"{path.relative_to(root)} is a release-pinned fixture and must include jig_version.")
    jig_toml_paths.add(path)
if not jig_toml_paths:
    raise SystemExit("No .jig.toml or fixture TOML files found to update.")
for path in sorted(jig_toml_paths):
    update_jig_toml(path)
PY
}

release_notes() {
  local version="$1"
  run "$ROOT_DIR/scripts/release-notes.sh" update "$version"
}

release_stage() {
  local release_path

  for release_path in \
    Cargo.toml Cargo.lock crates/jig/Cargo.toml scripts/jig .agent/jig-contract.json CHANGELOG.md \
    scripts/fixtures/rendered-repos.sh scripts/fixtures/runtime-smoke.sh
  do
    if [[ -e "$ROOT_DIR/$release_path" ]]; then
      run git add "$release_path"
    fi
  done

  git ls-files -z |
    while IFS= read -r -d '' release_path; do
      case "$release_path" in
        .jig.toml|*/.jig.toml)
          run git add "$release_path"
          ;;
      esac
    done

  git ls-files -z -- "${RELEASE_FIXTURE_FILES[@]}" |
    while IFS= read -r -d '' release_path; do
      run git add "$release_path"
    done
}

require_changelog_update_allowed() {
  local version="$1"

  # Check before rewriting version files so an existing generated section fails
  # without leaving a partially prepared release in the worktree.
  if [[ "${RELEASE_NOTES_FORCE:-}" == "1" ]]; then
    return 0
  fi
  if "$ROOT_DIR/scripts/release-notes.sh" print "$version" >/dev/null 2>&1; then
    echo "CHANGELOG.md already has a v$version section. Set RELEASE_NOTES_FORCE=1 to replace it." >&2
    exit 1
  fi
}

release_prepare() {
  local version="$1"

  require_changelog_update_allowed "$version"
  update_version_files "$version"
  run cargo metadata --format-version 1 --no-deps >/dev/null
  release_notes "$version"

  echo "Prepared release files for $PACKAGE_NAME v$version."
}

release_github() {
  local version="$1"
  local tag="v$version"
  local notes_file

  if ! command -v gh >/dev/null 2>&1; then
    echo "GitHub CLI 'gh' is required to create GitHub releases." >&2
    exit 1
  fi

  if gh release view "$tag" >/dev/null 2>&1; then
    echo "GitHub Release $tag already exists."
    return 0
  fi

  notes_file="$(mktemp)"
  if ! "$ROOT_DIR/scripts/release-notes.sh" print "$version" >"$notes_file"; then
    rm -f "$notes_file"
    return 1
  fi
  if ! run gh release create "$tag" --verify-tag --title "$tag" --notes-file "$notes_file"; then
    rm -f "$notes_file"
    return 1
  fi
  rm -f "$notes_file"
}

cargo_dirty_flag=()
cargo_publish_patch_args=()
cargo_dirty_flags() {
  cargo_dirty_flag=()
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    cargo_dirty_flag=(--allow-dirty)
  fi
}

cargo_publish_patch_args_for() {
  local package_name="$1"
  local dependency_name
  local dependency_dir

  cargo_publish_patch_args=()
  for dependency_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    if [[ "$dependency_name" == "$package_name" ]]; then
      continue
    fi
    dependency_dir="$(crate_dir_for_package "$dependency_name")"
    cargo_publish_patch_args+=(--config "patch.crates-io.$dependency_name.path=\"$ROOT_DIR/$dependency_dir\"")
  done
}

crate_version_status() {
  local package_name="$1"
  local version="$2"
  local status
  status="$(curl --max-time 20 -sS -o /dev/null -w '%{http_code}' \
    --user-agent 'jig-sh-release-script (+https://github.com/bpcakes/jig-sh)' \
    "https://crates.io/api/v1/crates/$package_name/$version" || true)"
  printf '%s\n' "${status:-000}"
}

wait_for_crate_version() {
  local package_name="$1"
  local version="$2"
  local attempt
  local status

  for attempt in {1..60}; do
    status="$(crate_version_status "$package_name" "$version")"
    if [[ "$status" == "200" ]]; then
      return 0
    fi
    if [[ "$status" != "404" && "$status" != "000" ]]; then
      echo "crates.io version probe for $package_name v$version returned HTTP $status." >&2
    fi
    sleep 10
  done

  echo "Timed out waiting for $package_name v$version to appear on crates.io." >&2
  exit 1
}

publish_package_if_missing() {
  local package_name="$1"
  local version="$2"
  local status

  status="$(crate_version_status "$package_name" "$version")"
  case "$status" in
    200)
      echo "$package_name v$version is already published; skipping cargo publish."
      ;;
    404)
      run cargo publish -p "$package_name" --locked
      wait_for_crate_version "$package_name" "$version"
      ;;
    *)
      echo "Could not determine whether $package_name v$version is already published; crates.io returned HTTP $status." >&2
      exit 1
      ;;
  esac
}

check_launcher_template() {
  normalize_launcher() {
    sed \
      -e '/^# Keep launcher behavior synchronized /,+1d' \
      -e '/^\[% if repo_name == "jig-sh" %\]$/d' \
      -e '/^\[% endif %\]$/d' \
      -e 's/^JIG_VERSION="[^"]*"$/JIG_VERSION="<<[ jig_version ]>>"/' \
      "$1"
  }

  if ! diff -u \
    <(normalize_launcher "$ROOT_DIR/scripts/jig") \
    <(normalize_launcher "$ROOT_DIR/templates/project/scripts/jig.jinja")
  then
    echo "scripts/jig and templates/project/scripts/jig.jinja drifted." >&2
    exit 1
  fi
}

run_ci_checks() {
  local base_ref

  run cargo build -p jig-sh --bin jig --locked
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check fmt --no-receipt
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check clippy --no-receipt
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check test-locked --no-receipt
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check contract --no-receipt

  if git rev-parse --verify origin/master >/dev/null 2>&1; then
    base_ref="$(git merge-base HEAD origin/master)"
  elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
    base_ref="HEAD^"
  else
    base_ref="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
  fi
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check rust-file-loc --changed-against "$base_ref"
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check no-mod-rs
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check agent-map
  run env JIG_DEV_BIN=target/debug/jig scripts/jig check agent-guides
  check_launcher_template
}

release_check() {
  local version="$1"
  local package_name
  local dependency_status
  cargo_dirty_flags

  require_clean_tree
  require_version_consistency "$version"
  require_expected_binary_name
  require_changelog_entry "$version"

  run_ci_checks
  run bash scripts/validate-fixtures.sh
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    dependency_status="$(crate_version_status "$package_name" "$version")"
    case "$dependency_status" in
      200)
        run cargo publish -p "$package_name" --locked --dry-run "${cargo_dirty_flag[@]}"
        ;;
      404)
        echo "$package_name v$version is not on crates.io yet; dry-running with local workspace dependency patches before any real crate publish."
        cargo_publish_patch_args_for "$package_name"
        run cargo publish -p "$package_name" --locked --dry-run "${cargo_dirty_flag[@]}" "${cargo_publish_patch_args[@]}"
        ;;
      *)
        echo "Could not determine whether $package_name v$version is already published; crates.io returned HTTP $dependency_status." >&2
        exit 1
        ;;
    esac
  done

  echo "Release check passed for workspace crates v$version."
}

release_tag() {
  local version="$1"
  local tag="v$version"

  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 is only supported for release checks, not tagging." >&2
    exit 1
  fi

  if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    echo "Tag $tag already exists." >&2
    exit 1
  fi

  release_check "$version"

  run git tag -a "$tag" -m "$PACKAGE_NAME $tag"
  echo "Created tag $tag. release-publish will push it to origin after all crates publish successfully."
}

remote_tag_commit() {
  local tag="$1"
  local output

  output="$(git ls-remote origin "refs/tags/$tag^{}" 2>/dev/null || true)"
  if [[ -z "$output" ]]; then
    output="$(git ls-remote origin "refs/tags/$tag" 2>/dev/null || true)"
  fi

  awk 'NR == 1 { print $1 }' <<<"$output"
}

ensure_remote_tag_at_head() {
  local tag="$1"
  local head_commit="$2"
  local remote_commit

  remote_commit="$(remote_tag_commit "$tag")"
  if [[ -z "$remote_commit" ]]; then
    run git push origin "refs/tags/$tag"
    remote_commit="$(remote_tag_commit "$tag")"
  fi

  if [[ -z "$remote_commit" ]]; then
    echo "Tag $tag is not present on origin after push." >&2
    exit 1
  fi

  if [[ "$remote_commit" != "$head_commit" ]]; then
    echo "Remote tag $tag points at $remote_commit, but HEAD is $head_commit." >&2
    exit 1
  fi
}

release_publish() {
  local version="$1"
  local tag="v$version"
  local tag_commit
  local head_commit

  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 is only supported for release checks, not publishing." >&2
    exit 1
  fi

  if ! git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    echo "Missing release tag $tag." >&2
    exit 1
  fi

  tag_commit="$(git rev-list -n 1 "$tag")"
  head_commit="$(git rev-parse HEAD)"
  if [[ "$tag_commit" != "$head_commit" ]]; then
    echo "Tag $tag points at $tag_commit, but HEAD is $head_commit." >&2
    exit 1
  fi

  release_check "$version"
  local package_name
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    publish_package_if_missing "$package_name" "$version"
  done
  ensure_remote_tag_at_head "$tag" "$head_commit"
}

if [[ $# -lt 1 ]]; then
  usage
fi

command="$1"
shift

cd "$ROOT_DIR"

case "$command" in
  check)
    version="$(release_version "$@")" || exit $?
    release_check "$version"
    ;;
  prepare)
    version="$(release_version "$@")" || exit $?
    release_prepare "$version"
    ;;
  notes)
    version="$(release_version "$@")" || exit $?
    release_notes "$version"
    ;;
  stage)
    if [[ $# -ne 0 ]]; then
      usage
    fi
    release_stage
    ;;
  tag)
    version="$(release_version "$@")" || exit $?
    release_tag "$version"
    ;;
  publish)
    version="$(release_version "$@")" || exit $?
    release_publish "$version"
    ;;
  github)
    version="$(release_version "$@")" || exit $?
    release_github "$version"
    ;;
  next-version)
    next_version "$@"
    ;;
  version)
    if [[ $# -ne 0 ]]; then
      usage
    fi
    manifest_version
    ;;
  -h|--help|help)
    print_usage
    ;;
  *)
    usage
    ;;
esac
