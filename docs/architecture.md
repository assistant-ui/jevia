# Architecture

Jevia separates semantic judgment from deterministic policy and execution.

```text
task
  -> local configuration and recent outcomes
  -> routing-input fingerprint and local cache lookup
  -> cached decision or typed Jev choice on a cache miss
  -> local confidence policy
  -> route decision
  -> configured harness adapter
  -> optional post-run verifier
  -> observed outcome
  -> local outcome history
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
root and mirrors its exit code. An optional shell-free verification process can
make the final outcome depend on project checks instead of trusting a
successful harness exit alone.

### Routing cache

The CLI owns a bounded local cache of Jev decision signals. The core crate owns
the canonical SHA-256 key contract so every input that can affect a decision is
fingerprinted consistently: task, endpoint, model, policy, tiers, harness
mapping, and the completed evidence included in the request. Raw task text is
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
- Only completed outcomes are supplied as evidence to later decisions.
- Harness templates must map every configured tier to a model.
- Harness arguments are executed directly without shell interpolation.
- A harness that cannot start leaves its routing outcome unknown.
- A failed harness skips verification and records failure.
- A configured verifier is authoritative after a successful harness run.
- A verifier that cannot start leaves the outcome unknown.
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

Schema-version-1 history remains readable. New or mutated records use version 2
to preserve lifecycle metadata from older writers. A per-run OS file lease is
held from before the running transition until the terminal record is saved.
Explicit recovery obtains that same lease non-blockingly before marking a
running/verifying record interrupted; it never reruns work or infers task failure.
The durable phases are routed, running, verifying, completed, launch_failed, and
interrupted. Start and finish timestamps are distinct from routing time.
Manual feedback is refused on active runs and never invents harness metadata.
