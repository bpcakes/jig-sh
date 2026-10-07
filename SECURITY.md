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
keeps plaintext out of repository state, structured command output, and
receipts, but a child process that receives a value can still use or disclose
it. Output redaction reduces accidental exposure; it cannot stop a
malicious child, transformed output, operating-system inspection, or side
channels. Jig commands other than `vault`, and `init` or `adopt --write` with
vault setup, remove `JIG_VAULT_PASSPHRASE` and `JIG_VAULT_NEW_PASSPHRASE` at
startup, so configured commands, dev apps, workers, and launched agents do not
inherit the vault passphrase itself. Repo-scoped vault namespaces are
path-bound: a linked Git worktree shares its main checkout's vault only after
Jig verifies the link from Git metadata that only a writer of that
repository's `.git` can create. The operator-owned vault rules in generated
`AGENTS.md` guidance are advice for coding agents, not an enforcement boundary:
a passphrase exported into an agent session is visible to every process that
agent starts.

Format 3 vaults add two narrower protections. A per-user rollback witness
outside every vault home lets authenticated commands refuse older or forked
copies of previously witnessed state; it is local rather than a remote
authority, and it does not survive whole-profile rollback, deletion or
replacement of the witness, or same-user or root compromise. A format 3
passphrase change rotates the data-encryption key, so a key recovered from an
earlier copy cannot decrypt later state. Rotation is not revocation: earlier
vault files and backups remain decryptable with the passphrase they were made
with, the audit key does not change, and values revealed earlier stay exposed.

Witness checkpoints and locks are local to one user profile. Independently
enrolled profiles sharing a vault diverge after a mutation; a profile with
an older checkpoint refuses the newer state as an unwitnessed fork. Use one
persistent owning profile for ongoing access. Recovery from an accepting
profile uses an authenticated encrypted backup and restore to an absent
target on the refusing profile; it does not synchronize witnesses. See
[rollback witness and recovery](docs/configuration.md#rollback-witness-and-recovery).
Never delete or edit witness records to bypass a refusal.
