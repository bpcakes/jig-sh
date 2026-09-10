Reproduce concurrent initial no-follow journal opens failing with ENOENT. Use exclusive creation with an AlreadyExists fallback to no-follow append opening. Verify shared inode, preserved contents, symlink rejection, lock deadlines and journal replacement; run the failing no-default-features dispatcher test, stress the creation regression, and complete repository checks.

## Diagnosis and implementation

Baseline 4104ad9d (PR 27). The preceding macOS cleanup correction passed its formerly failing CI job. The remaining failure was concurrent_dispatchers_execute_one_due_occurrence_once in macOS no-default-features: first receipt creation reported ENOENT. One hundred dispatcher repetitions passed locally, but two simultaneous no-follow creating opens reproduced ENOENT on iteration zero.

Use create_new to elect the creator and fall back on AlreadyExists to a non-creating, no-follow open. Preserve append, nonblocking open, regular-file validation, both writer locks, and post-lock identity checks. Do not retry journal appends. The regression checks 100 simultaneous first creations, equal inode identities, and preservation of both writes plus a later append. All nine journal tests passed, including symlink rejection and bounded locking. Twenty stress repetitions passed (2,000 concurrent first-creation cases).

Beads: jig-sh-vnb5. Complete the focused no-default-features dispatcher regression, then the repository test check and required work gates before publishing. Final gate results are retained in append-only receipts.
