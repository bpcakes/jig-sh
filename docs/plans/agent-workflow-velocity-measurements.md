# T-03: resource and prerequisite observations

Date: 2026-09-21. Baseline: `79290f75` (T-02 complete).
Work plan: `plan_01M30F0R951Z8DAZDZMA703R4X`.
The [compact event records](agent-workflow-velocity-measurements.json) contain the
selected observations. These are controlled reproductions, not throughput benchmarks.

## Boundary and oracle

`scripts/fixtures/workflow-velocity.py` creates one disposable generic repository
per hypothesis and executes real Jig native argv actions. Each run compares one
target, two targets in one parallel execution layer, and two concurrent requests.
The checked-in actions have read-only/process effects, no dependencies, and no
source mutations; logs, build artifacts, HTTP results, and the database are outside
the repository. Jig's own run state is ignored in the fixture. The driver checks
child launches, terminal statuses, and matching Jig request outcomes. It does not
use summed child durations as wall time or pretend child lifetime excludes waits.

Cargo uses a real dependency-free crate. Its build script holds the real Cargo
target-directory lock until an explicit release file appears. For overlapping
conditions, release requires an observed **build-directory** wait, not a package
cache wait or elapsed sleep. The reported queue value is the lower bound from
that wait notice to controlled release; exact lock acquisition and pure compiler
CPU time are not observable here.

Browser ownership follows the generated
`templates/scaffolds/rust-react/frontend/vite-react/playwright.config.ts.jinja`:
two managed loopback servers, `workers: 1`, and `reuseExistingServer: false`.
The generated package pins Playwright 1.62.1 and runs `playwright test` through
`scripts/check-webapps.sh run-script APP test:e2e`. This probe invokes that same
Playwright runner directly after dependency setup; it does not measure the
dependency-readiness wrapper. Tiny HTTP servers replace application compilation,
and request-only tests replace Chromium. Each server returns its invocation's
identity so an assertion can reject another invocation's server. This is a test
oracle, not a proposed application protocol. Supported `E2E_API_PORT` and
`E2E_WEB_PORT` overrides provide the distinct-port control. The native actions are
explicit fixture configuration, not a claim that generated repos already declare
an E2E native target.

Database checks use SQLx CLI **0.9.0**, PostgreSQL **18.6**, and the generated
`CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx sqlx prepare --check
--workspace -- --workspace --all-targets` boundary. An instrumented Cargo proxy
distinguishes cheap `metadata` calls from expensive `check` launches. The fixture
owns a temporary Unix-socket PostgreSQL cluster, database, and role. Setup alone
creates/grants resources; no diagnostic repairs a database. There are no query
macros: this tests connection prerequisites and compilation launch ordering, not
schema correctness or query-level privileges.

## Observed results

Cargo 1.98.0; Jig `0.4.1-dev.21+g79290f75.dirty`. The dirty suffix reflects active
workflow metadata, not a different runtime implementation.

| Case | Isolated | Same-run overlap | Two-request overlap |
| --- | --- | --- | --- |
| Shared Cargo target | 1 child, pass; wall 2.644 s | 2 children, both pass; about 17 ms from build-lock notice to release; wall 2.639 s | 2 children, both pass; about 13 ms from build-lock notice to release; wall 2.560 s |
| Shared browser server pair, identity oracle | 1 child, pass; wall 3.711 s | 2 children; one fails; address-in-use errors; wall 3.730 s | 2 children; one fails; address-in-use errors; wall 3.851 s |
| Distinct browser port pairs | 1 child, pass; wall 3.878 s | 2 overlapping children, both pass; wall 3.735 s | 2 overlapping children, both pass; wall 3.766 s |
| Missing database | 1 SQLx child, fail; 0 compilation launches; wall 2.603 s | 2 SQLx children fail; 0 compilation launches; wall 2.599 s | 2 SQLx children fail; 0 compilation launches; wall 2.598 s |
| Database exists, role lacks CONNECT | 1 SQLx child, fail; 0 compilation launches; wall 2.597 s | 2 SQLx children fail; 0 compilation launches; wall 2.623 s | 2 SQLx children fail; 0 compilation launches; wall 2.585 s |
| CONNECT restored | 1 compilation launch, pass; wall 2.758 s | 2 compilation launches, both pass; wall 2.706 s | 2 compilation launches, both pass; wall 2.623 s |

SQLx invokes Cargo metadata before its database connection guard. Missing database
and permission-denied messages were asserted separately; a generic nonzero exit
was not accepted as proof of either failure. Its existing connection guard stops
before compilation in all six negative conditions. After the grant changes,
the real check executes successfully without any source change.

An initial browser observation with identical server bodies produced two passing
Playwright invocations **despite address-in-use errors**. That was not evidence
of correct ownership. Adding per-invocation response assertions exposed the wrong
instance/racing startup boundary. `reuseExistingServer: false` is not by itself
an atomic cross-process ownership guarantee.

