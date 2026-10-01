# Architecture

Jevia separates semantic judgment from deterministic policy and execution.

```text
task
  -> local configuration, outcomes, and passive execution history
  -> routing-input fingerprint and local cache lookup
  -> cached decision or typed Jev choice on a cache miss
  -> local confidence policy
  -> route decision
  -> configured harness adapter
  -> automatic process facts and supported native hooks
  -> optional explicitly enabled verifier / optional reported outcome
  -> history (unknown outcomes remain unknown)
```

## Boundaries

### `jevia-core`

Owns versioned configuration, the Jev wire contract, confidence fallback,
route records, bounded observation types, and outcome types. It does not read
environment variables or write files.

### `jevia-cli`

Owns command parsing, project discovery, credential lookup, harness supervision,
and the selected JSONL, SQLite, or PostgreSQL store. JSONL is the default; SQL is
explicitly configured, not an automatic mirror or fallback. Local history and
recording artifacts are ignored by Git, but Git ignore rules are not access
control and do not untrack files already committed.

`jevia doctor` validates configuration, selected storage, cache, and credential
presence without calling Jev. PostgreSQL storage checks still contact the
configured database and use a rolled-back write probe. `jevia check`
adds one live Jev round trip to verify credentials, connectivity, and response
decoding, but deliberately does not persist that synthetic check as routing
history or cache data.

On a live route, the current task and eligible historical task text are sent to
the configured Jev endpoint along with bounded outcome/observation summaries.
`privacy.store_task_text = false` omits task text from **new history records**;
it does not hide the current routing request or redact previously stored tasks.
Raw native hook payloads, transcripts, and tool contents are not persisted, and
feedback reasons, session/agent IDs, and raw event lists are not included in the
routing summaries. A cache hit avoids the Jev classification call, not the
history read used to construct its key.

### `packages/jevia-node`

The Node SDK invokes the local CLI with structured arguments and validates its
JSON output. It does not connect directly to databases. Storage selection,
recording, and routing-history behavior therefore use the same CLI contracts.
`route()` records the decision but does not run or observe an application-owned
agent. Apps may call `recordExecution()` for finished work and optionally report
feedback; neither a verifier nor a known outcome is required to save those facts.
Application-reported observations retain their own source, separate from native
hooks. The recording API requires CLI 0.1.6 or newer.

### `website`

Owns the public Farm.js product site and copyable onboarding flow. It is a
static product surface with no access to routing history, API credentials, or
managed-service data. The private dashboard remains a separate repository.

### Harness adapters

Adapters translate a selected capability tier into a harness-specific model
and argument list. Tier definitions remain stable when individual model
catalogs change. Templates are rendered into a process and argument vector;
they are never passed through a shell. Jevia launches the child in the project
root and mirrors its exit code. Recording is automatic and language-independent.
After a successful exit, Jevia runs extra checks only when explicitly configured.
CLI 0.1.4–0.1.5 enabled root-test detection by default; the CLI 0.1.6 default is
opt-in and preserves existing explicit choices. Passive observations enter future
routing without a known task outcome or manual feedback/completion call.
A passing optional check is evidence, not proof of every requirement.

