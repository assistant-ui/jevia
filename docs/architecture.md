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
route records, and outcome types. It does not read environment variables or
write files.

### `jevia-cli`

Owns command parsing, project discovery, credential lookup, and the local
JSONL store. Task text is local and ignored by Git unless a user explicitly
moves or publishes it.

`jevia doctor` validates local state without a network request. `jevia check`
adds one live Jev round trip to verify credentials, connectivity, and response
decoding, but deliberately does not persist that synthetic check as routing
history or cache data.

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
CLI 0.1.4–0.1.5 enabled root-test detection by default; the unreleased default is
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
cache key and Jev request. Separate outcome and observation windows each use `router.history_limit`
apply equally to SDK and CLI callers; no separate SDK history payload is needed.

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

`.jevia/config.toml` is reviewable project policy. `.jevia/runs.jsonl` is
local operational data. Feedback updates are written to a temporary file,
flushed, synchronized, and renamed over the previous history.

All history reads take a shared lock and all mutations take an exclusive lock
on `.jevia/runs.lock`. The separate lock file remains stable when the history
file is atomically replaced. Appends are encoded before locking and written as
one buffer; updates hold the lock across the complete read-modify-replace
transaction. On Unix, Jevia also synchronizes the parent directory after a
history mutation.

`.jevia/cache.jsonl` is bounded, ignored local data protected by the stable
`.jevia/cache.lock` sidecar. Inserts remove expired entries, replace an existing
fingerprint, evict the oldest entries above the configured limit, then use the
same synchronized atomic-replacement pattern as history updates. Cache
corruption is reported without rewriting the file; `jevia cache clear` is the
explicit recovery operation.

On a cache miss, a bounded set of 256 stable OS-lock stripes coordinates live
requests across processes. A lease covers the live request and cache insertion,
not global history/cache locks. Waiters reload evidence and recheck the cache;
changed evidence selects a new fingerprint. Waits are bounded (Jev timeout plus
one second, capped at 30 seconds), then fail open to live routing. API failures
and process exits release the lease, and `--no-cache` bypasses coordination.

Schema-version-1, -2, and -3 history remains readable. New or mutated records use
version 4 to protect native observations from older writers. A per-run OS file lease is
held from before the running transition until the terminal record is saved.
Explicit recovery obtains that same lease non-blockingly before marking a
running/verifying record interrupted; it never reruns work or infers task failure.
The durable phases are routed, running, verifying, completed, launch_failed, and
interrupted. Start and finish timestamps are distinct from routing time.
Manual feedback is refused on active runs and never invents harness metadata.
For CLI-run work, a configured verifier takes precedence; otherwise root-project
test detection requires `auto_verify = true`. Detected checks use the same durable verification state and
evidence, run with CI semantics and owned process-tree cleanup, and have a default
five-minute deadline. Missing/ambiguous checks leave process-only evidence; they
are never silently treated as verification. No manual completion is needed after
`run`. Route-only/SDK integrations remain explicit and do not execute tests.
External completion is a separate, explicitly confirmed operation. It holds the
per-run lease and atomically records manual feedback plus a completed lifecycle
only for pending, unowned external work. It does not invent a start time or
execution/verifier evidence. Ordinary feedback never changes lifecycle state.

Native hooks append normalized events to a private, bounded per-run journal under
`.jevia`. The supervisor merges the snapshot into its selected backend at process
termination using the existing ownership lease. Hook subprocesses never mutate
SQL records or compete for supervisor ownership. Successful persistence removes
the journal; a crash/failed write leaves it for manual inspection. `runs recover`
does not replay journals, and event snapshots are not streamed live to SQL.

History maintenance is explicit and preview-first. Apply takes the same exclusive
history lock, revalidates the complete input, durably saves an exact original-byte
backup, and then atomically replaces the active file. Archival also saves removed
records before replacement. Raw record bytes are retained, including additive
metadata; unknown schemas and duplicate identities are refused. Repair only
handles a truncated final JSON line without a newline, or a missing final newline
on otherwise valid history. Active/pending and legacy-unknown records cannot be
archived. Archives are outside the active learning/feedback history and are never
deleted automatically. Maintenance still scans the full active history in memory.
