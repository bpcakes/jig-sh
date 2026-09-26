# Cargo resource coordination

An action can opt into cross-process Cargo coordination with a strict resource
declaration. Jig resolves the effective artifact directories before starting the
action, waits for exclusive use, then revalidates source, configuration and the
resource identity. Actions without declarations keep their existing behavior.

Add `resources` to an existing read-only process check in authored configuration,
then regenerate its resolved manifest through the normal update workflow:

```toml
[[repository.actions]]
target = { component = "api", action = "test-focused" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "rust_nextest_v1", configuration = { workspace_manifest = "Cargo.toml", focused = true } }
arguments = { focus = { type = "rust_focus_v1" } }
resources = [{ kind = "cargo_v1", workspace_manifest = "Cargo.toml" }]
```

The referenced component must already exist. This is a runtime capability, not a
promise made by the contract version alone: older runtimes reject the unknown
field or variant before executing it. Do not add the field to a repository still
using an unsupported pinned runtime. No release pin changes automatically.

Upgrade the selected runtime before opting in. To stop acquiring claims, remove
the declarations and regenerate configuration; retain historical receipts and
runs. Disabling the policy does not make new `reused_from` run records readable by
older strict readers, so keep a capable runtime when rolling configuration back.

## Declaration and identity

A `cargo_v1` declaration carries `workspace_manifest`, optional
`working_directory`, and the existing structured Cargo `context`. Paths are
repository-relative. The working directory describes the actual Cargo invocation,
which can differ from a wrapper process's initial directory. For a typed Rust runner, manifest, configured context and repository
root directory must agree with the runner; prepared feature selections remain
authoritative. At most eight distinct declarations are accepted.

Command, shell and argv runners may declare their Cargo context explicitly. Jig
does not parse shell text, guess whether a wrapper changes directory, or discover
arbitrary command-line overrides. The declaration must describe the actual Cargo
invocation, including any directory change performed by a wrapper. Arbitrary
internal artifact-path overrides cannot be inferred. Native, mutating and external actions cannot use
this first policy.

Resolution runs bounded `cargo metadata --no-deps --locked --offline` with the
declared directory and process environment. It does not compile or update the
lockfile. Both Cargo's target and build directories participate, deduplicated
when they coincide. Features, profiles and targets do not split a shared artifact
directory into separate claims. Existing symlinks are resolved, including the
existing ancestor of a not-yet-created directory. Existing directories also use
their filesystem device/inode identity, covering aliases that canonical path
spelling alone cannot establish. Explicit shared existing target directories
therefore coordinate across repositories and worktrees.

If an artifact directory does not yet exist, its physical identity is unproved.
Jig reports **partial coordination** and serializes opted-in Cargo actions in
that repository. It retains known canonical-path claims as a best-effort bridge
through creation, but does not promise cross-worktree exclusion for unproved
aliases such as case-folded nonexistent suffixes. Metadata failures, missing
artifact-directory fields and inaccessible directory authority also take the
conservative repository fallback. Post-spawn metadata supervision failures
(including unconfirmed cleanup or incomplete capture) instead stop admission;
they never authorize a target under partial coordination.
A shared repository guard makes known and
partial same-repository claims conflict. If a directory is created or replaced
while waiting and the resolved claims change, Jig blocks and requests replanning;
it does not silently proceed with obsolete claims.

## Waiting, evidence and cancellation

The target's one monotonic timeout starts before resource resolution and
acquisition. Waiting, post-acquisition checks and child execution all consume
that budget. Existing repository execution-lease waiting remains outside it.
An expired or canceled waiter does not start its target or unlock another
request's claim. Source or resource-authority changes while waiting require
replanning.

After an actual wait, ordinary `work check` may reuse the latest equivalent
passing receipt under the existing exact-invocation and original-proof policy.
It retains the original receipt, run and plan IDs in `reused_from`; the waiting
run does not fabricate a child start, exit status or new target receipt. Failed,
narrower, stale or unprovable evidence does not suppress execution. Direct checks,
direct runs and explicitly forced gates still execute. Targets with scheduled
consumers execute conservatively so their dependency proof retains its current
meaning. Resource sharing never grants cross-worktree receipt sharing.

Legacy tool aliases for opted-in actions enter the same prepared-target executor,
including declared read-only prerequisites. The requested alias and literal
arguments remain attached to the original target receipt. An alias does not
authorize effectful prerequisites: those require an explicitly approved canonical
`jig run` request. Aliases without resource declarations remain unchanged.

Resource claims are scheduling constraints, not `depends_on` edges. A Clippy
failure cannot become a prerequisite for tests merely because both use Cargo.
Within a read-only dependency layer, Jig admits batches of available targets.
Targets with distinct resources can execute concurrently in the same request,
alongside ordinary checks. Conflicting targets wait for a later batch. Admission
never waits for another claim while holding an admitted batch's claims: it runs
the available batch, checks its shared source postcondition and publishes its
results before trying pending targets again. Independent requests use the same
resource ownership rules. Fail-fast execution remains sequential.

A coordinated target retains its original timeout while pending and while its
batch finishes validation. Waiting for a batch peer does not grant a new budget.
A source mutation during a batch invalidates its otherwise successful results;
an earlier-finishing sibling cannot publish a passing receipt ahead of that
shared check.

In read-only plans that combine parallel work with dependency chains, ordinary
checks run outside the resource batch. Their validated results can release
ordinary dependents while a Cargo sibling is still running. There is at most
one active resource batch. Only members admitted to its current wave reserve
slots against the shared eight-target limit, until that wave releases its
claims. Resource waiters leave capacity available for ordinary checks and
their dependents. Resource members continue to use the shared validation and
publication rules above.

If a batch exhausts its source observation budget, Jig verifies source
independently before cancelling unrelated checks. Cancellation skips or
interrupts this independent observation; result publication still completes
before claims are released.

## Ownership and recovery

Claims live in a private, owner-checked per-user namespace under the fixed system
temporary directory, outside `.agent`. Caller `TMPDIR` does not split the
namespace. Advisory ownership, not file existence or a recorded PID, determines
whether a resource is held. Lock names are opaque hashes; computed absolute
resource paths and raw Cargo metadata are not written to public evidence.

Jig holds a claim through child cleanup, source postcondition, receipt publication
and durable target-result publication. Its target child deliberately inherits
the claim, while unrelated subprocesses do not. If Jig is killed, a surviving
child retains ownership until its descriptors close; an exited owner with no
surviving holder leaves no permanent live claim. Commands that deliberately
close inherited descriptors cannot provide this lifetime guarantee.

Do not delete lock files to resolve contention: that can split ownership. Let the
active owner finish, cancel your waiter if needed, then retry through normal
planning. There is no daemon, live output fan-out, distributed lock or automatic
CPU tuning.
