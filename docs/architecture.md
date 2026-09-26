# Architecture

Jevia separates semantic judgment from deterministic policy and execution.

```text
task
  -> local configuration and recent outcomes
  -> typed Jev choice over stable capability tiers
  -> local confidence policy
  -> route decision
  -> configured harness adapter
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

### Harness adapters

Adapters translate a selected capability tier into a harness-specific model
and argument list. Tier definitions remain stable when individual model
catalogs change. Templates are rendered into a process and argument vector;
they are never passed through a shell. Jevia launches the child in the project
root, mirrors its exit code, and records success or failure from the resulting
process status.

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
- Completed harness runs retain the concrete model, duration, and exit code.

## Persistence

`.jevia/config.toml` is reviewable project policy. `.jevia/runs.jsonl` is
local operational data. Feedback updates are written to a temporary file,
flushed, synchronized, and renamed over the previous history.

Execution evidence is an optional additive field so existing schema-version-1
history remains readable. Manual feedback changes the outcome without
inventing harness metadata.
