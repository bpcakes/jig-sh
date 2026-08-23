Prevent cancellation from consuming PR repair attempts or obscuring committed work results; align worker overflow documentation; validate contract and full configured tests.

Typed cancellation now survives worker and GitHub supervision until the PR repair attempt boundary. Cancelled repairs release leases, record cancelled worker evidence, preserve partial thread-update diagnostics, and do not consume attempt budget. Targeted PR-manager tests and strict crate Clippy pass.

Removed the global post-success cancellation check. Operations now own cancellation before their durable commit point, preventing successful state mutations from being reclassified as errors. Added a regression that verifies work start returns the committed plan id even when cancellation becomes visible only after entry.

Decoupled the Codex last-message result file from optional schema validation. All review, refinement, and PR-repair workers now use a bounded -o file as authoritative output while provider transcripts remain truncatable diagnostics. Updated both public contract documents and added schema-less result-channel coverage.