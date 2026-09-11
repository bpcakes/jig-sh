# Distinguish an unborn symbolic branch from an invalid/dangling HEAD.
# Keep diagnostics visible to the strict owned Git observer.
set -e
if jig_head=$(git rev-parse --verify --quiet HEAD); then
    printf 'commit:%s\n' "$jig_head"
else
    jig_result=$?
    [ "$jig_result" -eq 1 ] || exit "$jig_result"
    jig_branch=$(git symbolic-ref -q HEAD)
    jig_result=0
    git show-ref --verify --quiet "$jig_branch" || jig_result=$?
    [ "$jig_result" -eq 1 ] || exit 1
    printf 'unborn\n'
fi
if jig_branch=$(git symbolic-ref -q HEAD); then
    printf 'branch:%s\n' "$jig_branch"
else
    jig_result=$?
    [ "$jig_result" -eq 1 ] || exit "$jig_result"
    printf 'detached\n'
fi
