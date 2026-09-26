# Architecture

Jevia separates semantic judgment from deterministic policy and execution.

```text
task
  -> local configuration and recent outcomes
  -> typed Jev choice over stable capability tiers
  -> local confidence policy
  -> route decision
  -> harness adapter (future milestone)
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

Adapters will translate a selected capability tier into a harness-specific
model and reasoning setting. Tier definitions remain stable when individual
model catalogs change. Adapters are intentionally not part of the first
milestone so the routing and persistence contracts can be reviewed first.

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

## Persistence

`.jevia/config.toml` is reviewable project policy. `.jevia/runs.jsonl` is
local operational data. Feedback updates are written to a temporary file,
flushed, synchronized, and renamed over the previous history.
