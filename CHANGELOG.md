# Changelog

## 0.1.0

First experimental crates.io release of `jevia` and `jevia-core`.

- Route tasks through Jev into configurable capability tiers, with a
  deterministic confidence fallback.
- Record outcomes locally and supply completed evidence to later routing
  requests.
- Launch shell-free harness adapters and optionally verify their results with
  a configured command.
- Preserve run history safely across concurrent processes.
- Cache equivalent decisions with expiration and evidence-aware invalidation.
- Inspect configuration and storage with `jevia doctor`, and verify a live
  routing round trip with `jevia check`.
- Expose machine-readable routing records and run history for integrations.

Requires Rust 1.92 or newer. Managed services and the dashboard are not part of
this release. Routing alone does not execute a task or prove that it succeeded.
