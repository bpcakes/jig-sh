# Security Policy

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitHub's private
vulnerability reporting for `bpcakes/jig-sh` when it is available. If that
option is not available, contact the repository owner through GitHub without
including exploit details or sensitive repository data in the first message.

Include the affected Jig version or commit, host platform, contract version,
reproduction steps, and expected impact. Remove secrets, private paths, and
downstream project identifiers from reports and attached output.

## Supported versions

Jig is pre-1.0 and does not currently publish a security-support matrix. Test
reports against the latest release or the current development branch when
possible. Runtime readability for an older contract epoch does not imply that
an older Jig product release receives security backports.

## Vault boundary

Jig Vault is a local development tool, not a production secret manager. It
keeps plaintext out of repository state, structured command output, MCP
results, and receipts, but a child process that receives a value can still use
or disclose it. Output redaction reduces accidental exposure; it cannot stop a
malicious child, transformed output, operating-system inspection, or side
channels.