Process completion and optional verification use the launched session boundary.
Supported adapters also capture internal activity: Claude Code uses silent
exec-form hooks, Codex uses trusted inline hooks in an isolated local session,
and OpenCode uses a private plugin via a per-child runtime config override.
Version/config gates fall back to process facts without breaking the harness.
Neutral tool completion and request-model observations are not success labels.
See the [native adapter contract](reference.md#native-harness-observations).
Native events are bounded and allowlisted; raw
prompts, tool contents, and transcript files are never retained. Model IDs are
stored only when reported, not inferred from configured aliases or successful tools.
Coverage is best-effort, not complete attribution or an acceptance signal.
Harness installation, credentials, model mapping, and optional project test
prerequisites are still user setup. `route` and SDK routing methods stop at the
routing decision; their caller owns execution, verification, and outcome reporting.
SDK feedback and verification are optional. A route without feedback is still
recorded with an unknown outcome and does not block subsequent routing. Existing
eligible outcomes continue to inform those decisions automatically.
Explicit SDK feedback is application-reported evidence and does not require a
CLI verifier or external-completion call to become eligible. Every subsequent
`route` reads eligible history from the selected backend before building its
cache key and Jev request. Separate outcome and observation windows each use
`router.history_limit` and apply equally to SDK and CLI callers; no separate SDK
history payload is needed. Learning here means automatically supplying recorded
context to subsequent decisions, not training model weights or labeling every
completed process as a successful task.

### Routing cache

The CLI owns a bounded local cache of Jev decision signals. The core crate owns
the canonical SHA-256 key contract so every input that can affect a decision is
fingerprinted consistently: task, endpoint, model, policy, tiers, harness
mapping, and both history windows included in the request. Native summaries omit
session/agent IDs and raw event lists. Raw task text is
not persisted in the cache.

Cache hits reuse only the decision signal. Each hit receives a new run ID,
timestamp, and `source=cache` marker before it becomes a run record. Cache
read/write failures fail open to a live request; API errors are not inserted.
Expired entries are never used as an offline fallback.

### Managed services

Managed classification, synchronization, and analytics will use explicit API
contracts and opt-in data movement. The private dashboard has its own
repository and security boundary; no dashboard code belongs here.

## Routing invariants

- Jev chooses only from tiers declared in project configuration.
- Unknown tiers and malformed responses fail closed.
- Confidence below the configured floor selects the configured safe tier.
- API credentials are read from the process environment and never persisted.
- Every persisted record carries a schema version.
- Known outcomes and passive execution facts remain separate routing inputs.
- Harness templates must map every configured tier to a model.
- Harness arguments are executed directly without shell interpolation.
- A harness that cannot start leaves its routing outcome unknown.
- A failed harness skips verification, records its exit, and leaves task outcome unknown.
- A configured verifier is authoritative after a successful harness run.
- A verifier that cannot start leaves the outcome unknown.
- Only verifier-backed or explicitly reported outcomes count as quality evidence.
  Finished execution observations also inform routing, without inventing success.
  Active/routed-only records are excluded from the observation window.
- Manual feedback preserves prior outcomes and original execution evidence. Local
  feedback reasons are never included in a Jev request.
- Completed harness runs retain model, harness, verification, duration, and
  exit evidence.
- Cache hits receive fresh run identities and remain distinguishable from live
  decisions.
- Only equivalent, unexpired routing inputs may reuse a cached decision.

## Persistence

`.jevia/config.toml` is reviewable project policy. With the default JSONL backend,
`.jevia/runs.jsonl` is local operational data. Feedback updates are written to a
temporary file, flushed, synchronized, and renamed over the previous history.

JSONL history reads take a shared lock and mutations take an exclusive lock
on `.jevia/runs.lock`. The separate lock file remains stable when the history
file is atomically replaced. Appends are encoded before locking and written as
one buffer; updates hold the lock across the complete read-modify-replace
transaction. On Unix, Jevia also synchronizes the parent directory after a
history mutation.

SQLite stores history at its configured filesystem path; PostgreSQL stores it in
an explicitly named project namespace with credentials resolved from an
environment variable. SQL keeps the complete versioned record plus derived query indexes.
Transactions and project-scoped write coordination protect updates; SQLite uses
file-based execution leases alongside the database, while PostgreSQL uses
session advisory locks. PostgreSQL requires direct or session-pooled connections,
not transaction-mode pooling. Selecting SQL does not import old JSONL history or
fall back to it on an error; migration is an explicit, preview-first operation.
See the [storage reference](reference.md#storage).

The CLI 0.1.7 SQL passive-history lookup uses an additive partial index on
`(project, ordinal)` in SQLite and PostgreSQL 16+. Its predicate mirrors
`is_execution_observation() && !is_learning_evidence()`, with parity tests across
supported record schemas. The database maintains membership as records change;
no application-maintained observation flag can become stale under older writers.
The query selects a bounded, newest-first window in one statement/snapshot and
reverses it to append order before routing. It does not fetch all intervening
pending/active runs. Selected records are still decoded/validated; deep checking
remains the full-history integrity diagnostic. Explicit `storage init` adds the
index to an existing store without a schema bump or record rewrite; normal opens
do not migrate. Older PostgreSQL versions retain the paginated snapshot fallback.
See [upgrade details](reference.md#passive-history-lookup).

`.jevia/cache.jsonl` is bounded, ignored local data protected by the stable
`.jevia/cache.lock` sidecar. Inserts remove expired entries, replace an existing
fingerprint, evict the oldest entries above the configured limit, then use the
same synchronized atomic-replacement pattern as history updates. Cache
corruption is reported without rewriting the file; `jevia cache clear` is the
explicit recovery operation.

On a cache miss, a bounded set of 256 stable OS-lock stripes coordinates live
requests across processes. Waiters poll only the lease with capped backoff and
reload history after waiting, preserving fresh feedback without repeated full
history scans during contention. A lease covers the live request and cache insertion,
not global history/cache locks. Waiters reload evidence and recheck the cache;
changed evidence selects a new fingerprint. Waits are bounded (Jev timeout plus
one second, capped at 30 seconds), then fail open to live routing. API failures
and process exits release the lease, and `--no-cache` bypasses coordination.

Record schemas 1–5 remain readable. New records and typed record updates use
**schema 6**, including application-reported observations; the SQL database
schema remains **1**. Older readers/writers may reject newer records, so upgrade
all clients sharing a store together and back up before upgrading. Do not
downgrade against updated history or silently switch back to an old JSONL copy.

A per-run execution lease is held from before the running transition until the
terminal record is saved. Explicit recovery attempts that same lease without
waiting for another supervisor to release it before marking a
running/verifying record interrupted; it never reruns work or infers task failure.
PostgreSQL recovery also requires confirmation that remote work has stopped;
losing a database connection does not prove its child process exited.
The durable phases are routed, running, verifying, completed, launch_failed,
interrupted, cancelled, and timed_out. Start and finish timestamps are distinct
from routing time. Stable lock sidecars are not removed to force an unlock;
file-lock guards explicitly release ownership when the operation ends.
Manual feedback is refused on active runs and never invents harness metadata.
For CLI-run work, a configured verifier takes precedence; otherwise root-project
test detection requires `auto_verify = true`. Detected checks use the same durable
verification state and evidence, run with CI semantics and owned process-tree cleanup, and have a default
five-minute deadline. Missing/ambiguous checks leave process-only evidence; they
are never silently treated as verification. No manual completion is needed after
`run`. Route-only/SDK integrations remain explicit and do not execute tests.
External completion is a separate, explicitly confirmed operation. It holds the
per-run lease and atomically records manual feedback plus a completed lifecycle
only for pending, unowned external work. It does not invent a start time or
execution/verifier evidence. Ordinary feedback never changes lifecycle state.

### Native checkpoint and replay lifecycle

Native hooks append normalized events to a private, bounded per-run journal under
`.jevia`. Hook subprocesses never mutate SQL records or compete for supervisor
ownership. The supervisor attempts to checkpoint changed snapshots to the
**selected backend, including SQL**, every two seconds during harness execution,
and saves a final snapshot with terminal execution state. These are periodic
snapshots, not a durable per-event SQL stream.

The recent sample holds at most 256 events; whole-session totals and bounded
per-model counters survive sampling. Coverage and discarded-input counts remain
separate from outcomes. A private, payload-free loss marker can indicate a hook
write was missed; its contribution is a conservative lower bound, not an exact
count of lost events. Repeated reads/replay do not multiply that contribution.

Checkpoint waits have a one-second supervisor budget, while process cancellation
and deadlines stay responsive. Busy JSONL checkpoint locks are skipped, and
late supervisor writes cannot overwrite terminal state or revoked SQL ownership.
An unreadable final journal preserves earlier durable observations and marks
coverage partial rather than erasing them. Cleanup removes the journal/loss
marker only after confirming they still match the saved snapshot; failed writes,
unreadable files, or later events retain the retry source.

Before routing a new `route`/`run`, Jevia attempts best-effort journal replay
under execution and journal leases. `runs recover` also attempts targeted replay
after its lifecycle safety checks. Replay updates observations without changing
task outcomes or inferring that surviving children stopped. Active runs remain
active and retain their journals for later events. Ambiguous, corrupt, missing,
or contended inputs remain for retry or inspection. Read-only `runs show` and
`stats` do not trigger replay.

Each replay attempt has one two-second budget covering scanning, locking,
reading, and persistence. A complete directory enumeration selects only the next
128 owners in cursor order, retaining at most two paths per owner to detect
duplicates. There is no fixed entry-count cutoff; an incomplete/expired scan
still defers all work. Explicit recording maintenance inventories all names.
Work can be deferred; this is not an unbounded repair job or a
guarantee that the entire route command completes in two seconds. The budget
bounds awaiting replay, not cancellation of an already-running filesystem
operation. Required history reads, cache coordination, provider requests, and
harness execution are separate.
See the [recording reference](reference.md#native-harness-observations) for
adapter support, privacy, replay limits, and version-specific caveats.

### History maintenance

History maintenance is explicit and preview-first. JSONL apply takes the same
exclusive history lock, revalidates the complete input, durably saves an exact original-byte
backup, and then atomically replaces the active file. Archival also saves removed
records before replacement. Raw record bytes are retained, including additive
metadata; unknown schemas and duplicate identities are refused. Repair only
handles a truncated final JSON line without a newline, or a missing final newline
on otherwise valid history. Active/pending and legacy-unknown records cannot be
archived. Archives are outside the active learning/feedback history and are never
deleted automatically. JSONL maintenance still scans the full active history in
memory.

SQL archival instead scans in bounded pages under project write coordination and
saves a full backup plus an archive of removed records before deleting rows in a
transaction. Export/import and deep checks are separate explicit operations;
JSONL tail repair is not SQL repair. Archives are not database-native disaster
recovery, and an ambiguous commit error requires inspection before retrying.
See [history maintenance](reference.md#history-maintenance).

## Implementation map

- [CLI routing and supervision](../crates/jevia-cli/src/main.rs): history loading,
  cache decisions, checkpoint scheduling, and lifecycle commands.
- [Native recording](../crates/jevia-cli/src/observations.rs): bounded journals,
  snapshot persistence, replay, and safe final cleanup.
- [Storage facade](../crates/jevia-cli/src/storage.rs): shared JSONL/SQL contracts
  and execution leases; [SQL implementation](../crates/jevia-cli/src/storage/database.rs).
- [Record schema](../crates/jevia-core/src/route.rs) and
  [Jev request construction](../crates/jevia-core/src/jev.rs): outcome eligibility,
  passive history, and cache-key material.
- [Node SDK](../packages/jevia-node/src/index.ts): CLI-backed application APIs.
