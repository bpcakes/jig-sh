# Opt-in generated Playwright endpoint coordination

Current source supports `resources = [{ kind = "playwright_servers_v1" }]` on an
authored read-only process check. Older strict runtimes reject this tag; upgrade
the runtime before opting in. Existing actions, generated CI and package scripts
are not automatically changed. Removing the declaration restores prior admission
behavior without modifying historical receipts.

The declaration attests that the runner follows Jig's generated Playwright
environment contract. Invoke the existing public checker, for example:

```toml
[[repository.actions]]
target = { component = "web", action = "e2e" }
intent = "check"
effects = ["read_only", "process"]
resources = [{ kind = "playwright_servers_v1" }]
runner = { kind = "argv", program = "scripts/check-webapps.sh", args = ["run-script", "frontend", "test:e2e"] }
```

Use the actual declared component and configured app directory. The policy is
not permission to mutate source or external systems: retain truthful effects,
ordinary approvals, and appropriate test isolation. Arbitrary scripts or custom
Playwright configurations are not inferred. An inner wrapper must not silently
change the endpoint environment after admission; custom ownership needs its own
supported policy, not an inaccurate declaration.

## What is owned

`E2E_WEB_PORT` and `E2E_API_PORT` default to 4173 and 4174. The effective runner
environment is interpreted with the generated configuration's JavaScript trim,
numeric conversion and integer rules. Ports must be distinct integers from 1
through 65535, including when external URL mode is used. A bounded Node probe
performs this parsing without loading application code. Probe-only Node startup
options/module paths are removed; this does not rewrite the validator's
environment. Missing Node or invalid/unknown authority blocks the target.

A nonempty trimmed `E2E_BASE_URL` means the generated servers are not owned, so
the target claims no endpoints. Otherwise it exclusively claims each of the two
`127.0.0.1:port` endpoints. The role of a port is irrelevant: swapped and partially
overlapping pairs conflict. Distinct pairs remain concurrent, both within one
run and across requests/repositories. URL values are not resource keys or probe
diagnostics. This is advisory coordination among opted-in Jig requests, not a
port reservation against unrelated programs or a distributed lock.

The existing V06 resource owner handles private machine-local claims, one target
deadline, cancellation, source/configuration revalidation, owned-process cleanup
and publication. Waiting launches no validator. No global browser limit, new
daemon, shell-command inference or second Cargo coordinator is introduced.
Browser-only declarations do not run Cargo metadata or take a repository-wide
Cargo fallback claim. Backend compilation inside Playwright is not separately
coordinated by this endpoint policy. An independently authored Cargo declaration
can share the existing Cargo coordinator where that broader exclusion is wanted.

## Readiness remains current execution behavior

Resource ownership is scheduling, not proof that a database or server is ready.
Browser targets do not skip their actual validator using evidence produced while
they waited. Source/configuration are revalidated after waiting. The ordinary
work-check source-reuse policy outside resource admission is unchanged; use a
direct check or forced native gate when current live validation is required.

SQLx 0.9 already checks database existence and CONNECT access on each actual
invocation before compilation. T-03's bounded live measurements confirmed that
boundary; T-04 preserves the existing online `sqlx prepare --check` invocation
after Cargo admission. There is no duplicate generic connection probe, automatic
database creation, grant modification, or claim about unmeasured query/schema
privileges. Readiness never replaces the real validator's success.

Continue entering generated package scripts through
`scripts/check-webapps.sh run-script APP test:e2e`; this retains explicit E2E
environment and the existing ambient npm-routing protections. Authored resource
declarations and their complete runner survive update/readoption.
