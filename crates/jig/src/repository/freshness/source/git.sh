# Static protocol, run by sanitized Bash inside one owned process tree.
# Each block has a NUL-terminated name, NUL-terminated records, and one empty
# record terminator. Git paths cannot contain NUL. Scalar Git outputs receive
# their record terminator here. Failure or diagnostics invalidates the batch.
set -e
set -o pipefail
allow_unborn="$1"
shift
scope_count="$1"
shift
scope=()
for ((scope_index=0; scope_index<scope_count; scope_index++)); do
    scope+=("$1")
    shift
done
printf 'format\0'
git rev-parse --show-object-format
printf '\0\0head\0'
if jig_head=$(git rev-parse --verify --quiet HEAD); then
    printf '%s\n' "$jig_head"
else
    jig_result=$?
    [ "$allow_unborn" -eq 1 ] && [ "$jig_result" -eq 1 ] || exit 1
    jig_branch=$(git symbolic-ref -q HEAD)
    jig_result=0
    git show-ref --verify --quiet "$jig_branch" || jig_result=$?
    [ "$jig_result" -eq 1 ] || exit 1
    jig_head=unborn
    printf 'unborn\n'
fi
printf '\0\0tree\0'
if [ "$jig_head" != unborn ]; then
    git --no-replace-objects --literal-pathspecs ls-tree -r -z HEAD -- "${scope[@]}"
fi
printf '\0index\0'
git --no-replace-objects --literal-pathspecs ls-files --stage -t -z -- "${scope[@]}"
printf '\0ita_visible\0'
git --no-replace-objects --literal-pathspecs diff --cached --relative --name-only -z --no-renames --no-ext-diff --no-textconv --ita-visible-in-index -- "${scope[@]}"
printf '\0ita_invisible\0'
git --no-replace-objects --literal-pathspecs diff --cached --relative --name-only -z --no-renames --no-ext-diff --no-textconv --ita-invisible-in-index -- "${scope[@]}"
printf '\0ignored\0'
git --literal-pathspecs ls-files --others --ignored --exclude-standard --directory -z -- "${scope[@]}"
printf '\0declarations\0'
if [ "$#" -gt 0 ]; then
    # Arguments are data, including quotes, spaces, and shell metacharacters.
    # A pipeline error is accepted only for Git's documented no-match status.
    result=0
    printf '%s\0' "$@" | git check-ignore --stdin -z || result=$?
    [ "$result" -eq 0 ] || [ "$result" -eq 1 ] || exit "$result"
fi
printf '\0'
