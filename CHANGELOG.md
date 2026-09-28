# Changelog

## Unreleased

## 0.1.3 — 2026-09-28

- Keep non-interactive Unix harness and verifier phases supervised until their
  process groups are empty, including descendants whose parent already exited.
  Keep cancellation and deadlines active and preserve the main command's exit code.
- Retry denied process-group probes within the existing deadline. Only confirmed
  absence permits completion; permanent uncertainty still reports interruption.
- Preserve JSONL record boundaries when an existing valid final record has no
  newline. Validate the unterminated history under its lock before appending;
  leave malformed or unsupported data untouched for explicit repair.
- Redact parser payloads in config, history, cache, and maintenance diagnostics,
  including error source chains and nonfatal routing-cache warnings. Retain safe
  validation guidance and line/column positions where available.

Existing configuration and history formats are unchanged. Rust 1.92 remains the
minimum supported version.

## 0.1.2 — 2026-09-27

- Explicitly release configuration setup locks on drop so duplicated Unix file
  descriptors cannot retain a completed setup's lock.
- Add read-only `harness check <name> [--json]` for project/template/model-mapping
  validation and local agent/verifier executable-candidate checks. Return versioned,
  redacted diagnostics with meaningful failure codes and verifier/Windows warnings.
  Never spawn a program, inspect credentials/history/cache, or connect to APIs or
  storage; document static-check and platform-lookup limitations explicitly.
- Add preview-first `harness setup` with explicit executable/argument templates,
  complete tier-to-model mappings, and optional verification. Require `--apply`
  to save and `--replace` to change an existing harness; retain existing verifiers
  unless explicitly changed/removed. Reuse database setup's config lock, private
  backups, concurrent-edit checks, and atomic replacement without launching
  programs or accessing APIs, storage, or credentials.
- Add a typed, shell-free `jevia` package for Node.js applications to route
  tasks, inspect run records, and submit explicit outcome feedback through the
  CLI JSON contract. Include cancellation, structured errors, tests, packaging
  checks, CI, and an adapter example for arbitrary coding harnesses.
- Stream `storage import-jsonl` through a private unnamed snapshot instead of
  loading every record into memory. Validate before taking the SQL write lock,
  keep duplicate/active-run checks and atomic preview/apply behavior, redact parse
  diagnostics, and roll back inserts and ordering counters on late read failures
  or conflicts. Preserve source files; document temporary-disk and ID-set costs.
- Add `storage check --deep` for non-mutating logical history validation across
  JSONL, SQLite, and PostgreSQL. Detect invalid schemas/identities, duplicate JSONL
  IDs, SQL learning-index drift, and invalid append ordering/counters. Scan SQL
  in a consistent bounded-page snapshot, redact record contents in diagnostics,
  and leave existing access checks unchanged. Never repair automatically.
- Stream `storage export` across JSONL, SQLite, and PostgreSQL without loading all
  history into memory. Use consistent SQL read snapshots with bounded keyset
  pages, retain JSONL shared locking, and publish only a fully written/synced file
  without overwriting destinations. Preserve append order and known provenance.
- Extend preview-first `jevia runs archive --keep N` to SQLite and PostgreSQL.
  Explicit apply saves private local project-record snapshots and archives before
  transactional, bounded-batch deletion. Preserve active, pending, legacy-unknown,
  and execution-owned rows; reject invalid or changed records and retain recovery
  files on failure. Document explicit restoration, cache/stats effects, and limits.
- Add preview-first `jevia storage setup sqlite|postgres` to configure a database
  without hand-editing TOML. Explicit apply validates/initializes the destination,
  optionally imports JSONL, backs up config, and switches config last while
  preserving unrelated settings/comments. Require stopped-writer confirmation;
  retain source history and keep PostgreSQL credentials in environment variables.
- Add `jevia stats [--limit <records>] [--json]` with totals and per-tier verified
  success rates, separate manual feedback, learning evidence, and recorded cache
  hits across JSONL, SQLite, and PostgreSQL. Report sample limits and undefined
  rates explicitly; never expose task text or infer correctness from process exit.
- Bound JSONL recent-history memory to the selected window while preserving full
  stream validation, append ordering, evidence filtering, and shared read locks.
- Add opt-in SQLite and PostgreSQL history backends while preserving JSONL as
  the default, with indexed evidence queries and transactional feedback/lifecycle
  updates. Decision caching stays local and respects shared evidence changes.
- Add explicit storage initialization/checks, preview-first atomic JSONL imports,
  and non-overwriting JSONL exports. Existing history is never moved implicitly.
- Require verified TLS for remote PostgreSQL, redact driver errors, and use
  project-scoped history plus session execution guards and fenced recovery.
  PostgreSQL recovery requires explicit confirmation that remote work stopped.
- Add PostgreSQL-backed CI and end-to-end storage/cache/lifecycle coverage.

### Compatibility

Existing configs still default to JSONL. SQL database schema 1 stores history
record schemas 1–3; no history format version is changed. Older CLI versions
cannot read the new `[storage]` configuration: do not downgrade against a SQL
project by silently switching it back to an outdated JSONL history. Rust callers
constructing `Config` directly must supply `storage` or use `..Config::default()`.
Despite the patch version, this experimental release includes that Rust source
API change; it is not source-compatible with every `jevia-core` 0.1.1 caller.
Review direct struct construction before upgrading the library. Existing CLI
configs without `[storage]` continue to work without migration. Back up history
and configuration before opting into SQL or planning a downgrade.

Requires Rust 1.92 or newer when building from source. SQLx remains on 0.8.6
to preserve that minimum.

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