## Contamination, limits, and repetition budget

- Cargo had three complete repetitions per condition. The first also observed
  global package-cache waits; the selected final run isolates the build-directory
  wait for admission decisions. Cold versus warm compilation and concurrent host
  work make absolute timings unsuitable for a speedup claim.
- Browser had at most three observations per condition: the initial same-body
  oracle (stopped after same-run), the identity-aware shared-pair run, and the
  distinct-pair control. No further live browser repetition is part of this task.
- Database had one invalid-URI attempt (`empty host`, rejected as setup failure)
  and two valid matrices. The corrected oracle requires the specific server error
  and observes zero `cargo check` calls. The initially selected PATH SQLx 0.8.6
  was not used for measurement; the existing 0.9.0 tool was selected explicitly.
- The initial Cargo attempt failed fixture config parsing before any child and
  was setup debugging, not a resource observation. Later edits clarified output
  names and used file-backed subprocess capture to avoid pipe backpressure.
- The final matrices ran concurrently, so shared Cargo package-cache contention
  remains a possible contaminant. Per-fixture build directories and PostgreSQL
  sockets were distinct; no conclusion depends on package-cache timing.
- Ports are selected from unused loopback ports and then released before server
  startup. Unrelated host bind races remain possible; a setup collision is
  inconclusive, not permission to repeatedly rerun until a preferred result.
- Chromium, application bootstrap/migrations, remote databases, grants beyond
  CONNECT, and multi-host resource coordination remain unmeasured. No fixed
  percentage improvement or query-readiness guarantee is claimed.

This delivery has exhausted its three-repetition ceiling for Cargo and the
browser control sequence. Review should inspect the code, recorded events, and
syntax/contract checks, not run more live matrices. A future executor can reproduce
the experiment under a fresh, separately bounded investigation.

## Decision for T-04

1. **Accept Cargo coordination.** Consume the existing V06 owner and typed Cargo
   identity. An opted-in competitor sharing the canonical effective target
   directory must wait before spawning Cargo; cancellation while waiting launches
   nothing, and admission must revalidate source/configuration under one deadline.
   Different targets remain concurrent. Do not build a second Cargo lock manager.
2. **Accept the narrow browser ownership conflict, not a global browser limit.**
   Any admission policy must key the explicitly owned loopback endpoint resources,
   coordinate overlapping requests as well as same-run targets, and allow distinct
   pairs to overlap. Preserve externally supplied `E2E_BASE_URL` semantics: that
   mode does not own the generated servers. Do not serialize all browsers or infer
   ownership by parsing an arbitrary shell command. Before production changes,
   T-04 must record the opt-in/versioned boundary and compatibility tests, reusing
   the resource owner/cleanup machinery rather than inventing a separate daemon.
   Unopted repositories retain current behavior; the current supported mitigation
   is explicitly distinct port pairs for independent invocations.
3. **Reject a duplicate generic SQLx connection preflight for these cases.**
   SQLx 0.9 already fails before compilation for missing databases and CONNECT
   denial, and checks again on each invocation. Preserve this guard after resource
   admission. Doctor's driver capability probe remains distinct from project
   readiness. Query/schema privileges remain the real validator's responsibility;
   no demonstrated late prerequisite failure justifies speculative grant checks,
   database creation, or treating an old source receipt as current readiness.

These are falsifiable acceptance conditions: launch counters and synchronized
events must show admission before competing child launch, independent resources
must overlap, and current SQLx guard failures must still launch zero compilation
children. Readiness never substitutes for the actual validator's success.

## Reproduction commands

From the repository root, build the edited runtime with `scripts/jig-dev --version`.
Use installed Cargo, Python 3, Node, SQLx CLI 0.9.0, and PostgreSQL 18 executables.
For browser setup only, install the template's pinned package into a disposable
tool directory with `npm install --prefix DIRECTORY --ignore-scripts --no-audit
--no-fund @playwright/test@1.62.1`; no browser download is required.

```sh
python3 scripts/fixtures/workflow-velocity.py cargo --jig target/debug/jig
python3 scripts/fixtures/workflow-velocity.py browser --jig target/debug/jig --playwright DIRECTORY/node_modules/@playwright/test
python3 scripts/fixtures/workflow-velocity.py browser --jig target/debug/jig --playwright DIRECTORY/node_modules/@playwright/test --separate-browser-ports
python3 scripts/fixtures/workflow-velocity.py database --jig target/debug/jig --sqlx SQLX_090_EXECUTABLE --pg-bin POSTGRES_18_BIN_DIRECTORY
```

Each command prints sanitized JSON and asserts its expected launch/ownership
outcome. The driver stops its owned PostgreSQL cluster and removes its disposable
fixture; remove the separately installed browser-tool directory after inspection.
Syntax checks use `node --check` for the two CJS files and `rustfmt --check` for
the fixture build script. No production scheduler, wrapper, or persistent-state
format changes are made by T-03.
