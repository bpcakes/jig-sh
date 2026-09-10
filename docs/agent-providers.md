# Agent providers

Claude and Codex implement the internal `AgentProvider` interface in
`crates/jig/src/agent_provider.rs`. Their CLI parsers dispatch homes and launch
requests to `crates/jig/src/cli/agent_run.rs`. The common workflow checks usage
support and JSON/launch combinations, supervises inspection, builds the picker,
revalidates the selected configuration, emits dry runs, and executes prepared
commands through the transparent child launcher.

## Provider responsibilities

Each provider supplies metadata (command/display names, executable and override
variable, usage support, plain-list inspection policy, and primary subscription
bucket), a provider-owned `Home` type, and implementations of these operations:

- `resolve`: interpret an explicit CLI home argument.
- `discover`: return choices with display metadata and an optional inspection source.
- `revalidate`: check the chosen configuration after an interactive wait.
- `prepare`: build exact native argv/environment and the existing dry-run JSON report,
  without launching, reading credentials, or inspecting usage.
- `homes_report`: produce the provider's public report with cooperative cancellation
  and optional progress notifications.

Providers own report schemas and inspection scheduling because existing contracts
differ. Codex plain listing inspects accounts through its app-server; Claude plain
listing only discovers directories, and `--usage` opts into credential/network
access. Claude's picker permits bounded Keychain prompting; its headless report
never does. These policies remain in provider adapters rather than the common CLI.

`SessionProvider` is an optional extension implemented by Codex for session-home
lookup. Other providers do not need a dummy implementation. Provider-specific
resume argument construction and session validation remain with the Codex CLI.

## Identity and lifetime rules

A discovery choice stores both a display path and the actual provider-owned
selection. The picker returns the original index, so two modes sharing one path
remain distinct. Never reconstruct a launch identity from JSON, sanitized text,
or a display path. Claude uses this to distinguish native default (unset
CLAUDE_CONFIG_DIR, available before directory creation) from an explicit override.

Inspection sources emit secret-free normalized account/usage updates indexed by
original discovery order. They must poll cancellation and retire all owned child
processes before returning. The picker owns the worker and cancels and joins it
before terminal restoration. A provider without inspection returns `None` and
receives a static configuration picker.

## Usage interpretation

Provider metadata identifies the main subscription bucket. The picker and CLI
usage formatter receive that identity explicitly, so an unfamiliar provider ID
can receive the same quota labels and recommendations without changing a list of
known names. Other buckets retain generic duration labels. With no primary bucket,
usage may still be displayed but the picker does not recommend a configuration
based on subscription headroom. Missing/invalid usage remains unknown.

The crate package name `jig-codex-tui` and its old public selection functions remain
for compatibility. New integrations use `select_provider_with_cancellation`, which
accepts provider presentation, optional inspection, and explicit quota semantics.
No new provider-dependent JSON fields are required.

## Adding another provider

Implement the provider interface in its own module and keep its configuration,
authentication, and usage transport there. Register its CLI command and output
adapter, then route homes/launch to the existing shared functions. Update root
command metadata, launcher classification/templates, documentation, and applicable
verification inputs together. Actual Cursor support requires verifying Cursor's
configuration and usage capabilities; this refactor does not assume those details
or add a Cursor command.

The test-only ExampleProvider exercises shared preflight, report dispatch,
selection, cancellation, and dry-run paths. It uses a distinct configuration enum
and a nonexistent executable, proving that dry runs do not spawn. Picker tests
also cover a primary subscription bucket unknown to Claude/Codex and providers
without inspection or recommendation support.

Validate changes with the affected provider/picker tests, the Claude and Codex
launcher integration tests, and the repository test check using a newly built
`JIG_DEV_BIN=target/debug/jig`. Preserve the existing JSON schemas, forwarded native
arguments, invocation directory, child exit status, and signal cleanup behavior.
