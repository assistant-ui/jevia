# Changelog

## Unreleased

- Read known outcomes and passive observations from one consistent snapshot;
  concurrent feedback cannot move an attempt out of both routing windows. JSONL
  collects both bounded windows in one validated pass under a shared lock.
- Index passive execution history in SQLite and PostgreSQL 16+ so unrelated
  routed/active runs do not require a full history scan before cache lookup.
  Existing stores can add the index explicitly with `storage init`; record and
  SQL schemas, history ordering, and optional feedback/verification are unchanged.
- Bound cache/JSONL file-lock acquisition; route live on cache contention and
  explicitly fail on unavailable history without dropping recorded evidence.
- Ignore native recording journals, loss markers, event locks, temporary snapshots,
  and generated plugins in Git; safely repair older project ignore rules before
  capture without changing configuration or untracking existing files.
- Add payload-free `recordings inspect` and preview-first `recordings cleanup`;
  explicitly confirmed cleanup archives only provably redundant auxiliary files,
  retaining active runs, journals, loss markers, and unpersisted snapshots.
- Keep agent deadlines/cancellation responsive during observation checkpoints;
  skip busy JSONL checkpoint locks and reject stale terminal supervisor writes.
- Bound recovery replay to one two-second budget, skip busy journals/history,
  and retain pending snapshots for later retry.
- Rotate bounded observation replay across retained candidates, preventing a
  corrupt/busy prefix from starving later journals; defer incomplete scans safely.
- Expose detected hook-write loss as partial coverage and a conservative lost-input
  lower bound, with a private, payload-free marker and idempotent recovery.
- Add opt-in `route --explain` and `run --explain` diagnostics on stderr for
  cache hits/miss reasons, coordination/cache writes, separate counts of known
  outcomes and passive execution observations,
  confidence fallback, and routing time. Keep task text out of explanations and
  preserve existing JSON output and persisted record formats.

## 0.1.6 — 2026-09-29

- Record harness execution automatically without requiring feedback or additional
  tests. Keep task outcomes unknown unless separately assessed; use passive
  observations and known outcomes as distinct routing context and cache inputs.
- Make additional verification opt-in while preserving explicitly configured
  verifiers and `auto_verify = true` settings.
- Capture allowlisted native activity for Claude Code 2.1.251+, tested Codex
  0.158.x on macOS/Linux with normal hook trust, and OpenCode v1 >= 1.18.33.
  Unsupported versions/configurations retain process-level recording.
- Checkpoint live observations, replay recoverable journals, retain whole-session
  counters beyond the bounded recent sample, and preserve durable events when
  final reads fail. Keep unreadable or subsequently changed journals for recovery.
- Add passive per-model activity/coverage to `stats` and read-only
  `harness health <name> [--json]` diagnostics. Neither infers task correctness.
- Add CI tests against pinned real harness binaries and loopback model endpoints,
  preserving normal hook trust and avoiding paid model calls.
- Release Node SDK `jevia@0.1.2` with optional `recordExecution()` for finished
  app-owned work. Save application-reported events without requiring feedback or
  verification; automatically reuse their summaries in subsequent routes.
- Bound and validate recording input, keep it off command arguments, support
  idempotent retries, and prevent replacement of active or supervised executions
  consistently across JSONL, SQLite, and PostgreSQL.
- Explicitly release execution, journal, history, and cache locks when their
  operation ends, even while a duplicated/inherited Unix descriptor remains open.
  Prevent false-active recovery failures and delayed lock release.

### Compatibility and upgrade

Back up history and upgrade every CLI/SDK reader and writer sharing a store
together: CLI/core 0.1.6 and Node SDK 0.1.2 use record schema 6. Schemas 1–5 remain
readable, but older clients reject newer records. SQL database schema 1 is
unchanged. Do not downgrade against updated history or silently switch to an old
JSONL copy. The SDK recording API requires CLI 0.1.6 or newer.

In CLI 0.1.4–0.1.5, omitted `auto_verify` enabled test discovery. In 0.1.6 it
defaults to false; opt in explicitly if you want Jevia to run additional tests.
Native events are best-effort operational facts, not verified success or complete
model attribution. Existing task-text privacy settings still apply to routing.

This experimental patch also adds Rust struct fields and enum variants; callers
constructing harness/execution structs or exhaustively matching observation types
must review their code. It is not source-compatible with every 0.1.5 library
caller. Rust 1.92 and Node.js 20 remain the minimum supported versions.

## 0.1.5 — 2026-09-29

- Add guided, shell-free harness presets for Codex, Claude Code, OpenCode, and
  Gemini CLI while keeping model IDs, credentials, and permissions explicit.
- Add a native Windows release binary and checksum-verifying PowerShell
  installer alongside the existing macOS and Linux installers.
- Document persisted run snapshots, default history locations, custom SQLite
  paths, and the distinction between routing decisions and verified outcomes.
- Automate real installer smoke tests on Linux, macOS, and Windows before
  updating the website's pinned release.
- Publish an SPDX SBOM and signed build-provenance and SBOM attestations for
  every release binary, then verify both from the installed executables.
- Add required CodeQL scanning for GitHub Actions, JavaScript/TypeScript, and
  Rust, and pin third-party workflow actions to immutable commits.

Existing configuration and history formats are unchanged. Rust 1.92 and Node.js
20 remain the minimum supported versions.

## 0.1.4 — 2026-09-28

- Validate live and cached routing decisions against the current tier policy
  before recording or reusing them. Invalid cached values fail open to a live
  request; invalid live responses fail without entering history or cache.
- Add `jevia runs complete` for explicitly finishing externally executed work
  with manual outcome evidence after the caller confirms that work has stopped.
- Automatically discover existing root Rust or Node test commands for harnesses
  without an explicit verifier, with preview, persistent opt-out, bounded
  execution, and process-tree cleanup.
- Redact provider, transport, SDK, and CLI diagnostics while retaining stable,
  allowlisted error categories and useful status/location metadata.
- Release `jevia@0.1.1` with storage setup/check methods, external completion,
  stricter runtime protocol validation, bounded command execution, process-tree
  cleanup, and privacy-safe error categories.
- Upgrade `process-wrap` to 10.0.1 after passing the full Rust, Node, storage,
  website, Linux, macOS, and Windows test matrix.

Existing configuration and history formats are unchanged. Rust 1.92 and Node.js
20 remain the minimum supported versions.

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
