# Harden repository model boundaries

Centralize check dispatch policy, preserve typed evidence gates during update, correct affected selection, restore bounded action execution, and bound durable run and fingerprint queries. Add regression coverage and run repository gates.

## Progress

- Check failure and human-output policy now derive from the runtime response.
- Work gate reconciliation uses typed gate definitions and preserves custom evidence plus explicit `required` overrides.
- Root affected selection no longer treats explicit-input components as catch-all owners.
- MCP plans bind closed action arguments and execution requires exact mutating-effect approval.
- Durable cancellation scans only appended run events; committed fingerprints use top-level tree identities.
- Targeted regressions, Clippy, formatting, and the full `jig-sh` test suite pass.
