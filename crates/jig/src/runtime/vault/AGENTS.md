# Vault runtime guide

## Purpose

Own vault scope, passphrase capture, raw-output dispatch, lifecycle, and the CLI-owned Vault TUI adapter. These rules also cover the sibling [vault.rs](../vault.rs), [vault_env.rs](../vault_env.rs), [vault_withholding.rs](../vault_withholding.rs), vault dispatch in [runtime.rs](../../runtime.rs), CLI scope application in [vault/run.rs](../../cli/vault/run.rs), and the CLI startup boundary in [vault_environment.rs](../../cli/run/vault_environment.rs); they do not apply to unrelated runtime modules.

## Key entrypoints

- [vault.rs](../vault.rs): raw and structured command dispatch.
- [lifecycle.rs](lifecycle.rs): passphrase and backup lifecycle.
- [scope.rs](scope.rs): repo-scope namespace derivation and cutover guards; [scope/worktree.rs](scope/worktree.rs) proves linked-worktree sharing.
- [tui.rs](tui.rs): fixed-scope backend adapter.
- [vault_withholding.rs](../vault_withholding.rs): startup passphrase withholding and the withheld-passphrase note.

## Edit here for X

- Change scope, environment capture, and core calls here.
- Change repo-scope namespace derivation or linked-worktree sharing in [scope.rs](scope.rs) and [scope/worktree.rs](scope/worktree.rs).
- Change storage/broker internals in [jig-vault](../../../../jig-vault/AGENTS.md).
- Change terminal forms/navigation in [jig-vault-tui](../../../../jig-vault-tui/AGENTS.md).

## Invariants

- Vault references stay project-relative as `jig://ITEM/FIELD`; repository scope (which a verified linked Git worktree shares with its main checkout), `--global`, or `--home` selects the vault and a reference must never override that selection.
- Repo scope hashes the canonical repo root with the unchanged v2 recipe. A verified linked Git worktree hashes the literal corresponding path in its main checkout instead and must never canonicalize it, so only a writer of a repository's own `.git` can join that repository's namespace. Prove linkage only from bounded no-follow regular Git files, never from `GIT_*` environment or `git` output: a current-user regular `.git` pointer, an admin directory directly in `<common>/worktrees`, a `commondir` resolving to that common directory, and a `gitdir` back-link naming the checkout's literal `.git`. A failed claim fails closed. Submodules, separate-git-dir or bare commons, `core.bare` or `core.worktree` found by the best-effort common-config scan, and independent repositories nested in the main checkout keep checkout scope. Existing data wins: when the checkout's own namespace already holds `vault.json`, keep resolving it for verified and unverified worktree claims alike (and on Git metadata inspection errors, without guidance), report `vault_worktree_local` with value-free, absolute-path guidance (operator-routed migration steps when verified, repair steps when unverified), and never strand, shadow, or refuse it; guidance construction is infallible, and sharing starts only after the operator moves the vault. Recovery text that passes `--home` or moves, renames, or removes vault directories follows the shared `VAULT_STORAGE_OPERATOR_STEP` routing in [scope.rs](scope.rs), because the generated Vault rules make both operator-only. Scope derivation is read-only.
- A discovered repository configuration that fails to load blocks repo and `--global` selection; never fall back to the user-level vault or parse `[vault]` leniently. The error keeps the load diagnostic and adds value-free guidance that never suggests deleting configuration or writing harness files and routes `--home` to the operator. For direct `jig vault` invocations only `--home` skips repository context; the generated launcher always validates the repository first. An invalid `JIG_REPO_ROOT` override is ignored with a warning and discovery proceeds, as for other contextless commands.
- Validate vault raw input, repository scope, `vault run` mappings, import sources/destinations, and lifecycle paths before passphrase capture. Scope preflight resolves metadata only and must not open or create vault storage. Revealed values and transparent child output must bypass structured emitters, JSON and run or loop records; errors and recovery commands must remain value-free.
- Passphrase-unavailable diagnostics, including init/adopt bootstrap, append the shared `VAULT_PASSPHRASE_OPERATOR_GUIDANCE` from [runtime.rs](../../runtime.rs), through `vault_passphrase_operator_guidance` where a missing passphrase may have been withheld: route passphrase entry to the operator's terminal or operator-managed automation, and never tell callers to choose, request, print, store, export, or set a passphrase themselves.
- Keep `vault exec` as transparent inherited-stdin/environment streaming with exact child status, and keep the compatible `vault run` broker constrained, buffered, capped, timed, and process-tree-owned. Successful vault capture and every spawned resolver/child must strip both reserved passphrase variables.
- Only `vault` commands and vault-requesting `init`/`adopt --write` keep the reserved passphrase variables past CLI startup. [vault_environment.rs](../../cli/run/vault_environment.rs) withholds both from every other command right after parsing, through an exhaustive `CommandKind` match, and sets the non-secret `JIG_VAULT_PASSPHRASE_WITHHELD=1` marker only when it removed one; bootstrap vault preparation clears both before rendering. Code before a capture point (launcher validation including contract validation, repository-scope application, and vault preflight for `vault` commands; init destination and interaction preflight) must not start child processes. The withheld note is value-free, ignored while a passphrase is present, and directs the operator to run the task directly through `vault exec` outside recording runners. Never recommend wrapping `scripts/jig check` or `scripts/jig run`: those commands can persist plaintext failure output before an outer wrapper redacts it. Wrapping `scripts/jig dev` or an agent launch remains unsupported. Withholding is process hygiene, not isolation: same-user processes can still read the Jig process's original environment block, so never describe it as hiding the passphrase from the invoking session.
- Backup restore must use the static absent-target path; it may prepare missing private parent directories, but must never resolve or create the selected vault home before restore preflight and installation.
- The Vault TUI fixes one resolved scope for its lifetime, retains only a process-local credential in the CLI-owned backend, and must join its sole action worker before lock or terminal restoration. TUI action results and ordinary Ratatui frames remain metadata-only; private export and transient Peek consume plaintext only in their immediate hardened/terminal-safe sinks and never return it to the model.

## Common commands

Run from the repository root:

- `scripts/jig-dev check source-vault-test`
