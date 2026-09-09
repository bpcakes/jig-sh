# Scheduled Codex Tasks

Jig can run a repository-owned prompt through unattended `codex exec` on a
five-field cron schedule. Jig owns schedule evaluation, occurrence identity,
leases, receipts, retained-worktree evidence, and at-most-once execution after a
worker starts. An external scheduler must invoke `scripts/jig loop dispatch`;
Jig does not install or run a resident service.

## Configure a task

Keep the prompt in the repository so configuration and instructions change
together:

```toml
[loop]
lease_ttl_seconds = 3600

[[loop.workflows]]
id = "daily-audit"
kind = "codex_task"
schedule = "0 20 * * *"
timezone = "Europe/Prague"
prompt_file = ".agent/tasks/daily-audit.md"
codex_home = "codex"
sandbox = "workspace-write"
checkout = "repo"
```

Use an IANA timezone such as `Europe/Prague` when the task should follow local
daylight-saving changes. `codex_home = "codex"` selects `~/.codex`, while
`codex_home = "work"` selects `~/.codex-work`; a configured home must exist.
See [Loop configuration](configuration.md#loop-shape) for the full schema and
schedule semantics.

## Choose the checkout deliberately

| Checkout | Use it when | Result handling |
| --- | --- | --- |
| `worktree` | Results should remain isolated from the selected checkout. This is the default. | Jig removes a clean, unchanged worktree. Any file change or local commit causes Jig to retain the detached worktree for inspection; Jig does not merge it into the main checkout. |
| `repo` | The task must update the selected checkout, such as recording Beads issues. | The checkout must be clean before the worker starts, apart from Jig's receipt journal. The prompt must leave it clean, normally by committing the explicitly authorized files. A dirty or unverifiable result requires attention and blocks later occurrences. |

For a mutating repo-mode task, state its write and commit authority narrowly in
the prompt. For example:

```markdown
Create only deduplicated, evidence-backed bug records with `br`. Run
`br sync --flush-only`, confirm that only `.beads/` changed, and commit those
changes. Do not change product code, push, publish, or deploy. If there are no
new findings, create nothing and do not make an empty commit.
```

Do not run receipt-producing Jig commands from inside a repo-mode task. For
example, a nested `scripts/jig check ...` appends another receipt while the
worker is active. Repo-mode completion accepts only the scheduled worker's exact
receipt append; another append makes provenance ambiguous and the occurrence
requires attention. Use direct, focused test commands inside the prompt, or use
an isolated worktree task when its changes do not need to land in the selected
checkout.

## Install a dispatcher

Run `scripts/jig loop dispatch` once per minute. The command executes only due
occurrences, coalesces missed schedule times, and uses leases to defer overlap.
Use absolute paths and ensure the scheduler's `PATH` contains `codex` and any
tools used by the prompt.

For cron:

```sh
mkdir -p "$HOME/.local/state"
```

```cron
* * * * * cd /absolute/path/to/repository && ./scripts/jig loop dispatch --json >> "$HOME/.local/state/jig-loop.log" 2>&1
```

On macOS, save this as
`~/Library/LaunchAgents/sh.jig.example.loop-dispatch.plist`, replacing the
repository, user-home, and log paths:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>sh.jig.example.loop-dispatch</string>
  <key>ProgramArguments</key>
  <array>
    <string>/absolute/path/to/repository/scripts/jig</string>
    <string>loop</string>
    <string>dispatch</string>
    <string>--json</string>
  </array>
  <key>WorkingDirectory</key>
  <string>/absolute/path/to/repository</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/Users/example/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StartInterval</key>
  <integer>60</integer>
  <key>StandardOutPath</key>
  <string>/absolute/path/to/jig-loop.log</string>
  <key>StandardErrorPath</key>
  <string>/absolute/path/to/jig-loop.log</string>
</dict>
</plist>
```

Validate and load it with:

```sh
plutil -lint ~/Library/LaunchAgents/sh.jig.example.loop-dispatch.plist
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/sh.jig.example.loop-dispatch.plist
launchctl print "gui/$(id -u)/sh.jig.example.loop-dispatch"
```

Stop and unload it with:

```sh
launchctl bootout "gui/$(id -u)" ~/Library/LaunchAgents/sh.jig.example.loop-dispatch.plist
```

On Linux, use a user service and timer:

```ini
# ~/.config/systemd/user/jig-example-loop.service
[Unit]
Description=Dispatch Jig workflows

[Service]
Type=oneshot
WorkingDirectory=/absolute/path/to/repository
Environment=PATH=/home/example/.local/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=/absolute/path/to/repository/scripts/jig loop dispatch --json
```

```ini
# ~/.config/systemd/user/jig-example-loop.timer
[Unit]
Description=Dispatch Jig workflows every minute

[Timer]
OnCalendar=*-*-* *:*:00
Persistent=true

[Install]
WantedBy=timers.target
```

Enable it with:

```sh
systemctl --user daemon-reload
systemctl --user enable --now jig-example-loop.timer
systemctl --user list-timers jig-example-loop.timer
```

Disable it with `systemctl --user disable --now jig-example-loop.timer`.

A CI scheduler is suitable only when runs preserve the same checkout and its Git
metadata. The authoritative occurrence ledger and leases live in
worktree-specific Git metadata; an ephemeral checkout that starts empty on every
run cannot preserve at-most-once schedule history.

## Validate and operate

Inspect configuration and current state without running the prompt:

```sh
scripts/jig loop status --workflow daily-audit
```

Run one immediate manual occurrence before enabling the external scheduler:

```sh
scripts/jig loop tick --workflow daily-audit
```

`loop tick` executes the prompt regardless of its cron time, so a repo-mode test
can commit authorized changes. Inspect the final response, repository status,
receipts, and any retained worktree before enabling unattended runs.

Use `scripts/jig loop status` and scheduler logs for routine monitoring. When an
occurrence reports `needs_attention`, inspect its receipt and retained checkout,
then acknowledge the exact occurrence only after resolving its result:

```sh
scripts/jig loop acknowledge-occurrence --occurrence '<reported-id>'
```

Acknowledgement does not delete a retained worktree. Preserve or discard its
changes deliberately, remove the reported worktree with Git, and run status
again before expecting the workflow to resume.
