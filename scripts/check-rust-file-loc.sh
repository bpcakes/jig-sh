#!/usr/bin/env bash
set -euo pipefail

export GIT_LITERAL_PATHSPECS=1

readonly EMPTY_TREE_HASH="4b825dc642cb6eb9a060e54bf8d69288fbee4904"
readonly TARGET_HIGH=400
readonly SOFT_LIMIT_START=500
readonly SOFT_LIMIT_END=600
readonly HARD_LIMIT=800
readonly ABSOLUTE_MAX=1000

readonly rust_root_count=1
rust_roots=(crates)

usage() {
  printf '%s\n' "Usage: scripts/check-rust-file-loc.sh <default-branch> | --changed-against <ref> | --staged | --all" >&2
  printf '%s\n' "       --all checks every tracked Rust file without a legacy baseline." >&2
  exit 2
}

operational_error() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 2
}

if [[ "$#" -eq 0 && -n "${JIG_DEFAULT_BRANCH:-}" ]]; then
  set -- "$JIG_DEFAULT_BRANCH"
fi

mode=""
comparison_ref=""
case "${1:-}" in
  --changed-against)
    [[ "$#" -eq 2 && -n "$2" ]] || usage
    mode="changed"
    comparison_ref="$2"
    ;;
  --staged)
    [[ "$#" -eq 1 ]] || usage
    mode="staged"
    ;;
  --all)
    [[ "$#" -eq 1 ]] || usage
    mode="all"
    ;;
  --* | "")
    usage
    ;;
  *)
    [[ "$#" -eq 1 ]] || usage
    mode="default-branch"
    comparison_ref="$1"
    ;;
esac

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || operational_error "not inside a Git worktree"
cd "$repo_root"

if [[ "$mode" == "default-branch" ]]; then
  default_branch="$comparison_ref"
  if ! git check-ref-format --branch "$default_branch" >/dev/null 2>&1; then
    printf 'Invalid default branch name: %s\n' "$default_branch" >&2
    exit 2
  fi
  remote_ref="refs/remotes/origin/$default_branch"
  if remote_oid="$(git rev-parse --verify "$remote_ref^{commit}" 2>/dev/null)"; then
    comparison_ref="$(git merge-base HEAD "$remote_oid")" \
      || operational_error "cannot find a merge base between HEAD and $remote_ref"
  elif comparison_ref="$(git rev-parse --verify 'HEAD^{commit}^' 2>/dev/null)"; then
    :
  else
    comparison_ref="$EMPTY_TREE_HASH"
  fi
  printf 'Using Rust LOC base ref: %s\n' "$comparison_ref"
  mode="changed"
fi

if [[ "$mode" == "changed" ]]; then
  [[ "$comparison_ref" != -* ]] || operational_error "comparison refs beginning with '-' are unsupported"
  if [[ "$comparison_ref" != "$EMPTY_TREE_HASH" ]]; then
    unresolved_ref="$comparison_ref"
    comparison_ref="$(git rev-parse --verify "$unresolved_ref^{tree}" 2>/dev/null)" \
      || operational_error "comparison ref does not resolve to a tree: $unresolved_ref"
  fi
fi

if [[ "$mode" == "staged" ]]; then
  if previous_ref="$(git rev-parse --verify 'HEAD^{commit}' 2>/dev/null)"; then
    :
  else
    previous_ref="$EMPTY_TREE_HASH"
  fi
elif [[ "$mode" == "changed" ]]; then
  previous_ref="$comparison_ref"
else
  previous_ref="$EMPTY_TREE_HASH"
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/jig-rust-file-loc.XXXXXX")" \
  || operational_error "cannot create a temporary directory"
cleanup() {
  rm -rf -- "$work_dir"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

git_with_roots() {
  if [[ "$rust_root_count" -eq 0 ]]; then
    git "$@" --
  else
    git "$@" -- "${rust_roots[@]}"
  fi
}

if [[ "$rust_root_count" -ne 0 ]]; then
  root_probe="$work_dir/root-probe"
  configured_root_matches=false
  for configured_root in "${rust_roots[@]}"; do
    if ! git ls-files -z -- "$configured_root" >"$root_probe"; then
      operational_error "Git root validation failed"
    fi
    if [[ -s "$root_probe" ]]; then
      configured_root_matches=true
      break
    fi
    if [[ "$previous_ref" != "$EMPTY_TREE_HASH" ]]; then
      if ! git ls-tree -r -z --name-only "$previous_ref" -- "$configured_root" >"$root_probe"; then
        operational_error "Git baseline root validation failed"
      fi
      if [[ -s "$root_probe" ]]; then
        configured_root_matches=true
        break
      fi
    fi
  done
  if [[ "$configured_root_matches" != true ]]; then
    operational_error "configured Rust roots match no tracked files"
  fi
fi

write_raw_changes() {
  case "$mode" in
    all)
      git_with_roots ls-files -z
      ;;
    staged)
      git_with_roots diff --cached --no-ext-diff --find-renames --name-status --diff-filter=ACMRT -z
      ;;
    changed)
      git_with_roots diff --no-ext-diff --find-renames --name-status --diff-filter=ACMRT -z \
        "$comparison_ref" HEAD
      ;;
  esac
}

raw_change_file="$work_dir/raw-changes"
candidate_file="$work_dir/candidates"
if ! write_raw_changes >"$raw_change_file"; then
  operational_error "Git candidate discovery failed"
fi

if [[ "$mode" == "all" ]]; then
  while IFS= read -r -d '' file; do
    printf '%s\0%s\0' "$file" "$file"
  done <"$raw_change_file" >"$candidate_file"
