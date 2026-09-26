# Changelog

## Unreleased

## 0.1.1

- Track run lifecycle separately from task outcome, inspect complete records with
  `jevia runs show`, and explicitly recover interrupted executions without reruns.
- Add opt-in non-interactive process-tree supervision with independent harness
  and verifier deadlines, cancellation handling, and cleanup evidence.
- Distinguish process-exit, verification, and manual outcome sources. Only known
  verifier-backed or explicitly manual outcomes inform future routing. Feedback
  keeps a local audit trail and requires a reason when changing a known outcome.
- Coordinate concurrent equivalent cache misses across processes, with bounded
  waits, failure recovery, and a fresh run ID for every caller.
- Add preview-first history repair and archival with exact-byte backups before
  replacement; preserve active and pending runs and never delete snapshots
  automatically.
- Install checksum-verified prebuilt binaries from the landing page without
  requiring Rust or Cargo.

### Compatibility

New or updated history records use schema 3. Schemas 1 and 2 remain readable;
older Jevia versions refuse schema 3 rather than discarding its metadata. Back up
local history before upgrading if a downgrade may be needed. Legacy outcomes
without provenance stay inspectable but require explicit feedback confirmation
before they inform routing. Rust consumers constructing record structs directly
or exhaustively matching lifecycle states may need to update their code for the
new fields and variants.

Requires Rust 1.92 or newer when building from source. Existing interactive
harness execution remains the default; deadlines require `--non-interactive`.

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
