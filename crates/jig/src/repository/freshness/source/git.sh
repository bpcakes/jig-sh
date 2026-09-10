# Static protocol, run by sanitized Bash inside one owned process tree.
# Each block has a NUL-terminated name, NUL-terminated records, and one empty
# record terminator. Git paths cannot contain NUL. Scalar Git outputs receive
# their record terminator here. Failure or diagnostics invalidates the batch.
set -e
set -o pipefail
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
git rev-parse --verify HEAD
printf '\0\0tree\0'
git --no-replace-objects --literal-pathspecs ls-tree -r -z HEAD -- "${scope[@]}"
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