else
  while IFS= read -r -d '' change_status; do
    case "$change_status" in
      A* | M* | T*)
        IFS= read -r -d '' file \
          || operational_error "Git returned an incomplete change record"
        printf '%s\0%s\0' "$file" "$file"
        ;;
      R*)
        IFS= read -r -d '' previous_path \
          || operational_error "Git returned an incomplete rename record"
        IFS= read -r -d '' file \
          || operational_error "Git returned an incomplete rename record"
        printf '%s\0%s\0' "$previous_path" "$file"
        ;;
      C*)
        IFS= read -r -d '' copied_from \
          || operational_error "Git returned an incomplete copy record"
        IFS= read -r -d '' file \
          || operational_error "Git returned an incomplete copy record"
        # Copies are new policy candidates and never inherit the source baseline.
        printf '%s\0%s\0' "$file" "$file"
        ;;
      *)
        operational_error "Git returned an unexpected change status: $change_status"
        ;;
    esac
  done <"$raw_change_file" >"$candidate_file"
fi

line_count() {
  LC_ALL=C awk 'END { print NR }' "$1"
}

has_exception_marker() {
  LC_ALL=C awk '
    NR <= 40 && (index($0, "agentic-loc-exception:") || index($0, "@generated")) {
      found = 1
      exit
    }
    END { exit found ? 0 : 1 }
  ' "$1"
}

display_path() {
  local value="$1"
  local LC_ALL=C
  local character code index
  for ((index = 0; index < ${#value}; index++)); do
    character="${value:index:1}"
    case "$character" in
      [[:print:]])
        if [[ "$character" == \\ ]]; then
          printf '\\\\'
        else
          printf '%s' "$character"
        fi
        ;;
      $'\n') printf '\\n' ;;
      $'\r') printf '\\r' ;;
      $'\t') printf '\\t' ;;
      *)
        printf -v code '%d' "'$character"
        printf '\\x%02x' "$code"
        ;;
    esac
  done
}

error_count=0
while IFS= read -r -d '' previous_path; do
  IFS= read -r -d '' file \
    || operational_error "normalized candidate stream is incomplete"
  case "$file" in
    *.rs) ;;
    *) continue ;;
  esac

  current_file="$work_dir/current"
  if [[ "$mode" == "staged" ]]; then
    if ! git show ":$file" >"$current_file"; then
      operational_error "cannot read staged Rust source: $(display_path "$file")"
    fi
  else
    [[ -f "$repo_root/$file" ]] || continue
    if ! cat "$repo_root/$file" >"$current_file"; then
      operational_error "cannot read Rust source: $(display_path "$file")"
    fi
  fi

  current_count="$(line_count "$current_file")"
  previous_count=0
  if [[ "$current_count" -gt "$HARD_LIMIT" && "$previous_ref" != "$EMPTY_TREE_HASH" ]]; then
    previous_file="$work_dir/previous"
    : >"$previous_file"
    previous_blob_path="$file"
    previous_blob_found=false
    if git cat-file -e "$previous_ref:$previous_blob_path" 2>/dev/null; then
      previous_blob_found=true
    elif [[ "$previous_path" != "$file" ]] \
      && git cat-file -e "$previous_ref:$previous_path" 2>/dev/null; then
      previous_blob_path="$previous_path"
      previous_blob_found=true
    fi
    if [[ "$previous_blob_found" == true ]]; then
      if ! git show "$previous_ref:$previous_blob_path" >"$previous_file"; then
        operational_error "cannot read previous Rust source: $(display_path "$previous_blob_path")"
      fi
      previous_count="$(line_count "$previous_file")"
    fi
  fi

  display_file="$(display_path "$file")"
  if [[ "$current_count" -gt "$ABSOLUTE_MAX" ]]; then
    if [[ "$current_count" -le "$previous_count" && "$previous_count" -gt "$ABSOLUTE_MAX" ]]; then
      printf 'WARNING: %s remains above the absolute max at %s LOC but did not increase.\n' "$display_file" "$current_count"
    else
      printf 'ERROR: %s is %s LOC, above the absolute max of %s.\n' "$display_file" "$current_count" "$ABSOLUTE_MAX" >&2
      error_count=$((error_count + 1))
    fi
  elif [[ "$current_count" -gt "$HARD_LIMIT" ]]; then
    if [[ "$current_count" -le "$previous_count" && "$previous_count" -gt "$HARD_LIMIT" ]]; then
      printf 'WARNING: %s remains above the hard limit at %s LOC but did not increase.\n' "$display_file" "$current_count"
    elif has_exception_marker "$current_file"; then
      printf 'WARNING: %s is %s LOC and uses an explicit exception annotation.\n' "$display_file" "$current_count"
    else
      printf 'ERROR: %s is %s LOC, above the hard limit of %s.\n' "$display_file" "$current_count" "$HARD_LIMIT" >&2
      error_count=$((error_count + 1))
    fi
  elif [[ "$current_count" -gt "$SOFT_LIMIT_END" ]]; then
    printf 'WARNING: %s is %s LOC and is approaching the hard limit.\n' "$display_file" "$current_count"
  elif [[ "$current_count" -gt "$SOFT_LIMIT_START" ]]; then
    printf 'WARNING: %s is %s LOC and is above the soft limit.\n' "$display_file" "$current_count"
  elif [[ "$current_count" -gt "$TARGET_HIGH" ]]; then
    printf 'INFO: %s is %s LOC and is approaching the soft limit.\n' "$display_file" "$current_count"
  fi
done <"$candidate_file"

if [[ "$error_count" -ne 0 ]]; then
  printf 'Rust LOC policy failed with %s error(s).\n' "$error_count" >&2
  exit 1
fi

printf '%s\n' "Rust LOC policy passed."
